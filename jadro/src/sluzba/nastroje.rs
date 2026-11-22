//! Pomocné programy: yt-dlp (YouTube a ďalšie služby) a FFmpeg (len ak je po ruke – vlastné
//! spájanie ho väčšinou nepotrebuje). yt-dlp sa dá doinštalovať jedným klikom: stiahne sa
//! z oficiálnych vydaní a pred použitím sa overí jeho kontrolný súčet SHA-256.

use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Hlasic};
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Nastroje {
    priecinok: PathBuf,
}

/// Program v ceste PATH (vo Windows aj s príponami z PATHEXT).
pub fn v_ceste(meno: &str) -> Option<PathBuf> {
    let cesta = std::env::var_os("PATH")?;
    let pripony: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|p| p.to_ascii_lowercase())
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&cesta).find_map(|d| {
        pripony
            .iter()
            .map(|p| d.join(format!("{meno}{p}")))
            .find(|k| k.is_file())
    })
}

/// Názov súboru yt-dlp vo vydaniach pre tento systém.
fn subor_vydania() -> Option<&'static str> {
    if cfg!(windows) {
        Some("yt-dlp.exe")
    } else if cfg!(target_os = "macos") {
        Some("yt-dlp_macos")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("yt-dlp_linux")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("yt-dlp_linux_aarch64")
    } else {
        None
    }
}

impl Nastroje {
    pub fn new(priecinok_nastaveni: &Path) -> Self {
        Nastroje {
            priecinok: priecinok_nastaveni.join("nastroje"),
        }
    }

    fn lokalny(&self, meno: &str) -> PathBuf {
        self.priecinok.join(if cfg!(windows) {
            format!("{meno}.exe")
        } else {
            meno.to_string()
        })
    }

    /// `KOMETA_YTDLP` = cesta k programu, alebo `nie` (ako keby chýbal – pre skúšky).
    pub fn ytdlp(&self) -> Option<PathBuf> {
        match std::env::var("KOMETA_YTDLP") {
            Ok(v) if v == "nie" => return None,
            Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
            _ => {}
        }
        let l = self.lokalny("yt-dlp");
        if l.is_file() {
            return Some(l);
        }
        v_ceste("yt-dlp")
    }

    pub fn ffmpeg(&self) -> Option<PathBuf> {
        match std::env::var("KOMETA_FFMPEG") {
            Ok(v) if v == "nie" => return None,
            Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
            _ => {}
        }
        let l = self.lokalny("ffmpeg");
        if l.is_file() {
            return Some(l);
        }
        v_ceste("ffmpeg")
    }

    /// Stiahne yt-dlp z oficiálnych vydaní do `~/.kometa/nastroje` a overí SHA-256.
    pub async fn doinstaluj_ytdlp(&self, k: &Client, hlasic: &Hlasic) -> Vysledok<PathBuf> {
        let meno = subor_vydania().ok_or_else(|| {
            Chyba::Ine(
                "Pre tento systém nie je hotový yt-dlp – nainštaluj ho cez pip install yt-dlp."
                    .into(),
            )
        })?;
        let zaklad = std::env::var("KOMETA_YTDLP_ZAKLAD")
            .unwrap_or_else(|_| "https://github.com/yt-dlp/yt-dlp/releases/latest/download".into());
        let zaklad = zaklad.trim_end_matches('/');
        udalosti::info(hlasic, "Sťahujem yt-dlp z oficiálnych vydaní…");
        let sucty = k
            .get(format!("{zaklad}/SHA2-256SUMS"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let ocakavany = sucty
            .lines()
            .find_map(|r| {
                let (h, n) = r.split_once(char::is_whitespace)?;
                (n.trim().trim_start_matches('*') == meno).then(|| h.trim().to_ascii_lowercase())
            })
            .ok_or_else(|| Chyba::Format(format!("v SHA2-256SUMS chýba {meno}")))?;
        let r = k
            .get(format!("{zaklad}/{meno}"))
            .send()
            .await?
            .error_for_status()?;
        let celkom = r.content_length();
        let mut data: Vec<u8> = Vec::with_capacity(celkom.unwrap_or(0) as usize);
        let mut s = r.bytes_stream();
        let zaciatok = std::time::Instant::now();
        while let Some(c) = s.next().await {
            data.extend_from_slice(&c?);
            hlasic(udalosti::Udalost::Priebeh {
                stiahnute: data.len() as u64,
                celkom,
                rychlost: data.len() as f64 / zaciatok.elapsed().as_secs_f64().max(0.001),
                cast: Some("yt-dlp".into()),
            });
        }
        let sucet: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if sucet != ocakavany {
            return Err(Chyba::Format(
                "stiahnutý yt-dlp nesedí s kontrolným súčtom – neuložil som ho".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.priecinok).await?;
        let ciel = self.lokalny("yt-dlp");
        let tmp = ciel.with_extension("stahuje");
        tokio::fs::write(&tmp, &data).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).await?;
        }
        tokio::fs::rename(&tmp, &ciel).await?;
        udalosti::info(hlasic, "yt-dlp je pripravený.");
        Ok(ciel)
    }
}
