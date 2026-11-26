//! Automatická aktualizácia Komety z vydaní na GitHube.
//!
//! 1. Po spustení (a potom raz za 12 hodín) sa opýta GitHubu na posledné vydanie.
//! 2. Ak je novšie, stiahne súbor pre tento systém a spôsob inštalácie (inštalátor Windows,
//!    AppImage, DEB, DMG alebo samostatný program) a overí ho súčtom SHA-256 zo zoznamu
//!    `SHA256SUMS.txt` toho istého vydania, ak ho GitHub uvádza, aj jeho vlastným odtlačkom.
//!    Neoverený súbor sa nikdy nespustí.
//! 3. Okno ukáže, že je pripravená nová verzia – nainštalovať hneď, alebo pri ďalšom spustení.
//!    Pri ďalšom spustení sa nainštaluje sama, ale len raz: ak sa inštalácia nedokončí, neskúša
//!    sa dookola a okno to povie.
//!
//! Inštalátor beží viditeľne (Windows s ukazovateľom priebehu, bez otázok), nič sa nespúšťa skryto.

use super::Vnutro;
use crate::chyba::{Chyba, Vysledok};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const ADRESA: &str = "https://api.github.com/repos/tarzanislav/kometa/releases/latest";

/// Ako sa Kometa na tomto počítači inštaluje (a teda aký súbor z vydania potrebuje).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Balik {
    /// Windows: inštalátor `…-windows-x64-setup.exe`
    Instalator,
    /// Linux: AppImage – nahradí sa súbor, z ktorého Kometa beží
    Appimage,
    /// Linux: balík DEB (inštalácia si vypýta heslo správcu)
    Deb,
    /// macOS: obraz DMG – aplikácia sa v ňom vymení
    Dmg,
    /// samostatný program bez okna (`kometa`) – vymení sa súbor programu
    Program,
    /// tu sa nedá inštalovať samo (iný procesor, vývojová verzia) – len oznam a odkaz
    #[default]
    Rucne,
}

impl Balik {
    /// Spôsob inštalácie podľa okna („aplikacia“ = Tauri, inak samostatný program) a systému.
    pub fn zisti(okno: &str) -> Balik {
        // skúška: spôsob sa dá určiť zvonka (inak by testovací program z cargo bol „vývojový“)
        if std::env::var_os("KOMETA_AKTUALIZACIE_ZAKLAD").is_some() {
            match std::env::var("KOMETA_AKTUALIZACIE_BALIK").as_deref() {
                Ok("instalator") => return Balik::Instalator,
                Ok("program") => return Balik::Program,
                _ => {}
            }
        }
        let x64 = cfg!(target_arch = "x86_64");
        let exe = std::env::current_exe().unwrap_or_default();
        let s = exe.to_string_lossy().replace('\\', "/");
        if s.contains("/target/debug/") || s.contains("/target/release/") {
            return Balik::Rucne; // vývojová verzia z cargo – nič sa nevymieňa
        }
        if okno == "aplikacia" {
            if cfg!(windows) && x64 {
                Balik::Instalator
            } else if cfg!(target_os = "macos") {
                Balik::Dmg
            } else if cfg!(target_os = "linux") && x64 {
                if std::env::var_os("APPIMAGE").is_some() {
                    Balik::Appimage
                } else {
                    Balik::Deb
                }
            } else {
                Balik::Rucne
            }
        } else if (cfg!(windows) || cfg!(target_os = "linux")) && x64 || cfg!(target_os = "macos") {
            Balik::Program
        } else {
            Balik::Rucne
        }
    }

    /// Názov súboru vo vydaní verzie `v`.
    pub fn subor(self, v: &str) -> Option<String> {
        Some(match self {
            Balik::Instalator => format!("Kometa-{v}-windows-x64-setup.exe"),
            Balik::Appimage => format!("Kometa-{v}-linux-x86_64.AppImage"),
            Balik::Deb => format!("Kometa-{v}-linux-amd64.deb"),
            Balik::Dmg => format!("Kometa-{v}-macos.dmg"),
            Balik::Program if cfg!(windows) => format!("kometa-{v}-windows-x64-noGUI.exe"),
            Balik::Program if cfg!(target_os = "macos") => format!("kometa-{v}-macos-noGUI"),
            Balik::Program => format!("kometa-{v}-linux-x86_64-noGUI"),
            Balik::Rucne => return None,
        })
    }
}

/// Čo o aktualizácii vidí okno.
#[derive(Clone, Debug, Default, Serialize)]
pub struct StavAktualizacie {
    pub verzia: String,
    pub balik: Balik,
    /// najnovšia verzia na GitHube (ak je novšia ako táto)
    pub najnovsia: Option<String>,
    /// stránka vydania (čo je nové)
    pub stranka: Option<String>,
    pub stahuje: bool,
    /// stiahnutá a overená – dá sa inštalovať
    pub pripravena: bool,
    /// nainštaluje sa sama pri ďalšom spustení
    pub pri_starte: bool,
    pub instaluje: bool,
    /// samostatný program: nová verzia je na mieste, platí od ďalšieho spustenia
    pub nainstalovana: bool,
    pub kontrola: Option<String>,
    pub chyba: Option<String>,
}

/// Stiahnutá aktualizácia čakajúca na inštaláciu (aktualizacia/caka.json).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Caka {
    verzia: String,
    subor: String,
    sha256: String,
    pri_starte: bool,
    pokusy: u32,
}

/// Je verzia `a` novšia ako `b`? (čísla oddelené bodkami, „v“ na začiatku sa ignoruje)
pub fn novsia(a: &str, b: &str) -> bool {
    let cisla = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split(['.', '-', '+'])
            .map_while(|c| c.parse().ok())
            .collect()
    };
    let (x, y) = (cisla(a), cisla(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    false
}

fn skuska() -> bool {
    std::env::var("KOMETA_AKTUALIZACIE_SKUSKA").is_ok_and(|x| x == "1")
}

fn adresa() -> String {
    std::env::var("KOMETA_AKTUALIZACIE_ZAKLAD").unwrap_or_else(|_| ADRESA.into())
}

fn priecinok(v: &Vnutro) -> PathBuf {
    v.priecinok.join("aktualizacia")
}

fn nacitaj_caka(v: &Vnutro) -> Option<Caka> {
    let t = std::fs::read_to_string(priecinok(v).join("caka.json")).ok()?;
    serde_json::from_str(&t).ok()
}

fn uloz_caka(v: &Vnutro, c: &Caka) -> Vysledok<()> {
    let d = priecinok(v);
    std::fs::create_dir_all(&d)?;
    let t = serde_json::to_string_pretty(c).map_err(|e| Chyba::Ine(e.to_string()))?;
    std::fs::write(d.join("caka.json.tmp"), t)?;
    std::fs::rename(d.join("caka.json.tmp"), d.join("caka.json"))?;
    Ok(())
}

fn sha256_suboru(p: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(hex(&h.finalize()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Súčet SHA-256 súboru `meno` zo zoznamu SHA256SUMS.txt („súčet  názov“ na riadok).
pub fn sucet_zo_zoznamu(zoznam: &str, meno: &str) -> Option<String> {
    zoznam.lines().find_map(|r| {
        let mut casti = r.split_whitespace();
        let (sucet, subor) = (casti.next()?, casti.next()?);
        let subor = subor.trim_start_matches('*');
        (subor == meno && sucet.len() == 64 && sucet.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| sucet.to_ascii_lowercase())
    })
}

fn nedokoncena(verzia: &str) -> String {
    format!(
        "Inštalácia verzie {verzia} sa minule nedokončila. Skús „Nainštalovať teraz“, alebo inštalačku stiahni z GitHubu."
    )
}

fn nastav(v: &Vnutro, f: impl FnOnce(&mut StavAktualizacie)) {
    f(&mut v.aktualizacia.lock().unwrap());
}

/// Pri spustení: dokončenú aktualizáciu uprace, čakajúcu nainštaluje (raz), potom kontroluje.
pub fn spusti(v: &Arc<Vnutro>) {
    nastav(v, |s| {
        s.verzia = v.verzia.to_string();
        s.balik = Balik::zisti(v.okno);
    });
    // po výmene samostatného programu ostane jeho stará verzia vedľa – už netreba
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("stara"));
    }
    if let Some(c) = nacitaj_caka(v) {
        let subor = priecinok(v).join(&c.subor);
        if !novsia(&c.verzia, v.verzia) {
            let _ = std::fs::remove_dir_all(priecinok(v));
            if c.verzia == v.verzia {
                v.zapis(format!("✔ Kometa je aktualizovaná na verziu {}.", v.verzia));
            }
        } else if sha256_suboru(&subor).as_deref() != Some(c.sha256.as_str()) {
            let _ = std::fs::remove_dir_all(priecinok(v)); // poškodený alebo zmenený – stiahne sa znova
        } else {
            nastav(v, |s| {
                s.najnovsia = Some(c.verzia.clone());
                s.pripravena = true;
                s.pri_starte = c.pri_starte;
                if c.pokusy > 0 {
                    s.chyba = Some(nedokoncena(&c.verzia));
                }
            });
            if c.pri_starte && c.pokusy == 0 {
                let v2 = v.clone();
                v.rt_spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    v2.zapis(format!(
                        "⬆ Inštalujem Kometu {} (pripravenú minule)…",
                        c.verzia
                    ));
                    if let Err(e) = instaluj(&v2) {
                        v2.zapis(format!("Aktualizácia: {e}"));
                    }
                });
                return;
            }
        }
    }
    let v2 = v.clone();
    v.rt_spawn(async move {
        let prvy = if std::env::var_os("KOMETA_AKTUALIZACIE_ZAKLAD").is_some() {
            Duration::from_millis(300)
        } else {
            Duration::from_secs(8)
        };
        tokio::time::sleep(prvy).await;
        loop {
            let zapnute = v2.nastavenia.lock().unwrap().ano("aktualizacie", true);
            if zapnute {
                let _ = skontroluj(&v2).await;
            }
            tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
        }
    });
}

/// Opýta sa GitHubu na posledné vydanie; novšie stiahne a overí. Pri vypnutej automatickej
/// aktualizácii (kontrola na žiadosť z okna) sa pri ďalšom spustení nenainštaluje sama.
pub async fn skontroluj(v: &Arc<Vnutro>) -> Value {
    {
        let s = v.aktualizacia.lock().unwrap();
        if s.stahuje || s.instaluje {
            return json!(*s);
        }
    }
    nastav(v, |s| {
        s.stahuje = true;
        s.chyba = None;
    });
    let r = skontroluj_vnutri(v).await;
    nastav(v, |s| {
        s.stahuje = false;
        s.kontrola = Some(cas_teraz());
        if let Err(e) = &r {
            s.chyba = Some(format!("Aktualizáciu sa nepodarilo overiť: {e}"));
        }
    });
    json!(*v.aktualizacia.lock().unwrap())
}

fn cas_teraz() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    s.to_string()
}

async fn skontroluj_vnutri(v: &Arc<Vnutro>) -> Vysledok<()> {
    let k = crate::klient::klient(&crate::klient::Nastavenia::default())?;
    let text = k
        .get(adresa())
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let info: Value =
        serde_json::from_str(&text).map_err(|e| Chyba::Format(format!("odpoveď GitHubu: {e}")))?;
    let tag = info.get("tag_name").and_then(Value::as_str).unwrap_or("");
    let verzia = tag.trim_start_matches(['v', 'V']).to_string();
    if verzia.is_empty() || !novsia(&verzia, v.verzia) {
        nastav(v, |s| s.najnovsia = None);
        return Ok(());
    }
    let stranka = info
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::to_string);
    nastav(v, |s| {
        s.najnovsia = Some(verzia.clone());
        s.stranka = stranka.clone();
    });
    let balik = v.aktualizacia.lock().unwrap().balik;
    let Some(meno) = balik.subor(&verzia) else {
        return Ok(()); // len oznam s odkazom na stránku vydania
    };
    if let Some(c) = nacitaj_caka(v) {
        if c.verzia == verzia
            && sha256_suboru(&priecinok(v).join(&c.subor)).as_deref() == Some(&c.sha256)
        {
            nastav(v, |s| {
                s.pripravena = true;
                s.pri_starte = c.pri_starte;
                if c.pokusy > 0 {
                    s.chyba = Some(nedokoncena(&c.verzia));
                }
            });
            return Ok(());
        }
    }
    let assets = info
        .get("assets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let najdi = |m: &str| {
        assets
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(m))
    };
    let subor = najdi(&meno).ok_or_else(|| Chyba::Ine(format!("vo vydaní chýba {meno}")))?;
    let zoznam = najdi("SHA256SUMS.txt")
        .ok_or_else(|| Chyba::Ine("vo vydaní chýba SHA256SUMS.txt".into()))?;
    let url = |a: &Value| {
        a.get("browser_download_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let sucty = k
        .get(url(zoznam))
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if !sucty.lines().next().unwrap_or("").contains(&verzia) {
        return Err(Chyba::Ine("zoznam súčtov nepatrí k tejto verzii".into()));
    }
    let ocakavany = sucet_zo_zoznamu(&sucty, &meno)
        .ok_or_else(|| Chyba::Ine(format!("v SHA256SUMS.txt chýba {meno}")))?;
    // GitHub uvádza aj vlastný odtlačok nahraného súboru – ak ho má, musí sedieť tiež
    if let Some(d) = subor.get("digest").and_then(Value::as_str) {
        if let Some(h) = d.strip_prefix("sha256:") {
            if !h.eq_ignore_ascii_case(&ocakavany) {
                return Err(Chyba::Ine("súčty zo zoznamu a z GitHubu sa líšia".into()));
            }
        }
    }
    let velkost = subor.get("size").and_then(Value::as_u64).unwrap_or(0);
    if velkost > 400 << 20 {
        return Err(Chyba::Ine("súbor aktualizácie je nezvyčajne veľký".into()));
    }
    let d = priecinok(v);
    let _ = tokio::fs::remove_dir_all(&d).await;
    tokio::fs::create_dir_all(&d).await?;
    let docasny = d.join(format!("{meno}.part"));
    let odp = k.get(url(subor)).send().await?.error_for_status()?;
    let mut f = tokio::fs::File::create(&docasny).await?;
    let mut h = Sha256::new();
    let mut spolu = 0u64;
    let mut prud = odp.bytes_stream();
    while let Some(kus) = prud.next().await {
        let kus = kus?;
        spolu += kus.len() as u64;
        if spolu > 400 << 20 {
            return Err(Chyba::Ine("súbor aktualizácie je nezvyčajne veľký".into()));
        }
        h.update(&kus);
        f.write_all(&kus).await?;
    }
    f.flush().await?;
    drop(f);
    if hex(&h.finalize()) != ocakavany {
        let _ = tokio::fs::remove_file(&docasny).await;
        return Err(Chyba::Ine(
            "stiahnutý súbor nesedí so súčtom SHA-256 – nepoužijem ho".into(),
        ));
    }
    tokio::fs::rename(&docasny, d.join(&meno)).await?;
    let auto = v.nastavenia.lock().unwrap().ano("aktualizacie", true);
    let c = Caka {
        verzia: verzia.clone(),
        subor: meno,
        sha256: ocakavany,
        // automatická aktualizácia zapnutá → nainštaluje sa pri ďalšom spustení sama
        pri_starte: auto,
        pokusy: 0,
    };
    uloz_caka(v, &c)?;
    v.zapis(format!(
        "⬆ Pripravená nová verzia Komety {verzia} – nainštaluješ ju hneď, alebo sa nainštaluje pri ďalšom spustení."
    ));
    v.prostredie.oznam(
        "Nová verzia Komety",
        &format!("Kometa {verzia} je pripravená na inštaláciu."),
    );
    nastav(v, |s| {
        s.pripravena = true;
        s.pri_starte = c.pri_starte;
    });
    Ok(())
}

/// Odloží inštaláciu na ďalšie spustenie (aj keď je automatická aktualizácia vypnutá).
pub fn pri_starte(v: &Vnutro) -> Value {
    if let Some(mut c) = nacitaj_caka(v) {
        c.pri_starte = true;
        c.pokusy = 0;
        let _ = uloz_caka(v, &c);
        nastav(v, |s| {
            s.pri_starte = true;
            s.chyba = None;
        });
    }
    json!(*v.aktualizacia.lock().unwrap())
}

/// Nainštaluje pripravenú verziu. Aplikácia sa pri tom zavrie a po inštalácii sa spustí znova.
pub fn instaluj(v: &Arc<Vnutro>) -> Vysledok<Value> {
    let mut c = nacitaj_caka(v)
        .ok_or_else(|| Chyba::Ine("Nie je pripravená žiadna aktualizácia.".into()))?;
    let subor = priecinok(v).join(&c.subor);
    if sha256_suboru(&subor).as_deref() != Some(c.sha256.as_str()) {
        let _ = std::fs::remove_dir_all(priecinok(v));
        nastav(v, |s| s.pripravena = false);
        return Err(Chyba::Ine(
            "Súbor aktualizácie sa zmenil alebo poškodil – stiahne sa znova.".into(),
        ));
    }
    c.pokusy += 1;
    uloz_caka(v, &c)?;
    let balik = v.aktualizacia.lock().unwrap().balik;
    nastav(v, |s| {
        s.instaluje = true;
        s.chyba = None;
    });
    let vysledok = spusti_instalaciu(v, balik, &subor, &c.verzia);
    if let Err(e) = &vysledok {
        nastav(v, |s| {
            s.instaluje = false;
            s.chyba = Some(format!("Inštalácia sa nespustila: {e}"));
        });
    }
    let zavriet = vysledok?;
    if zavriet && !skuska() {
        // zavrieť aplikáciu, aby inštalátor mohol vymeniť jej súbory
        let v2 = v.clone();
        v.rt_spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            v2.prostredie.ukonci();
        });
    }
    Ok(json!(*v.aktualizacia.lock().unwrap()))
}

/// Spustí inštaláciu podľa balíka; vráti, či sa má Kometa zavrieť.
fn spusti_instalaciu(v: &Vnutro, balik: Balik, subor: &Path, verzia: &str) -> Vysledok<bool> {
    let exe = std::env::current_exe()?;
    if skuska() {
        // skúška: nič sa nespúšťa ani nevymieňa, len sa zapíše, čo by sa stalo
        std::fs::write(
            priecinok(v).join("spustene.txt"),
            format!("{balik:?} {} {verzia}", subor.display()),
        )?;
        return Ok(true);
    }
    match balik {
        Balik::Instalator => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // inštalátor s priebehom a bez otázok (/P), po skončení znova Kometa
                let prikaz = format!(
                    "start \"\" /wait \"{}\" /P & start \"\" \"{}\"",
                    subor.display(),
                    exe.display()
                );
                std::process::Command::new("cmd")
                    .arg("/C")
                    .raw_arg(prikaz)
                    .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                    .spawn()?;
                Ok(true)
            }
            #[cfg(not(windows))]
            Err(Chyba::Nepodporovane("inštalátor Windows".into()))
        }
        Balik::Appimage => {
            let ciel = PathBuf::from(
                std::env::var_os("APPIMAGE").ok_or_else(|| Chyba::Ine("chýba APPIMAGE".into()))?,
            );
            let nova = ciel.with_extension("nova");
            std::fs::copy(subor, &nova)?;
            nastav_spustitelny(&nova)?;
            std::fs::rename(&nova, &ciel)?;
            spusti_po_skonceni(&format!("exec \"{}\"", ciel.display()))?;
            Ok(true)
        }
        Balik::Deb => {
            spusti_po_skonceni(&format!(
                "pkexec apt-get install -y \"{}\" || xdg-open \"{}\"; exec \"{}\"",
                subor.display(),
                subor.display(),
                exe.display()
            ))?;
            Ok(true)
        }
        Balik::Dmg => {
            let app = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or_else(|| Chyba::Ine("Kometa nebeží z balíka .app".into()))?
                .to_path_buf();
            spusti_po_skonceni(&format!(
                "m=$(mktemp -d) && hdiutil attach -nobrowse -readonly -mountpoint \"$m\" \"{d}\" >/dev/null \
                 && ditto \"$m/Kometa.app\" \"{a}.nova\" && rm -rf \"{a}\" && mv \"{a}.nova\" \"{a}\"; \
                 hdiutil detach \"$m\" >/dev/null 2>&1; open \"{a}\"",
                d = subor.display(),
                a = app.display()
            ))?;
            Ok(true)
        }
        Balik::Program => {
            // samostatný program: bežiaci súbor sa dá premenovať, nová verzia ide na jeho miesto
            let stara = exe.with_extension("stara");
            let _ = std::fs::remove_file(&stara);
            std::fs::rename(&exe, &stara)?;
            if let Err(e) = std::fs::copy(subor, &exe)
                .and_then(|_| nastav_spustitelny(&exe).map_err(std::io::Error::other))
            {
                let _ = std::fs::rename(&stara, &exe);
                return Err(e.into());
            }
            nastav(v, |s| {
                s.instaluje = false;
                s.nainstalovana = true;
            });
            v.zapis(format!(
                "✔ Kometa {verzia} je nainštalovaná – platí od ďalšieho spustenia programu."
            ));
            Ok(false)
        }
        Balik::Rucne => Err(Chyba::Nepodporovane(
            "automatická inštalácia na tomto počítači".into(),
        )),
    }
}

fn nastav_spustitelny(p: &Path) -> Vysledok<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = p;
    Ok(())
}

/// Spustí príkaz shellu o 2 s neskôr – až po zatvorení Komety (inak by nová Kometa len ukázala
/// okno tej starej, ktorá ešte beží).
fn spusti_po_skonceni(prikaz: &str) -> Vysledok<()> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("sleep 2; {prikaz}"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn porovnanie_verzii() {
        assert!(novsia("1.10.0", "1.9.0"));
        assert!(novsia("v2.0", "1.99.9"));
        assert!(!novsia("1.9.0", "1.9.0"));
        assert!(!novsia("1.8.9", "1.9"));
        assert!(novsia("1.9.1", "1.9"));
    }

    #[test]
    fn sucet_zo_zoznamu_vydania() {
        let z = "# Kometa 1.10.0\n\
                 aac36fdaa3b5f667316a833d548a1b6d5813b1653631dd438c723aa39157d363  Kometa-1.10.0-windows-x64-setup.exe\n\
                 4b7874c24f959b1f90acfb96ef01de87366daa2327ad48364e79ba10e9b60153 *kometa-1.10.0-windows-x64-noGUI.exe\n";
        assert_eq!(
            sucet_zo_zoznamu(z, "kometa-1.10.0-windows-x64-noGUI.exe").as_deref(),
            Some("4b7874c24f959b1f90acfb96ef01de87366daa2327ad48364e79ba10e9b60153")
        );
        assert_eq!(sucet_zo_zoznamu(z, "Kometa-1.10.0.zip"), None);
        assert_eq!(
            Balik::Instalator.subor("1.10.0").as_deref(),
            Some("Kometa-1.10.0-windows-x64-setup.exe")
        );
    }
}
