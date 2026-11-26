//! Vlastné čítanie a zápis mediálnych kontajnerov – bez FFmpeg.
//!
//! Čítanie: MPEG-TS (HLS), ISO BMFF (MP4, fragmentované MP4 z HLS a DASH) a Matroska/WebM.
//! Zápis: Matroska (MKV/MKA) a MP4. Nič sa neprekóduje – vzorky sa len prebalia.
//!
//! Všetky časy sú v nanosekundách (`i64`). Kodek, ktorý čítač nepozná, vráti
//! [`Chyba::Nepodporovane`](crate::chyba::Chyba::Nepodporovane) – nič sa potichu nevynechá.

pub mod aac;
pub mod bity;
pub mod h264;
pub mod mkv;
pub mod mkvcit;
pub mod mp4;
pub mod mp4zapis;
pub mod ogg;
pub mod spajac;
pub mod srt;
pub mod ts;

use crate::chyba::Vysledok;

pub const NS: i64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Druh {
    Video,
    Zvuk,
    Titulky,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kodek {
    H264,
    Hevc,
    Av1,
    Vp9,
    Aac,
    Opus,
    Mp3,
    Ac3,
    Eac3,
    /// titulky ako čistý text (SRT, v MKV S_TEXT/UTF8)
    Srt,
}

impl Kodek {
    pub fn druh(self) -> Druh {
        match self {
            Kodek::H264 | Kodek::Hevc | Kodek::Av1 | Kodek::Vp9 => Druh::Video,
            Kodek::Srt => Druh::Titulky,
            _ => Druh::Zvuk,
        }
    }
}

/// Opis jednej stopy, ako ju našiel čítač.
#[derive(Clone, Debug)]
pub struct InfoStopy {
    pub kodek: Kodek,
    /// Nastavenie dekodéra v tvare, aký čaká MP4 aj MKV: avcC, hvcC, av1C, vpcC (obsah boxu),
    /// AudioSpecificConfig (AAC), dOps (Opus, obsah boxu), dac3/dec3.
    pub konfig: Vec<u8>,
    pub sirka: u32,
    pub vyska: u32,
    pub frekvencia: u32,
    pub kanaly: u16,
    /// Jazyk zo zdroja (ISO 639-2), ak ho zdroj uvádza.
    pub jazyk: Option<String>,
    /// Názov stopy zo zdroja (Matroska Name), ak ho zdroj uvádza.
    pub nazov: Option<String>,
}

impl InfoStopy {
    pub fn druh(&self) -> Druh {
        self.kodek.druh()
    }
}

/// Jedna vzorka (snímka obrazu alebo rámec zvuku).
#[derive(Clone, Debug)]
pub struct Vzorka {
    /// index stopy v rámci čítača
    pub stopa: usize,
    pub dts: i64,
    pub pts: i64,
    /// 0 = neznáme (doplní [`spajac`])
    pub trvanie: i64,
    pub kluc: bool,
    pub data: Vec<u8>,
}

/// Kapitola videa: začiatok (ns od začiatku súboru) a názov.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kapitola {
    pub od: i64,
    pub nazov: String,
}

/// Obrázok videa (obal) – len JPEG alebo PNG, ktoré prehrávače a správcovia súborov ukážu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obal {
    pub data: Vec<u8>,
}

impl Obal {
    /// Prijme len JPEG a PNG (podľa obsahu, nie prípony) a nie viac ako 20 MB.
    pub fn z_dat(data: Vec<u8>) -> Option<Obal> {
        let jpeg = data.starts_with(&[0xFF, 0xD8, 0xFF]);
        let png = data.starts_with(b"\x89PNG\r\n\x1a\n");
        ((jpeg || png) && data.len() <= 20 << 20).then_some(Obal { data })
    }
    pub fn png(&self) -> bool {
        self.data.starts_with(b"\x89PNG")
    }
    pub fn mime(&self) -> &'static str {
        if self.png() {
            "image/png"
        } else {
            "image/jpeg"
        }
    }
    pub fn pripona(&self) -> &'static str {
        if self.png() {
            "png"
        } else {
            "jpg"
        }
    }
}

/// Údaje o videu, ktoré sa vložia do výsledného súboru: názov, autor, dátum, zdroj, popis,
/// kapitoly a obrázok. Prázdne položky sa nezapisujú.
#[derive(Clone, Debug, Default)]
pub struct Doplnky {
    pub titul: String,
    pub autor: Option<String>,
    /// dátum zverejnenia v tvare RRRR-MM-DD (alebo len RRRR)
    pub datum: Option<String>,
    /// adresa, odkiaľ video pochádza
    pub zdroj: Option<String>,
    pub popis: Option<String>,
    pub kapitoly: Vec<Kapitola>,
    pub obal: Option<Obal>,
    /// dĺžka videa podľa zdroja (ns) – len pre koniec poslednej kapitoly pri spájaní cez FFmpeg
    pub trvanie: Option<i64>,
}

impl Doplnky {
    pub fn s_titulom(titul: &str) -> Self {
        Doplnky {
            titul: titul.to_string(),
            ..Default::default()
        }
    }

    /// Kapitoly pripravené na zápis do súboru dĺžky `koniec` (ns; 0 = neznáma): zoradené, bez
    /// rovnakých začiatkov, bez tých za koncom; prvá vždy od nuly (kratší úvod sa pripojí k nej,
    /// dlhší dostane vlastnú kapitolu „Úvod“), prázdny názov dostane poradové číslo.
    pub fn kapitoly_do(&self, koniec: i64) -> Vec<Kapitola> {
        let mut k: Vec<Kapitola> = self
            .kapitoly
            .iter()
            .filter(|x| x.od >= 0 && (koniec <= 0 || x.od < koniec))
            .cloned()
            .collect();
        k.sort_by_key(|x| x.od);
        k.dedup_by_key(|x| x.od);
        if let Some(prva) = k.first_mut() {
            if prva.od > 0 && prva.od < NS {
                prva.od = 0;
            }
        }
        if k.first().is_some_and(|x| x.od > 0) {
            k.insert(
                0,
                Kapitola {
                    od: 0,
                    nazov: "Úvod".into(),
                },
            );
        }
        for (i, x) in k.iter_mut().enumerate() {
            let t: String = x.nazov.split_whitespace().collect::<Vec<_>>().join(" ");
            x.nazov = if t.is_empty() {
                format!("Kapitola {}", i + 1)
            } else {
                t
            };
        }
        k
    }

    /// Je čo zapísať okrem názvu?
    pub fn ma_udaje(&self) -> bool {
        self.autor.is_some() || self.datum.is_some() || self.zdroj.is_some() || self.popis.is_some()
    }
}

/// Text skrátený na najviac `max` bajtov tak, aby sa neroztrhol znak UTF-8.
pub fn orez_utf8(t: &str, max: usize) -> &str {
    let mut k = t.len().min(max);
    while !t.is_char_boundary(k) {
        k -= 1;
    }
    &t[..k]
}

/// Čítač kontajnera: najprv opis stôp, potom vzorky v poradí dekódovania (v rámci stopy).
pub trait Citac: Send {
    fn stopy(&self) -> &[InfoStopy];
    fn dalsia(&mut self) -> Vysledok<Option<Vzorka>>;
}

/// Prevod medzi časovými mierkami bez pretečenia.
pub fn na_ns(hodnota: i64, mierka: u32) -> i64 {
    ((hodnota as i128 * NS as i128) / mierka.max(1) as i128) as i64
}

pub fn z_ns(ns: i64, mierka: u32) -> i64 {
    let n = ns as i128 * mierka as i128;
    let d = NS as i128;
    // zaokrúhlenie k najbližšej hodnote (aj pre záporné časy)
    (if n >= 0 {
        (n + d / 2) / d
    } else {
        (n - d / 2) / d
    }) as i64
}

/// Otvorí súbor podľa obsahu (MPEG-TS alebo MP4).
pub fn otvor(cesta: &std::path::Path) -> Vysledok<Box<dyn Citac>> {
    use std::io::Read;
    if cesta
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("srt") || e.eq_ignore_ascii_case("vtt"))
    {
        return Ok(Box::new(srt::CitacSrt::otvor(cesta)?));
    }
    let mut hlava = [0u8; 12];
    let mut f = std::fs::File::open(cesta)?;
    let n = f.read(&mut hlava)?;
    drop(f);
    if n >= 4 && hlava[..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return Ok(Box::new(mkvcit::CitacMkv::otvor(cesta)?));
    }
    if n >= 1 && hlava[0] == 0x47 {
        return Ok(Box::new(ts::CitacTs::otvor(cesta)?));
    }
    if n >= 8
        && matches!(
            &hlava[4..8],
            b"ftyp" | b"styp" | b"moov" | b"free" | b"skip" | b"sidx" | b"mdat"
        )
    {
        return Ok(Box::new(mp4::CitacMp4::otvor(cesta)?));
    }
    Err(crate::chyba::Chyba::Nepodporovane(format!(
        "neznámy formát súboru {}",
        cesta.file_name().and_then(|x| x.to_str()).unwrap_or("?")
    )))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn prevody_casu() {
        assert_eq!(na_ns(90_000, 90_000), NS);
        assert_eq!(z_ns(NS, 48_000), 48_000);
        assert_eq!(z_ns(21_333_333, 48_000), 1024);
        assert_eq!(z_ns(-NS / 2, 1000), -500);
        assert_eq!(na_ns(1 << 33, 90_000), (1i64 << 33) * NS / 90_000);
    }

    #[test]
    fn kapitoly_upratane() {
        let k = |od: i64, n: &str| Kapitola {
            od: od * NS,
            nazov: n.into(),
        };
        let d = Doplnky {
            kapitoly: vec![
                k(60, "Druhá"),
                k(5, "Prvá"),
                k(60, "dupl"),
                k(500, "za koncom"),
                k(90, "  "),
            ],
            ..Default::default()
        };
        let v = d.kapitoly_do(300 * NS);
        let mena: Vec<_> = v.iter().map(|x| (x.od / NS, x.nazov.as_str())).collect();
        assert_eq!(
            mena,
            [(0, "Úvod"), (5, "Prvá"), (60, "Druhá"), (90, "Kapitola 4")]
        );
        // krátky úvod sa pripojí k prvej kapitole
        let d = Doplnky {
            kapitoly: vec![
                Kapitola {
                    od: NS / 2,
                    nazov: "A".into(),
                },
                k(10, "B"),
            ],
            ..Default::default()
        };
        assert_eq!(
            d.kapitoly_do(0)[0],
            Kapitola {
                od: 0,
                nazov: "A".into()
            }
        );
        assert_eq!(orez_utf8("čšž", 3), "č");
        assert!(Obal::z_dat(b"RIFF....WEBP".to_vec()).is_none());
        assert_eq!(
            Obal::z_dat(vec![0xFF, 0xD8, 0xFF, 0xE0]).unwrap().pripona(),
            "jpg"
        );
    }
}
