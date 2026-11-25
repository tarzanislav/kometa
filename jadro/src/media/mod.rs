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
}
