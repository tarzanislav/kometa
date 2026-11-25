//! Strážca schránky (len aplikácia): keď skopíruješ odkaz na YouTube, úložisko (Webshare,
//! Fastshare, Datoid) alebo inú známu službu, Kometa ho zachytí. Podľa nastavení ho len odloží
//! do karty „Zo schránky“, alebo ho hneď pridá do sťahovania. Playlisty a kanály sa nikdy
//! nespúšťajú samy – čakajú na výber videí.
//!
//! Schránka sa číta raz za sekundu a len keď je strážca zapnutý. Text, ktorý bol v schránke
//! v čase zapnutia, sa nezachytí – len to, čo skopíruješ potom.

use super::{zdroje, Vnutro};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zachyteny {
    pub id: String,
    pub url: String,
    pub zdroj: String,
    #[serde(default)]
    pub nazov: String,
    #[serde(default)]
    pub nahlad: String,
    #[serde(default)]
    pub kolekcia: bool,
    #[serde(default)]
    pub ulozisko: bool,
    #[serde(default)]
    pub cas: f64,
}

#[derive(Default)]
pub struct Strazca {
    pub zoznam: Vec<Zachyteny>,
    posledny: Option<String>,
    videne: HashSet<String>,
}

fn teraz() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

const PRIPONY: [&str; 14] = [
    "mp4", "mkv", "webm", "mov", "avi", "m3u8", "mpd", "mp3", "m4a", "zip", "rar", "7z", "iso",
    "flac",
];

/// Oplatí sa odkaz zachytiť? Známe služby, playlisty a priame odkazy na médiá či archívy.
pub fn zaujimavy(url: &str) -> bool {
    if zdroje::nazov_zdroja(url) != "Web" || zdroje::je_kolekcia(url) {
        return true;
    }
    let cesta = url
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    cesta
        .rsplit_once('.')
        .is_some_and(|(_, p)| PRIPONY.contains(&p))
}

/// Predbežný názov bez siete: pri Fastshare z odkazu, inak koniec adresy.
fn predbezny_nazov(url: &str) -> String {
    if let Some(n) = super::ucty::nazov_z_odkazu_fastshare(url) {
        return n;
    }
    if zdroje::nazov_zdroja(url) == "YouTube" {
        return String::new();
    }
    let n = super::stahovanie::nazov_z_adresy(url);
    if n == "kometa" {
        String::new()
    } else {
        n
    }
}

fn subor(v: &Vnutro) -> std::path::PathBuf {
    v.priecinok.join("schranka.json")
}

/// Načíta odložené odkazy z minulého spustenia.
pub fn nacitaj(v: &Vnutro) {
    let zoznam: Vec<Zachyteny> = std::fs::read_to_string(subor(v))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut s = v.strazca.lock().unwrap();
    s.videne = zoznam.iter().map(|z| z.url.clone()).collect();
    s.zoznam = zoznam;
}

pub fn uloz(v: &Vnutro) {
    let t = serde_json::to_string_pretty(&v.strazca.lock().unwrap().zoznam).unwrap_or_default();
    let c = subor(v);
    let tmp = c.with_extension("tmp");
    if std::fs::write(&tmp, t).is_ok() {
        let _ = std::fs::rename(tmp, c);
    }
}

pub fn stav(v: &Vnutro) -> Value {
    json!(v.strazca.lock().unwrap().zoznam)
}

/// Spustí sledovanie na pozadí (vlákno skončí spolu s jadrom).
pub fn spusti(v: &Arc<Vnutro>) {
    if !v.prostredie.sleduje_schranku() {
        return;
    }
    nacitaj(v);
    let w: Weak<Vnutro> = Arc::downgrade(v);
    let _ = std::thread::Builder::new()
        .name("kometa-schranka".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(1000));
            let Some(v) = w.upgrade() else {
                return;
            };
            if v.fronta.lock().unwrap().zatvara {
                return;
            }
            skontroluj(&v);
        });
}

/// Jedno pozretie do schránky.
pub fn skontroluj(v: &Arc<Vnutro>) {
    let (zapnuty, rezim, format) = {
        let n = v.nastavenia.lock().unwrap();
        (
            n.ano("strazca", false),
            n.text("strazca_rezim"),
            n.text("strazca_format"),
        )
    };
    if !zapnuty {
        v.strazca.lock().unwrap().posledny = None;
        return;
    }
    let text = v.prostredie.schranka();
    {
        let mut s = v.strazca.lock().unwrap();
        match &s.posledny {
            // prvé pozretie po zapnutí: to, čo už v schránke bolo, sa nezachytí
            None => {
                s.posledny = Some(text);
                return;
            }
            Some(t) if *t == text => return,
            _ => s.posledny = Some(text.clone()),
        }
    }
    let odkazy: Vec<String> = zdroje::odkazy(&text)
        .into_iter()
        .filter(|u| zaujimavy(u))
        .take(50)
        .collect();
    if odkazy.is_empty() {
        return;
    }
    let vo_fronte: HashSet<String> = v
        .fronta
        .lock()
        .unwrap()
        .polozky
        .iter()
        .filter(|p| p.stav != super::fronta::Stav::Hotovo)
        .map(|p| p.url.clone())
        .collect();
    let nove: Vec<String> = {
        let mut s = v.strazca.lock().unwrap();
        odkazy
            .into_iter()
            .filter(|u| !vo_fronte.contains(u) && s.videne.insert(u.clone()))
            .collect()
    };
    if nove.is_empty() {
        return;
    }
    let stiahnut = rezim == "stiahnut";
    let mut do_frontu = vec![];
    let mut odlozene = vec![];
    for url in nove {
        let kolekcia = zdroje::je_kolekcia(&url);
        if stiahnut && !kolekcia {
            do_frontu.push(url);
        } else {
            odlozene.push(Zachyteny {
                id: super::fronta::nove_id(),
                zdroj: zdroje::nazov_zdroja(&url).to_string(),
                nazov: predbezny_nazov(&url),
                nahlad: zdroje::nahlad_youtube(&url),
                kolekcia,
                ulozisko: zdroje::je_ulozisko(&url),
                cas: teraz(),
                url,
            });
        }
    }
    if !do_frontu.is_empty() {
        let n = super::Sluzba::from(v.clone()).pridat_odkazy(&do_frontu, &format, "");
        v.prostredie.oznam(
            "Kometa – zo schránky",
            &if n == 1 {
                format!("Sťahujem: {}", do_frontu[0])
            } else {
                format!("Pridané do sťahovania: {n}")
            },
        );
        v.zapis(format!("📋 Zo schránky do sťahovania: {n}"));
    }
    if !odlozene.is_empty() {
        let pocet = odlozene.len();
        let prvy = odlozene[0].url.clone();
        for z in &odlozene {
            nacitaj_nazov(v, z);
        }
        v.strazca.lock().unwrap().zoznam.extend(odlozene);
        uloz(v);
        v.prostredie.oznam(
            "Kometa – zo schránky",
            &if pocet == 1 {
                format!("Zachytený odkaz: {prvy}")
            } else {
                format!("Zachytené odkazy: {pocet}")
            },
        );
    }
}

/// Názov videa z YouTube (oEmbed – bez kľúča a prihlásenia), doplní sa na pozadí.
fn nacitaj_nazov(v: &Arc<Vnutro>, z: &Zachyteny) {
    if z.zdroj != "YouTube" || z.kolekcia {
        return;
    }
    let (v2, id, url) = (v.clone(), z.id.clone(), z.url.clone());
    v.rt_spawn(async move {
        let Ok(k) = crate::klient::klient(&crate::klient::Nastavenia::default()) else {
            return;
        };
        let zaklad = std::env::var("KOMETA_OEMBED_ZAKLAD")
            .unwrap_or_else(|_| "https://www.youtube.com/oembed".into());
        let Ok(r) = k
            .get(zaklad)
            .query(&[("url", url.as_str()), ("format", "json")])
            .send()
            .await
        else {
            return;
        };
        let Some(j) = r
            .text()
            .await
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return;
        };
        if let Some(t) = j.get("title").and_then(Value::as_str) {
            let mut s = v2.strazca.lock().unwrap();
            if let Some(z) = s.zoznam.iter_mut().find(|z| z.id == id) {
                z.nazov = t.to_string();
            }
            drop(s);
            uloz(&v2);
        }
    });
}

/// Odloží odkazy (alebo všetky pri `None`) zo zoznamu a vráti ich.
pub fn vyber(v: &Vnutro, ids: Option<&[String]>) -> Vec<Zachyteny> {
    let mut s = v.strazca.lock().unwrap();
    let (vybrane, ostatne): (Vec<_>, Vec<_>) = std::mem::take(&mut s.zoznam)
        .into_iter()
        .partition(|z| ids.map_or(true, |ids| ids.contains(&z.id)));
    s.zoznam = ostatne;
    drop(s);
    uloz(v);
    vybrane
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn co_sa_zachyti() {
        assert!(zaujimavy("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(zaujimavy("https://fastshare.cloud/123/film.mkv"));
        assert!(zaujimavy("https://datoid.cz/abc/film"));
        assert!(zaujimavy("https://webshare.cz/#/file/abc"));
        assert!(zaujimavy("https://cdn.example.com/video/klip.mp4?x=1"));
        assert!(zaujimavy("https://example.com/subory/balik.rar"));
        assert!(!zaujimavy("https://www.sme.sk/clanok/123"));
        assert!(!zaujimavy("https://example.com/obrazok.jpg"));
    }
}
