//! Ďalšie prehrávače s verejným rozhraním: JW Player, Wistia, Cloudflare Stream, Bunny Stream.
//!
//! Každý vráti adresu streamu alebo súboru, názov a Referer, ak je potrebný. Adresy serverov sa
//! dajú pre testy zmeniť premennými KOMETA_JW_ZAKLAD, KOMETA_WISTIA_ZAKLAD a KOMETA_CF_ZAKLAD.

use super::vzory;
use crate::chyba::{Chyba, Vysledok};
use reqwest::header::{ACCEPT, REFERER};
use reqwest::Client;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Zdroj {
    Stream(String),
    Subor { url: String, pripona: String },
}

#[derive(Clone, Debug)]
pub struct Video {
    pub zdroj: Zdroj,
    pub nazov: String,
}

fn zaklad(premenna: &str, predvolena: &str) -> String {
    std::env::var(premenna)
        .ok()
        .filter(|z| z.starts_with("http://") || z.starts_with("https://"))
        .unwrap_or_else(|| predvolena.into())
        .trim_end_matches('/')
        .to_string()
}

fn posledny_segment(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .path_segments()?
        .rfind(|s| !s.is_empty())
        .map(str::to_string)
}

async fn json(k: &Client, url: &str, referer: Option<&str>) -> Vysledok<Value> {
    let mut z = k.get(url).header(ACCEPT, "application/json");
    if let Some(r) = referer {
        z = z.header(REFERER, r);
    }
    let r = z.send().await?;
    if !r.status().is_success() {
        return Err(Chyba::Http(r.status().as_u16()));
    }
    let t = r.text().await?;
    serde_json::from_str(&t).map_err(|_| Chyba::Format(format!("{url}: nečitateľná odpoveď")))
}

/// JW Player: `cdn.jwplayer.com/v2/media/ID` → HLS (alebo najlepšie MP4).
pub fn rozober_jw(v: &Value) -> Option<Video> {
    let p = v.get("playlist")?.as_array()?.first()?;
    let nazov = p
        .get("title")
        .or(v.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let zdroje = p.get("sources")?.as_array()?;
    let hls = zdroje.iter().find_map(|s| {
        let f = s.get("file")?.as_str()?;
        let t = s.get("type").and_then(Value::as_str).unwrap_or("");
        (t.contains("mpegurl") || f.contains(".m3u8")).then(|| f.to_string())
    });
    if let Some(h) = hls {
        return Some(Video {
            zdroj: Zdroj::Stream(h),
            nazov,
        });
    }
    let mp4 = zdroje
        .iter()
        .filter_map(|s| {
            let f = s.get("file")?.as_str()?;
            let t = s.get("type").and_then(Value::as_str).unwrap_or("");
            (t.contains("mp4") || f.contains(".mp4"))
                .then(|| (s.get("height").and_then(Value::as_u64).unwrap_or(0), f))
        })
        .max_by_key(|(h, _)| *h)?;
    Some(Video {
        zdroj: Zdroj::Subor {
            url: mp4.1.to_string(),
            pripona: "mp4".into(),
        },
        nazov,
    })
}

/// Wistia: `fast.wistia.com/embed/medias/ID.json` → najlepšie MP4, inak HLS.
pub fn rozober_wistia(v: &Value, id: &str, zaklad: &str) -> Option<Video> {
    let m = v.get("media")?;
    let nazov = m
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let najlepsie = m
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let typ = a.get("type").and_then(Value::as_str).unwrap_or("");
            let kont = a
                .get("container")
                .or(a.get("ext"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let url = a.get("url")?.as_str()?;
            let video = typ.ends_with("_video") || (typ == "original" && kont == "mp4");
            (video && (kont == "mp4" || typ.contains("mp4")))
                .then(|| (a.get("height").and_then(Value::as_u64).unwrap_or(0), url))
        })
        .max_by_key(|(h, _)| *h);
    match najlepsie {
        Some((_, u)) => Some(Video {
            zdroj: Zdroj::Subor {
                url: u.to_string(),
                pripona: "mp4".into(),
            },
            nazov,
        }),
        None => Some(Video {
            zdroj: Zdroj::Stream(format!("{zaklad}/embed/medias/{id}.m3u8")),
            nazov,
        }),
    }
}

/// Zistí stream/súbor prehrávača. `None` = prehrávač nepatrí medzi tieto.
pub async fn zisti(
    k: &Client,
    p: &vzory::Prehravac,
    referer: Option<&str>,
) -> Vysledok<Option<Video>> {
    match p.poskytovatel {
        "JW Player" => {
            let id = posledny_segment(&p.url).unwrap_or_default();
            let z = zaklad("KOMETA_JW_ZAKLAD", "https://cdn.jwplayer.com");
            let v = json(k, &format!("{z}/v2/media/{id}"), referer).await?;
            rozober_jw(&v)
                .map(Some)
                .ok_or_else(|| Chyba::Format("JW Player neposkytol stream ani súbor".into()))
        }
        "Wistia" => {
            let id = posledny_segment(&p.url).unwrap_or_default();
            let z = zaklad("KOMETA_WISTIA_ZAKLAD", "https://fast.wistia.com");
            let v = json(k, &format!("{z}/embed/medias/{id}.json"), referer).await?;
            rozober_wistia(&v, &id, &z)
                .map(Some)
                .ok_or_else(|| Chyba::Format("Wistia neposkytla video".into()))
        }
        "Cloudflare Stream" => {
            let id = posledny_segment(&p.url).unwrap_or_default();
            let z = zaklad("KOMETA_CF_ZAKLAD", "https://videodelivery.net");
            Ok(Some(Video {
                zdroj: Zdroj::Stream(format!("{z}/{id}/manifest/video.m3u8")),
                nazov: String::new(),
            }))
        }
        "Bunny Stream" => {
            // stránka prehrávača obsahuje adresu playlistu; bez správneho Referera ju server nevydá
            let mut z = k.get(&p.url);
            if let Some(r) = referer {
                z = z.header(REFERER, r);
            }
            let r = z.send().await?;
            if !r.status().is_success() {
                return Err(Chyba::Http(r.status().as_u16()));
            }
            let text = super::html::odkry(&r.text().await?);
            let hls = vzory::adresy_v_texte(&text)
                .into_iter()
                .find(|a| matches!(vzory::medium(a), Some((vzory::Druh::Stream, _))))
                .ok_or_else(|| {
                    Chyba::Format(
                        "Bunny Stream: playlist sa nenašiel (obmedzenie na doménu?)".into(),
                    )
                })?;
            let d = super::html::citaj(&text);
            Ok(Some(Video {
                zdroj: Zdroj::Stream(hls),
                nazov: d.titul,
            }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn jw_player() {
        let v: Value = serde_json::from_str(
            r#"{"title":"Zoznam","playlist":[{"title":"Ukážka","sources":[
            {"file":"https://cdn.jwplayer.com/videos/AbCd1234-a.mp4","type":"video/mp4","height":720},
            {"file":"https://cdn.jwplayer.com/manifests/AbCd1234.m3u8","type":"application/vnd.apple.mpegurl"}]}]}"#,
        )
        .unwrap();
        let r = rozober_jw(&v).unwrap();
        assert_eq!(r.nazov, "Ukážka");
        assert_eq!(
            r.zdroj,
            Zdroj::Stream("https://cdn.jwplayer.com/manifests/AbCd1234.m3u8".into())
        );
        let v: Value = serde_json::from_str(
            r#"{"playlist":[{"sources":[{"file":"https://x/a-360.mp4","type":"video/mp4","height":360},{"file":"https://x/a-1080.mp4","type":"video/mp4","height":1080}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            rozober_jw(&v).unwrap().zdroj,
            Zdroj::Subor {
                url: "https://x/a-1080.mp4".into(),
                pripona: "mp4".into()
            }
        );
    }

    #[test]
    fn wistia() {
        let v: Value = serde_json::from_str(
            r#"{"media":{"name":"Webinár","assets":[
            {"type":"original","url":"https://embed-ssl.wistia.com/deliveries/o.bin","container":"mov","height":2160},
            {"type":"hd_mp4_video","url":"https://embed-ssl.wistia.com/deliveries/hd.bin","container":"mp4","height":1080},
            {"type":"md_mp4_video","url":"https://embed-ssl.wistia.com/deliveries/md.bin","container":"mp4","height":540},
            {"type":"still_image","url":"https://embed-ssl.wistia.com/deliveries/s.jpg"}]}}"#,
        )
        .unwrap();
        let r = rozober_wistia(&v, "abcd1234ef", "https://fast.wistia.com").unwrap();
        assert_eq!(r.nazov, "Webinár");
        assert_eq!(
            r.zdroj,
            Zdroj::Subor {
                url: "https://embed-ssl.wistia.com/deliveries/hd.bin".into(),
                pripona: "mp4".into()
            }
        );
        let v: Value = serde_json::from_str(r#"{"media":{"name":"X","assets":[]}}"#).unwrap();
        assert_eq!(
            rozober_wistia(&v, "abcd1234ef", "https://fast.wistia.com")
                .unwrap()
                .zdroj,
            Zdroj::Stream("https://fast.wistia.com/embed/medias/abcd1234ef.m3u8".into())
        );
    }
}
