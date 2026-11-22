//! HTTP klient, nastavenia a pomocné funkcie pre súbory.

use crate::chyba::{Chyba, Vysledok};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ORIGIN, REFERER};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Čo používateľ chce – spoločné pre všetky druhy sťahovania.
#[derive(Clone, Debug)]
pub struct Nastavenia {
    /// Adresa stránky, na ktorej je prehrávač vložený (Vimeo s obmedzením na doménu a pod.).
    pub referer: Option<String>,
    pub hlavicky: Vec<(String, String)>,
    /// Počet spojení na jeden súbor / súbežných kúskov streamu.
    pub spojenia: usize,
    pub max_vyska: Option<u32>,
    /// Poradie jazykov zvuku, napr. ["cs", "sk", "en"].
    pub jazyky: Vec<String>,
}

impl Default for Nastavenia {
    fn default() -> Self {
        Nastavenia {
            referer: None,
            hlavicky: vec![],
            spojenia: 4,
            max_vyska: None,
            jazyky: vec![],
        }
    }
}

pub fn klient(n: &Nastavenia) -> Vysledok<reqwest::Client> {
    let mut h = HeaderMap::new();
    if let Some(r) = &n.referer {
        let v = HeaderValue::from_str(r).map_err(|_| Chyba::Ine("Neplatný Referer.".into()))?;
        h.insert(REFERER, v);
        if let Ok(u) = url::Url::parse(r) {
            if let Ok(o) = HeaderValue::from_str(&u.origin().ascii_serialization()) {
                h.insert(ORIGIN, o);
            }
        }
    }
    for (k, v) in &n.hlavicky {
        if let (Ok(k), Ok(v)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            h.insert(k, v);
        }
    }
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(h)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(32)
        .build()
        .map_err(Chyba::from)
}

/// Stiahne text (stránku, playlist, manifest). Vráti (text, konečná adresa po presmerovaní).
pub async fn text(k: &reqwest::Client, url: &str) -> Vysledok<(String, String)> {
    let r = k.get(url).send().await?;
    let status = r.status();
    if !status.is_success() {
        return Err(Chyba::Http(status.as_u16()));
    }
    let konecna = r.url().to_string();
    Ok((r.text().await?, konecna))
}

/// Názov súboru bezpečný pre Windows, macOS aj Linux.
pub fn bezpecny_nazov(text: &str) -> String {
    let mut s: String = text
        .chars()
        .map(|c| match c {
            ':' => '-',
            '"' => '\'',
            '/' | '\\' | '|' => '-',
            '<' => '(',
            '>' => ')',
            c if (c as u32) < 32 => ' ',
            c => c,
        })
        .filter(|c| *c != '?' && *c != '*')
        .collect();
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.trim_matches(|c: char| c == '.' || c == ' ');
    let s: String = s.chars().take(150).collect();
    if s.is_empty() {
        "subor".into()
    } else {
        s
    }
}

/// Ak súbor existuje, pridá „ (2)“, „ (3)“…
pub fn volna_cesta(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 2.. {
        let c = p.with_file_name(format!("{stem} ({n}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    unreachable!()
}

pub fn s_priponou(p: &Path, pripona: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(pripona);
    PathBuf::from(s)
}

/// Celková veľkosť z hlavičky Content-Range („bytes 0-0/12345“).
pub fn celkom_z_rozsahu(h: Option<&HeaderValue>) -> Option<u64> {
    let s = h?.to_str().ok()?;
    s.rsplit('/').next()?.trim().parse().ok()
}

/// Začiatok z hlavičky Content-Range („bytes 100-199/1000“ → 100).
pub fn zaciatok_z_rozsahu(h: Option<&HeaderValue>) -> Option<u64> {
    let s = h?.to_str().ok()?;
    let s = s.trim().strip_prefix("bytes")?.trim();
    s.split('-').next()?.trim().parse().ok()
}

pub fn je_html(h: Option<&HeaderValue>) -> bool {
    h.and_then(|v| v.to_str().ok())
        .map(|s| s.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn nazov() {
        assert_eq!(
            bezpecny_nazov("Test: \"Video\" / ukážka?"),
            "Test- 'Video' - ukážka"
        );
        assert_eq!(bezpecny_nazov("  ..  "), "subor");
    }

    #[test]
    fn rozsah() {
        let v = HeaderValue::from_static("bytes 100-199/1000");
        assert_eq!(celkom_z_rozsahu(Some(&v)), Some(1000));
        assert_eq!(zaciatok_z_rozsahu(Some(&v)), Some(100));
    }
}
