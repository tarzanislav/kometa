//! Skener stránok – nájde videá, streamy, prehrávače a súbory na akejkoľvek stránke.
//!
//! 1. stiahne stránku (bez prehliadača), 2. prejde značky, atribúty `data-*`, OpenGraph a JSON-LD,
//! 3. v celom texte vrátane JavaScriptu a JSON hľadá adresy (aj zakódované `\/`, `/`, `&amp;`),
//! 4. rozpozná vložené prehrávače a vnorené rámčeky prehľadá do hĺbky 2,
//! 5. každému nálezu dá skóre a Referer – adresu stránky, kde je vložený (Vimeo s obmedzením
//!    na doménu sa bez neho nedá prehrať ani stiahnuť).

use super::html::{self, Znacka};
use super::vzory::{self, Druh};
use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Hlasic};
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, REFERER};
use reqwest::Client;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use url::Url;

const MAX_STRANKA: usize = 4 << 20;
/// Koľko bajtov stačí na rozpoznanie, či ide o stránku, stream alebo súbor.
const OCHUTNAJ: usize = 4096;

#[derive(Debug, PartialEq)]
enum Obsah {
    Text,
    Stream,
    Subor,
}

/// Podľa prvých bajtov (a typu zo servera): stránka/JSON, playlist HLS/DASH alebo iný súbor.
fn ochutnaj(zaciatok: &[u8], typ: &str) -> Obsah {
    let b = zaciatok.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(zaciatok);
    let t = b.trim_ascii_start();
    if t.starts_with(b"#EXTM3U") || typ.contains("mpegurl") || typ.contains("dash+xml") {
        return Obsah::Stream;
    }
    if t.starts_with(b"<MPD") || (t.starts_with(b"<?xml") && obsahuje(t, b"<MPD")) {
        return Obsah::Stream;
    }
    const PODPISY: &[&[u8]] = &[
        b"\x1A\x45\xDF\xA3", // Matroska / WebM
        b"ID3",
        b"fLaC",
        b"OggS",
        b"RIFF",
        b"PK\x03\x04",
        b"%PDF",
        b"Rar!",
        b"7z\xBC\xAF",
        b"\x1F\x8B",
        b"FLV",
    ];
    let mp4 = b.len() >= 8
        && matches!(
            &b[4..8],
            b"ftyp" | b"moov" | b"styp" | b"moof" | b"free" | b"mdat"
        );
    let ts = b.len() > 188 && b[0] == 0x47 && b[188] == 0x47;
    let adts = b.len() > 1 && b[0] == 0xFF && b[1] & 0xF0 == 0xF0;
    if mp4
        || ts
        || adts
        || PODPISY.iter().any(|p| b.starts_with(p))
        || b[..b.len().min(1024)].contains(&0)
    {
        return Obsah::Subor;
    }
    let textovy = ["html", "xml", "text/", "json", "javascript"]
        .iter()
        .any(|x| typ.contains(x));
    if !textovy && !typ.is_empty() {
        return Obsah::Subor;
    }
    Obsah::Text
}

fn obsahuje(kde: &[u8], co: &[u8]) -> bool {
    kde.windows(co.len()).any(|w| w == co)
}

/// Výsledok prieskumu adresy: priamo médium alebo nálezy zo stránky.
#[derive(Debug)]
pub enum Prieskum {
    /// adresa (po presmerovaní) je sama médium; `stream` = HLS/DASH
    Priamo {
        adresa: String,
        stream: bool,
    },
    Nalezy(Vec<Nalez>),
}

#[derive(Clone, Debug, Serialize)]
pub struct Nalez {
    pub url: String,
    pub druh: Druh,
    pub druh_text: &'static str,
    pub poskytovatel: String,
    /// stránka, na ktorej je nález vložený (posiela sa ako Referer)
    pub referer: String,
    pub nazov: String,
    pub skore: i32,
    pub poznamka: String,
    pub odporucane: bool,
    #[serde(skip)]
    kluc: String,
}

/// Čo vrátil server: stránka alebo priamo médium.
enum Odpoved {
    Stranka {
        text: String,
        adresa: String,
    },
    /// priamo médium; `stream` = HLS/DASH podľa typu alebo obsahu (aj bez prípony v adrese)
    Medium {
        adresa: String,
        stream: bool,
    },
}

const ATRIBUTY_ADRIES: [&str; 19] = [
    "src",
    "href",
    "data-src",
    "data-url",
    "data-video",
    "data-video-url",
    "data-video-src",
    "data-file",
    "data-stream",
    "data-hls",
    "data-dash",
    "data-mp4",
    "data-embed",
    "data-iframe",
    "data-lazy-src",
    "data-vimeo-url",
    "data-player",
    "data-media",
    "data-href",
];

pub struct Skener<'a> {
    k: &'a Client,
    hlasic: &'a Hlasic,
    pub max_ramcov: usize,
    pub max_hlbka: usize,
}

#[derive(Default)]
struct Zber {
    nalezy: HashMap<String, Nalez>,
}

impl Zber {
    fn vloz(&mut self, mut n: Nalez) {
        if vzory::je_reklama(&n.url) {
            n.skore -= 70;
            n.poznamka = [n.poznamka.as_str(), "pravdepodobne reklama"]
                .iter()
                .filter(|x| !x.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
        }
        match self.nalezy.get_mut(&n.kluc) {
            Some(stary) if n.skore > stary.skore => {
                if n.nazov.is_empty() {
                    n.nazov = std::mem::take(&mut stary.nazov);
                }
                *stary = n;
            }
            Some(stary) => {
                if stary.nazov.is_empty() {
                    stary.nazov = n.nazov;
                }
            }
            None => {
                self.nalezy.insert(n.kluc.clone(), n);
            }
        }
    }

    fn prehravac(&mut self, p: vzory::Prehravac, referer: &str, nazov: &str, skore: i32) {
        let mut poznamka = String::new();
        if p.poskytovatel.starts_with("Vimeo") {
            if p.url.contains("?h=") {
                poznamka.push_str("neverejné video (kľúč h=…), ");
            }
            poznamka.push_str("pošle sa Referer stránky (pre videá obmedzené na doménu)");
        }
        self.vloz(Nalez {
            url: p.url,
            druh: Druh::Prehravac,
            druh_text: Druh::Prehravac.text(),
            poskytovatel: p.poskytovatel.to_string(),
            referer: referer.to_string(),
            nazov: nazov.to_string(),
            skore,
            poznamka,
            odporucane: false,
            kluc: p.kluc,
        });
    }

    /// Adresa, ktorá je priamo stream (podľa typu alebo obsahu odpovede).
    fn stream_priamo(&mut self, adresa: &str, referer: &str) {
        let poskytovatel = if adresa.to_ascii_lowercase().contains(".mpd") {
            "DASH"
        } else {
            "HLS"
        };
        self.vloz(Nalez {
            url: adresa.to_string(),
            druh: Druh::Stream,
            druh_text: Druh::Stream.text(),
            poskytovatel: poskytovatel.into(),
            referer: referer.to_string(),
            nazov: String::new(),
            skore: 100,
            poznamka: String::new(),
            odporucane: false,
            kluc: adresa.split('#').next().unwrap_or(adresa).to_string(),
        });
    }

    /// Mediálny súbor; `vynutit` = aj bez známej prípony (napr. `<video src>`).
    fn medium(
        &mut self,
        adresa: &str,
        referer: &str,
        nazov: &str,
        zaklad: i32,
        vynutit: bool,
    ) -> bool {
        if !adresa.starts_with("http") {
            return false;
        }
        let (druh, poskytovatel) = match vzory::medium(adresa) {
            Some(x) => x,
            None if vynutit => (Druh::Video, "Video"),
            None => return false,
        };
        let mut skore = zaklad
            + match druh {
                Druh::Stream => 4,
                Druh::Video => 0,
                Druh::Zvuk => -25,
                Druh::Titulky => -40,
                Druh::Subor => -30,
                Druh::Prehravac => 0,
            };
        if druh == Druh::Stream {
            let a = adresa.to_ascii_lowercase();
            if ["master", "playlist", "manifest", "index.m3u8"]
                .iter()
                .any(|s| a.contains(s))
            {
                skore += 3; // hlavný zoznam (všetky kvality) je lepší ako jedna kvalita
            }
            if ["/chunklist", "/media_", "/segment"]
                .iter()
                .any(|s| a.contains(s))
                || jedna_kvalita(&a)
            {
                skore -= 6;
            }
        }
        let bez_fragmentu = adresa.split('#').next().unwrap_or(adresa);
        let kluc = if druh == Druh::Stream {
            bez_fragmentu.to_string()
        } else {
            bez_fragmentu
                .split('?')
                .next()
                .unwrap_or(bez_fragmentu)
                .to_string()
        };
        self.vloz(Nalez {
            url: adresa.to_string(),
            druh,
            druh_text: druh.text(),
            poskytovatel: poskytovatel.to_string(),
            referer: referer.to_string(),
            nazov: nazov.to_string(),
            skore,
            poznamka: String::new(),
            odporucane: false,
            kluc,
        });
        true
    }
}

/// Cesta s časťou ako „/720p/“ alebo „/1080/“ – jedna kvalita, nie hlavný zoznam.
fn jedna_kvalita(a: &str) -> bool {
    a.split('/').any(|s| {
        let t = s.trim_end_matches('p');
        (3..=4).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()) && s.len() <= 5
    })
}

fn pripoj(zaklad: &Url, surova: &str) -> Option<String> {
    let s = surova.trim();
    if s.is_empty()
        || [
            "data:",
            "blob:",
            "javascript:",
            "#",
            "mailto:",
            "tel:",
            "about:",
        ]
        .iter()
        .any(|p| s.starts_with(p))
    {
        return None;
    }
    zaklad.join(s).ok().map(|u| u.to_string())
}

fn prechadzaj_json(
    v: &serde_json::Value,
    f: &mut impl FnMut(&serde_json::Map<String, serde_json::Value>),
) {
    match v {
        serde_json::Value::Object(o) => {
            f(o);
            for x in o.values() {
                prechadzaj_json(x, f);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                prechadzaj_json(x, f);
            }
        }
        _ => {}
    }
}

impl<'a> Skener<'a> {
    pub fn new(k: &'a Client, hlasic: &'a Hlasic) -> Self {
        Skener {
            k,
            hlasic,
            max_ramcov: 6,
            max_hlbka: 2,
        }
    }

    async fn stiahni(&self, adresa: &str, referer: Option<&str>) -> Vysledok<Odpoved> {
        let mut z = self
            .k
            .get(adresa)
            .header(
                ACCEPT,
                "text/html,application/xhtml+xml,application/json;q=0.9,*/*;q=0.8",
            )
            .header(ACCEPT_LANGUAGE, "sk,cs;q=0.9,en;q=0.8");
        if let Some(r) = referer {
            z = z.header(REFERER, r);
        }
        let mut r = z.send().await?;
        if !r.status().is_success() {
            return Err(Chyba::Http(r.status().as_u16()));
        }
        let konecna = r.url().to_string();
        let typ = r
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        // začiatok obsahu rozhodne lepšie ako Content-Type (servery ho často nastavia zle,
        // napr. „.ts“ ako text/vnd.trolltech.linguist alebo playlist ako octet-stream)
        let mut telo = Vec::new();
        while telo.len() < OCHUTNAJ {
            match r.chunk().await? {
                Some(c) => telo.extend_from_slice(&c),
                None => break,
            }
        }
        match ochutnaj(&telo, &typ) {
            Obsah::Stream => {
                return Ok(Odpoved::Medium {
                    adresa: konecna,
                    stream: true,
                })
            }
            Obsah::Subor => {
                return Ok(Odpoved::Medium {
                    adresa: konecna,
                    stream: false,
                })
            }
            Obsah::Text => {}
        }
        while let Some(c) = r.chunk().await? {
            telo.extend_from_slice(&c);
            if telo.len() > MAX_STRANKA {
                break;
            }
        }
        Ok(Odpoved::Stranka {
            text: String::from_utf8_lossy(&telo).into_owned(),
            adresa: konecna,
        })
    }

    /// Prehľadá stránku aj vnorené rámčeky. Nálezy sú zoradené od najlepšieho; odporúčané sú označené.
    /// Ak je adresa sama médium, vráti ho ako jediný (odporúčaný) nález.
    pub async fn prehladaj(&self, adresa: &str, referer: Option<&str>) -> Vysledok<Vec<Nalez>> {
        Ok(match self.preskumaj(adresa, referer).await? {
            Prieskum::Nalezy(n) => n,
            Prieskum::Priamo { adresa, stream } => {
                let mut zber = Zber::default();
                let r = referer.unwrap_or("");
                if stream {
                    zber.stream_priamo(&adresa, r);
                } else {
                    zber.medium(&adresa, r, "", 100, true);
                }
                let mut n: Vec<Nalez> = zber.nalezy.into_values().collect();
                n.iter_mut().for_each(|x| x.odporucane = true);
                n
            }
        })
    }

    /// Ako [`Skener::prehladaj`], ale priame médium vráti zvlášť (bez hodnotenia).
    pub async fn preskumaj(&self, adresa: &str, referer: Option<&str>) -> Vysledok<Prieskum> {
        let mut zber = Zber::default();
        let mut fronta: VecDeque<(String, Option<String>, usize)> = VecDeque::new();
        fronta.push_back((adresa.to_string(), referer.map(str::to_string), 0));
        let mut navstivene = HashSet::new();
        let mut ramcov = self.max_ramcov;
        let mut titul_stranky = String::new();
        while let Some((url, ref_, hlbka)) = fronta.pop_front() {
            if !navstivene.insert(url.clone()) {
                continue;
            }
            let odpoved = match self.stiahni(&url, ref_.as_deref()).await {
                Ok(o) => o,
                Err(e) if hlbka == 0 => return Err(e),
                Err(e) => {
                    udalosti::varovanie(
                        self.hlasic,
                        format!("Rámček {url} sa nepodarilo načítať: {e}"),
                    );
                    continue;
                }
            };
            match odpoved {
                Odpoved::Medium { adresa, stream } if hlbka == 0 => {
                    return Ok(Prieskum::Priamo { adresa, stream });
                }
                Odpoved::Medium { adresa, stream } => {
                    let r = ref_.clone().unwrap_or_else(|| adresa.clone());
                    if stream {
                        zber.stream_priamo(&adresa, &r);
                    } else {
                        zber.medium(&adresa, &r, "", 70, true);
                    }
                }
                Odpoved::Stranka { text, adresa } => {
                    udalosti::info(
                        self.hlasic,
                        if hlbka == 0 {
                            format!("Prehľadávam {adresa}")
                        } else {
                            format!("Prehľadávam rámček {adresa} (úroveň {hlbka})")
                        },
                    );
                    let (titul, ramce) = self.stranka(&text, &adresa, hlbka, &mut zber);
                    if hlbka == 0 {
                        titul_stranky = titul;
                    }
                    if hlbka < self.max_hlbka {
                        for r in ramce {
                            if ramcov == 0 {
                                break;
                            }
                            if vzory::je_reklama(&r)
                                || vzory::prehravac(&r).is_some()
                                || navstivene.contains(&r)
                            {
                                continue;
                            }
                            ramcov -= 1;
                            fronta.push_back((r, Some(adresa.clone()), hlbka + 1));
                        }
                    }
                }
            }
        }
        let mut out: Vec<Nalez> = zber.nalezy.into_values().filter(|n| n.skore > 0).collect();
        out.sort_by(|a, b| b.skore.cmp(&a.skore).then_with(|| a.url.cmp(&b.url)));
        for n in &mut out {
            if n.nazov.is_empty() {
                n.nazov = titul_stranky.clone();
            }
        }
        for i in najlepsie(&out, 10) {
            out[i].odporucane = true;
        }
        Ok(Prieskum::Nalezy(out))
    }

    /// Jedna stránka: vráti jej titul a adresy rámčekov na ďalšie prehľadanie.
    fn stranka(
        &self,
        text: &str,
        adresa: &str,
        hlbka: usize,
        zber: &mut Zber,
    ) -> (String, Vec<String>) {
        let zaklad = match Url::parse(adresa) {
            Ok(u) => u,
            Err(_) => return (String::new(), vec![]),
        };
        let d = html::citaj(text);
        let bonus = if hlbka == 0 { 5 } else { 0 };
        let mut meta: HashMap<String, String> = HashMap::new();
        let mut ramce: Vec<String> = vec![];
        let mut odkazy: HashSet<String> = HashSet::new();
        for z in d.znacky.iter().filter(|z| z.nazov == "meta") {
            let k = z
                .atr("property")
                .or(z.atr("name"))
                .or(z.atr("itemprop"))
                .unwrap_or("")
                .to_ascii_lowercase();
            if let (false, Some(c)) = (k.is_empty(), z.atr("content")) {
                meta.entry(k).or_insert_with(|| c.to_string());
            }
        }
        let titul: String = meta
            .get("og:title")
            .cloned()
            .unwrap_or_else(|| d.titul.clone())
            .trim()
            .chars()
            .take(200)
            .collect();
        // 1) značky a atribúty
        for z in d.znacky.iter().filter(|z| z.nazov != "meta") {
            for (tag, surova) in kandidati(z) {
                let Some(abs) = pripoj(&zaklad, &surova) else {
                    continue;
                };
                if let Some(p) = vzory::prehravac(&abs) {
                    // vložený prehrávač je hlavný obsah; obyčajný odkaz <a> na iné video len možnosť
                    if tag == "a" {
                        odkazy.insert(p.kluc.clone());
                    }
                    zber.prehravac(p, adresa, &titul, if tag == "a" { 40 } else { 90 } + bonus);
                    continue;
                }
                if matches!(tag.as_str(), "video" | "source" | "audio")
                    && zber.medium(&abs, adresa, &titul, 85 + bonus, tag != "audio")
                {
                    continue;
                }
                if matches!(tag.as_str(), "iframe" | "frame" | "embed" | "object")
                    && abs.starts_with("http")
                {
                    if !zber.medium(&abs, adresa, &titul, 80 + bonus, false) {
                        ramce.push(abs);
                    }
                    continue;
                }
                zber.medium(&abs, adresa, &titul, 45 + bonus, false);
            }
        }
        // 2) OpenGraph, Twitter, itemprop
        for k in [
            "og:video:secure_url",
            "og:video:url",
            "og:video",
            "twitter:player:stream",
            "contenturl",
            "embedurl",
            "twitter:player",
        ] {
            let Some(v) = meta.get(k).and_then(|v| pripoj(&zaklad, v)) else {
                continue;
            };
            if let Some(p) = vzory::prehravac(&v) {
                zber.prehravac(p, adresa, &titul, 82 + bonus);
            } else if !zber.medium(&v, adresa, &titul, 80 + bonus, k != "twitter:player")
                && k == "twitter:player"
            {
                ramce.push(v);
            }
        }
        // 3) JSON-LD (VideoObject a pod.)
        for (typ, obsah) in &d.skripty {
            if !typ.contains("ld+json") {
                continue;
            }
            let Ok(j) = serde_json::from_str::<serde_json::Value>(obsah.trim()) else {
                continue;
            };
            prechadzaj_json(&j, &mut |o| {
                let typy: Vec<String> = match o.get("@type") {
                    Some(serde_json::Value::String(s)) => vec![s.to_ascii_lowercase()],
                    Some(serde_json::Value::Array(a)) => a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .map(str::to_ascii_lowercase)
                        .collect(),
                    _ => vec![],
                };
                if !typy.iter().any(|t| {
                    matches!(
                        t.as_str(),
                        "videoobject" | "audioobject" | "clip" | "movie" | "episode"
                    )
                }) {
                    return;
                }
                let meno: String = o
                    .get("name")
                    .and_then(|x| x.as_str())
                    .map(html::entity)
                    .unwrap_or_else(|| titul.clone())
                    .chars()
                    .take(200)
                    .collect();
                for k in ["contentUrl", "embedUrl", "url"] {
                    let Some(v) = o
                        .get(k)
                        .and_then(|x| x.as_str())
                        .and_then(|v| pripoj(&zaklad, v))
                    else {
                        continue;
                    };
                    if let Some(p) = vzory::prehravac(&v) {
                        zber.prehravac(p, adresa, &meno, 88 + bonus);
                    } else {
                        zber.medium(&v, adresa, &meno, 84 + bonus, k == "contentUrl");
                    }
                }
            });
        }
        // 4) celý text vrátane JavaScriptu a JSON
        let odkryty = html::odkry(text);
        for a in vzory::adresy_v_texte(&odkryty) {
            if let Some(p) = vzory::prehravac(&a) {
                if !odkazy.contains(&p.kluc) {
                    zber.prehravac(p, adresa, &titul, 66 + bonus);
                }
            } else {
                zber.medium(&a, adresa, &titul, 60 + bonus, false);
            }
        }
        // Wistia vložená cez triedu „wistia_async_ID“
        let mut od = 0;
        while let Some(i) = odkryty[od..].find("wistia_async_") {
            let zac = od + i + "wistia_async_".len();
            let id: String = odkryty[zac..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if id.len() >= 8 {
                if let Some(p) =
                    vzory::prehravac(&format!("https://fast.wistia.net/embed/iframe/{id}"))
                {
                    zber.prehravac(p, adresa, &titul, 88 + bonus);
                }
            }
            od = zac;
        }
        ramce.dedup();
        (titul, ramce)
    }
}

/// (značka, surová adresa) z jednej značky – bežné atribúty, `data-*`, srcset, Vimeo/YouTube ID.
fn kandidati(z: &Znacka) -> Vec<(String, String)> {
    let mut out = vec![];
    let t = z.nazov.clone();
    if t == "param" {
        if let (Some(n), Some(v)) = (z.atr("name"), z.atr("value")) {
            if matches!(
                n.to_ascii_lowercase().as_str(),
                "movie" | "src" | "flashvars"
            ) {
                out.push((t.clone(), v.to_string()));
            }
        }
    }
    for k in ATRIBUTY_ADRIES {
        if let Some(v) = z.atr(k) {
            if !v.is_empty() {
                out.push((t.clone(), v.to_string()));
            }
        }
    }
    if t == "object" {
        if let Some(v) = z.atr("data") {
            out.push((t.clone(), v.to_string()));
        }
    }
    if let Some(s) = z.atr("srcset").filter(|_| t == "source") {
        for cast in s.split(',') {
            if let Some(u) = cast.split_whitespace().next() {
                out.push((t.clone(), u.to_string()));
            }
        }
    }
    for k in ["data-vimeo-id", "data-vimeo-video-id", "data-vimeo"] {
        if let Some(id) = z
            .atr(k)
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        {
            let h = z
                .atr("data-vimeo-hash")
                .map(|h| format!("?h={h}"))
                .unwrap_or_default();
            out.push((
                "prehravac".into(),
                format!("https://player.vimeo.com/video/{id}{h}"),
            ));
        }
    }
    for k in ["data-youtube-id", "data-yt-id"] {
        if let Some(id) = z.atr(k).filter(|v| v.len() == 11) {
            out.push((
                "prehravac".into(),
                format!("https://www.youtube.com/watch?v={id}"),
            ));
        }
    }
    out
}

/// Výber na automatické stiahnutie: hlavné videá a prehrávače, bez reklám a duplicít.
/// Vráti indexy do `nalezy` (zoradených od najlepšieho).
pub fn najlepsie(nalezy: &[Nalez], limit: usize) -> Vec<usize> {
    let mut vyber: Vec<usize> = nalezy
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            matches!(n.druh, Druh::Prehravac | Druh::Stream | Druh::Video) && n.skore >= 50
        })
        .map(|(i, _)| i)
        .collect();
    if vyber.is_empty() {
        vyber = nalezy
            .iter()
            .enumerate()
            .filter(|(_, n)| n.druh == Druh::Zvuk && n.skore >= 30)
            .map(|(i, _)| i)
            .collect();
    }
    // ak je na stránke prehrávač, streamy z toho istého prehrávača bývajú duplicitné
    if vyber.iter().any(|&i| nalezy[i].druh == Druh::Prehravac) {
        let top = vyber.iter().map(|&i| nalezy[i].skore).max().unwrap_or(0);
        vyber.retain(|&i| nalezy[i].druh == Druh::Prehravac || nalezy[i].skore >= top - 3);
    }
    vyber.truncate(limit);
    vyber
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn ochutnanie_obsahu() {
        assert_eq!(
            ochutnaj(b"\n#EXTM3U\n#EXT-X-VERSION:3", "application/octet-stream"),
            Obsah::Stream
        );
        assert_eq!(
            ochutnaj(
                b"<?xml version=\"1.0\"?>\n<!-- x -->\n<MPD xmlns=\"urn:mpeg:dash\">",
                "text/xml"
            ),
            Obsah::Stream
        );
        let mut ts = vec![0u8; 400];
        ts[0] = 0x47;
        ts[188] = 0x47;
        assert_eq!(ochutnaj(&ts, "text/vnd.trolltech.linguist"), Obsah::Subor);
        assert_eq!(
            ochutnaj(b"\0\0\0\x20ftypisom\0\0\x02\0", "text/plain"),
            Obsah::Subor
        );
        assert_eq!(ochutnaj(b"%PDF-1.7\n", "application/pdf"), Obsah::Subor);
        assert_eq!(
            ochutnaj(b"\xEF\xBB\xBF<!doctype html><html>", "text/html"),
            Obsah::Text
        );
        assert_eq!(ochutnaj(b"{\"video\":1}", "application/json"), Obsah::Text);
        assert_eq!(ochutnaj(b"<html>bez typu</html>", ""), Obsah::Text);
    }
    use crate::udalosti::tichy;

    fn nalezy(html: &str, adresa: &str) -> Vec<Nalez> {
        let k = Client::new();
        let h = tichy();
        let s = Skener::new(&k, &h);
        let mut zber = Zber::default();
        s.stranka(html, adresa, 0, &mut zber);
        let mut out: Vec<Nalez> = zber.nalezy.into_values().filter(|n| n.skore > 0).collect();
        out.sort_by(|a, b| b.skore.cmp(&a.skore).then_with(|| a.url.cmp(&b.url)));
        for i in najlepsie(&out, 10) {
            out[i].odporucane = true;
        }
        out
    }

    #[test]
    fn stranka_s_prehravacom_a_reklamou() {
        let n = nalezy(
            r#"<html><head><title>Lekcia 1</title>
            <meta property="og:image" content="/obr.jpg">
            <script>var ad = "https://pubads.g.doubleclick.net/vast/preroll.mp4";</script></head>
            <body><iframe src="https://player.vimeo.com/video/76979871?h=8272103f6e&amp;title=0"></iframe>
            <a href="https://vimeo.com/111111">Iné video autora</a>
            <a href="/materialy/skripta.pdf">Skriptá</a></body></html>"#,
            "https://kurz.sk/lekcia-1",
        );
        let prvy = &n[0];
        assert_eq!(
            (prvy.poskytovatel.as_str(), prvy.url.as_str()),
            (
                "Vimeo",
                "https://player.vimeo.com/video/76979871?h=8272103f6e"
            )
        );
        assert_eq!(prvy.referer, "https://kurz.sk/lekcia-1");
        assert_eq!(prvy.nazov, "Lekcia 1");
        assert!(prvy.odporucane && prvy.poznamka.contains("kľúč h="));
        let odkaz = n.iter().find(|x| x.url.ends_with("111111")).unwrap();
        assert!(!odkaz.odporucane && odkaz.skore < prvy.skore);
        assert!(n.iter().any(|x| x.druh == Druh::Subor
            && x.url == "https://kurz.sk/materialy/skripta.pdf"
            && !x.odporucane));
        assert!(
            !n.iter().any(|x| x.url.contains("doubleclick")),
            "reklama nemá kladné skóre"
        );
    }

    #[test]
    fn video_json_ld_a_zakodovany_js() {
        let n = nalezy(
            r#"<video poster="/p.jpg"><source src="/video/master.m3u8" type="application/x-mpegURL">
            <source src="/video/720p/index.m3u8"></video>
            <script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"VideoObject","name":"Prednáška &amp; diskusia","contentUrl":"https://cdn.sk/p.mp4"}]}</script>
            <script>window.cfg={"file":"https:\/\/cdn2.sk\/hls\/playlist.m3u8?token=abc&exp=1"}</script>"#,
            "https://skola.sk/prednaska",
        );
        let m = n
            .iter()
            .find(|x| x.url == "https://skola.sk/video/master.m3u8")
            .unwrap();
        let k = n
            .iter()
            .find(|x| x.url == "https://skola.sk/video/720p/index.m3u8")
            .unwrap();
        assert!(m.skore > k.skore, "hlavný zoznam pred jednou kvalitou");
        let ld = n.iter().find(|x| x.url == "https://cdn.sk/p.mp4").unwrap();
        assert_eq!(ld.nazov, "Prednáška & diskusia");
        assert!(n
            .iter()
            .any(|x| x.url == "https://cdn2.sk/hls/playlist.m3u8?token=abc&exp=1"));
        assert!(n[0].odporucane);
        assert!(!n.iter().any(|x| x.url.ends_with("p.jpg")));
    }

    #[test]
    fn atributy_data_a_wistia() {
        let n = nalezy(
            r#"<div class="player" data-vimeo-id="123456" data-vimeo-hash="abcdef12"></div>
               <div class="wistia_embed wistia_async_abcd1234ef"></div>
               <button data-video-url="/media/ukazka.mp4">Prehrať</button>"#,
            "https://x.sk/",
        );
        assert!(n
            .iter()
            .any(|x| x.url == "https://player.vimeo.com/video/123456?h=abcdef12"));
        assert!(n.iter().any(|x| x.poskytovatel == "Wistia"));
        assert!(n.iter().any(|x| x.url == "https://x.sk/media/ukazka.mp4"));
    }
}
