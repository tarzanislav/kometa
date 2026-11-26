//! Údaje o videu YouTube pre službu: najprv vlastné čítanie (`crate::youtube`), a keď nevyjde,
//! yt-dlp. Výsledok má v oboch prípadoch ten istý tvar.

use super::ytdlp::{self, Spolocne};
use super::Vnutro;
use crate::chyba::{Chyba, Vysledok};
use serde_json::Value;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Má sa YouTube čítať vlastným kódom? (`KOMETA_YOUTUBE=ytdlp` alebo nastavenie vypne.)
pub fn vlastne(v: &Vnutro) -> bool {
    std::env::var("KOMETA_YOUTUBE").map_or(true, |x| x != "ytdlp")
        && v.nastavenia.lock().unwrap().ano("youtube_vlastne", true)
}

/// Údaje o jednom videe: YouTube vlastným kódom (s yt-dlp ako zálohou), ostatné cez yt-dlp.
pub async fn info(
    v: &Vnutro,
    exe: &Path,
    url: &str,
    s: &Spolocne,
    z: &CancellationToken,
) -> Vysledok<Value> {
    if vlastne(v) && crate::youtube::id_videa(url).is_some() {
        let profil = crate::youtube::Profil::nacitaj(&v.priecinok);
        let k = crate::klient::klient(&crate::klient::Nastavenia::default())?;
        let r = tokio::select! {
            _ = z.cancelled() => return Err(Chyba::Pauza),
            r = crate::youtube::info(&k, url, &profil) => r,
        };
        match r {
            Ok(i) => return Ok(i),
            Err(e) => v.zapis(format!(
                "YouTube: vlastné čítanie nevyšlo ({e}) – použijem yt-dlp."
            )),
        }
    }
    ytdlp::info(exe, url, s, z).await
}
