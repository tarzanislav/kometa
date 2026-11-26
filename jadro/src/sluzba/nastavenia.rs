//! Nastavenia v `~/.kometa/nastavenia.json` (ten istý súbor používala aj Kometa pre Python,
//! takže prechod nič nestratil). Neznáme kľúče sa zachovajú.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Poradie jazykov: CZ, SK, EN, PL, HU, RU – ostatné vybrané idú za nimi.
pub const PREDVOLENE_JAZYKY: [&str; 6] = ["cs", "sk", "en", "pl", "hu", "ru"];
/// Predvolené poradie starších verzií – pri načítaní sa nahradí novým.
const STARE_JAZYKY: [&str; 6] = ["cs", "sk", "en", "en-US", "pl", "hu"];
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

/// Formáty sťahovania: (kľúč, popis). Kľúče sú tie isté ako v Kometa pre Python (nastavenia ostanú platné).
pub const FORMATY: [(&str, &str); 7] = [
    ("video", "Video – najlepšia kvalita"),
    ("jazyky", "Video s jazykmi (YouTube, podľa karty Jazyky)"),
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
pub const KLUCE_OKNA: [&str; 26] = [
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
    "rozbalovat",
    "po_rozbaleni",
    "strazca",
    "strazca_rezim",
    "strazca_format",
    "strazca_ukazat",
    "titulky",
    "titulky_auto",
    "vlozit_udaje",
    "vlozit_kapitoly",
    "vlozit_obal",
    "aktualizacie",
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

/// Systémové priečinky podľa operačného systému – nie podľa pevného názvu v domovskom priečinku.
/// Vo Windows aj presmerované (napr. Videá na D:), v Linuxe preložené (XDG: „Videá“), v macOS „Movies“.
#[derive(Clone, Copy)]
pub enum Znamy {
    Videa,
    Stiahnute,
}

#[cfg(windows)]
mod windows {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    #[repr(C)]
    pub struct Guid(u32, u16, u16, [u8; 8]);

    /// FOLDERID_Videos {18989B1D-99B5-455B-841C-AB7C74E4DDFC}
    pub const VIDEA: Guid = Guid(
        0x1898_9B1D,
        0x99B5,
        0x455B,
        [0x84, 0x1C, 0xAB, 0x7C, 0x74, 0xE4, 0xDD, 0xFC],
    );
    /// FOLDERID_Downloads {374DE290-123F-4565-9164-39C4925E467B}
    pub const STIAHNUTE: Guid = Guid(
        0x374D_E290,
        0x123F,
        0x4565,
        [0x91, 0x64, 0x39, 0xC4, 0x92, 0x5E, 0x46, 0x7B],
    );

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut c_void,
            cesta: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(p: *mut c_void);
    }

    pub fn znamy(id: &Guid) -> Option<PathBuf> {
        let mut p: *mut u16 = std::ptr::null_mut();
        // SAFETY: platné GUID; výsledok treba uvoľniť CoTaskMemFree aj pri chybe (null je povolený)
        unsafe {
            let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
            let out = if hr == 0 && !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                Some(PathBuf::from(OsString::from_wide(
                    std::slice::from_raw_parts(p, n),
                )))
            } else {
                None
            };
            CoTaskMemFree(p.cast());
            out
        }
    }
}

/// Priečinok z `~/.config/user-dirs.dirs` (napr. `XDG_VIDEOS_DIR="$HOME/Videá"`).
pub fn xdg_z_textu(text: &str, kluc: &str, domov: &Path) -> Option<PathBuf> {
    text.lines().find_map(|r| {
        let hodnota = r
            .trim()
            .strip_prefix(kluc)?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .trim_matches('"');
        let cesta = match hodnota.strip_prefix("$HOME") {
            Some(z) => domov.join(z.trim_start_matches('/')),
            None if hodnota.starts_with('/') => PathBuf::from(hodnota),
            None => return None,
        };
        // XDG nastaví priečinok na samotný domov, keď ho používateľ zrušil
        (cesta.as_path() != domov && !z_domova_je_domov(&cesta, domov)).then_some(cesta)
    })
}

fn z_domova_je_domov(c: &Path, domov: &Path) -> bool {
    c.components().eq(domov.components())
}

pub fn znamy_priecinok(k: Znamy) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows::znamy(match k {
            Znamy::Videa => &windows::VIDEA,
            Znamy::Stiahnute => &windows::STIAHNUTE,
        })
    }
    #[cfg(target_os = "macos")]
    {
        Some(domov().join(match k {
            Znamy::Videa => "Movies",
            Znamy::Stiahnute => "Downloads",
        }))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let konf = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| domov().join(".config"));
        let text = std::fs::read_to_string(konf.join("user-dirs.dirs")).unwrap_or_default();
        let (kluc, inak) = match k {
            Znamy::Videa => ("XDG_VIDEOS_DIR", "Videos"),
            Znamy::Stiahnute => ("XDG_DOWNLOAD_DIR", "Downloads"),
        };
        xdg_z_textu(&text, kluc, &domov()).or_else(|| Some(domov().join(inak)))
    }
}

/// Predvolený priečinok na sťahovanie: `Videá/Kometa`, inak `Stiahnuté/Kometa`.
pub fn predvoleny_vystup() -> PathBuf {
    let videa = znamy_priecinok(Znamy::Videa).filter(|p| p.is_dir());
    let stiahnute = || {
        znamy_priecinok(Znamy::Stiahnute)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| domov().join("Downloads"))
    };
    videa.unwrap_or_else(stiahnute).join("Kometa")
}

/// Predvolené priečinky starších verzií (pevne `~/Videos/Kometa` a `~/Downloads/Kometa`) –
/// pri presmerovaných alebo preložených priečinkoch ukazovali na zlé miesto.
fn stare_predvolene() -> [PathBuf; 2] {
    [
        domov().join("Videos").join("Kometa"),
        domov().join("Downloads").join("Kometa"),
    ]
}

fn rovnaka_cesta(a: &str, b: &Path) -> bool {
    let n = |s: &str| {
        let s = s.trim_end_matches(['/', '\\']).replace('\\', "/");
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    n(a) == n(&b.to_string_lossy())
}

pub struct Nastavenia {
    subor: PathBuf,
    data: Map<String, Value>,
    /// ak sa pri načítaní opravil zlý predvolený priečinok: (pôvodný, nový)
    pub opraveny_priecinok: Option<(String, String)>,
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
        let ulozeny = data
            .get("outdir")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let novy = predvoleny_vystup();
        let mut opraveny_priecinok = None;
        if ulozeny.is_empty() {
            data.insert("outdir".into(), json!(novy.to_string_lossy()));
        } else if !rovnaka_cesta(&ulozeny, &novy)
            && stare_predvolene()
                .iter()
                .any(|s| rovnaka_cesta(&ulozeny, s))
        {
            // starší predvolený priečinok, ktorý systém presmeroval – nie voľba používateľa
            data.insert("outdir".into(), json!(novy.to_string_lossy()));
            opraveny_priecinok = Some((ulozeny, novy.to_string_lossy().into_owned()));
        }
        if data
            .get("langs")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().map(|x| x.as_str().unwrap_or("")).eq(STARE_JAZYKY))
        {
            data.insert("langs".into(), json!(PREDVOLENE_JAZYKY));
        }
        let n = Nastavenia {
            subor,
            data,
            opraveny_priecinok,
        };
        if n.opraveny_priecinok.is_some() {
            n.uloz();
        }
        n
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

    /// Zmeny z okna: neznáme kľúče a neplatné hodnoty sa ticho vynechajú.
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
        "datoid_user": "",
        "rozbalovat": true,
        "po_rozbaleni": "nechat",
        "strazca": false,
        "strazca_rezim": "zachytit",
        "strazca_format": "video",
        "strazca_ukazat": true,
        "titulky": "vypnute",
        "titulky_auto": false,
        "vlozit_udaje": true,
        "vlozit_kapitoly": true,
        "vlozit_obal": true,
        "aktualizacie": true,
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
        "autoresume" | "include_original" | "keep_separate" | "rozbalovat" | "strazca"
        | "strazca_ukazat" | "titulky_auto" | "vlozit_udaje" | "vlozit_kapitoly"
        | "vlozit_obal" | "aktualizacie" => {
            json!(v.as_bool()?)
        }
        "titulky" => jedno_z(v, &["vypnute", "jazyky"], "vypnute"),
        "po_rozbaleni" => jedno_z(v, &["nechat", "kos"], "nechat"),
        "strazca_rezim" => jedno_z(v, &["zachytit", "stiahnut"], "zachytit"),
        "strazca_format" => jedno_z(v, &formaty, "video"),
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
    fn priecinky_xdg() {
        let d = Path::new("/home/jano");
        let t = "# komentár\nXDG_DESKTOP_DIR=\"$HOME/Plocha\"\nXDG_VIDEOS_DIR=\"$HOME/Videá\"\nXDG_DOWNLOAD_DIR=\"$HOME/\"\n";
        assert_eq!(
            xdg_z_textu(t, "XDG_VIDEOS_DIR", d),
            Some(PathBuf::from("/home/jano/Videá"))
        );
        assert_eq!(xdg_z_textu(t, "XDG_DOWNLOAD_DIR", d), None);
        assert_eq!(
            xdg_z_textu("XDG_VIDEOS_DIR=\"/data/filmy\"", "XDG_VIDEOS_DIR", d),
            Some(PathBuf::from("/data/filmy"))
        );
        assert!(rovnaka_cesta("/a/b/", Path::new("/a/b")));
    }

    #[test]
    fn znamy_priecinok_systemu() {
        let v = znamy_priecinok(Znamy::Videa).expect("priečinok Videá");
        assert!(v.is_absolute(), "{v:?}");
        assert!(predvoleny_vystup().ends_with("Kometa"));
    }

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
