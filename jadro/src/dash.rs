//! DASH (`.mpd`, ISO/IEC 23009-1) – vlastný parser manifestu a výber kvality a jazykov.
//!
//! Podporuje SegmentTemplate (s číslom aj s časovou osou SegmentTimeline), SegmentList,
//! SegmentBase a samotný BaseURL. Reťaz BaseURL (MPD → Period → AdaptationSet → Representation)
//! sa skladá podľa normy. Šifrovaný obsah (`ContentProtection`) jadro zámerne nesťahuje – vráti
//! [`Chyba::Drm`]; živé vysielanie (`type="dynamic"`) vráti [`Chyba::Zive`].

use crate::chyba::{Chyba, Vysledok};
use crate::kusky::{Kusok, Stopa};
use roxmltree::{Document, Node};
use serde::Serialize;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Druh {
    Video,
    Zvuk,
    Titulky,
    Ine,
}

/// Jedna kvalita jednej stopy (Representation) aj so zoznamom kúskov.
#[derive(Clone, Debug, Serialize)]
pub struct Reprezentacia {
    pub id: String,
    pub druh: Druh,
    pub jazyk: Option<String>,
    pub bandwidth: u64,
    pub sirka: Option<u32>,
    pub vyska: Option<u32>,
    pub kodeky: Option<String>,
    pub mime: Option<String>,
    pub stopa: Stopa,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Mpd {
    pub trvanie: f64,
    pub reprezentacie: Vec<Reprezentacia>,
}

/// Trvanie v tvare ISO 8601 (`PT1H2M3.5S`, `P1DT2H`) na sekundy.
pub fn trvanie_iso(s: &str) -> Option<f64> {
    let s = s.trim();
    let rest = s.strip_prefix('P')?;
    let (datum, cas) = match rest.split_once('T') {
        Some((d, c)) => (d, c),
        None => (rest, ""),
    };
    let mut sek = 0.0;
    let mut cislo = String::new();
    for ch in datum.chars() {
        match ch {
            '0'..='9' | '.' => cislo.push(ch),
            'Y' => sek += cislo.parse::<f64>().ok()? * 365.0 * 86400.0,
            'M' => sek += cislo.parse::<f64>().ok()? * 30.0 * 86400.0,
            'W' => sek += cislo.parse::<f64>().ok()? * 7.0 * 86400.0,
            'D' => sek += cislo.parse::<f64>().ok()? * 86400.0,
            _ => return None,
        }
        if !ch.is_ascii_digit() && ch != '.' {
            cislo.clear();
        }
    }
    for ch in cas.chars() {
        match ch {
            '0'..='9' | '.' => cislo.push(ch),
            'H' => sek += cislo.parse::<f64>().ok()? * 3600.0,
            'M' => sek += cislo.parse::<f64>().ok()? * 60.0,
            'S' => sek += cislo.parse::<f64>().ok()?,
            _ => return None,
        }
        if !ch.is_ascii_digit() && ch != '.' {
            cislo.clear();
        }
    }
    Some(sek)
}

/// Doplní premenné šablóny: `$RepresentationID$`, `$Number$`, `$Number%05d$`, `$Time$`, `$Bandwidth$`, `$$`.
pub fn sablona(t: &str, id: &str, cislo: u64, cas: u64, bandwidth: u64) -> String {
    let mut out = String::with_capacity(t.len() + 16);
    let mut casti = t.split('$');
    if let Some(prvy) = casti.next() {
        out.push_str(prvy);
    }
    let mut v_premennej = true;
    for cast in casti {
        if v_premennej {
            if cast.is_empty() {
                out.push('$');
            } else {
                let (meno, format) = match cast.split_once('%') {
                    Some((m, f)) => (m, Some(f)),
                    None => (cast, None),
                };
                let hodnota = match meno {
                    "RepresentationID" => Some(id.to_string()),
                    "Number" => Some(formatuj(cislo, format)),
                    "Time" => Some(formatuj(cas, format)),
                    "Bandwidth" => Some(formatuj(bandwidth, format)),
                    _ => None,
                };
                match hodnota {
                    Some(h) => out.push_str(&h),
                    None => {
                        out.push('$');
                        out.push_str(cast);
                        out.push('$');
                    }
                }
            }
        } else {
            out.push_str(cast);
        }
        v_premennej = !v_premennej;
    }
    out
}

fn formatuj(n: u64, format: Option<&str>) -> String {
    // jediný povolený tvar: %0Nd
    if let Some(f) = format {
        if let Some(sirka) = f
            .strip_prefix('0')
            .and_then(|x| x.strip_suffix('d'))
            .and_then(|x| x.parse::<usize>().ok())
        {
            return format!("{n:0sirka$}");
        }
    }
    n.to_string()
}

fn deti<'a, 'i>(n: Node<'a, 'i>, meno: &'static str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == meno)
}

fn dieta<'a, 'i>(n: Node<'a, 'i>, meno: &'static str) -> Option<Node<'a, 'i>> {
    deti(n, meno).next()
}

fn cislo<T: std::str::FromStr>(n: Node, attr: &str) -> Option<T> {
    n.attribute(attr).and_then(|v| v.trim().parse().ok())
}

fn base(n: Node, rodic: &Url) -> Vysledok<Url> {
    match dieta(n, "BaseURL").and_then(|b| b.text()) {
        Some(t) if !t.trim().is_empty() => rodic
            .join(t.trim())
            .map_err(|e| Chyba::Format(format!("BaseURL: {e}"))),
        _ => Ok(rodic.clone()),
    }
}

/// Rozsah `100-199` → (posun, dĺžka).
fn rozsah(s: Option<&str>) -> Option<(u64, u64)> {
    let (a, b) = s?.split_once('-')?;
    let (a, b): (u64, u64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (b >= a).then_some((a, b - a + 1))
}

fn je_sifrovane(n: Node) -> Option<String> {
    n.descendants()
        .find(|d| d.is_element() && d.tag_name().name() == "ContentProtection")
        .map(|d| {
            d.attribute("schemeIdUri")
                .unwrap_or("ContentProtection")
                .to_string()
        })
}

/// Spojí atribúty SegmentTemplate z vyšších úrovní (AdaptationSet) a nižších (Representation).
#[derive(Default, Clone)]
struct Sablona<'a, 'b> {
    media: Option<String>,
    init: Option<String>,
    start: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    pto: Option<u64>,
    timeline: Option<Node<'a, 'b>>,
}

impl<'a, 'b> Sablona<'a, 'b> {
    fn prekry(&mut self, n: Node<'a, 'b>) {
        if let Some(v) = n.attribute("media") {
            self.media = Some(v.to_string());
        }
        if let Some(v) = n.attribute("initialization") {
            self.init = Some(v.to_string());
        }
        self.start = cislo(n, "startNumber").or(self.start);
        self.timescale = cislo(n, "timescale").or(self.timescale);
        self.duration = cislo(n, "duration").or(self.duration);
        self.pto = cislo(n, "presentationTimeOffset").or(self.pto);
        if let Some(t) = dieta(n, "SegmentTimeline") {
            self.timeline = Some(t);
        }
    }
}

fn kusky_zo_sablony(s: &Sablona, url: &Url, id: &str, bw: u64, trvanie: f64) -> Vysledok<Stopa> {
    let media = s
        .media
        .as_deref()
        .ok_or_else(|| Chyba::Format("SegmentTemplate bez media".into()))?;
    let join = |t: &str| {
        url.join(t)
            .map(|u| u.to_string())
            .map_err(|e| Chyba::Format(e.to_string()))
    };
    let init = match &s.init {
        Some(i) => Some(Kusok::new(join(&sablona(i, id, 0, 0, bw))?)),
        None => None,
    };
    let start = s.start.unwrap_or(1);
    let ts = s.timescale.unwrap_or(1).max(1);
    let mut kusky = vec![];
    if let Some(tl) = s.timeline {
        let koniec = s.pto.unwrap_or(0) + (trvanie * ts as f64).round() as u64;
        let mut t: u64 = s.pto.unwrap_or(0);
        let mut n = start;
        let segmenty: Vec<_> = deti(tl, "S").collect();
        for (i, seg) in segmenty.iter().enumerate() {
            if let Some(st) = cislo::<u64>(*seg, "t") {
                t = st;
            }
            let d: u64 =
                cislo(*seg, "d").ok_or_else(|| Chyba::Format("SegmentTimeline bez d".into()))?;
            if d == 0 {
                return Err(Chyba::Format("SegmentTimeline s nulovým d".into()));
            }
            let r: i64 = cislo(*seg, "r").unwrap_or(0);
            let opakovani = if r >= 0 {
                r as u64 + 1
            } else {
                // r = -1: až po ďalší S s časom t, inak po koniec obdobia
                let hranica = segmenty
                    .get(i + 1)
                    .and_then(|x| cislo::<u64>(*x, "t"))
                    .unwrap_or(koniec);
                hranica.saturating_sub(t).div_ceil(d)
            };
            for _ in 0..opakovani {
                kusky.push(Kusok::new(join(&sablona(media, id, n, t, bw))?));
                t += d;
                n += 1;
            }
        }
    } else {
        let d = s.duration.ok_or_else(|| {
            Chyba::Format("SegmentTemplate bez duration aj SegmentTimeline".into())
        })?;
        if trvanie <= 0.0 {
            return Err(Chyba::Format("neznáme trvanie videa".into()));
        }
        let pocet = ((trvanie * ts as f64) / d as f64).ceil() as u64;
        if pocet > 500_000 {
            return Err(Chyba::Format("priveľa kúskov".into()));
        }
        for k in 0..pocet {
            kusky.push(Kusok::new(join(&sablona(media, id, start + k, k * d, bw))?));
        }
    }
    Ok(Stopa { init, kusky })
}

fn kusky_zo_zoznamu(list: Node, url: &Url) -> Vysledok<Stopa> {
    let join = |t: &str| {
        url.join(t)
            .map(|u| u.to_string())
            .map_err(|e| Chyba::Format(e.to_string()))
    };
    let init = match dieta(list, "Initialization") {
        Some(i) => Some(Kusok {
            url: match i.attribute("sourceURL") {
                Some(s) => join(s)?,
                None => url.to_string(),
            },
            rozsah: rozsah(i.attribute("range")),
        }),
        None => None,
    };
    let mut kusky = vec![];
    for s in deti(list, "SegmentURL") {
        kusky.push(Kusok {
            url: match s.attribute("media") {
                Some(m) => join(m)?,
                None => url.to_string(),
            },
            rozsah: rozsah(s.attribute("mediaRange")),
        });
    }
    Ok(Stopa { init, kusky })
}

fn druh(mime: Option<&str>, typ: Option<&str>) -> Druh {
    let t = typ
        .or_else(|| mime.and_then(|m| m.split('/').next()))
        .unwrap_or("");
    match t {
        "video" => Druh::Video,
        "audio" => Druh::Zvuk,
        "text" | "application" => Druh::Titulky,
        _ => Druh::Ine,
    }
}

/// Načíta manifest. `adresa` je adresa súboru .mpd (od nej sa počítajú relatívne odkazy).
pub fn parse(text: &str, adresa: &Url) -> Vysledok<Mpd> {
    let doc = Document::parse(text).map_err(|e| Chyba::Format(format!("MPD: {e}")))?;
    let mpd = doc.root_element();
    if mpd.tag_name().name() != "MPD" {
        return Err(Chyba::Format("súbor nie je manifest DASH".into()));
    }
    if mpd.attribute("type") == Some("dynamic") {
        return Err(Chyba::Zive);
    }
    if let Some(s) = je_sifrovane(mpd) {
        return Err(Chyba::Drm(s));
    }
    let url_mpd = base(mpd, adresa)?;
    let mut trvanie = mpd
        .attribute("mediaPresentationDuration")
        .and_then(trvanie_iso)
        .unwrap_or(0.0);
    let period = dieta(mpd, "Period").ok_or_else(|| Chyba::Format("MPD bez Period".into()))?;
    if let Some(d) = period.attribute("duration").and_then(trvanie_iso) {
        if trvanie <= 0.0 || deti(mpd, "Period").count() == 1 {
            trvanie = d;
        }
    }
    let url_per = base(period, &url_mpd)?;
    let mut out = Mpd {
        trvanie,
        reprezentacie: vec![],
    };
    for aset in deti(period, "AdaptationSet") {
        let url_as = base(aset, &url_per)?;
        let mut sab_as = Sablona::default();
        if let Some(st) = dieta(period, "SegmentTemplate") {
            sab_as.prekry(st);
        }
        if let Some(st) = dieta(aset, "SegmentTemplate") {
            sab_as.prekry(st);
        }
        for rep in deti(aset, "Representation") {
            let url = base(rep, &url_as)?;
            let id = rep.attribute("id").unwrap_or("").to_string();
            let bw: u64 = cislo(rep, "bandwidth").unwrap_or(0);
            let mime = rep.attribute("mimeType").or(aset.attribute("mimeType"));
            let mut sab = sab_as.clone();
            let vlastna = dieta(rep, "SegmentTemplate");
            if let Some(st) = vlastna {
                sab.prekry(st);
            }
            let stopa = if sab.media.is_some() {
                kusky_zo_sablony(&sab, &url, &id, bw, trvanie)?
            } else if let Some(list) =
                dieta(rep, "SegmentList").or_else(|| dieta(aset, "SegmentList"))
            {
                kusky_zo_zoznamu(list, &url)?
            } else {
                // SegmentBase alebo len BaseURL: celý súbor naraz
                Stopa {
                    init: None,
                    kusky: vec![Kusok::new(url.to_string())],
                }
            };
            out.reprezentacie.push(Reprezentacia {
                id,
                druh: druh(mime, aset.attribute("contentType")),
                jazyk: aset
                    .attribute("lang")
                    .or(rep.attribute("lang"))
                    .map(str::to_string),
                bandwidth: bw,
                sirka: cislo(rep, "width").or(cislo(aset, "width")),
                vyska: cislo(rep, "height").or(cislo(aset, "height")),
                kodeky: rep
                    .attribute("codecs")
                    .or(aset.attribute("codecs"))
                    .map(str::to_string),
                mime: mime.map(str::to_string),
                stopa,
            });
        }
    }
    Ok(out)
}

/// Najlepšie video do zadanej výšky (bez obmedzenia najlepšie vôbec); pri `presne` práve tá
/// kvalita, inak najbližšia – pozri [`crate::kvalita`].
pub fn vyber_video(m: &Mpd, max_vyska: Option<u32>, presne: bool) -> Option<&Reprezentacia> {
    let videa = m.reprezentacie.iter().filter(|r| r.druh == Druh::Video);
    let trieda = |r: &Reprezentacia| crate::kvalita::trieda(r.sirka, r.vyska);
    let vyhovuju: Vec<_> = match (max_vyska, presne) {
        (Some(ciel), true) => {
            let t = crate::kvalita::presna_trieda(videa.clone().filter_map(trieda), ciel);
            videa
                .clone()
                .filter(|r| t.is_none() || trieda(r) == t)
                .collect()
        }
        _ => videa
            .clone()
            .filter(|r| crate::kvalita::do_stropu(trieda(r).or(Some(0)), max_vyska))
            .collect(),
    };
    let z = if vyhovuju.is_empty() {
        videa.collect()
    } else {
        vyhovuju
    };
    z.into_iter()
        .max_by_key(|r| (r.vyska.unwrap_or(0), r.bandwidth))
}

/// Zvukové stopy v poradí obľúbených jazykov (každý jazyk raz, najlepšia kvalita);
/// keď sa žiadny nezhoduje, najlepšia stopa vôbec.
pub fn vyber_zvuk<'a>(m: &'a Mpd, jazyky: &[String]) -> Vec<&'a Reprezentacia> {
    let zvuky: Vec<_> = m
        .reprezentacie
        .iter()
        .filter(|r| r.druh == Druh::Zvuk)
        .collect();
    let najlepsia = |pred: &dyn Fn(&Reprezentacia) -> bool| {
        zvuky
            .iter()
            .copied()
            .filter(|r| pred(r))
            .max_by_key(|r| r.bandwidth)
    };
    let mut out: Vec<&Reprezentacia> = vec![];
    for j in jazyky {
        let j = j.to_lowercase();
        let zhoda = najlepsia(&|r| {
            r.jazyk.as_deref().map(str::to_lowercase).as_deref() == Some(j.as_str())
        })
        .or_else(|| {
            let zaklad = j.split('-').next().unwrap_or("");
            najlepsia(&|r| {
                r.jazyk
                    .as_deref()
                    .map(|x| x.to_lowercase().split('-').next().unwrap_or("").to_string())
                    .as_deref()
                    == Some(zaklad)
            })
        });
        if let Some(r) = zhoda {
            if !out.iter().any(|x| std::ptr::eq(*x, r)) {
                out.push(r);
            }
        }
    }
    if out.is_empty() {
        if let Some(r) = najlepsia(&|_| true) {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod testy {
    use super::*;

    fn u() -> Url {
        Url::parse("https://cdn.example.com/film/manifest.mpd").unwrap()
    }

    #[test]
    fn trvanie() {
        assert_eq!(trvanie_iso("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(trvanie_iso("PT0S"), Some(0.0));
        assert_eq!(trvanie_iso("P1DT1S"), Some(86401.0));
        assert_eq!(trvanie_iso("1H"), None);
    }

    #[test]
    fn premenne_sablony() {
        assert_eq!(
            sablona("v/$RepresentationID$/$Number%05d$.m4s", "720p", 42, 0, 0),
            "v/720p/00042.m4s"
        );
        assert_eq!(
            sablona("$Bandwidth$-$Time$.m4s", "a", 0, 9000, 128000),
            "128000-9000.m4s"
        );
        assert_eq!(sablona("cena$$1.m4s", "a", 0, 0, 0), "cena$1.m4s");
        assert_eq!(sablona("$Nezname$x", "a", 0, 0, 0), "$Nezname$x");
    }

    #[test]
    fn sablona_s_cislom() {
        let m = parse(
            r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT10S">
              <Period><AdaptationSet contentType="video" mimeType="video/mp4">
                <SegmentTemplate media="$RepresentationID$/seg-$Number$.m4s" initialization="$RepresentationID$/init.mp4" duration="4" timescale="1" startNumber="1"/>
                <Representation id="v1" bandwidth="1000000" width="1280" height="720"/>
              </AdaptationSet></Period></MPD>"#,
            &u(),
        )
        .unwrap();
        let s = &m.reprezentacie[0].stopa;
        assert_eq!(
            s.init.as_ref().unwrap().url,
            "https://cdn.example.com/film/v1/init.mp4"
        );
        assert_eq!(s.kusky.len(), 3); // 10 s / 4 s = 3 kúsky
        assert_eq!(s.kusky[2].url, "https://cdn.example.com/film/v1/seg-3.m4s");
    }

    #[test]
    fn casova_os_s_opakovanim() {
        let m = parse(
            r#"<MPD type="static" mediaPresentationDuration="PT20S"><Period>
              <AdaptationSet contentType="audio" lang="sk">
                <SegmentTemplate timescale="1000" media="a/$Time$.m4s" initialization="a/init.mp4">
                  <SegmentTimeline><S t="0" d="4000" r="2"/><S d="2000"/><S d="3000" r="-1"/></SegmentTimeline>
                </SegmentTemplate>
                <Representation id="a" bandwidth="128000"/>
              </AdaptationSet></Period></MPD>"#,
            &u(),
        )
        .unwrap();
        let k: Vec<_> = m.reprezentacie[0]
            .stopa
            .kusky
            .iter()
            .map(|k| k.url.rsplit('/').next().unwrap().to_string())
            .collect();
        // 0,4000,8000 (r=2), 12000, potom po 3000 až do 20000: 14000, 17000
        assert_eq!(
            k,
            [
                "0.m4s",
                "4000.m4s",
                "8000.m4s",
                "12000.m4s",
                "14000.m4s",
                "17000.m4s"
            ]
        );
        assert_eq!(m.reprezentacie[0].druh, Druh::Zvuk);
        assert_eq!(m.reprezentacie[0].jazyk.as_deref(), Some("sk"));
    }

    #[test]
    fn zoznam_a_baseurl_retaz() {
        let m = parse(
            r#"<MPD type="static" mediaPresentationDuration="PT8S"><BaseURL>https://media.example.org/</BaseURL>
              <Period><BaseURL>p1/</BaseURL><AdaptationSet mimeType="video/mp4"><BaseURL>video/</BaseURL>
                <Representation id="v" bandwidth="5" height="1080"><BaseURL>hd.mp4</BaseURL>
                  <SegmentList><Initialization range="0-999"/>
                    <SegmentURL mediaRange="1000-4999"/><SegmentURL mediaRange="5000-8999"/></SegmentList>
                </Representation></AdaptationSet>
              <AdaptationSet mimeType="audio/mp4" lang="cs"><Representation id="a" bandwidth="64000"><BaseURL>zvuk.m4a</BaseURL><SegmentBase indexRange="0-100"/></Representation></AdaptationSet>
              </Period></MPD>"#,
            &u(),
        )
        .unwrap();
        let v = &m.reprezentacie[0].stopa;
        assert_eq!(
            v.init.as_ref().unwrap().url,
            "https://media.example.org/p1/video/hd.mp4"
        );
        assert_eq!(v.init.as_ref().unwrap().rozsah, Some((0, 1000)));
        assert_eq!(v.kusky[1].rozsah, Some((5000, 4000)));
        let a = &m.reprezentacie[1].stopa;
        assert_eq!(
            a.kusky,
            vec![Kusok::new("https://media.example.org/p1/zvuk.m4a")]
        );
    }

    #[test]
    fn sifrovanie_a_zive() {
        let drm = r#"<MPD type="static"><Period><AdaptationSet><ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/>
            <Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(matches!(parse(drm, &u()), Err(Chyba::Drm(_))));
        let zive = r#"<MPD type="dynamic"><Period/></MPD>"#;
        assert!(matches!(parse(zive, &u()), Err(Chyba::Zive)));
        assert!(matches!(parse("<html/>", &u()), Err(Chyba::Format(_))));
    }

    #[test]
    fn vyber_kvality_a_jazykov() {
        let m = parse(
            r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
              <AdaptationSet contentType="video"><SegmentTemplate media="$RepresentationID$-$Number$.m4s" duration="4"/>
                <Representation id="360" bandwidth="1" height="360"/><Representation id="1080" bandwidth="9" height="1080"/>
                <Representation id="720" bandwidth="5" height="720"/></AdaptationSet>
              <AdaptationSet contentType="audio" lang="en"><SegmentTemplate media="en-$Number$.m4s" duration="4"/><Representation id="en" bandwidth="128"/></AdaptationSet>
              <AdaptationSet contentType="audio" lang="sk"><SegmentTemplate media="sk-$Number$.m4s" duration="4"/><Representation id="sk" bandwidth="96"/></AdaptationSet>
              <AdaptationSet contentType="audio" lang="cs-CZ"><SegmentTemplate media="cs-$Number$.m4s" duration="4"/><Representation id="cs" bandwidth="96"/></AdaptationSet>
            </Period></MPD>"#,
            &u(),
        )
        .unwrap();
        assert_eq!(vyber_video(&m, None, false).unwrap().id, "1080");
        assert_eq!(vyber_video(&m, Some(720), false).unwrap().id, "720");
        assert_eq!(vyber_video(&m, Some(100), false).unwrap().id, "1080"); // nič nevyhovuje → najlepšie
        assert_eq!(vyber_video(&m, Some(1080), true).unwrap().id, "1080");
        assert_eq!(vyber_video(&m, Some(600), true).unwrap().id, "720"); // najbližšia
        let j: Vec<_> = vyber_zvuk(&m, &["cs".into(), "sk".into(), "de".into()])
            .iter()
            .map(|r| r.id.clone())
            .collect();
        assert_eq!(j, ["cs", "sk"]);
        assert_eq!(vyber_zvuk(&m, &["de".into()])[0].id, "en");
    }
}
