//! Front sťahovania: položky, stavy, uloženie na disk a výber, čo sa má spustiť.
//!
//! Front aplikácie v Ruste má vlastný súbor (`fronta-jadro.json`) – rozpracované súbory jadra
//! a yt-dlp v Pythone sa nemiešajú. Nastavenia sú spoločné.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stav {
    Caka,
    Stahuje,
    Pauza,
    Hotovo,
    Chyba,
    Rozbalene,
}

/// Jazykové stopy pre položku frontu (playlist, kanál, alebo video s voľbou „Video s jazykmi“).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VolbyJazykov {
    /// želané jazyky v poradí (napr. cs, sk, en); video ich má len niektoré
    pub poradie: Vec<String>,
    /// pridať pôvodnú stopu na koniec
    #[serde(default)]
    pub original: bool,
    /// zvuk pre všetky jazyky: "oboje" | "aac" | "opus"
    #[serde(default)]
    pub zvuk: String,
    /// voľba pri jednotlivých jazykoch
    #[serde(default)]
    pub zvuky: HashMap<String, String>,
    /// "mkv" | "mp4"
    #[serde(default)]
    pub kontajner: String,
    /// "Najlepšia", "1080p", …
    #[serde(default)]
    pub kvalita: String,
    /// "video" | "audio"
    #[serde(default)]
    pub rezim: String,
    /// keď video nemá žiadny z jazykov: "povodny" (stiahnuť s pôvodným zvukom) | "preskocit"
    #[serde(default)]
    pub bez_jazykov: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Polozka {
    pub id: String,
    pub url: String,
    pub zdroj: String,
    pub format: String,
    pub priecinok: String,
    #[serde(default)]
    pub nazov: String,
    pub stav: Stav,
    #[serde(default)]
    pub hotovo: u64,
    #[serde(default)]
    pub celkom: Option<u64>,
    #[serde(skip)]
    pub rychlost: f64,
    /// výsledný súbor
    #[serde(default)]
    pub subor: String,
    /// cieľ – pri pokračovaní ten istý (kvôli rozpracovaným častiam)
    #[serde(default)]
    pub ciel: String,
    #[serde(default)]
    pub chyba: String,
    #[serde(default)]
    pub podrobnosti: String,
    #[serde(default)]
    pub pridane: f64,
    #[serde(default)]
    pub referer: String,
    #[serde(default)]
    pub heslo: String,
    #[serde(default)]
    pub zo_skenu: bool,
    #[serde(default)]
    pub nahlad: String,
    #[serde(default)]
    pub dlzka: Option<f64>,
    #[serde(default)]
    pub autor: String,
    /// rozpracované súbory yt-dlp (na zmazanie pri odstránení)
    #[serde(default)]
    pub docasne: Vec<String>,
    /// jazykové stopy (YouTube) – inak bežné sťahovanie
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jazyky: Option<VolbyJazykov>,
    /// podiel hotovej práce, keď sa nedá rátať v bajtoch (jazyky: viac súborov za sebou)
    #[serde(skip)]
    pub podiel: Option<f64>,
    /// čo sa práve robí (napr. „Sťahujem: Čeština (2/3)“)
    #[serde(skip)]
    pub krok: String,
}

/// Čo pridať do frontu.
#[derive(Default, Clone)]
pub struct Nova {
    pub url: String,
    pub format: String,
    pub priecinok: String,
    pub nazov: String,
    pub zdroj: String,
    pub referer: String,
    pub heslo: String,
    pub zo_skenu: bool,
    pub jazyky: Option<VolbyJazykov>,
}

pub struct Beh {
    pub zrusenie: CancellationToken,
    pub uloha: Option<JoinHandle<()>>,
}

pub struct Fronta {
    pub polozky: Vec<Polozka>,
    pub behy: HashMap<String, Beh>,
    subor: PathBuf,
    posledne_ulozenie: Instant,
    pub zatvara: bool,
}

pub fn teraz() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn nove_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static POCITADLO: AtomicU64 = AtomicU64::new(0);
    let n = POCITADLO.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0) as u64;
    // krátke, jedinečné v rámci frontu: čas + počítadlo + číslo procesu, premiešané
    let mut x = t ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ ((std::process::id() as u64) << 32);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    format!("{:010x}", x & 0xff_ffff_ffff)
}

impl Fronta {
    pub fn nacitaj(priecinok: &Path, pokracovat: bool) -> Self {
        let subor = priecinok.join("fronta-jadro.json");
        let mut polozky: Vec<Polozka> = std::fs::read_to_string(&subor)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| serde_json::from_value(v.get("polozky")?.clone()).ok())
            .unwrap_or_default();
        for p in &mut polozky {
            if p.stav == Stav::Stahuje {
                p.stav = if pokracovat { Stav::Caka } else { Stav::Pauza };
            }
        }
        Fronta {
            polozky,
            behy: HashMap::new(),
            subor,
            posledne_ulozenie: Instant::now(),
            zatvara: false,
        }
    }

    /// Uloží front; `vzdy = false` najviac raz za 3 s (počas sťahovania).
    pub fn uloz(&mut self, vzdy: bool) {
        if !vzdy && self.posledne_ulozenie.elapsed().as_secs_f64() < 3.0 {
            return;
        }
        self.posledne_ulozenie = Instant::now();
        if let Some(p) = self.subor.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let tmp = self.subor.with_extension("tmp");
        let data = serde_json::json!({ "polozky": self.polozky });
        if let Ok(t) = serde_json::to_string_pretty(&data) {
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, &self.subor);
            }
        }
    }

    pub fn najdi(&self, id: &str) -> Option<&Polozka> {
        self.polozky.iter().find(|p| p.id == id)
    }

    pub fn uprav(&mut self, id: &str, f: impl FnOnce(&mut Polozka)) {
        if let Some(p) = self.polozky.iter_mut().find(|p| p.id == id) {
            f(p);
        }
    }

    /// Pridá nové položky; odkaz, ktorý už je vo fronte (a nie je hotový), sa vynechá.
    pub fn pridaj(&mut self, nove: Vec<Nova>) -> Vec<String> {
        let mut pridane = vec![];
        for n in nove {
            let url = n.url.trim().to_string();
            if url.is_empty()
                || self
                    .polozky
                    .iter()
                    .any(|p| p.url == url && p.stav != Stav::Hotovo)
            {
                continue;
            }
            let id = nove_id();
            let mut zdroj = super::zdroje::nazov_zdroja(&url).to_string();
            if zdroj == "Web" && !n.zdroj.is_empty() {
                zdroj = n.zdroj.clone();
            }
            self.polozky.push(Polozka {
                id: id.clone(),
                nahlad: super::zdroje::nahlad_youtube(&url),
                url,
                zdroj,
                format: n.format,
                priecinok: n.priecinok,
                nazov: n.nazov,
                stav: Stav::Caka,
                hotovo: 0,
                celkom: None,
                rychlost: 0.0,
                subor: String::new(),
                ciel: String::new(),
                chyba: String::new(),
                podrobnosti: String::new(),
                pridane: teraz(),
                referer: n.referer,
                heslo: n.heslo,
                zo_skenu: n.zo_skenu,
                dlzka: None,
                autor: String::new(),
                docasne: vec![],
                jazyky: n.jazyky,
                podiel: None,
                krok: String::new(),
            });
            pridane.push(id);
        }
        self.uloz(true);
        pridane
    }

    pub fn spusti(&mut self, ids: &[String]) {
        for p in self.polozky.iter_mut().filter(|p| ids.contains(&p.id)) {
            if matches!(p.stav, Stav::Pauza | Stav::Chyba) {
                p.stav = Stav::Caka;
                p.chyba.clear();
                p.podrobnosti.clear();
            }
        }
        self.uloz(true);
    }

    /// Bežiace sa zastavia (stav „pauza“ nastaví ich úloha), čakajúce sa hneď pozastavia.
    pub fn pozastav(&mut self, ids: &[String]) {
        for p in self.polozky.iter_mut().filter(|p| ids.contains(&p.id)) {
            match p.stav {
                Stav::Stahuje => {
                    if let Some(b) = self.behy.get(&p.id) {
                        b.zrusenie.cancel();
                    }
                }
                Stav::Caka => p.stav = Stav::Pauza,
                _ => {}
            }
        }
        self.uloz(true);
    }

    /// Odstráni položky z frontu; vráti ich spolu s úlohami, ktoré ešte bežia (na dobehnutie).
    pub fn odstran(&mut self, ids: &[String]) -> Vec<(Polozka, Option<JoinHandle<()>>)> {
        let mut out = vec![];
        for id in ids {
            let Some(i) = self.polozky.iter().position(|p| &p.id == id) else {
                continue;
            };
            let p = self.polozky.remove(i);
            let uloha = self.behy.remove(id).and_then(|b| {
                b.zrusenie.cancel();
                b.uloha
            });
            out.push((p, uloha));
        }
        self.uloz(true);
        out
    }

    pub fn vycisti_hotove(&mut self) {
        self.polozky.retain(|p| p.stav != Stav::Hotovo);
        self.uloz(true);
    }

    pub fn ids(&self) -> Vec<String> {
        self.polozky.iter().map(|p| p.id.clone()).collect()
    }

    pub fn rychlost(&self) -> f64 {
        self.polozky
            .iter()
            .filter(|p| p.stav == Stav::Stahuje)
            .map(|p| p.rychlost)
            .sum()
    }

    /// Čakajúce položky, ktoré sa zmestia do limitu súbežných sťahovaní – označí ich „stahuje“.
    pub fn na_spustenie(&mut self, limit: usize) -> Vec<(String, CancellationToken)> {
        if self.zatvara {
            return vec![];
        }
        let mut bezi = self
            .polozky
            .iter()
            .filter(|p| p.stav == Stav::Stahuje)
            .count();
        let mut out = vec![];
        for p in self.polozky.iter_mut() {
            if bezi >= limit {
                break;
            }
            if p.stav != Stav::Caka {
                continue;
            }
            p.stav = Stav::Stahuje;
            p.chyba.clear();
            p.podrobnosti.clear();
            let z = CancellationToken::new();
            self.behy.insert(
                p.id.clone(),
                Beh {
                    zrusenie: z.clone(),
                    uloha: None,
                },
            );
            out.push((p.id.clone(), z));
            bezi += 1;
        }
        out
    }
}

/// Zmaže rozpracované súbory položky (časti `.part`, stav úsekov, stopy streamov, kúsky, súbory yt-dlp).
pub fn zmaz_rozpracovane(p: &Polozka) {
    let mut kandidati: Vec<PathBuf> = p.docasne.iter().map(PathBuf::from).collect();
    if !p.ciel.is_empty() {
        let ciel = PathBuf::from(&p.ciel);
        if let (Some(dir), Some(meno)) = (ciel.parent(), ciel.file_name().and_then(|m| m.to_str()))
        {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if n.len() > meno.len() && n.starts_with(meno) {
                        let zvysok = &n[meno.len()..];
                        if [".part", ".kometa", ".tmp"].contains(&zvysok)
                            || zvysok.starts_with(".stopa")
                        {
                            kandidati.push(e.path());
                        }
                    }
                }
            }
        }
    }
    for k in kandidati {
        for c in [
            k.clone(),
            PathBuf::from(format!("{}.part", k.display())),
            PathBuf::from(format!("{}.ytdl", k.display())),
        ] {
            if c.is_dir() {
                let _ = std::fs::remove_dir_all(&c);
            } else if c.is_file() {
                let _ = std::fs::remove_file(&c);
            }
        }
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    fn nova(url: &str) -> Nova {
        Nova {
            url: url.into(),
            format: "video".into(),
            priecinok: "/tmp".into(),
            ..Default::default()
        }
    }

    #[test]
    fn pridanie_planovanie_pauza() {
        let d = std::env::temp_dir().join(format!("kometa_fronta_{}", std::process::id()));
        let mut f = Fronta::nacitaj(&d, true);
        let ids = f.pridaj(vec![
            nova("https://a.sk/1.mp4"),
            nova("https://a.sk/2.mp4"),
            nova("https://a.sk/1.mp4"),
        ]);
        assert_eq!(ids.len(), 2);
        assert_eq!(f.pridaj(vec![nova("https://a.sk/2.mp4")]).len(), 0);
        let s = f.na_spustenie(1);
        assert_eq!(s.len(), 1);
        assert_eq!(f.najdi(&ids[0]).unwrap().stav, Stav::Stahuje);
        f.pozastav(&ids);
        assert!(s[0].1.is_cancelled());
        assert_eq!(f.najdi(&ids[1]).unwrap().stav, Stav::Pauza);
        // po reštarte: rozbehnuté pokračuje, ak je zapnuté automatické pokračovanie
        let f2 = Fronta::nacitaj(&d, true);
        assert_eq!(f2.najdi(&ids[0]).unwrap().stav, Stav::Caka);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn zmazanie_rozpracovanych() {
        let d = std::env::temp_dir().join(format!("kometa_rozpr_{}", std::process::id()));
        std::fs::create_dir_all(d.join("film.mkv.stopa0.ts.kusky")).unwrap();
        for n in [
            "film.mkv.part",
            "film.mkv.kometa",
            "film.mkv.stopa0.ts",
            "film.mkv.stopa1.mp4",
            "film.mkv2",
            "iny.mkv.part",
        ] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        let mut p = Fronta::nacitaj(&d, true);
        let id = p.pridaj(vec![nova("https://a.sk/f")])[0].clone();
        p.uprav(&id, |x| {
            x.ciel = d.join("film.mkv").to_string_lossy().into_owned()
        });
        zmaz_rozpracovane(p.najdi(&id).unwrap());
        let mut ostalo: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        ostalo.sort();
        assert_eq!(
            ostalo,
            vec!["film.mkv2", "fronta-jadro.json", "iny.mkv.part"]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
