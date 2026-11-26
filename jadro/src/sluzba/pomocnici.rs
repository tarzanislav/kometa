//! Údržba pomocníkov: Kometa si sama doinštaluje vlastný yt-dlp a Deno a udržiava ich aktuálne.
//!
//! - Chvíľu po spustení (a potom raz denne): ak chýba vlastná kópia yt-dlp, doinštaluje ju; ak je
//!   na GitHube novšia verzia, vymení ju (YouTube sa mení často a starý yt-dlp prestane stačiť).
//! - Deno (JavaScript, ktorý yt-dlp potrebuje na YouTube): vlastná kópia, obnoví sa pri novej
//!   hlavnej verzii (2.x → 3.x) – menšie verzie yt-dlp nevyžaduje.
//! - yt-dlp dostane cestu k Deno priamo (`--js-runtimes deno:…`), nič sa nehľadá v systéme.
//!
//! Kometa tak nezávisí od toho, čo je nainštalované v systéme.
//! Všetko sa sťahuje z oficiálnych vydaní a overuje súčtom SHA-256.

use super::{Instalacia, Vnutro};
use crate::udalosti::{self, Udalost};
use std::sync::Arc;
use std::time::Duration;

/// Doinštaluje pomocníka („yt-dlp“, „Deno“, „FFmpeg“). Naraz beží len jedna inštalácia;
/// vráti, či sa podarila (false aj vtedy, keď už iná beží).
pub async fn instaluj(v: &Arc<Vnutro>, co: &'static str) -> bool {
    {
        let mut i = v.instalacia.lock().unwrap();
        if i.bezi {
            return false;
        }
        *i = Instalacia {
            co,
            bezi: true,
            text: format!("Sťahujem {co}…"),
            podiel: 0.0,
        };
    }
    let v2 = Arc::downgrade(v);
    let h: udalosti::Hlasic = Arc::new(move |u| {
        if let (
            Some(v),
            Udalost::Priebeh {
                stiahnute,
                celkom: Some(c),
                ..
            },
        ) = (v2.upgrade(), u)
        {
            v.instalacia.lock().unwrap().podiel = stiahnute as f64 / c.max(1) as f64;
        }
    });
    let r = match crate::klient::klient(&crate::klient::Nastavenia::default()) {
        Ok(k) => match co {
            "FFmpeg" => v.nastroje.doinstaluj_ffmpeg(&k, &h).await,
            "Deno" => v.nastroje.doinstaluj_deno(&k, &h).await,
            _ => v.nastroje.doinstaluj_ytdlp(&k, &h).await,
        },
        Err(e) => Err(e),
    };
    let ok = r.is_ok();
    let text = match r {
        Ok(_) => format!("{co} je pripravený."),
        Err(e) => format!("{co} sa nepodarilo doinštalovať: {e}"),
    };
    {
        let mut i = v.instalacia.lock().unwrap();
        i.bezi = false;
        i.podiel = 1.0;
        i.text = text.clone();
    }
    if co == "Deno" {
        super::ytdlp::nastav_deno(v.nastroje.deno());
    }
    v.zapis(text);
    ok
}

/// Hlavná verzia (číslo pred prvou bodkou).
fn hlavna(v: &str) -> &str {
    v.split('.').next().unwrap_or(v)
}

/// Jedno kolo údržby: chýbajúce doinštaluje, zastarané vymení.
pub async fn udrzuj(v: &Arc<Vnutro>) {
    super::ytdlp::nastav_deno(v.nastroje.deno());
    let Ok(k) = crate::klient::klient(&crate::klient::Nastavenia::default()) else {
        return;
    };
    // yt-dlp: vlastná kópia a vždy posledná verzia
    if std::env::var_os("KOMETA_YTDLP").is_none() {
        if !v.nastroje.ma_vlastny_ytdlp() {
            if instaluj(v, "yt-dlp").await {
                v.zapis("⬇ Doinštaloval som vlastný yt-dlp (YouTube a ďalšie služby).");
            }
        } else if let Ok(najnovsia) = v.nastroje.najnovsia(&k, "yt-dlp/yt-dlp").await {
            if v.nastroje.verzia("yt-dlp").as_deref() != Some(najnovsia.as_str())
                && instaluj(v, "yt-dlp").await
            {
                v.zapis(format!("⬆ yt-dlp je aktualizovaný na {najnovsia}."));
            }
        }
    }
    // profil YouTube (klienti pre vlastné čítanie) – dáta z repozitára Komety
    if let Some(n) = crate::youtube::obnov_profil(&k, &v.priecinok).await {
        v.zapis(format!("⬆ Profil YouTube je obnovený (verzia {n})."));
    }
    // Deno: vlastná kópia; nová hlavná verzia sa stiahne, menšie nie
    if std::env::var_os("KOMETA_DENO").is_none() {
        if !v.nastroje.ma_vlastne_deno() {
            if instaluj(v, "Deno").await {
                v.zapis("⬇ Doinštaloval som Deno (YouTube ho cez yt-dlp potrebuje).");
            }
        } else if let Ok(najnovsia) = v.nastroje.najnovsia(&k, "denoland/deno").await {
            let mam = v.nastroje.verzia("deno").unwrap_or_default();
            if hlavna(&mam) != hlavna(&najnovsia) && instaluj(v, "Deno").await {
                v.zapis(format!("⬆ Deno je aktualizované na {najnovsia}."));
            }
        }
    }
    super::ytdlp::nastav_deno(v.nastroje.deno());
}

/// Spustí údržbu na pozadí: o chvíľu po štarte a potom raz denne.
/// `KOMETA_POMOCNICI=nie` ju vypne (skúšky), `KOMETA_POMOCNICI=hned` spustí prvé kolo hneď.
pub fn spusti(v: &Arc<Vnutro>) {
    super::ytdlp::nastav_deno(v.nastroje.deno());
    let rezim = std::env::var("KOMETA_POMOCNICI").unwrap_or_default();
    if rezim == "nie" {
        return;
    }
    let v2 = v.clone();
    v.rt.spawn(async move {
        let prvy = if rezim == "hned" {
            Duration::from_millis(300)
        } else {
            Duration::from_secs(15)
        };
        tokio::time::sleep(prvy).await;
        loop {
            udrzuj(&v2).await;
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    });
}

#[cfg(test)]
mod testy {
    #[test]
    fn hlavna_verzia() {
        assert_eq!(super::hlavna("2.5.6"), "2");
        assert_eq!(super::hlavna(""), "");
    }
}
