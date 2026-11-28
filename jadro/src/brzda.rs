//! Brzda: strop rýchlosti sťahovania (šetrné sťahovanie, aby sa nevyťažila domáca sieť ani
//! server). Platí pre úlohu, ktorá sťahuje (`s_brzdou`), aj pre jej spojenia spustené zvlášť
//! (`aktualna` + `s_brzdou`). Vedro tokenov: krátky náraz do jednej sekundy, priemer pod stropom.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Brzda {
    bajtov_za_s: f64,
    /// (posledná zmena, bajty „na dlh“ – kladné = sťahuje sa rýchlejšie, než smie)
    stav: Mutex<(Instant, f64)>,
}

impl Brzda {
    pub fn new(bajtov_za_s: u64) -> Arc<Brzda> {
        Arc::new(Brzda {
            bajtov_za_s: bajtov_za_s.max(16 * 1024) as f64,
            stav: Mutex::new((Instant::now(), 0.0)),
        })
    }

    /// Koľko počkať po prijatí `n` bajtov.
    fn kolko(&self, n: u64) -> Duration {
        let mut s = self.stav.lock().unwrap();
        let teraz = Instant::now();
        let uplynulo = teraz.duration_since(s.0).as_secs_f64();
        // za uplynulý čas sa dlh splatí; náraz najviac jedna sekunda
        let dlh = (s.1 - uplynulo * self.bajtov_za_s).max(-self.bajtov_za_s) + n as f64;
        *s = (teraz, dlh);
        if dlh > 0.0 {
            Duration::from_secs_f64(dlh / self.bajtov_za_s)
        } else {
            Duration::ZERO
        }
    }
}

tokio::task_local! {
    static BRZDA: Arc<Brzda>;
}

/// Spustí sťahovanie s brzdou (bez brzdy = bez obmedzenia).
pub async fn s_brzdou<F: Future>(b: Option<Arc<Brzda>>, f: F) -> F::Output {
    match b {
        Some(b) => BRZDA.scope(b, f).await,
        None => f.await,
    }
}

/// Brzda tejto úlohy (na prenesenie do spojení spustených cez `tokio::spawn`).
pub fn aktualna() -> Option<Arc<Brzda>> {
    BRZDA.try_with(|b| b.clone()).ok()
}

/// Po prijatí `n` bajtov počká, ak sa sťahuje rýchlejšie, než brzda dovolí.
pub async fn pockaj(n: u64) {
    if let Some(b) = aktualna() {
        let d = b.kolko(n);
        if !d.is_zero() {
            tokio::time::sleep(d).await;
        }
    }
}

#[cfg(test)]
mod skusky {
    use super::*;

    #[tokio::test]
    async fn priemer_pod_stropom() {
        // 1 MB/s: 3 MB po kúskoch 256 kB trvá aspoň ~2 s (prvá sekunda je náraz)
        let b = Brzda::new(1024 * 1024);
        let start = Instant::now();
        s_brzdou(Some(b), async {
            for _ in 0..12 {
                pockaj(256 * 1024).await;
            }
        })
        .await;
        let t = start.elapsed().as_secs_f64();
        assert!((1.8..3.5).contains(&t), "{t}");
        // bez brzdy sa nečaká
        let start = Instant::now();
        for _ in 0..12 {
            pockaj(256 * 1024).await;
        }
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn brzda_prejde_do_spojenia() {
        let b = Brzda::new(64 * 1024);
        s_brzdou(Some(b), async {
            let prenesena = aktualna();
            let h = tokio::spawn(s_brzdou(prenesena, async { aktualna().is_some() }));
            assert!(h.await.unwrap());
        })
        .await;
    }
}
