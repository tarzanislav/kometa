//! Rozbaľovanie archívov po stiahnutí: ZIP (aj šifrovaný AES/ZipCrypto), 7z (aj AES) a RAR
//! (knižnica UnRAR od RARLAB – vie len rozbaľovať).
//!
//! Rozdelené archívy: `.partN.rar` a `.rNN` (UnRAR si ďalšie časti nájde sám), `.7z.001`
//! a `.zip.001` (časti sa čítajú ako jeden súbor). Rozbaľuje sa, až keď sú stiahnuté všetky časti.
//!
//! Bezpečnosť: nič sa nezapíše mimo cieľového priečinka – cesty s `..`, absolútne cesty
//! a symbolické odkazy z archívu sa preskočia. Rozbaľuje sa najprv do dočasného priečinka,
//! ktorý sa po úspechu premenuje; pri chybe sa zmaže (stiahnutý archív ostane).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Druh {
    Zip,
    SedemZ,
    Rar,
}

/// Rozpoznaný súbor archívu: druh, meno skupiny (bez prípony a čísla časti) a číslo časti (1 = prvá).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cast {
    pub druh: Druh,
    pub meno: String,
    pub cislo: u32,
}

fn cislo_konca(s: &str) -> Option<u32> {
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// Rozpozná archív podľa názvu súboru.
pub fn rozpoznaj(cesta: &Path) -> Option<Cast> {
    let n = cesta.file_name()?.to_string_lossy().into_owned();
    let m = n.to_lowercase();
    let cast = |druh, dlzka_konca: usize, cislo| {
        Some(Cast {
            druh,
            meno: n[..n.len() - dlzka_konca].to_string(),
            cislo,
        })
    };
    // meno.partN.rar
    if let Some(z) = m.strip_suffix(".rar") {
        if let Some(i) = z.rfind(".part") {
            if let Some(c) = cislo_konca(&z[i + 5..]) {
                return cast(Druh::Rar, m.len() - i, c.max(1));
            }
        }
        return cast(Druh::Rar, 4, 1);
    }
    let (zaklad, koniec) = m.rsplit_once('.')?;
    // meno.r00, meno.r01 … (starý spôsob delenia RAR: .rar je prvá časť)
    if koniec.len() == 3 && koniec.starts_with('r') {
        if let Some(c) = cislo_konca(&koniec[1..]) {
            return cast(Druh::Rar, 4, c + 2);
        }
    }
    match koniec {
        "zip" => return cast(Druh::Zip, 4, 1),
        "7z" => return cast(Druh::SedemZ, 3, 1),
        _ => {}
    }
    // meno.7z.001, meno.zip.001
    if let Some(c) = cislo_konca(koniec).filter(|_| koniec.len() == 3) {
        if zaklad.ends_with(".7z") {
            return cast(Druh::SedemZ, 7, c);
        }
        if zaklad.ends_with(".zip") {
            return cast(Druh::Zip, 8, c);
        }
    }
    None
}

/// Je to časť rozdeleného archívu (`.partN.rar`, `.rNN`, `.7z.001`, `.zip.001`)?
pub fn je_delene(cesta: &Path) -> bool {
    let n = cesta
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    rozpoznaj(cesta).is_some()
        && (n.contains(".part") && n.ends_with(".rar")
            || n.rsplit_once('.')
                .is_some_and(|(_, k)| k.len() == 3 && (k[1..].chars().all(|c| c.is_ascii_digit()))))
}

/// Časti rozdeleného archívu v tom istom priečinku, zoradené (pri RAR stačí prvá časť).
pub fn casti(prva_alebo_hocijaka: &Path) -> Vec<PathBuf> {
    let Some(c) = rozpoznaj(prva_alebo_hocijaka) else {
        return vec![];
    };
    let dir = prva_alebo_hocijaka.parent().unwrap_or(Path::new("."));
    let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let x = rozpoznaj(&p)?;
            (x.druh == c.druh && x.meno == c.meno && p.is_file()).then_some((x.cislo, p))
        })
        .collect();
    v.sort();
    v.dedup_by_key(|x| x.0);
    v.into_iter().map(|x| x.1).collect()
}

#[derive(Debug)]
pub enum ChybaRozbalenia {
    /// Archív je zaheslovaný: `false` = heslo chýba, `true` = zadané heslo nesedí.
    Heslo(bool),
    Ina(String),
}

impl std::fmt::Display for ChybaRozbalenia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChybaRozbalenia::Heslo(false) => f.write_str("Archív je chránený heslom – zadaj ho."),
            ChybaRozbalenia::Heslo(true) => f.write_str("Heslo k archívu nesedí – skús ho znova."),
            ChybaRozbalenia::Ina(t) => f.write_str(t),
        }
    }
}

impl From<io::Error> for ChybaRozbalenia {
    fn from(e: io::Error) -> Self {
        ChybaRozbalenia::Ina(format!("Chyba disku pri rozbaľovaní: {e}"))
    }
}

/// Bezpečná relatívna cesta z archívu (len bežné časti, žiadne `..` ani koreň).
pub fn bezpecna(cesta: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in cesta.components() {
        match c {
            Component::Normal(x) => {
                let s = x.to_string_lossy();
                // vo Windows by „C:“ či „\\“ v mene mohli ujsť z priečinka
                if s.contains([':', '\\']) && cfg!(windows) {
                    return None;
                }
                out.push(x)
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Viac súborov čítaných ako jeden (časti `.7z.001`, `.7z.002` …).
pub struct Spojene {
    subory: Vec<(File, u64)>,
    pozicia: u64,
    dlzka: u64,
}

impl Spojene {
    pub fn otvor(casti: &[PathBuf]) -> io::Result<Self> {
        let mut subory = vec![];
        let mut dlzka = 0;
        for c in casti {
            let f = File::open(c)?;
            let d = f.metadata()?.len();
            dlzka += d;
            subory.push((f, d));
        }
        Ok(Spojene {
            subory,
            pozicia: 0,
            dlzka,
        })
    }
}

impl Read for Spojene {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut zac = 0;
        for (f, d) in &mut self.subory {
            if self.pozicia < zac + *d {
                f.seek(SeekFrom::Start(self.pozicia - zac))?;
                let max = ((zac + *d - self.pozicia) as usize).min(buf.len());
                let n = f.read(&mut buf[..max])?;
                self.pozicia += n as u64;
                return Ok(n);
            }
            zac += *d;
        }
        Ok(0)
    }
}

impl Seek for Spojene {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let nova = match pos {
            SeekFrom::Start(x) => x as i128,
            SeekFrom::End(x) => self.dlzka as i128 + x as i128,
            SeekFrom::Current(x) => self.pozicia as i128 + x as i128,
        };
        if nova < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pred začiatkom",
            ));
        }
        self.pozicia = nova as u64;
        Ok(self.pozicia)
    }
}

/// Zápis s priebehom (spolu zapísané bajty).
struct SPriebehom<'a, W: Write> {
    w: W,
    hotovo: &'a mut u64,
    priebeh: &'a dyn Fn(u64),
}

impl<W: Write> Write for SPriebehom<'_, W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.w.write(b)?;
        *self.hotovo += n as u64;
        (self.priebeh)(*self.hotovo);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

fn vytvor_subor(koren: &Path, rel: &Path) -> io::Result<File> {
    let c = koren.join(rel);
    if let Some(r) = c.parent() {
        std::fs::create_dir_all(r)?;
    }
    File::create(c)
}

fn zip(
    casti: &[PathBuf],
    koren: &Path,
    heslo: &str,
    priebeh: &dyn Fn(u64),
) -> Result<usize, ChybaRozbalenia> {
    let r = Spojene::otvor(casti)?;
    let mut a = zip::ZipArchive::new(r)
        .map_err(|e| ChybaRozbalenia::Ina(format!("ZIP sa nedá otvoriť: {e}")))?;
    let mut hotovo = 0u64;
    let mut suborov = 0;
    for i in 0..a.len() {
        let mut f = if heslo.is_empty() {
            a.by_index(i)
        } else {
            a.by_index_decrypt(i, heslo.as_bytes())
        }
        .map_err(|e| match e {
            zip::result::ZipError::UnsupportedArchive(t)
                if t == zip::result::ZipError::PASSWORD_REQUIRED =>
            {
                ChybaRozbalenia::Heslo(false)
            }
            zip::result::ZipError::InvalidPassword => ChybaRozbalenia::Heslo(true),
            e => ChybaRozbalenia::Ina(format!("ZIP: {e}")),
        })?;
        if f.is_symlink() {
            continue;
        }
        let Some(rel) = f.enclosed_name().and_then(|n| bezpecna(&n)) else {
            continue;
        };
        if f.is_dir() {
            std::fs::create_dir_all(koren.join(&rel))?;
            continue;
        }
        let sifrovany = f.encrypted();
        let mut w = SPriebehom {
            w: io::BufWriter::new(vytvor_subor(koren, &rel)?),
            hotovo: &mut hotovo,
            priebeh,
        };
        io::copy(&mut f, &mut w).map_err(|e| {
            if sifrovany {
                ChybaRozbalenia::Heslo(true) // pri ZipCrypto sa zlé heslo prejaví až kontrolným súčtom
            } else {
                ChybaRozbalenia::Ina(format!("ZIP je poškodený: {e}"))
            }
        })?;
        w.flush()?;
        suborov += 1;
    }
    Ok(suborov)
}

fn sedem_z(
    casti: &[PathBuf],
    koren: &Path,
    heslo: &str,
    priebeh: &dyn Fn(u64),
) -> Result<usize, ChybaRozbalenia> {
    use sevenz_rust2::{ArchiveReader, Error, Password};
    let mapuj = |e: Error, heslo: &str| match e {
        Error::PasswordRequired => ChybaRozbalenia::Heslo(false),
        Error::MaybeBadPassword(_) => ChybaRozbalenia::Heslo(!heslo.is_empty()),
        e => ChybaRozbalenia::Ina(format!("7z: {e}")),
    };
    let pw = if heslo.is_empty() {
        Password::empty()
    } else {
        Password::new(heslo)
    };
    let mut a = ArchiveReader::new(Spojene::otvor(casti)?, pw).map_err(|e| mapuj(e, heslo))?;
    let mut hotovo = 0u64;
    let mut suborov = 0;
    let mut chyba_disku: Option<io::Error> = None;
    a.for_each_entries(|e, r| {
        let Some(rel) = bezpecna(Path::new(e.name())) else {
            return Ok(true);
        };
        if e.is_directory() {
            if let Err(x) = std::fs::create_dir_all(koren.join(&rel)) {
                chyba_disku = Some(x);
                return Ok(false);
            }
            return Ok(true);
        }
        let f = match vytvor_subor(koren, &rel) {
            Ok(f) => f,
            Err(x) => {
                chyba_disku = Some(x);
                return Ok(false);
            }
        };
        let mut w = SPriebehom {
            w: io::BufWriter::new(f),
            hotovo: &mut hotovo,
            priebeh,
        };
        if e.has_stream() {
            io::copy(r, &mut w).map_err(Error::from)?;
        }
        w.flush().map_err(Error::from)?;
        suborov += 1;
        Ok(true)
    })
    .map_err(|e| mapuj(e, heslo))?;
    if let Some(e) = chyba_disku {
        return Err(e.into());
    }
    Ok(suborov)
}

fn rar(
    prva: &Path,
    koren: &Path,
    heslo: &str,
    priebeh: &dyn Fn(u64),
) -> Result<usize, ChybaRozbalenia> {
    use unrar::error::Code;
    let mapuj = |e: unrar::error::UnrarError| match e.code {
        Code::MissingPassword => ChybaRozbalenia::Heslo(false),
        Code::BadPassword => ChybaRozbalenia::Heslo(true),
        Code::BadData if !heslo.is_empty() => ChybaRozbalenia::Heslo(true),
        Code::EOpen => ChybaRozbalenia::Ina(
            "Chýba časť rozdeleného archívu RAR – stiahni všetky časti.".into(),
        ),
        _ => ChybaRozbalenia::Ina(format!("RAR: {e}")),
    };
    let a = if heslo.is_empty() {
        unrar::Archive::new(prva)
    } else {
        unrar::Archive::with_password(prva, heslo)
    };
    let mut a = a.as_first_part().open_for_processing().map_err(mapuj)?;
    let mut hotovo = 0u64;
    let mut suborov = 0;
    while let Some(h) = a.read_header().map_err(mapuj)? {
        let e = h.entry();
        let rel = bezpecna(&e.filename);
        let velkost = e.unpacked_size;
        a = match rel {
            Some(rel) if e.is_directory() => {
                std::fs::create_dir_all(koren.join(&rel))?;
                h.skip().map_err(mapuj)?
            }
            Some(rel) if e.is_file() => {
                let c = koren.join(&rel);
                if let Some(r) = c.parent() {
                    std::fs::create_dir_all(r)?;
                }
                let a = h.extract_to(&c).map_err(mapuj)?;
                hotovo += velkost;
                priebeh(hotovo);
                suborov += 1;
                a
            }
            _ => h.skip().map_err(mapuj)?,
        };
    }
    Ok(suborov)
}

/// Celková veľkosť po rozbalení (na priebeh); 0, ak sa nedá zistiť bez hesla.
pub fn velkost_obsahu(druh: Druh, casti: &[PathBuf], heslo: &str) -> u64 {
    match druh {
        Druh::Zip => Spojene::otvor(casti)
            .ok()
            .and_then(|r| zip::ZipArchive::new(r).ok())
            .map(|mut a| {
                (0..a.len())
                    .filter_map(|i| a.by_index_raw(i).ok().map(|f| f.size()))
                    .sum()
            })
            .unwrap_or(0),
        Druh::SedemZ => {
            let pw = if heslo.is_empty() {
                sevenz_rust2::Password::empty()
            } else {
                sevenz_rust2::Password::new(heslo)
            };
            Spojene::otvor(casti)
                .ok()
                .and_then(|r| sevenz_rust2::ArchiveReader::new(r, pw).ok())
                .map(|a| a.archive().files.iter().map(|f| f.size()).sum())
                .unwrap_or(0)
        }
        Druh::Rar => {
            let a = if heslo.is_empty() {
                unrar::Archive::new(&casti[0])
            } else {
                unrar::Archive::with_password(&casti[0], heslo)
            };
            a.as_first_part()
                .open_for_listing()
                .map(|l| {
                    l.flatten()
                        .filter(|e| e.is_file())
                        .map(|e| e.unpacked_size)
                        .sum()
                })
                .unwrap_or(0)
        }
    }
}

/// Voľné meno v priečinku: „meno“, „meno (2)“ … (pri súbore s príponou pred ňou).
pub fn volne(dir: &Path, meno: &str) -> PathBuf {
    let c = dir.join(meno);
    if !c.exists() {
        return c;
    }
    let (kmen, pr) = match meno.rsplit_once('.') {
        Some((k, p)) if !k.is_empty() && p.len() <= 5 && !Path::new(&c).is_dir() => {
            (k.to_string(), format!(".{p}"))
        }
        _ => (meno.to_string(), String::new()),
    };
    (2..)
        .map(|i| dir.join(format!("{kmen} ({i}){pr}")))
        .find(|c| !c.exists())
        .unwrap_or(c)
}

/// Rozbalí archív vedľa neho. Jeden súbor alebo jeden priečinok v archíve sa dá rovno vedľa
/// archívu, viac vecí do priečinka s menom archívu. Vráti (výsledná cesta, počet súborov).
/// Rozbalí obyčajný (nezaheslovaný, nedelený) ZIP do priečinka `ciel` – vlastným kódom, bez
/// systémových nástrojov. Cesty mimo `ciel` sa nezapíšu.
pub fn rozbal_zip_do(archiv: &Path, ciel: &Path) -> Result<usize, ChybaRozbalenia> {
    zip(&[archiv.to_path_buf()], ciel, "", &|_| {})
}

pub fn rozbal(
    archiv: &Path,
    heslo: &str,
    priebeh: &dyn Fn(u64, u64),
) -> Result<(PathBuf, usize), ChybaRozbalenia> {
    let c = rozpoznaj(archiv).ok_or_else(|| ChybaRozbalenia::Ina("Toto nie je archív.".into()))?;
    let casti = casti(archiv);
    if casti.is_empty() {
        return Err(ChybaRozbalenia::Ina("Archív sa nenašiel.".into()));
    }
    if c.druh != Druh::Rar {
        // časti musia ísť za sebou: 1, 2, 3 …
        for (i, p) in casti.iter().enumerate() {
            if rozpoznaj(p).map(|x| x.cislo) != Some(i as u32 + 1) {
                return Err(ChybaRozbalenia::Ina(format!(
                    "Chýba časť {} rozdeleného archívu – stiahni všetky časti.",
                    i + 1
                )));
            }
        }
    }
    let dir = archiv.parent().unwrap_or(Path::new("."));
    let celkom = velkost_obsahu(c.druh, &casti, heslo);
    let docasny = volne(dir, &format!("{}.rozbaluje", c.meno));
    std::fs::create_dir_all(&docasny)?;
    let p = |h: u64| priebeh(h, celkom);
    let r = match c.druh {
        Druh::Zip => zip(&casti, &docasny, heslo, &p),
        Druh::SedemZ => sedem_z(&casti, &docasny, heslo, &p),
        Druh::Rar => rar(&casti[0], &docasny, heslo, &p),
    };
    let suborov = match r {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&docasny);
            return Err(e);
        }
    };
    let obsah: Vec<PathBuf> = std::fs::read_dir(&docasny)?
        .flatten()
        .map(|e| e.path())
        .collect();
    let vysledok = if obsah.len() == 1 {
        // jediný súbor alebo priečinok rovno vedľa archívu
        let meno = obsah[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let ciel = volne(dir, &meno);
        std::fs::rename(&obsah[0], &ciel)?;
        let _ = std::fs::remove_dir(&docasny);
        ciel
    } else {
        let ciel = volne(dir, &c.meno);
        std::fs::rename(&docasny, &ciel)?;
        ciel
    };
    Ok((vysledok, suborov))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn rozpoznanie_casti() {
        let r = |n: &str| rozpoznaj(Path::new(n)).map(|c| (c.druh, c.meno, c.cislo));
        assert_eq!(r("Film.2024.rar"), Some((Druh::Rar, "Film.2024".into(), 1)));
        assert_eq!(r("Film.part1.rar"), Some((Druh::Rar, "Film".into(), 1)));
        assert_eq!(r("Film.Part02.rar"), Some((Druh::Rar, "Film".into(), 2)));
        assert_eq!(r("Film.r00"), Some((Druh::Rar, "Film".into(), 2)));
        assert_eq!(r("Film.zip"), Some((Druh::Zip, "Film".into(), 1)));
        assert_eq!(r("Film.7z"), Some((Druh::SedemZ, "Film".into(), 1)));
        assert_eq!(r("Film.7z.003"), Some((Druh::SedemZ, "Film".into(), 3)));
        assert_eq!(r("Film.zip.001"), Some((Druh::Zip, "Film".into(), 1)));
        assert_eq!(r("Film.mkv"), None);
        assert_eq!(r("Film.part1.mkv"), None);
        assert_eq!(r("data.001"), None);
        assert!(je_delene(Path::new("a.7z.001")) && je_delene(Path::new("a.part2.rar")));
        assert!(
            je_delene(Path::new("a.r01"))
                && !je_delene(Path::new("a.rar"))
                && !je_delene(Path::new("a.zip"))
        );
    }

    #[test]
    fn bezpecne_cesty() {
        assert_eq!(
            bezpecna(Path::new("a/b.txt")),
            Some(PathBuf::from("a/b.txt"))
        );
        assert_eq!(bezpecna(Path::new("./a")), Some(PathBuf::from("a")));
        assert_eq!(bezpecna(Path::new("../a")), None);
        assert_eq!(bezpecna(Path::new("a/../../b")), None);
        assert_eq!(bezpecna(Path::new("/etc/passwd")), None);
        assert_eq!(bezpecna(Path::new("")), None);
    }

    fn docasny(meno: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kometa-arch-{meno}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_s_heslom(cesta: &Path, subory: &[(&str, &[u8])], heslo: Option<&str>) {
        use zip::write::SimpleFileOptions;
        let mut w = zip::ZipWriter::new(File::create(cesta).unwrap());
        for (n, d) in subory {
            let mut o =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            if let Some(h) = heslo {
                o = o.with_aes_encryption(zip::AesMode::Aes256, h);
            }
            w.start_file(*n, o).unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn zip_aes_heslo() {
        let d = docasny("zip");
        let a = d.join("Balík.zip");
        zip_s_heslom(
            &a,
            &[("Film/a.txt", b"prvy"), ("Film/b.txt", b"druhy")],
            Some("tajné"),
        );
        let nic = |_: u64, _: u64| {};
        assert!(matches!(
            rozbal(&a, "", &nic),
            Err(ChybaRozbalenia::Heslo(false))
        ));
        assert!(matches!(
            rozbal(&a, "zle", &nic),
            Err(ChybaRozbalenia::Heslo(true))
        ));
        let (c, n) = rozbal(&a, "tajné", &nic).unwrap();
        assert_eq!((c.clone(), n), (d.join("Film"), 2));
        assert_eq!(std::fs::read(c.join("b.txt")).unwrap(), b"druhy");
        // po chybe ani po úspechu nezostal dočasný priečinok
        let mena: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!mena.iter().any(|m| m.contains("rozbaluje")), "{mena:?}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn zip_nevyjde_z_priecinka() {
        let d = docasny("zlo");
        let a = d.join("vnutri").join("x.zip");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        zip_s_heslom(
            &a,
            &[("ok.txt", b"ok"), ("../zlo.txt", b"zlo"), ("ine.txt", b"x")],
            None,
        );
        let (c, n) = rozbal(&a, "", &|_, _| {}).unwrap();
        assert_eq!((c.clone(), n), (d.join("vnutri").join("x"), 2));
        assert!(!d.join("zlo.txt").exists() && !d.join("vnutri").join("zlo.txt").exists());
        // jediný súbor ide rovno vedľa archívu
        let b = d.join("jeden.zip");
        zip_s_heslom(&b, &[("Video.mkv", b"obraz")], None);
        let (c, _) = rozbal(&b, "", &|_, _| {}).unwrap();
        assert_eq!(c, d.join("Video.mkv"));
        let (c, _) = rozbal(&b, "", &|_, _| {}).unwrap();
        assert_eq!(c, d.join("Video (2).mkv"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn sedem_z_rozdeleny_a_zaheslovany() {
        let d = docasny("7z");
        let zdroj = d.join("zdroj");
        std::fs::create_dir_all(zdroj.join("Seriál")).unwrap();
        std::fs::write(zdroj.join("Seriál/diel1.txt"), vec![7u8; 50_000]).unwrap();
        std::fs::write(zdroj.join("Seriál/diel2.txt"), b"dva").unwrap();
        let cely = d.join("cely.7z");
        sevenz_rust2::compress_to_path_encrypted(
            &zdroj,
            &cely,
            sevenz_rust2::Password::new("heslo7"),
        )
        .unwrap();
        let data = std::fs::read(&cely).unwrap();
        let polovica = data.len() / 2;
        std::fs::write(d.join("Seriál.7z.001"), &data[..polovica]).unwrap();
        std::fs::write(d.join("Seriál.7z.002"), &data[polovica..]).unwrap();
        std::fs::remove_file(&cely).unwrap();
        std::fs::remove_dir_all(&zdroj).unwrap();
        let prva = d.join("Seriál.7z.001");
        assert_eq!(casti(&d.join("Seriál.7z.002")).len(), 2);
        assert!(matches!(
            rozbal(&prva, "", &|_, _| {}),
            Err(ChybaRozbalenia::Heslo(false))
        ));
        assert!(matches!(
            rozbal(&prva, "zle", &|_, _| {}),
            Err(ChybaRozbalenia::Heslo(true))
        ));
        let (c, n) = rozbal(&prva, "heslo7", &|_, _| {}).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(c.join("diel1.txt")).unwrap().len(), 50_000);
        // chýbajúca časť
        std::fs::remove_file(d.join("Seriál.7z.001")).unwrap();
        assert!(matches!(
            rozbal(&d.join("Seriál.7z.002"), "heslo7", &|_, _| {}),
            Err(ChybaRozbalenia::Ina(_))
        ));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn rar_s_heslom() {
        let d = docasny("rar");
        let a = d.join("Archív.rar");
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/zaheslovany.rar"),
            &a,
        )
        .unwrap();
        assert!(matches!(
            rozbal(&a, "", &|_, _| {}),
            Err(ChybaRozbalenia::Heslo(false))
        ));
        let (c, n) = rozbal(&a, "unrar", &|_, _| {}).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read_to_string(&c).unwrap(), "target\nCargo.lock\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn spojene_citanie() {
        let d = std::env::temp_dir().join(format!("kometa-spoj-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let a = d.join("x.001");
        let b = d.join("x.002");
        std::fs::write(&a, b"abcd").unwrap();
        std::fs::write(&b, b"efghij").unwrap();
        let mut s = Spojene::otvor(&[a, b]).unwrap();
        let mut v = String::new();
        s.read_to_string(&mut v).unwrap();
        assert_eq!(v, "abcdefghij");
        s.seek(SeekFrom::End(-3)).unwrap();
        let mut t = [0u8; 2];
        s.read_exact(&mut t).unwrap();
        assert_eq!(&t, b"hi");
        s.seek(SeekFrom::Start(3)).unwrap();
        let mut t = [0u8; 3];
        s.read_exact(&mut t).unwrap();
        assert_eq!(&t, b"def");
        let _ = std::fs::remove_dir_all(d);
    }
}
