//! Nastavenia – ten istý súbor `~/.kometa/nastavenia.json` ako okno v Pythone, takže
//! prechod medzi verziami nič nestratí. Neznáme kľúče sa zachovajú.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const PREDVOLENE_JAZYKY: [&str; 6] = ["cs", "sk", "en", "en-US", "pl", "hu"];
pub const PREHLIADACE: [&str; 8] = [
    "firefox", "chrome", "edge", "brave", "opera", "vivaldi", "chromium", "safari",
];
pub const TEMY: [&str; 7] = [
    "kometa",
    "polarna",
    "sumrak",
    "les",
    "more",
    "levandula",
    "grafit",
];
pub const KVALITY: [&str; 6] = ["Najlepšia", "2160p", "1440p", "1080p", "720p", "480p"];

/// Formáty sťahovania: (kľúč, popis). Rovnaké kľúče ako v Pythone.
pub const FORMATY: [(&str, &str); 6] = [
    ("video", "Video – najlepšia kvalita"),
    ("mp4", "Video – MP4 pre TV a mobil (H.264)"),
    ("1080", "Video – najviac 1080p"),
    ("720", "Video – najviac 720p"),
    ("mp3", "Zvuk – MP3"),
    ("audio", "Zvuk – pôvodná kvalita (M4A/Opus)"),
];

pub const ZVUK: [(&str, &str); 3] = [
    ("oboje", "AAC aj Opus (odporúčané)"),
    ("aac", "Len AAC"),
    ("opus", "Len Opus"),
];

/// Kľúče, ktoré smie meniť okno.
pub const KLUCE_OKNA: [&str; 14] = [
    "outdir",
    "dl_preset",
    "parallel",
    "connections",
    "autoresume",
    "cookies_browser",
    "audio_mode",
    "tema",
    "rezim",
    "include_original",
    "keep_separate",
    "mode",
    "kontajner",
    "max_height",
];

/// Domovský priečinok používateľa.
pub fn domov() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Priečinok s nastaveniami a frontom (`KOMETA_PRIECINOK` ho zmení – pre skúšky).
pub fn priecinok_nastaveni() -> PathBuf {
    std::env::var_os("KOMETA_PRIECINOK")
        .map(PathBuf::from)
        .unwrap_or_else(|| domov().join(".kometa"))
}

pub fn predvoleny_vystup() -> PathBuf {
    let videa = domov().join("Videos");
    if videa.is_dir() {
        videa
    } else {
        domov().join("Downloads")
    }
    .join("Kometa")
}

pub struct Nastavenia {
    subor: PathBuf,
    data: Map<String, Value>,
}

impl Nastavenia {
    pub fn nacitaj(priecinok: &Path) -> Self {
        let subor = priecinok.join("nastavenia.json");
        let mut data = predvolene();
        if let Ok(t) = std::fs::read_to_string(&subor) {
            if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&t) {
                data.extend(m);
            }
        }
        if data
            .get("outdir")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            data.insert(
                "outdir".into(),
                json!(predvoleny_vystup().to_string_lossy()),
            );
        }
        Nastavenia { subor, data }
    }

    pub fn uloz(&self) {
        if let Some(p) = self.subor.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let tmp = self.subor.with_extension("tmp");
        if let Ok(t) = serde_json::to_string_pretty(&Value::Object(self.data.clone())) {
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, &self.subor);
            }
        }
    }

    pub fn hodnota(&self, k: &str) -> Option<&Value> {
        self.data.get(k)
    }

    pub fn text(&self, k: &str) -> String {
        self.data
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    pub fn cislo(&self, k: &str, predvolene: i64) -> i64 {
        self.data
            .get(k)
            .and_then(Value::as_i64)
            .unwrap_or(predvolene)
    }

    pub fn ano(&self, k: &str, predvolene: bool) -> bool {
        self.data
            .get(k)
            .and_then(Value::as_bool)
            .unwrap_or(predvolene)
    }

    pub fn nastav(&mut self, k: &str, v: Value) {
        self.data.insert(k.to_string(), v);
    }

    pub fn jazyky(&self) -> Vec<String> {
        let j: Vec<String> = self
            .data
            .get("langs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if j.is_empty() {
            PREDVOLENE_JAZYKY.iter().map(|s| s.to_string()).collect()
        } else {
            j
        }
    }

    /// Kľúče, ktoré vidí okno.
    pub fn pre_okno(&self) -> Value {
        let mut m = Map::new();
        for k in KLUCE_OKNA {
            m.insert(k.into(), self.data.get(k).cloned().unwrap_or(Value::Null));
        }
        Value::Object(m)
    }

    /// Zmeny z okna: neznáme kľúče a neplatné hodnoty sa ticho vynechajú (ako v Pythone).
    pub fn zmen(&mut self, zmeny: &Map<String, Value>) {
        for (k, v) in zmeny {
            if let Some(cista) = over(k, v) {
                self.data.insert(k.clone(), cista);
            }
        }
        self.uloz();
    }
}

fn predvolene() -> Map<String, Value> {
    let v = json!({
        "outdir": "",
        "langs": PREDVOLENE_JAZYKY,
        "mode": "video",
        "include_original": true,
        "audio_mode": "oboje",
        "keep_separate": false,
        "kontajner": "mkv",
        "max_height": "Najlepšia",
        "dl_preset": "video",
        "parallel": 2,
        "connections": 4,
        "autoresume": true,
        "cookies_browser": "",
        "tema": "kometa",
        "rezim": "auto",
        "webshare_user": "",
        "webshare_token": "",
        "fastshare_user": "",
    });
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn jedno_z(v: &Value, moznosti: &[&str], inak: &str) -> Value {
    let s = v.as_str().unwrap_or("");
    json!(if moznosti.contains(&s) { s } else { inak })
}

fn cele(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .or_else(|| v.as_str()?.trim().parse().ok())
}

/// Kontrola hodnoty z okna; `None` = zahodiť.
pub fn over(k: &str, v: &Value) -> Option<Value> {
    let formaty: Vec<&str> = FORMATY.iter().map(|f| f.0).collect();
    Some(match k {
        "outdir" => {
            let s = v.as_str().unwrap_or("").trim();
            json!(if s.is_empty() {
                predvoleny_vystup().to_string_lossy().into_owned()
            } else {
                s.to_string()
            })
        }
        "dl_preset" => jedno_z(v, &formaty, "video"),
        "parallel" => json!(cele(v)?.clamp(1, 6)),
        "connections" => json!(cele(v)?.clamp(1, 16)),
        "autoresume" | "include_original" | "keep_separate" => json!(v.as_bool()?),
        "cookies_browser" => jedno_z(v, &PREHLIADACE, ""),
        "audio_mode" => jedno_z(v, &["oboje", "aac", "opus"], "oboje"),
        "tema" => jedno_z(v, &TEMY, "kometa"),
        "rezim" => jedno_z(v, &["auto", "den", "noc"], "auto"),
        "mode" => jedno_z(v, &["video", "existing", "audio"], "video"),
        "kontajner" => jedno_z(v, &["mkv", "mp4"], "mkv"),
        "max_height" => jedno_z(v, &KVALITY, "Najlepšia"),
        _ => return None,
    })
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn kontrola_hodnot() {
        assert_eq!(over("parallel", &json!(40)), Some(json!(6)));
        assert_eq!(over("parallel", &json!("3")), Some(json!(3)));
        assert_eq!(over("parallel", &json!("x")), None);
        assert_eq!(over("tema", &json!("neznama")), Some(json!("kometa")));
        assert_eq!(over("autoresume", &json!("ano")), None);
        assert_eq!(over("heslo", &json!("x")), None);
        assert_eq!(over("dl_preset", &json!("mp3")), Some(json!("mp3")));
    }

    #[test]
    fn zachova_nezname_kluce() {
        let d = std::env::temp_dir().join(format!("kometa_nast_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("nastavenia.json"),
            r#"{"tema":"les","vlastny":"ostane","parallel":3}"#,
        )
        .unwrap();
        let mut n = Nastavenia::nacitaj(&d);
        assert_eq!(n.text("tema"), "les");
        let mut z = Map::new();
        z.insert("parallel".into(), json!(5));
        z.insert("vlastny".into(), json!("zmena"));
        n.zmen(&z);
        let t: Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("nastavenia.json")).unwrap())
                .unwrap();
        assert_eq!(t["parallel"], json!(5));
        assert_eq!(t["vlastny"], json!("ostane"));
        assert!(!t["outdir"].as_str().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
