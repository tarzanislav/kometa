//! Prehľad kurzu: interaktívna stránka „Prehľad kurzu.html“ v priečinku kurzu – osnova
//! s kapitolami a lekciami, prehrávač, texty, materiály a čo už je pozreté. Otvorí sa
//! v prehliadači priamo zo disku, bez internetu. Podklad je `kometa-kurz.json`: pri ďalšom
//! sťahovaní toho istého kurzu sa lekcie doplnia, po stiahnutí každej lekcie sa stránka obnoví.

use super::fronta::Polozka;
use super::kurzy::{je_text, Kurz, Miesto};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MANIFEST: &str = "kometa-kurz.json";
pub const STRANKA: &str = "Prehľad kurzu.html";
const SABLONA: &str = include_str!("prehlad.html");
/// zápisy z viacerých dokončených lekcií naraz idú po jednom
static ZAMOK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    #[serde(default)]
    pub verzia: u32,
    pub nazov: String,
    #[serde(default)]
    pub adresa: String,
    #[serde(default)]
    pub kapitoly: Vec<MKapitola>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MKapitola {
    pub cislo: usize,
    pub nazov: String,
    /// priečinok kapitoly voči priečinku kurzu („“ = priamo v ňom)
    #[serde(default)]
    pub priecinok: String,
    #[serde(default)]
    pub lekcie: Vec<MLekcia>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MLekcia {
    /// „02 Základy/03 Premenné“ – rovnaký ako pri položke frontu
    pub kluc: String,
    pub cislo: usize,
    pub nazov: String,
    /// „video“, „text“, „test“, „material“
    #[serde(default)]
    pub druh: String,
    #[serde(default)]
    pub dlzka: String,
    /// súbor voči priečinku kurzu (s „/“); prázdny = ešte nie je stiahnutý
    #[serde(default)]
    pub subor: String,
    /// lekcia na stránke kurzu
    #[serde(default)]
    pub adresa: String,
}

fn druh(m: &Miesto) -> &'static str {
    let l = m.lekcia;
    if l.material || l.druh == "material" {
        "material"
    } else if l.druh == "test" {
        "test"
    } else if l.druh == "text" || je_text(l) {
        "text"
    } else {
        "video"
    }
}

/// Cesta voči priečinku kurzu s „/“ (aj vo Windows – tak ju chce prehliadač).
fn relativna(koren: &Path, cesta: &Path) -> Option<String> {
    let r = cesta.strip_prefix(koren).ok()?;
    let casti: Vec<String> = r
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!casti.is_empty()).then(|| casti.join("/"))
}

fn nacitaj(koren: &Path) -> Option<Manifest> {
    let t = std::fs::read_to_string(koren.join(MANIFEST)).ok()?;
    serde_json::from_str(&t).ok()
}

fn uloz(koren: &Path, m: &Manifest) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(koren)?;
    let t = serde_json::to_string_pretty(m).map_err(std::io::Error::other)?;
    let docasny = koren.join(format!("{MANIFEST}.novy"));
    std::fs::write(&docasny, t)?;
    std::fs::rename(&docasny, koren.join(MANIFEST))?;
    let stranka = koren.join(STRANKA);
    let docasny = koren.join(".prehlad.novy");
    std::fs::write(&docasny, html(m, koren))?;
    std::fs::rename(&docasny, &stranka)?;
    Ok(stranka)
}

/// Zapíše (doplní) prehľad kurzu pre práve pridané lekcie. Vráti cestu k stránke.
pub fn zapis_kurz(
    kurz: &Kurz,
    koren: &Path,
    miesta: &[Miesto],
    texty: &[(String, PathBuf)],
) -> std::io::Result<PathBuf> {
    let _z = ZAMOK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = nacitaj(koren).unwrap_or_default();
    m.verzia = 1;
    m.nazov = koren
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| kurz.nazov.clone());
    if !kurz.adresa.is_empty() {
        m.adresa = kurz.adresa.clone();
    }
    for x in miesta {
        let ki = match m.kapitoly.iter().position(|k| k.priecinok == x.relativny) {
            Some(i) => i,
            None => {
                m.kapitoly.push(MKapitola {
                    cislo: x.cislo_kapitoly,
                    nazov: x.kapitola.nazov.trim().to_string(),
                    priecinok: x.relativny.clone(),
                    lekcie: vec![],
                });
                m.kapitoly.len() - 1
            }
        };
        let k = &mut m.kapitoly[ki];
        k.cislo = x.cislo_kapitoly;
        if !x.kapitola.nazov.trim().is_empty() {
            k.nazov = x.kapitola.nazov.trim().to_string();
        }
        let subor = texty
            .iter()
            .find(|(kluc, _)| *kluc == x.kluc)
            .and_then(|(_, c)| relativna(koren, c));
        let adresa = x
            .lekcia
            .url
            .split("#kometa-lekcia-")
            .next()
            .unwrap_or("")
            .to_string();
        let nova = MLekcia {
            kluc: x.kluc.clone(),
            cislo: x.cislo,
            nazov: x.lekcia.nazov.trim().to_string(),
            druh: druh(x).to_string(),
            dlzka: x.lekcia.dlzka.trim().to_string(),
            subor: subor.unwrap_or_default(),
            adresa,
        };
        match k.lekcie.iter_mut().find(|l| l.kluc == x.kluc) {
            Some(l) => {
                let subor = std::mem::take(&mut l.subor);
                let dlzka = std::mem::take(&mut l.dlzka);
                *l = MLekcia {
                    subor: if nova.subor.is_empty() {
                        subor
                    } else {
                        nova.subor
                    },
                    dlzka: if nova.dlzka.is_empty() {
                        dlzka
                    } else {
                        nova.dlzka
                    },
                    ..nova
                };
            }
            None => k.lekcie.push(nova),
        }
    }
    for k in m.kapitoly.iter_mut() {
        k.lekcie.sort_by_key(|l| l.cislo);
    }
    m.kapitoly.sort_by_key(|k| k.cislo);
    uloz(koren, &m)
}

/// Obnoví stránku prehľadu kurzu (napr. po súbore stiahnutom v prehliadači kurzov).
pub fn obnov(koren: &Path) {
    let _z = ZAMOK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = nacitaj(koren) {
        let _ = uloz(koren, &m);
    }
}

/// Priečinok kurzu nad priečinkom lekcie (priamo on alebo o úroveň vyššie).
fn najdi_koren(priecinok: &Path) -> Option<PathBuf> {
    [Some(priecinok), priecinok.parent()]
        .into_iter()
        .flatten()
        .find(|d| d.join(MANIFEST).is_file())
        .map(Path::to_path_buf)
}

/// Lekcia kurzu je stiahnutá: zapíše sa jej súbor (a dĺžka, ak ju stránka neukázala) a stránka
/// prehľadu sa obnoví.
pub fn po_stiahnuti(p: &Polozka, subor: &Path, dlzka: Option<f64>) {
    let Some(koren) = najdi_koren(Path::new(&p.priecinok)) else {
        return;
    };
    let _z = ZAMOK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut m) = nacitaj(&koren) else {
        return;
    };
    let Some(rel) = relativna(&koren, subor) else {
        return;
    };
    let mut zmena = false;
    for l in m.kapitoly.iter_mut().flat_map(|k| k.lekcie.iter_mut()) {
        if l.kluc == p.lekcia {
            l.subor = rel.clone();
            if l.dlzka.is_empty() {
                if let Some(d) = dlzka.filter(|d| *d >= 1.0) {
                    l.dlzka = cas(d);
                }
            }
            zmena = true;
        }
    }
    if zmena {
        let _ = uloz(&koren, &m);
    }
}

/// „5:07“, „1:02:09“.
fn cas(s: f64) -> String {
    let s = s.round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Dĺžka v sekundách z textu stránky: „12:34“, „1:02:03“, „5 min“, „1 h 5 min“, „45 s“.
pub fn sekundy(t: &str) -> Option<u64> {
    let t = t.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    if t.contains(':') {
        let casti: Vec<&str> = t.split(':').map(str::trim).collect();
        if casti.len() <= 3
            && casti
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|z| z.is_ascii_digit()))
        {
            return casti
                .iter()
                .try_fold(0u64, |a, c| Some(a * 60 + c.parse::<u64>().ok()?));
        }
    }
    // čísla s jednotkami
    let mut spolu = 0u64;
    let mut nasiel = false;
    let znaky: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < znaky.len() {
        if znaky[i].is_ascii_digit() {
            let zac = i;
            while i < znaky.len() && znaky[i].is_ascii_digit() {
                i += 1;
            }
            let n: u64 = znaky[zac..i].iter().collect::<String>().parse().ok()?;
            while i < znaky.len() && znaky[i] == ' ' {
                i += 1;
            }
            let zac = i;
            while i < znaky.len() && znaky[i].is_alphabetic() {
                i += 1;
            }
            let jednotka: String = znaky[zac..i].iter().collect();
            let nasobok = match jednotka.as_str() {
                "h" | "hod" | "hodina" | "hodiny" | "hodín" | "hr" | "hrs" | "hour" | "hours" => {
                    3600
                }
                "m" | "min" | "minúta" | "minúty" | "minút" | "mins" | "minute" | "minutes" => {
                    60
                }
                "s" | "sek" | "sekunda" | "sekundy" | "sekúnd" | "sec" | "secs" | "seconds" => 1,
                _ => continue,
            };
            spolu += n * nasobok;
            nasiel = true;
        } else {
            i += 1;
        }
    }
    nasiel.then_some(spolu)
}

/// Súbor lekcie na disku: zapísaný, inak hľadaný podľa názvu („03 Premenné.mp4“,
/// „03 Premenné (2).mkv“ – nie rozpracované časti ako „03 Premenné.mkv.f399.mp4“).
fn subor_lekcie(koren: &Path, k: &MKapitola, l: &MLekcia) -> Option<String> {
    if !l.subor.is_empty() && koren.join(&l.subor).is_file() {
        return Some(l.subor.clone());
    }
    let kmen = l.kluc.rsplit('/').next().unwrap_or(&l.kluc);
    let priecinok = if k.priecinok.is_empty() {
        koren.to_path_buf()
    } else {
        koren.join(&k.priecinok)
    };
    let zle = ["part", "kometa", "json", "tmp", "ytdl", "novy"];
    let mut najdene: Vec<String> = std::fs::read_dir(&priecinok)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|meno| {
            if meno == kmen {
                return true; // materiál s príponou v názve
            }
            let Some(zvysok) = meno.strip_prefix(kmen) else {
                return false;
            };
            let zvysok = match zvysok.strip_prefix(" (") {
                Some(z) => match z.split_once(')') {
                    Some((n, z)) if n.chars().all(|c| c.is_ascii_digit()) => z,
                    _ => return false,
                },
                None => zvysok,
            };
            zvysok
                .strip_prefix('.')
                .is_some_and(|p| !p.is_empty() && !p.contains('.') && !zle.contains(&p))
        })
        .collect();
    najdene.sort();
    let meno = najdene.into_iter().next()?;
    relativna(koren, &priecinok.join(meno))
}

/// Stránka prehľadu: šablóna s údajmi kurzu (čo je na disku, sa zistí teraz).
fn html(m: &Manifest, koren: &Path) -> String {
    let mut kapitoly: Vec<Value> = m
        .kapitoly
        .iter()
        .map(|k| {
            let lekcie: Vec<Value> = k
                .lekcie
                .iter()
                .map(|l| {
                    let subor = subor_lekcie(koren, k, l);
                    json!({
                        "kluc": l.kluc, "cislo": l.cislo, "nazov": l.nazov, "druh": l.druh,
                        "dlzka": l.dlzka, "sekund": sekundy(&l.dlzka),
                        "subor": subor.clone().unwrap_or_default(), "je": subor.is_some(),
                        "adresa": l.adresa,
                    })
                })
                .collect();
            json!({"cislo": k.cislo, "nazov": k.nazov, "lekcie": lekcie})
        })
        .collect();
    // ďalšie súbory v MATERIALY (stiahnuté v prehliadači kurzov, napr. ZIP z odkazu v .txt)
    let pouzite: std::collections::HashSet<String> = kapitoly
        .iter()
        .flat_map(|k| k["lekcie"].as_array().cloned().unwrap_or_default())
        .filter_map(|l| l["subor"].as_str().map(str::to_string))
        .collect();
    let zle = ["part", "tmp", "crdownload", "download", "novy"];
    let mut ine: Vec<String> = std::fs::read_dir(koren.join(super::kurzy::PRIECINOK_MATERIALOV))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let p = n
                .rsplit_once('.')
                .map(|x| x.1.to_ascii_lowercase())
                .unwrap_or_default();
            !n.starts_with('.') && !zle.contains(&p.as_str())
        })
        .map(|n| format!("{}/{n}", super::kurzy::PRIECINOK_MATERIALOV))
        .filter(|c| !pouzite.contains(c))
        .collect();
    ine.sort();
    if !ine.is_empty() {
        let cislo = m.kapitoly.iter().map(|k| k.cislo).max().unwrap_or(0) + 1;
        let lekcie: Vec<Value> = ine
            .iter()
            .enumerate()
            .map(|(i, c)| {
                json!({
                    "kluc": c, "cislo": i + 1, "nazov": c.rsplit('/').next().unwrap_or(c),
                    "druh": "material", "dlzka": "", "sekund": null, "subor": c, "je": true, "adresa": "",
                })
            })
            .collect();
        kapitoly.push(json!({"cislo": cislo, "nazov": "Materiály", "lekcie": lekcie}));
    }
    let data = json!({
        "nazov": m.nazov,
        "adresa": m.adresa,
        "kapitoly": kapitoly,
        "verzia": crate::VERZIA,
    });
    // „</script>“ v názve lekcie nesmie ukončiť blok s údajmi
    let data = data.to_string().replace('<', "\\u003c");
    let titul = m
        .nazov
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    SABLONA
        .replacen("__TITUL__", &titul, 1)
        .replacen("__UDAJE__", &data, 1)
}

#[cfg(test)]
mod skusky {
    use super::*;
    use crate::sluzba::kurzy::{plan, Kapitola, Lekcia};

    fn kurz() -> Kurz {
        let l = |n: &str, c: usize, d: &str| Lekcia {
            url: format!("https://kurzy.example.com/l/{c}"),
            nazov: n.into(),
            dlzka: d.into(),
            cislo: c,
            ..Default::default()
        };
        Kurz {
            nazov: "Excel pre pokročilých | Kurzy Example".into(),
            adresa: "https://kurzy.example.com/kurz/excel".into(),
            kapitoly: vec![
                Kapitola {
                    nazov: "Základy".into(),
                    lekcie: vec![l("Úvod", 1, "5:07"), l("Bunky </script>", 3, "12 min")],
                    cislo: 1,
                    ..Default::default()
                },
                Kapitola {
                    nazov: "Materiály".into(),
                    lekcie: vec![Lekcia {
                        url: "https://cdn.example.com/zosit.xlsx".into(),
                        nazov: "Zošit".into(),
                        material: true,
                        cislo: 1,
                        ..Default::default()
                    }],
                    cislo: 4,
                    ..Default::default()
                },
            ],
            kapitol: 4,
            ..Default::default()
        }
    }

    #[test]
    fn dlzky() {
        assert_eq!(sekundy("5:07"), Some(307));
        assert_eq!(sekundy("1:02:03"), Some(3723));
        assert_eq!(sekundy("12 min"), Some(720));
        assert_eq!(sekundy("1 h 5 min"), Some(3900));
        assert_eq!(sekundy("45 s"), Some(45));
        assert_eq!(sekundy("Lekcia"), None);
        assert_eq!(cas(307.4), "5:07");
        assert_eq!(cas(3723.0), "1:02:03");
    }

    #[test]
    fn prehlad_sa_doplna_a_najde_subory() {
        let d = std::env::temp_dir().join(format!("kometa-prehlad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let vystup = d.to_string_lossy().into_owned();
        let k = kurz();
        let (koren, miesta) = plan(&k, &vystup);
        assert_eq!(koren, d.join("KURZY").join("Excel pre pokročilých"));
        assert_eq!(miesta[1].kluc, "01 Základy/03 Bunky (-script)");
        zapis_kurz(&k, &koren, &miesta[..1], &[]).unwrap();
        // druhé sťahovanie: ďalšie lekcie sa doplnia, prvá ostane
        zapis_kurz(&k, &koren, &miesta, &[]).unwrap();
        let m = nacitaj(&koren).unwrap();
        assert_eq!(m.nazov, "Excel pre pokročilých");
        assert_eq!(m.kapitoly.len(), 2);
        assert_eq!(m.kapitoly[0].lekcie.len(), 2);
        assert_eq!(m.kapitoly[1].priecinok, "04 Materiály");
        assert_eq!(m.kapitoly[1].lekcie[0].druh, "material");
        // stiahnutá lekcia (iná prípona, než sa čakalo) a rozpracovaná časť
        std::fs::create_dir_all(koren.join("01 Základy")).unwrap();
        std::fs::write(koren.join("01 Základy/01 Úvod.mkv"), b"x").unwrap();
        std::fs::write(koren.join("01 Základy/01 Úvod.mkv.f399.mp4"), b"x").unwrap();
        let p: Polozka = serde_json::from_value(json!({
            "id": "1", "url": "https://x/1", "zdroj": "Kurz", "format": "video", "stav": "hotovo",
            "priecinok": koren.join("01 Základy"), "lekcia": "01 Základy/01 Úvod",
        }))
        .unwrap();
        po_stiahnuti(&p, &koren.join("01 Základy/01 Úvod.mkv"), Some(310.0));
        let m = nacitaj(&koren).unwrap();
        assert_eq!(m.kapitoly[0].lekcie[0].subor, "01 Základy/01 Úvod.mkv");
        assert_eq!(m.kapitoly[0].lekcie[0].dlzka, "5:07"); // zo stránky, nie z videa
        let html = std::fs::read_to_string(koren.join(STRANKA)).unwrap();
        assert!(html.contains("<title>Excel pre pokročilých</title>"));
        assert!(!html.contains("</script>\",") && html.contains("\\u003c/script>"));
        let udaje = html
            .split("type=\"application/json\">")
            .nth(1)
            .and_then(|x| x.split("</script>").next())
            .unwrap();
        let v: Value = serde_json::from_str(udaje).unwrap();
        assert_eq!(v["kapitoly"][0]["lekcie"][0]["je"], true);
        assert_eq!(v["kapitoly"][0]["lekcie"][0]["sekund"], 307);
        assert_eq!(v["kapitoly"][0]["lekcie"][1]["je"], false);
        // súbor nájdený podľa názvu, aj keď nie je zapísaný
        std::fs::create_dir_all(koren.join("04 Materiály")).unwrap();
        std::fs::write(koren.join("04 Materiály/01 Zošit.xlsx"), b"x").unwrap();
        let k2 = &m.kapitoly[1];
        assert_eq!(
            subor_lekcie(&koren, k2, &k2.lekcie[0]).as_deref(),
            Some("04 Materiály/01 Zošit.xlsx")
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
