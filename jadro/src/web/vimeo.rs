//! Vimeo: nastavenia prehrávača (`playerConfig`) → stream HLS alebo súbor MP4.
//!
//! Postup ako pri prehrávaní v prehliadači: stránka prehrávača `player.vimeo.com/video/ID`
//! (pri neverejnom videu s kľúčom `?h=`), pri obmedzení na doménu s Refererom stránky, kde je
//! video vložené. Ak stránka nastavenia nemá, skúsi sa `…/video/ID/config`. Šifrované video
//! (DRM) jadro nesťahuje.

use super::html;
use crate::chyba::{Chyba, Vysledok};
use reqwest::header::{ACCEPT, REFERER};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Serialize)]
pub struct VideoVimeo {
    pub id: String,
    pub nazov: String,
    pub autor: String,
    pub trvanie: f64,
    /// adresy HLS v poradí: predvolená CDN, H.264 (avc_url), ďalšie CDN
    pub hls: Vec<String>,
    /// priame MP4 od najvyššej kvality: (výška, adresa)
    pub mp4: Vec<(u32, String)>,
    /// titulky: (jazyk, názov, adresa)
    pub titulky: Vec<(String, String, String)>,
}

/// Adresa servera prehrávača (pre testy sa dá zmeniť premennou KOMETA_VIMEO_ZAKLAD).
fn zaklad() -> String {
    std::env::var("KOMETA_VIMEO_ZAKLAD")
        .ok()
        .filter(|z| z.starts_with("http://") || z.starts_with("https://"))
        .unwrap_or_else(|| "https://player.vimeo.com".into())
        .trim_end_matches('/')
        .to_string()
}

/// (ID, kľúč h) z ktorejkoľvek adresy Vimeo.
pub fn id_a_kluc(adresa: &str) -> Option<(String, Option<String>)> {
    let p = super::vzory::prehravac(adresa).filter(|p| p.poskytovatel == "Vimeo")?;
    let u = url::Url::parse(&p.url).ok()?;
    let mut s = u.path_segments()?;
    if s.next()? != "video" {
        return None;
    }
    let id = s.next()?.to_string();
    let h = u
        .query_pairs()
        .find(|(k, _)| k == "h")
        .map(|(_, v)| v.into_owned());
    Some((id, h))
}

/// Objekt JSON od pozície `{` – podľa zátvoriek mimo reťazcov.
pub fn objekt_json(text: &str, od: usize) -> Option<&str> {
    let b = text.as_bytes();
    let zac = od + text[od..].find('{')?;
    let (mut hlbka, mut v_retazci, mut unik) = (0i32, false, false);
    for (i, &c) in b.iter().enumerate().skip(zac) {
        if v_retazci {
            match (unik, c) {
                (true, _) => unik = false,
                (false, b'\\') => unik = true,
                (false, b'"') => v_retazci = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => v_retazci = true,
            b'{' => hlbka += 1,
            b'}' => {
                hlbka -= 1;
                if hlbka == 0 {
                    return Some(&text[zac..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Nastavenia prehrávača zo stránky (`window.playerConfig = {…}` alebo staršie `var config = {…}`).
pub fn nastavenia_zo_stranky(text: &str) -> Option<Value> {
    for znacka in [
        "playerConfig",
        "var config",
        "window.vimeo.clip_page_config",
        "\"config\":",
    ] {
        let mut od = 0;
        while let Some(i) = text[od..].find(znacka) {
            let za = od + i + znacka.len();
            let zvysok = text[za..].trim_start();
            if zvysok.starts_with('=') || zvysok.starts_with('{') || znacka.ends_with(':') {
                if let Some(o) = objekt_json(text, za) {
                    if let Ok(v) = serde_json::from_str::<Value>(o) {
                        if v.get("request").is_some() || v.get("video").is_some() {
                            return Some(v);
                        }
                    }
                }
            }
            od = za;
        }
    }
    None
}

fn retazec(v: &Value, cesta: &[&str]) -> Option<String> {
    let mut x = v;
    for k in cesta {
        x = x.get(*k)?;
    }
    x.as_str().map(str::to_string)
}

/// Rozoberie nastavenia prehrávača.
pub fn rozober(v: &Value, id: &str) -> Vysledok<VideoVimeo> {
    let subory = v.pointer("/request/files").cloned().unwrap_or(Value::Null);
    // šifrovanie: Vimeo OTT a niektoré platené videá
    let text = subory.to_string();
    if text.contains("\"drm\"")
        || text.contains("widevine")
        || text.contains("fairplay")
        || text.contains("playready")
    {
        return Err(Chyba::Drm("Vimeo".into()));
    }
    let mut hls = vec![];
    if let Some(h) = subory.get("hls") {
        let cdns = h.get("cdns").and_then(Value::as_object);
        let predvolena = h.get("default_cdn").and_then(Value::as_str).unwrap_or("");
        let mut poradie: Vec<&String> = cdns.map(|c| c.keys().collect()).unwrap_or_default();
        poradie.sort_by_key(|k| (k.as_str() != predvolena, k.as_str().to_string()));
        for k in poradie {
            let c = &cdns.unwrap()[k];
            for pole in ["url", "avc_url"] {
                if let Some(u) = c.get(pole).and_then(Value::as_str) {
                    if !hls.iter().any(|x| x == u) {
                        hls.push(u.to_string());
                    }
                }
            }
        }
    }
    let mut mp4: Vec<(u32, String)> = subory
        .get("progressive")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let u = p.get("url")?.as_str()?.to_string();
                    let h = p
                        .get("height")
                        .and_then(Value::as_u64)
                        .or_else(|| {
                            p.get("quality")?
                                .as_str()?
                                .trim_end_matches('p')
                                .parse()
                                .ok()
                        })
                        .unwrap_or(0) as u32;
                    Some((h, u))
                })
                .collect()
        })
        .unwrap_or_default();
    mp4.sort_by_key(|m| std::cmp::Reverse(m.0));
    let zaklad = zaklad();
    let titulky = v
        .pointer("/request/text_tracks")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let u = t.get("url")?.as_str()?;
                    let u = if u.starts_with('/') {
                        format!("{zaklad}{u}")
                    } else {
                        u.to_string()
                    };
                    Some((
                        t.get("lang")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        t.get("label")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        u,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if hls.is_empty() && mp4.is_empty() {
        if subory.get("dash").is_some() {
            return Err(Chyba::Nepodporovane(
                "Vimeo poskytlo len DASH vo vlastnom formáte".into(),
            ));
        }
        return Err(Chyba::Format(
            "Vimeo neposkytlo žiadny stream ani súbor".into(),
        ));
    }
    Ok(VideoVimeo {
        id: id.to_string(),
        nazov: retazec(v, &["video", "title"])
            .map(|t| html::entity(&t))
            .unwrap_or_default(),
        autor: retazec(v, &["video", "owner", "name"]).unwrap_or_default(),
        trvanie: v
            .pointer("/video/duration")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        hls,
        mp4,
        titulky,
    })
}

fn chyba_pristupu(kod: u16, telo: &str, referer: Option<&str>) -> Chyba {
    let t = telo.to_ascii_lowercase();
    if kod == 404 {
        return Chyba::Ine("Video na Vimeo neexistuje alebo ho autor odstránil.".into());
    }
    if t.contains("password") || t.contains("heslo") {
        return Chyba::Ine("Video na Vimeo je chránené heslom – jadro ho zatiaľ nevie odomknúť (použi okno Komety a zadaj heslo).".into());
    }
    if t.contains("privacy") || t.contains("cannot be played here") || kod == 403 {
        return Chyba::Ine(match referer {
            None => "Vimeo dovoľuje toto video prehrať len na stránke, kde je vložené. Zadaj adresu tej stránky (--referer) alebo stiahni rovno celú stránku – skener Referer doplní sám.".into(),
            Some(r) => format!("Vimeo nedovolilo prehrať video ani so stránkou {r} – video môže byť súkromné alebo viazané na prihlásenie."),
        });
    }
    Chyba::Http(kod)
}

/// Zistí stream alebo súbor videa Vimeo.
pub async fn zisti(k: &Client, adresa: &str, referer: Option<&str>) -> Vysledok<VideoVimeo> {
    let (id, h) =
        id_a_kluc(adresa).ok_or_else(|| Chyba::Format("nie je to adresa videa Vimeo".into()))?;
    let z = zaklad();
    let q = h.as_deref().map(|h| format!("?h={h}")).unwrap_or_default();
    let ref_ = referer.unwrap_or("https://vimeo.com/");
    let r = k
        .get(format!("{z}/video/{id}{q}"))
        .header(REFERER, ref_)
        .header(ACCEPT, "text/html,*/*")
        .send()
        .await?;
    let kod = r.status().as_u16();
    let telo = r.text().await.unwrap_or_default();
    match kod {
        200..=299 => {
            if let Some(v) = nastavenia_zo_stranky(&telo) {
                return rozober(&v, &id);
            }
            if telo.to_ascii_lowercase().contains("password") {
                return Err(chyba_pristupu(401, &telo, referer));
            }
        }
        // neexistuje, súkromné alebo obmedzené na doménu – iná adresa na tom nič nezmení
        401 | 403 | 404 => return Err(chyba_pristupu(kod, &telo, referer)),
        _ => {}
    }
    // záloha: priamo nastavenia (JSON)
    let r = k
        .get(format!("{z}/video/{id}/config{q}"))
        .header(REFERER, ref_)
        .header(ACCEPT, "application/json")
        .send()
        .await?;
    let kod = r.status().as_u16();
    let telo = r.text().await.unwrap_or_default();
    if !(200..300).contains(&kod) {
        return Err(chyba_pristupu(kod, &telo, referer));
    }
    let v: Value = serde_json::from_str(&telo)
        .map_err(|_| Chyba::Format("Vimeo vrátilo nečitateľné nastavenia".into()))?;
    rozober(&v, &id)
}

#[cfg(test)]
mod testy {
    use super::*;

    const NASTAVENIA: &str = r#"{"request":{"files":{"dash":{"cdns":{}},"hls":{"default_cdn":"fastly_skyfire","separate_av":true,
        "cdns":{"akfire_interconnect_quic":{"url":"https://vod-adaptive-ak.vimeocdn.com/x/playlist.m3u8?s=1","avc_url":"https://vod-adaptive-ak.vimeocdn.com/x/avc.m3u8"},
        "fastly_skyfire":{"url":"https://skyfire.vimeocdn.com/y/playlist.m3u8","avc_url":"https://skyfire.vimeocdn.com/y/avc.m3u8"}}},
        "progressive":[{"url":"https://v.sk/360.mp4","quality":"360p"},{"url":"https://v.sk/1080.mp4","height":1080}]},
        "text_tracks":[{"lang":"sk","label":"Slovenčina","url":"/texttrack/1.vtt?token=a"}]},
        "video":{"id":76979871,"title":"Sunrise &amp; mountains","duration":62.5,"owner":{"name":"Autor"}}}"#;

    #[test]
    fn nastavenia_prehravaca() {
        let stranka = format!(
            "<html><script>window.playerConfig = {NASTAVENIA}; var x = {{}};</script></html>"
        );
        let v = nastavenia_zo_stranky(&stranka).unwrap();
        let r = rozober(&v, "76979871").unwrap();
        assert_eq!(r.nazov, "Sunrise & mountains");
        assert_eq!(r.autor, "Autor");
        assert_eq!(r.trvanie, 62.5);
        assert_eq!(
            r.hls[0], "https://skyfire.vimeocdn.com/y/playlist.m3u8",
            "predvolená CDN prvá"
        );
        assert_eq!(r.hls.len(), 4);
        assert_eq!(r.mp4[0], (1080, "https://v.sk/1080.mp4".into()));
        assert_eq!(r.titulky[0].0, "sk");
        assert!(r.titulky[0].2.ends_with("/texttrack/1.vtt?token=a"));
    }

    #[test]
    fn json_so_zatvorkami_v_retazcoch() {
        let t = r#"x = {"a":"}{\"}","b":{"c":1}} ; y"#;
        assert_eq!(objekt_json(t, 0), Some(r#"{"a":"}{\"}","b":{"c":1}}"#));
    }

    #[test]
    fn drm_a_bez_streamu() {
        let v: Value = serde_json::from_str(
            r#"{"request":{"files":{"hls":{"cdns":{"x":{"url":"u"}},"drm":{"widevine":{}}}}}}"#,
        )
        .unwrap();
        assert!(matches!(rozober(&v, "1"), Err(Chyba::Drm(_))));
        let v: Value =
            serde_json::from_str(r#"{"request":{"files":{"dash":{"cdns":{}}}},"video":{}}"#)
                .unwrap();
        assert!(matches!(rozober(&v, "1"), Err(Chyba::Nepodporovane(_))));
    }

    #[test]
    fn adresy() {
        assert_eq!(
            id_a_kluc("https://vimeo.com/76979871/8272103f6e"),
            Some(("76979871".into(), Some("8272103f6e".into())))
        );
        assert_eq!(
            id_a_kluc("https://player.vimeo.com/video/123456"),
            Some(("123456".into(), None))
        );
        assert_eq!(
            id_a_kluc("https://www.youtube.com/watch?v=uL9-e9OlLfw"),
            None
        );
    }
}
