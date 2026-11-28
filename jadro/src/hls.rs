//! HLS (`.m3u8`, RFC 8216) – vlastný parser playlistov a výber kvality a jazykov.
//!
//! Šifrovanie (`EXT-X-KEY` / `EXT-X-SESSION-KEY` s METHOD inou ako NONE) jadro zámerne
//! nesťahuje: vráti chybu [`Chyba::Drm`].

use crate::chyba::{Chyba, Vysledok};
use crate::kusky::{Kusok, Stopa};
use serde::Serialize;
use url::Url;

#[derive(Clone, Debug, Serialize)]
pub struct Varianta {
    pub uri: String,
    pub bandwidth: u64,
    pub sirka: Option<u32>,
    pub vyska: Option<u32>,
    pub kodeky: Option<String>,
    pub audio: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Rendicia {
    pub typ: String,
    pub skupina: String,
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
    pub uri: Option<String>,
    pub predvolena: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Master {
    pub varianty: Vec<Varianta>,
    pub rendicie: Vec<Rendicia>,
    pub sifrovanie: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Media {
    pub stopa: Stopa,
    pub sifrovanie: Option<String>,
    pub koniec: bool,
    pub trvanie: f64,
    /// fragmentované MP4 (EXT-X-MAP / .m4s) – inak MPEG-TS
    pub fmp4: bool,
}

#[derive(Clone, Debug)]
pub enum Playlist {
    Master(Master),
    Media(Media),
}

/// Atribúty `KLUC=hodnota,KLUC2="hodnota, s čiarkou"`.
pub fn atributy(s: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] == b',' || b[i] == b' ') {
            i += 1;
        }
        let ks = i;
        while i < b.len() && b[i] != b'=' && b[i] != b',' {
            i += 1;
        }
        let kluc = s[ks..i].trim().to_string();
        if i >= b.len() || b[i] == b',' {
            if !kluc.is_empty() {
                out.push((kluc, String::new()));
            }
            continue;
        }
        i += 1; // '='
        let hodnota;
        if i < b.len() && b[i] == b'"' {
            let vs = i + 1;
            i = vs;
            while i < b.len() && b[i] != b'"' {
                i += 1;
            }
            hodnota = s[vs..i.min(b.len())].to_string();
            i += 1;
        } else {
            let vs = i;
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            hodnota = s[vs..i].trim().to_string();
        }
        out.push((kluc, hodnota));
    }
    out
}

fn atr<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter()
        .find(|(x, _)| x.eq_ignore_ascii_case(k))
        .map(|(_, v)| v.as_str())
}

fn spoj(base: &Url, u: &str) -> Vysledok<String> {
    base.join(u)
        .map(|x| x.to_string())
        .map_err(|_| Chyba::Format(format!("neplatná adresa v playliste: {u}")))
}

/// Rozsah `dĺžka[@posun]`.
fn rozsah(s: &str) -> Option<(u64, Option<u64>)> {
    let mut it = s.trim().splitn(2, '@');
    let dlzka = it.next()?.trim().parse().ok()?;
    let posun = it.next().and_then(|x| x.trim().parse().ok());
    Some((dlzka, posun))
}

fn opis_sifrovania(a: &[(String, String)]) -> Option<String> {
    let metoda = atr(a, "METHOD").unwrap_or("NONE");
    if metoda.eq_ignore_ascii_case("NONE") {
        return None;
    }
    Some(match atr(a, "KEYFORMAT") {
        Some(f) if !f.is_empty() && f != "identity" => format!("HLS {metoda}, {f}"),
        _ => format!("HLS {metoda}"),
    })
}

pub fn parse(text: &str, base: &Url) -> Vysledok<Playlist> {
    let riadky: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if riadky
        .first()
        .map(|l| !l.starts_with("#EXTM3U"))
        .unwrap_or(true)
    {
        return Err(Chyba::Format("toto nie je HLS playlist (#EXTM3U)".into()));
    }
    if riadky.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF")) {
        let mut m = Master::default();
        let mut i = 0;
        while i < riadky.len() {
            let l = riadky[i];
            if let Some(r) = l.strip_prefix("#EXT-X-STREAM-INF:") {
                let a = atributy(r);
                let mut j = i + 1;
                while j < riadky.len() && riadky[j].starts_with('#') {
                    j += 1;
                }
                if j < riadky.len() {
                    let (sirka, vyska) = atr(&a, "RESOLUTION")
                        .and_then(|r| r.split_once(['x', 'X']))
                        .map(|(w, h)| (w.parse().ok(), h.parse().ok()))
                        .unwrap_or((None, None));
                    m.varianty.push(Varianta {
                        uri: spoj(base, riadky[j])?,
                        bandwidth: atr(&a, "BANDWIDTH")
                            .or(atr(&a, "AVERAGE-BANDWIDTH"))
                            .and_then(|b| b.parse().ok())
                            .unwrap_or(0),
                        sirka,
                        vyska,
                        kodeky: atr(&a, "CODECS").map(String::from),
                        audio: atr(&a, "AUDIO").map(String::from),
                    });
                    i = j;
                }
            } else if let Some(r) = l.strip_prefix("#EXT-X-MEDIA:") {
                let a = atributy(r);
                m.rendicie.push(Rendicia {
                    typ: atr(&a, "TYPE").unwrap_or("").to_ascii_uppercase(),
                    skupina: atr(&a, "GROUP-ID").unwrap_or("").to_string(),
                    jazyk: atr(&a, "LANGUAGE").map(String::from),
                    nazov: atr(&a, "NAME").map(String::from),
                    uri: atr(&a, "URI").map(|u| spoj(base, u)).transpose()?,
                    predvolena: atr(&a, "DEFAULT")
                        .map(|d| d.eq_ignore_ascii_case("YES"))
                        .unwrap_or(false),
                });
            } else if let Some(r) = l.strip_prefix("#EXT-X-SESSION-KEY:") {
                m.sifrovanie = m.sifrovanie.or(opis_sifrovania(&atributy(r)));
            }
            i += 1;
        }
        return Ok(Playlist::Master(m));
    }

    let mut m = Media::default();
    let mut posledny_koniec: std::collections::HashMap<String, u64> = Default::default();
    let mut caka_rozsah: Option<(u64, Option<u64>)> = None;
    for l in &riadky {
        if let Some(r) = l.strip_prefix("#EXT-X-KEY:") {
            m.sifrovanie = m.sifrovanie.or(opis_sifrovania(&atributy(r)));
        } else if let Some(r) = l.strip_prefix("#EXT-X-MAP:") {
            let a = atributy(r);
            let uri = atr(&a, "URI").ok_or_else(|| Chyba::Format("EXT-X-MAP bez URI".into()))?;
            let rz = atr(&a, "BYTERANGE")
                .and_then(rozsah)
                .map(|(d, p)| (p.unwrap_or(0), d));
            m.stopa.init = Some(Kusok {
                url: spoj(base, uri)?,
                rozsah: rz,
            });
            m.fmp4 = true;
        } else if let Some(r) = l.strip_prefix("#EXTINF:") {
            m.trvanie += r
                .split(',')
                .next()
                .and_then(|d| d.trim().parse::<f64>().ok())
                .unwrap_or(0.0);
        } else if let Some(r) = l.strip_prefix("#EXT-X-BYTERANGE:") {
            caka_rozsah = rozsah(r);
        } else if *l == "#EXT-X-ENDLIST" || l.eq_ignore_ascii_case("#EXT-X-PLAYLIST-TYPE:VOD") {
            m.koniec = true;
        } else if !l.starts_with('#') {
            let url = spoj(base, l)?;
            let rz = caka_rozsah.take().map(|(dlzka, posun)| {
                let p = posun.unwrap_or_else(|| *posledny_koniec.get(&url).unwrap_or(&0));
                posledny_koniec.insert(url.clone(), p + dlzka);
                (p, dlzka)
            });
            if m.stopa.kusky.is_empty() && (l.contains(".m4s") || l.contains(".mp4")) {
                m.fmp4 = true;
            }
            m.stopa.kusky.push(Kusok { url, rozsah: rz });
        }
    }
    Ok(Playlist::Media(m))
}

/// Najlepšia varianta, nanajvýš `max_vyska` (ak je zadaná); pri `presne` práve tá kvalita
/// (inak najbližšia) – pozri [`crate::kvalita`].
pub fn vyber_variantu(v: &[Varianta], max_vyska: Option<u32>, presne: bool) -> Option<&Varianta> {
    let trieda = |x: &Varianta| crate::kvalita::trieda(x.sirka, x.vyska);
    let vhodne: Vec<&Varianta> = match (max_vyska, presne) {
        (Some(ciel), true) => {
            let t = crate::kvalita::presna_trieda(v.iter().filter_map(trieda), ciel);
            v.iter().filter(|x| t.is_none() || trieda(x) == t).collect()
        }
        _ => v
            .iter()
            .filter(|x| crate::kvalita::do_stropu(trieda(x), max_vyska))
            .collect(),
    };
    let zoznam = if vhodne.is_empty() {
        v.iter().collect()
    } else {
        vhodne
    };
    zoznam
        .into_iter()
        .max_by_key(|x| (x.bandwidth, x.vyska.unwrap_or(0)))
}

fn zhoda_jazyka(jazyk: Option<&str>, chcem: &str) -> bool {
    let Some(j) = jazyk else { return false };
    let (j, c) = (j.to_ascii_lowercase(), chcem.to_ascii_lowercase());
    j == c || j.split('-').next() == c.split('-').next() && !c.contains('-')
}

/// Zvukové rendície pre skupinu varianty v poradí jazykov (napr. cs, sk, en); inak predvolená.
pub fn vyber_zvuk<'a>(
    r: &'a [Rendicia],
    skupina: Option<&str>,
    jazyky: &[String],
) -> Vec<&'a Rendicia> {
    let zvuky: Vec<&Rendicia> = r
        .iter()
        .filter(|x| {
            x.typ == "AUDIO" && x.uri.is_some() && skupina.map(|s| s == x.skupina).unwrap_or(true)
        })
        .collect();
    let mut out: Vec<&Rendicia> = vec![];
    for j in jazyky {
        if let Some(x) = zvuky.iter().find(|x| zhoda_jazyka(x.jazyk.as_deref(), j)) {
            if !out.iter().any(|o| std::ptr::eq(*o, *x)) {
                out.push(x);
            }
        }
    }
    if out.is_empty() {
        if let Some(x) = zvuky.iter().find(|x| x.predvolena).or(zvuky.first()) {
            out.push(x);
        }
    }
    out
}

#[cfg(test)]
mod testy {
    use super::*;

    fn u() -> Url {
        Url::parse("https://cdn.example.com/video/master.m3u8").unwrap()
    }

    #[test]
    fn atributy_s_uvodzovkami() {
        let a = atributy(
            r#"BANDWIDTH=1280000,CODECS="avc1.4d401f,mp4a.40.2",RESOLUTION=1280x720,AUDIO="aud""#,
        );
        assert_eq!(atr(&a, "codecs"), Some("avc1.4d401f,mp4a.40.2"));
        assert_eq!(atr(&a, "RESOLUTION"), Some("1280x720"));
        assert_eq!(atr(&a, "AUDIO"), Some("aud"));
    }

    #[test]
    fn master_a_vyber() {
        let t = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"en\",NAME=\"English\",DEFAULT=YES,URI=\"en/a.m3u8\"\n\
                 #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"sk\",NAME=\"Slovenčina\",URI=\"sk/a.m3u8\"\n\
                 #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",LANGUAGE=\"cs\",NAME=\"Čeština\",URI=\"cs/a.m3u8\"\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,AUDIO=\"a\"\n360/v.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO=\"a\"\n1080/v.m3u8\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720,AUDIO=\"a\"\nhttps://iny.example.com/720.m3u8\n";
        let Playlist::Master(m) = parse(t, &u()).unwrap() else {
            panic!()
        };
        assert_eq!(m.varianty.len(), 3);
        assert_eq!(
            vyber_variantu(&m.varianty, None, false).unwrap().vyska,
            Some(1080)
        );
        let v720 = vyber_variantu(&m.varianty, Some(720), false).unwrap();
        assert_eq!(
            vyber_variantu(&m.varianty, Some(1080), true).unwrap().vyska,
            Some(1080)
        );
        assert_eq!(
            vyber_variantu(&m.varianty, Some(950), true).unwrap().vyska,
            Some(1080)
        );
        assert_eq!(
            vyber_variantu(&m.varianty, Some(900), true).unwrap().vyska,
            Some(720)
        ); // rovnako ďaleko → nižšia
        assert_eq!(v720.uri, "https://iny.example.com/720.m3u8");
        let z = vyber_zvuk(&m.rendicie, Some("a"), &["cs".into(), "sk".into()]);
        assert_eq!(
            z.iter()
                .map(|x| x.jazyk.clone().unwrap())
                .collect::<Vec<_>>(),
            vec!["cs", "sk"]
        );
        assert_eq!(
            z[0].uri.as_deref(),
            Some("https://cdn.example.com/video/cs/a.m3u8")
        );
        let predvolena = vyber_zvuk(&m.rendicie, Some("a"), &[]);
        assert_eq!(predvolena[0].jazyk.as_deref(), Some("en"));
    }

    #[test]
    fn media_rozsahy_a_init() {
        let t = "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"720@0\"\n\
                 #EXTINF:6.0,\n#EXT-X-BYTERANGE:1000@720\nmedia.mp4\n#EXTINF:5.5,\n#EXT-X-BYTERANGE:900\nmedia.mp4\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(t, &u()).unwrap() else {
            panic!()
        };
        assert!(m.koniec && m.fmp4 && m.sifrovanie.is_none());
        assert_eq!(m.stopa.init.as_ref().unwrap().rozsah, Some((0, 720)));
        assert_eq!(m.stopa.kusky[0].rozsah, Some((720, 1000)));
        assert_eq!(m.stopa.kusky[1].rozsah, Some((1720, 900)));
        assert!((m.trvanie - 11.5).abs() < 1e-9);
    }

    #[test]
    fn sifrovanie_sa_rozpozna() {
        let t = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST";
        let Playlist::Media(m) = parse(t, &u()).unwrap() else {
            panic!()
        };
        assert!(m.sifrovanie.unwrap().contains("SAMPLE-AES"));
        let t2 = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:6,\na.ts\n#EXT-X-ENDLIST";
        let Playlist::Media(m2) = parse(t2, &u()).unwrap() else {
            panic!()
        };
        assert!(m2.sifrovanie.is_none());
    }
}
