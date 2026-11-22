//! Streamy HLS a DASH: rozpoznanie, výber kvality a jazykov, stiahnutie kúskov a spojenie.
//!
//! Postup je rozdelený na dve časti: [`naplanuj`] len zistí, čo sa stiahne (rýchle, bez
//! sťahovania obsahu – použije sa aj pre `kometa info`), a [`stiahni`] plán vykoná.

use crate::chyba::{Chyba, Vysledok};
use crate::klient::{self, s_priponou, Nastavenia};
use crate::kusky::{self, Stopa};
use crate::spajanie::{self, Vstup};
use crate::udalosti::{self, Hlasic};
use crate::{dash, hls};
use reqwest::Client;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Hls,
    Dash,
}

/// Jedna stopa, ktorá sa stiahne.
#[derive(Clone, Debug, Serialize)]
pub struct StopaPlanu {
    /// "video", "zvuk" alebo "video+zvuk"
    pub druh: String,
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
    pub popis: String,
    pub kuskov: usize,
    /// fragmentované MP4 (inak MPEG-TS)
    pub fmp4: bool,
    #[serde(skip)]
    pub stopa: Stopa,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub format: Format,
    pub trvanie: f64,
    pub stopy: Vec<StopaPlanu>,
    /// všetky dostupné kvality obrazu (napr. „1920×1080, 5,0 Mb/s“)
    pub kvality: Vec<String>,
    /// všetky dostupné jazyky zvuku
    pub jazyky: Vec<String>,
}

/// Podľa obsahu (nie podľa prípony, tá často klame).
pub fn rozpoznaj(text: &str) -> Option<Format> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("#EXTM3U") {
        Some(Format::Hls)
    } else if t.starts_with("<?xml") && t.contains("<MPD") || t.starts_with("<MPD") {
        Some(Format::Dash)
    } else {
        None
    }
}

fn popis_kvality(sirka: Option<u32>, vyska: Option<u32>, bw: u64) -> String {
    let mbps = bw as f64 / 1_000_000.0;
    let bw = if bw >= 1_000_000 {
        format!("{mbps:.1} Mb/s").replace('.', ",")
    } else if bw > 0 {
        format!("{} kb/s", bw / 1000)
    } else {
        String::new()
    };
    match (sirka, vyska) {
        (Some(s), Some(v)) if bw.is_empty() => format!("{s}×{v}"),
        (Some(s), Some(v)) => format!("{s}×{v}, {bw}"),
        (None, Some(v)) => format!("{v}p {bw}").trim().to_string(),
        _ => bw,
    }
}

async fn hls_media(k: &Client, url: &str) -> Vysledok<hls::Media> {
    let (text, konecna) = klient::text(k, url).await?;
    let base = Url::parse(&konecna).map_err(|e| Chyba::Format(e.to_string()))?;
    match hls::parse(&text, &base)? {
        hls::Playlist::Media(m) => {
            if let Some(s) = m.sifrovanie.clone() {
                return Err(Chyba::Drm(s));
            }
            if !m.koniec {
                return Err(Chyba::Zive);
            }
            Ok(m)
        }
        hls::Playlist::Master(_) => Err(Chyba::Format("vnorený master playlist".into())),
    }
}

/// Je na adrese stream HLS/DASH? Prečíta len začiatok odpovede (nie celý súbor).
pub async fn je_stream(k: &Client, url: &str) -> Vysledok<bool> {
    let cesta = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if cesta.ends_with(".m3u8") || cesta.ends_with(".mpd") {
        return Ok(true);
    }
    let mut r = k
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-4095")
        .send()
        .await?;
    if !r.status().is_success() {
        return Err(Chyba::Http(r.status().as_u16()));
    }
    let typ = r
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if typ.contains("mpegurl") || typ.contains("dash+xml") {
        return Ok(true);
    }
    let mut zaciatok = Vec::new();
    while zaciatok.len() < 4096 {
        match r.chunk().await? {
            Some(c) => zaciatok.extend_from_slice(&c),
            None => break,
        }
    }
    Ok(rozpoznaj(&String::from_utf8_lossy(&zaciatok)).is_some())
}

/// Zistí, čo sa zo streamu stiahne: kvalita obrazu podľa `n.max_vyska`, zvuk podľa `n.jazyky`.
pub async fn naplanuj(k: &Client, url: &str, n: &Nastavenia) -> Vysledok<Plan> {
    let (text, konecna) = klient::text(k, url).await?;
    let base = Url::parse(&konecna).map_err(|e| Chyba::Format(e.to_string()))?;
    match rozpoznaj(&text) {
        Some(Format::Hls) => naplanuj_hls(k, &text, &base, n).await,
        Some(Format::Dash) => naplanuj_dash(&text, &base, n),
        None => Err(Chyba::Format(
            "adresa nevedie na stream HLS ani DASH".into(),
        )),
    }
}

async fn naplanuj_hls(k: &Client, text: &str, base: &Url, n: &Nastavenia) -> Vysledok<Plan> {
    match hls::parse(text, base)? {
        hls::Playlist::Media(m) => {
            if let Some(s) = m.sifrovanie {
                return Err(Chyba::Drm(s));
            }
            if !m.koniec {
                return Err(Chyba::Zive);
            }
            Ok(Plan {
                format: Format::Hls,
                trvanie: m.trvanie,
                kvality: vec![],
                jazyky: vec![],
                stopy: vec![StopaPlanu {
                    druh: "video+zvuk".into(),
                    jazyk: None,
                    nazov: None,
                    popis: String::new(),
                    kuskov: m.stopa.kusky.len(),
                    fmp4: m.fmp4,
                    stopa: m.stopa,
                }],
            })
        }
        hls::Playlist::Master(m) => {
            if let Some(s) = m.sifrovanie.clone() {
                return Err(Chyba::Drm(s));
            }
            let v = hls::vyber_variantu(&m.varianty, n.max_vyska)
                .ok_or_else(|| Chyba::Format("playlist nemá žiadnu kvalitu".into()))?;
            let zvuky: Vec<&hls::Rendicia> =
                hls::vyber_zvuk(&m.rendicie, v.audio.as_deref(), &n.jazyky)
                    .into_iter()
                    .filter(|r| r.uri.is_some())
                    .collect();
            let video = hls_media(k, &v.uri).await?;
            let mut stopy = vec![StopaPlanu {
                druh: if zvuky.is_empty() {
                    "video+zvuk"
                } else {
                    "video"
                }
                .into(),
                jazyk: None,
                nazov: None,
                popis: popis_kvality(v.sirka, v.vyska, v.bandwidth),
                kuskov: video.stopa.kusky.len(),
                fmp4: video.fmp4,
                stopa: video.stopa,
            }];
            let trvanie = video.trvanie;
            for r in zvuky {
                let md = hls_media(k, r.uri.as_deref().unwrap_or_default()).await?;
                stopy.push(StopaPlanu {
                    druh: "zvuk".into(),
                    jazyk: r.jazyk.clone(),
                    nazov: r.nazov.clone(),
                    popis: String::new(),
                    kuskov: md.stopa.kusky.len(),
                    fmp4: md.fmp4,
                    stopa: md.stopa,
                });
            }
            let mut kvality: Vec<String> = m
                .varianty
                .iter()
                .map(|x| popis_kvality(x.sirka, x.vyska, x.bandwidth))
                .collect();
            kvality.dedup();
            let mut jazyky: Vec<String> = m
                .rendicie
                .iter()
                .filter(|r| r.typ == "AUDIO")
                .filter_map(|r| r.jazyk.clone())
                .collect();
            jazyky.dedup();
            Ok(Plan {
                format: Format::Hls,
                trvanie,
                stopy,
                kvality,
                jazyky,
            })
        }
    }
}

fn naplanuj_dash(text: &str, base: &Url, n: &Nastavenia) -> Vysledok<Plan> {
    let m = dash::parse(text, base)?;
    let mut stopy = vec![];
    let video = dash::vyber_video(&m, n.max_vyska);
    let zvuky = dash::vyber_zvuk(&m, &n.jazyky);
    if let Some(v) = video {
        stopy.push(StopaPlanu {
            druh: "video".into(),
            jazyk: None,
            nazov: None,
            popis: popis_kvality(v.sirka, v.vyska, v.bandwidth),
            kuskov: v.stopa.kusky.len(),
            fmp4: true,
            stopa: v.stopa.clone(),
        });
    }
    for z in zvuky {
        stopy.push(StopaPlanu {
            druh: "zvuk".into(),
            jazyk: z.jazyk.clone(),
            nazov: None,
            popis: popis_kvality(None, None, z.bandwidth),
            kuskov: z.stopa.kusky.len(),
            fmp4: true,
            stopa: z.stopa.clone(),
        });
    }
    if stopy.is_empty() {
        return Err(Chyba::Format("manifest nemá obraz ani zvuk".into()));
    }
    let kvality = m
        .reprezentacie
        .iter()
        .filter(|r| r.druh == dash::Druh::Video)
        .map(|r| popis_kvality(r.sirka, r.vyska, r.bandwidth))
        .collect();
    let mut jazyky: Vec<String> = m
        .reprezentacie
        .iter()
        .filter(|r| r.druh == dash::Druh::Zvuk)
        .filter_map(|r| r.jazyk.clone())
        .collect();
    jazyky.dedup();
    Ok(Plan {
        format: Format::Dash,
        trvanie: m.trvanie,
        stopy,
        kvality,
        jazyky,
    })
}

/// Stiahne všetky stopy plánu (súbežne po kúskoch, s pokračovaním) a spojí ich do `ciel`
/// (.mkv alebo .mp4). Dočasné súbory stôp po úspechu zmaže.
pub async fn stiahni(
    k: &Client,
    plan: &Plan,
    ciel: &Path,
    titul: &str,
    spojenia: usize,
    zrusenie: &CancellationToken,
    hlasic: &Hlasic,
) -> Vysledok<PathBuf> {
    let mut vstupy = vec![];
    for (i, s) in plan.stopy.iter().enumerate() {
        if zrusenie.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        let pripona = if s.fmp4 { "mp4" } else { "ts" };
        let cesta = s_priponou(ciel, &format!(".stopa{i}.{pripona}"));
        let meno = match (&s.jazyk, s.druh.as_str()) {
            (Some(j), _) => format!("zvuk {j}"),
            (None, "zvuk") => "zvuk".to_string(),
            _ => "obraz".to_string(),
        };
        udalosti::info(
            hlasic,
            format!("Sťahujem {meno} ({})…", pocet_kuskov(s.kuskov)),
        );
        kusky::stiahni_stopu(k, &s.stopa, &cesta, spojenia, zrusenie, hlasic, &meno).await?;
        vstupy.push(Vstup {
            cesta,
            druh: s.druh.clone(),
            jazyk: s.jazyk.clone(),
            nazov: s.nazov.clone(),
        });
    }
    udalosti::info(hlasic, "Spájam stopy…");
    let vysledok = spajanie::spoj(&vstupy, ciel, titul).await?;
    for v in &vstupy {
        if v.cesta != vysledok {
            let _ = tokio::fs::remove_file(&v.cesta).await;
        }
    }
    Ok(vysledok)
}

/// „1 kúsok“, „3 kúsky“, „7 kúskov“.
fn pocet_kuskov(n: usize) -> String {
    match n {
        1 => "1 kúsok".into(),
        2..=4 => format!("{n} kúsky"),
        _ => format!("{n} kúskov"),
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn rozpoznanie_podla_obsahu() {
        assert_eq!(rozpoznaj("#EXTM3U\n#EXT-X-VERSION:3"), Some(Format::Hls));
        assert_eq!(rozpoznaj("\u{feff}  #EXTM3U"), Some(Format::Hls));
        assert_eq!(
            rozpoznaj("<?xml version=\"1.0\"?>\n<MPD type=\"static\">"),
            Some(Format::Dash)
        );
        assert_eq!(rozpoznaj("<MPD>"), Some(Format::Dash));
        assert_eq!(rozpoznaj("<!doctype html><html>"), None);
    }

    #[test]
    fn popis() {
        assert_eq!(
            popis_kvality(Some(1920), Some(1080), 5_000_000),
            "1920×1080, 5,0 Mb/s"
        );
        assert_eq!(popis_kvality(None, None, 128_000), "128 kb/s");
        assert_eq!(popis_kvality(None, Some(720), 0), "720p");
    }
}
