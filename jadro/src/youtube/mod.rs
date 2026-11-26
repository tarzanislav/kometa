//! Vlastné čítanie YouTube: údaje o videu priamo z rozhrania prehrávača (InnerTube), bez yt-dlp.
//!
//! Výsledok má ten istý tvar ako údaje, ktoré dáva yt-dlp (`formats`, `subtitles`,
//! `automatic_captions`, `chapters`, …), takže výber formátov, jazykové stopy, titulky aj spájanie
//! fungujú bez zmeny a yt-dlp ostáva len ako záloha.
//!
//! Klienti (meno, číslo, verzia, hlavička User-Agent) nie sú v kóde, ale v dátach `profil.json`.
//! Kometa si profil raz denne obnoví z GitHubu, takže keď YouTube zmení, ktorých klientov púšťa,
//! oprava sa dostane ku každému do jedného dňa – bez nového vydania. Profil je len dáta: adresa
//! rozhrania je pevná a nič z profilu sa nespúšťa.

mod kapitoly;
mod prevod;

pub use kapitoly::z_popisu as kapitoly_z_popisu;

use crate::chyba::{Chyba, Vysledok};
use reqwest::header::{CONTENT_TYPE, ORIGIN, RANGE, USER_AGENT};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::time::Duration;

/// Profil vložený do programu (zdroj pravdy je ten istý súbor v repozitári).
pub const PROFIL: &str = include_str!("profil.json");
/// Odkiaľ si Kometa obnovuje profil (vetva main repozitára Komety).
pub const ADRESA_PROFILU: &str =
    "https://raw.githubusercontent.com/tarzanislav/kometa/main/jadro/src/youtube/profil.json";
/// Obnovený profil v priečinku Komety (`~/.kometa`).
pub const SUBOR_PROFILU: &str = "youtube-profil.json";
const ADRESA_YOUTUBE: &str = "https://www.youtube.com";
const NAJVIAC_PROFIL: usize = 64 * 1024;

/// Klienti rozhrania prehrávača v poradí, v akom sa skúšajú.
#[derive(Clone, Debug, Deserialize)]
pub struct Profil {
    pub verzia: u32,
    pub klienti: Vec<Klient>,
}

/// Jeden klient rozhrania prehrávača (dáta, ktoré YouTube čaká v požiadavke).
#[derive(Clone, Debug, Deserialize)]
pub struct Klient {
    /// `clientName`
    pub meno: String,
    /// hlavička `X-YouTube-Client-Name`
    pub cislo: u32,
    /// `clientVersion`
    pub verzia: String,
    pub user_agent: String,
    /// ďalšie polia `context.client` (zariadenie, systém…)
    #[serde(default)]
    pub klient: Map<String, Value>,
}

impl Profil {
    /// Profil vložený do programu.
    pub fn vlozeny() -> Profil {
        Profil::z_textu(PROFIL).expect("vložený profil YouTube je platný")
    }

    /// Profil z textu – len ak je celý platný.
    pub fn z_textu(t: &str) -> Option<Profil> {
        let p: Profil = serde_json::from_str(t).ok()?;
        p.platny().then_some(p)
    }

    fn platny(&self) -> bool {
        let hlavicka = |t: &str| !t.is_empty() && t.bytes().all(|b| (0x20..0x7f).contains(&b));
        !self.klienti.is_empty()
            && self.klienti.iter().all(|k| {
                hlavicka(&k.meno)
                    && hlavicka(&k.verzia)
                    && hlavicka(&k.user_agent)
                    && k.cislo > 0
                    && k.klient
                        .values()
                        .all(|v| v.is_string() || v.is_number() || v.is_boolean())
            })
    }

    /// Novší z vloženého a obnoveného profilu (`priecinok/youtube-profil.json`).
    pub fn nacitaj(priecinok: &Path) -> Profil {
        let vlozeny = Profil::vlozeny();
        match std::fs::read_to_string(priecinok.join(SUBOR_PROFILU))
            .ok()
            .and_then(|t| Profil::z_textu(&t))
        {
            Some(p) if p.verzia > vlozeny.verzia => p,
            _ => vlozeny,
        }
    }
}

/// Obnoví profil z GitHubu (alebo z `KOMETA_YOUTUBE_PROFIL`). Vráti novú verziu, ak pribudla.
pub async fn obnov_profil(k: &reqwest::Client, priecinok: &Path) -> Option<u32> {
    let adresa =
        std::env::var("KOMETA_YOUTUBE_PROFIL").unwrap_or_else(|_| ADRESA_PROFILU.to_string());
    let odp = k
        .get(&adresa)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !odp.status().is_success() || odp.content_length().unwrap_or(0) > NAJVIAC_PROFIL as u64 {
        return None;
    }
    let t = odp.text().await.ok()?;
    if t.len() > NAJVIAC_PROFIL {
        return None;
    }
    let novy = Profil::z_textu(&t)?;
    if novy.verzia <= Profil::nacitaj(priecinok).verzia {
        return None;
    }
    let docasny = priecinok.join(format!("{SUBOR_PROFILU}.novy"));
    std::fs::write(&docasny, &t).ok()?;
    std::fs::rename(&docasny, priecinok.join(SUBOR_PROFILU)).ok()?;
    Some(novy.verzia)
}

/// Identifikátor videa (11 znakov) z odkazu YouTube; `None`, ak odkaz nevedie na jedno video.
pub fn id_videa(adresa: &str) -> Option<String> {
    let u = url::Url::parse(adresa.trim()).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let host = ["www.", "m.", "music."]
        .iter()
        .fold(host.as_str(), |h, p| h.strip_prefix(p).unwrap_or(h))
        .to_string();
    let mut casti = u.path_segments()?.filter(|c| !c.is_empty());
    let kandidat = match host.as_str() {
        "youtu.be" => casti.next()?.to_string(),
        "youtube.com" | "youtube-nocookie.com" => match casti.next()? {
            "watch" => u.query_pairs().find(|(k, _)| k == "v")?.1.into_owned(),
            "shorts" | "embed" | "live" | "v" | "e" => casti.next()?.to_string(),
            _ => return None,
        },
        _ => return None,
    };
    let platny = kandidat.len() == 11
        && kandidat
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    platny.then_some(kandidat)
}

fn zaklad() -> String {
    std::env::var("KOMETA_YOUTUBE_ZAKLAD")
        .unwrap_or_else(|_| ADRESA_YOUTUBE.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Údaje o videu v tvare, aký dáva yt-dlp. Skúša klientov z profilu po poradí; prvý, ktorého
/// adresy sa dajú naozaj stiahnuť, vyhráva. Chyba = služba použije yt-dlp.
pub async fn info(k: &reqwest::Client, adresa: &str, profil: &Profil) -> Vysledok<Value> {
    let id = id_videa(adresa)
        .ok_or_else(|| Chyba::Ine("Toto nie je odkaz na jedno video YouTube.".into()))?;
    let zaklad = zaklad();
    let mut chyby: Vec<String> = vec![];
    for kl in &profil.klienti {
        match skus(k, &zaklad, &id, kl).await {
            Ok(v) => return Ok(v),
            Err(e) => chyby.push(format!("{}: {e}", kl.meno)),
        }
    }
    Err(Chyba::Ine(format!("YouTube – {}", chyby.join("; "))))
}

/// Telo požiadavky na rozhranie prehrávača.
fn poziadavka(id: &str, kl: &Klient) -> Value {
    let mut klient = kl.klient.clone();
    klient.insert("clientName".into(), json!(kl.meno));
    klient.insert("clientVersion".into(), json!(kl.verzia));
    // angličtina: názvy zvukových stôp obsahujú „original“, podľa čoho sa pozná pôvodný jazyk
    klient.insert("hl".into(), json!("en"));
    klient.insert("gl".into(), json!("US"));
    klient.insert("timeZone".into(), json!("UTC"));
    klient.insert("utcOffsetMinutes".into(), json!(0));
    json!({
        "context": {"client": klient},
        "videoId": id,
        "playbackContext": {"contentPlaybackContext": {"html5Preference": "HTML5_PREF_WANTS"}},
        "contentCheckOk": true,
        "racyCheckOk": true,
    })
}

async fn skus(k: &reqwest::Client, zaklad: &str, id: &str, kl: &Klient) -> Vysledok<Value> {
    let odp = k
        .post(format!("{zaklad}/youtubei/v1/player?prettyPrint=false"))
        .header(CONTENT_TYPE, "application/json")
        .header(USER_AGENT, &kl.user_agent)
        .header(ORIGIN, ADRESA_YOUTUBE)
        .header("X-YouTube-Client-Name", kl.cislo.to_string())
        .header("X-YouTube-Client-Version", &kl.verzia)
        .body(poziadavka(id, kl).to_string())
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !odp.status().is_success() {
        return Err(Chyba::Http(odp.status().as_u16()));
    }
    let t = odp.text().await?;
    let o: Value =
        serde_json::from_str(&t).map_err(|e| Chyba::Format(format!("odpoveď YouTube: {e}")))?;
    let info = prevod::na_info(&o, id, kl)?;
    over_adresu(k, &info).await?;
    Ok(info)
}

/// Overí, že adresy z odpovede YouTube naozaj púšťa (jedna malá požiadavka). Všetky adresy jednej
/// odpovede majú ten istý podpis, takže stačí jedna – a ak nepustí, radšej hneď záloha.
async fn over_adresu(k: &reqwest::Client, info: &Value) -> Vysledok<()> {
    let formaty = info["formats"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let Some(f) = formaty
        .iter()
        .find(|f| f["vcodec"] == "none")
        .or(formaty.first())
    else {
        return Err(Chyba::Ine("bez formátov".into()));
    };
    let mut r = k
        .get(f["url"].as_str().unwrap_or(""))
        .header(RANGE, "bytes=0-1")
        .timeout(Duration::from_secs(20));
    if let Some(h) = f["http_headers"].as_object() {
        for (meno, hodnota) in h {
            if let Some(hodnota) = hodnota.as_str() {
                r = r.header(meno.as_str(), hodnota);
            }
        }
    }
    let odp = r.send().await?;
    if !odp.status().is_success() {
        return Err(Chyba::Http(odp.status().as_u16()));
    }
    Ok(())
}

#[cfg(test)]
mod skusky {
    use super::*;

    #[test]
    fn id_z_roznych_odkazov() {
        for (a, cakane) in [
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                Some("dQw4w9WgXcQ"),
            ),
            (
                "https://m.youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=30",
                Some("dQw4w9WgXcQ"),
            ),
            ("https://youtu.be/dQw4w9WgXcQ?si=abc", Some("dQw4w9WgXcQ")),
            (
                "https://www.youtube.com/shorts/abcdefghi_-",
                Some("abcdefghi_-"),
            ),
            (
                "https://www.youtube.com/embed/dQw4w9WgXcQ",
                Some("dQw4w9WgXcQ"),
            ),
            (
                "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
                Some("dQw4w9WgXcQ"),
            ),
            (
                "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
                Some("dQw4w9WgXcQ"),
            ),
            (
                "https://www.youtube.com/live/dQw4w9WgXcQ",
                Some("dQw4w9WgXcQ"),
            ),
            ("https://www.youtube.com/playlist?list=PL123", None),
            ("https://www.youtube.com/@kanal", None),
            ("https://www.youtube.com/watch?v=kratke", None),
            ("https://example.com/watch?v=dQw4w9WgXcQ", None),
            ("https://youtu.be/dQw4w9WgXc!", None),
        ] {
            assert_eq!(id_videa(a).as_deref(), cakane, "{a}");
        }
    }

    #[test]
    fn profil_vlozeny_a_kontrola() {
        let p = Profil::vlozeny();
        assert!(p.verzia >= 1 && !p.klienti.is_empty());
        // hlavička s koncom riadku, prázdny zoznam či vnorený objekt sa neprijmú
        assert!(Profil::z_textu(r#"{"verzia":9,"klienti":[]}"#).is_none());
        let zly_ua =
            r#"{"verzia":9,"klienti":[{"meno":"A","cislo":1,"verzia":"1","user_agent":"x\r\ny"}]}"#;
        assert!(Profil::z_textu(zly_ua).is_none());
        let vnoreny = r#"{"verzia":9,"klienti":[{"meno":"A","cislo":1,"verzia":"1","user_agent":"x","klient":{"a":{"b":1}}}]}"#;
        assert!(Profil::z_textu(vnoreny).is_none());
        let dobry = r#"{"verzia":9,"klienti":[{"meno":"A","cislo":1,"verzia":"1","user_agent":"x","klient":{"osName":"Android","sdk":32}}]}"#;
        assert_eq!(Profil::z_textu(dobry).unwrap().verzia, 9);
    }

    #[test]
    fn novsi_profil_z_priecinka() {
        let d = std::env::temp_dir().join(format!("kometa-yt-profil-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let vlozeny = Profil::vlozeny().verzia;
        // starší alebo rovnaký sa ignoruje, novší sa použije, poškodený sa ignoruje
        let s = |v: u32| {
            format!(
                r#"{{"verzia":{v},"klienti":[{{"meno":"TV","cislo":7,"verzia":"7.0","user_agent":"Mozilla/5.0"}}]}}"#
            )
        };
        std::fs::write(d.join(SUBOR_PROFILU), s(vlozeny)).unwrap();
        assert_eq!(
            Profil::nacitaj(&d).klienti[0].meno,
            Profil::vlozeny().klienti[0].meno
        );
        std::fs::write(d.join(SUBOR_PROFILU), s(vlozeny + 1)).unwrap();
        assert_eq!(Profil::nacitaj(&d).klienti[0].meno, "TV");
        std::fs::write(d.join(SUBOR_PROFILU), "{nie json").unwrap();
        assert_eq!(Profil::nacitaj(&d).verzia, vlozeny);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn telo_poziadavky() {
        let p = Profil::vlozeny();
        let t = poziadavka("dQw4w9WgXcQ", &p.klienti[0]);
        assert_eq!(t["videoId"], "dQw4w9WgXcQ");
        assert_eq!(
            t["context"]["client"]["clientName"],
            p.klienti[0].meno.as_str()
        );
        assert_eq!(t["context"]["client"]["hl"], "en");
        assert_eq!(t["contentCheckOk"], true);
    }
}
