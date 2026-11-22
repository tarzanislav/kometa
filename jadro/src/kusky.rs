//! Súbežné sťahovanie zoznamu kúskov (HLS/DASH) s pokračovaním.
//!
//! Každý kúsok sa uloží do `cieľ.kusky/000123` (najprv `.tmp`, po dokončení premenuje),
//! takže po prerušení sa sťahujú len chýbajúce kúsky. Na konci sa spoja v poradí.

use crate::chyba::{Chyba, Vysledok};
use crate::klient::s_priponou;
use crate::udalosti::{Hlasic, Udalost};
use futures_util::{stream, StreamExt};
use reqwest::header::{ACCEPT_ENCODING, RANGE};
use reqwest::Client;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// Jeden kúsok: adresa a voliteľný rozsah bajtov (posun, dĺžka).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Kusok {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rozsah: Option<(u64, u64)>,
}

impl Kusok {
    pub fn new(url: impl Into<String>) -> Self {
        Kusok {
            url: url.into(),
            rozsah: None,
        }
    }
}

/// Stopa = voliteľný inicializačný kúsok + kúsky v poradí.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Stopa {
    pub init: Option<Kusok>,
    pub kusky: Vec<Kusok>,
}

const POKUSY: u32 = 10;

pub async fn stiahni_stopu(
    k: &Client,
    stopa: &Stopa,
    ciel: &Path,
    spojenia: usize,
    zrusenie: &CancellationToken,
    hlasic: &Hlasic,
    nazov: &str,
) -> Vysledok<PathBuf> {
    let priecinok = s_priponou(ciel, ".kusky");
    tokio::fs::create_dir_all(&priecinok).await?;
    let vsetky: Vec<Kusok> = stopa
        .init
        .iter()
        .cloned()
        .chain(stopa.kusky.iter().cloned())
        .collect();
    let spolu = vsetky.len();
    if spolu == 0 {
        return Err(Chyba::Format("stopa nemá žiadne kúsky".into()));
    }
    let subor = |i: usize| priecinok.join(format!("{i:06}"));
    let bajty = Arc::new(AtomicU64::new(0));
    let hotove = Arc::new(AtomicUsize::new(0));
    let mut chyba_kusky = Vec::new();
    for i in 0..spolu {
        match tokio::fs::metadata(subor(i)).await {
            Ok(m) => {
                bajty.fetch_add(m.len(), Ordering::Relaxed);
                hotove.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => chyba_kusky.push(i),
        }
    }
    if hotove.load(Ordering::Relaxed) > 0 && !chyba_kusky.is_empty() {
        crate::udalosti::info(
            hlasic,
            format!(
                "{nazov}: pokračujem, hotových {} z {spolu} kúskov.",
                spolu - chyba_kusky.len()
            ),
        );
    }

    // priebeh každých 250 ms
    let koniec_hlasenia = CancellationToken::new();
    let hlasenie = {
        let (bajty, hotove, h, stop) = (
            bajty.clone(),
            hotove.clone(),
            hlasic.clone(),
            koniec_hlasenia.clone(),
        );
        let nazov = nazov.to_string();
        tokio::spawn(async move {
            let mut okno = vec![(Instant::now(), bajty.load(Ordering::Relaxed))];
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {}
                }
                let (b, d) = (
                    bajty.load(Ordering::Relaxed),
                    hotove.load(Ordering::Relaxed),
                );
                let teraz = Instant::now();
                okno.push((teraz, b));
                while okno.len() > 2 && teraz.duration_since(okno[0].0) > Duration::from_secs(5) {
                    okno.remove(0);
                }
                let dt = teraz.duration_since(okno[0].0).as_secs_f64().max(1e-3);
                h(Udalost::Priebeh {
                    stiahnute: b,
                    celkom: if d > 0 {
                        Some(b / d as u64 * spolu as u64)
                    } else {
                        None
                    },
                    rychlost: (b - okno[0].1) as f64 / dt,
                    cast: Some(format!("{nazov}: kúsok {d}/{spolu}")),
                });
            }
        })
    };

    let vysledky: Vec<Vysledok<()>> = stream::iter(chyba_kusky)
        .map(|i| {
            let (k, kus, cesta) = (k.clone(), vsetky[i].clone(), subor(i));
            let (bajty, hotove, z) = (bajty.clone(), hotove.clone(), zrusenie.clone());
            async move {
                let n = stiahni_kusok(&k, &kus, &cesta, &z).await?;
                bajty.fetch_add(n, Ordering::Relaxed);
                hotove.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        })
        .buffer_unordered(spojenia.max(1))
        .collect()
        .await;
    koniec_hlasenia.cancel();
    let _ = hlasenie.await;
    if zrusenie.is_cancelled() {
        return Err(Chyba::Pauza);
    }
    if let Some(e) = vysledky.into_iter().find_map(|v| v.err()) {
        return Err(e);
    }

    // spojenie kúskov v poradí
    let docasny = s_priponou(ciel, ".spajam");
    let mut out = tokio::fs::File::create(&docasny).await?;
    for i in 0..spolu {
        let mut f = tokio::fs::File::open(subor(i)).await?;
        tokio::io::copy(&mut f, &mut out).await?;
    }
    out.flush().await?;
    drop(out);
    tokio::fs::rename(&docasny, ciel).await?;
    let _ = tokio::fs::remove_dir_all(&priecinok).await;
    Ok(ciel.to_path_buf())
}

async fn stiahni_kusok(
    k: &Client,
    kus: &Kusok,
    cesta: &Path,
    z: &CancellationToken,
) -> Vysledok<u64> {
    let tmp = s_priponou(cesta, ".tmp");
    let mut posledna = String::new();
    for pokus in 0..POKUSY {
        if z.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        if pokus > 0 {
            tokio::select! {
                _ = z.cancelled() => return Err(Chyba::Pauza),
                _ = tokio::time::sleep(Duration::from_millis(500 * (1 << pokus.min(5)))) => {}
            }
        }
        let mut req = k.get(&kus.url).header(ACCEPT_ENCODING, "identity");
        if let Some((pos, dlzka)) = kus.rozsah {
            req = req.header(RANGE, format!("bytes={}-{}", pos, pos + dlzka - 1));
        }
        let r = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                posledna = e.to_string();
                continue;
            }
        };
        let s = r.status().as_u16();
        if s >= 500 || s == 429 {
            posledna = format!("chyba {s}");
            continue;
        }
        if s >= 400 {
            return Err(Chyba::Http(s));
        }
        let cely_subor = s == 200 && kus.rozsah.is_some(); // server ignoroval Range → vyrežeme si časť
        let data = tokio::select! {
            _ = z.cancelled() => return Err(Chyba::Pauza),
            d = r.bytes() => match d { Ok(d) => d, Err(e) => { posledna = e.to_string(); continue; } },
        };
        let data = match (cely_subor, kus.rozsah) {
            (true, Some((pos, dlzka))) => {
                let (a, b) = (pos as usize, (pos + dlzka) as usize);
                if b > data.len() {
                    return Err(Chyba::Format("rozsah kúska je mimo súboru".into()));
                }
                data.slice(a..b)
            }
            _ => data,
        };
        tokio::fs::write(&tmp, &data).await?;
        tokio::fs::rename(&tmp, cesta).await?;
        return Ok(data.len() as u64);
    }
    Err(Chyba::Siet(format!(
        "kúsok sa nepodarilo stiahnuť: {posledna}"
    )))
}
