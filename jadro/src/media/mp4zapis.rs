//! Zápis MP4 (ISO BMFF) – vlastný zapisovač s hlavičkou na začiatku („faststart“).
//!
//! Dvojprechodový: najprv sa zozbierajú údaje o vzorkách (veľkosti, časy), z nich sa postaví
//! `moov` a zapíše sa pred `mdat`; v druhom prechode sa do `mdat` zapíšu samotné vzorky
//! v rovnakom poradí. Jazyk ide do `mdhd` (ISO 639-2/T) a pri regióne aj do `elng` (BCP 47),
//! názov stopy do `hdlr`; predvolená zvuková stopa je jediná zapnutá v skupine zvukov.

use super::mkv::StopaVystupu;
use super::{z_ns, Druh, Kodek};
use crate::chyba::{Chyba, Vysledok};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Údaje o vzorke z prvého prechodu (časy v ns od začiatku súboru).
#[derive(Clone, Copy, Debug)]
pub struct VzorkaMeta {
    pub stopa: u16,
    pub velkost: u32,
    pub dts: i64,
    pub pts: i64,
    pub trvanie: i64,
    pub kluc: bool,
}

fn bx(typ: &[u8; 4], telo: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(telo.len() + 8);
    o.extend(((telo.len() + 8) as u32).to_be_bytes());
    o.extend(typ);
    o.extend_from_slice(telo);
    o
}

fn plny(typ: &[u8; 4], verzia: u8, priznaky: u32, telo: &[u8]) -> Vec<u8> {
    let mut t = vec![verzia];
    t.extend(&priznaky.to_be_bytes()[1..]);
    t.extend_from_slice(telo);
    bx(typ, &t)
}

const MATICA: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

/// ISO 639-2/B → /T pre mdhd (napr. cze → ces).
fn jazyk_t(b: &str) -> String {
    match b {
        "cze" => "ces",
        "slo" => "slk",
        "ger" => "deu",
        "fre" => "fra",
        "dut" => "nld",
        "rum" => "ron",
        "gre" => "ell",
        "chi" => "zho",
        "per" => "fas",
        "alb" => "sqi",
        "arm" => "hye",
        "baq" => "eus",
        "bur" => "mya",
        "geo" => "kat",
        "ice" => "isl",
        "mac" => "mkd",
        "may" => "msa",
        "wel" => "cym",
        "tib" => "bod",
        x => x,
    }
    .to_string()
}

fn zbal_jazyk(t: &str) -> u16 {
    let b = t.as_bytes();
    if b.len() != 3 || !b.iter().all(|c| c.is_ascii_lowercase()) {
        return 0x55C4; // und
    }
    ((b[0] - 0x60) as u16) << 10 | ((b[1] - 0x60) as u16) << 5 | (b[2] - 0x60) as u16
}

fn deskriptor(tag: u8, telo: &[u8]) -> Vec<u8> {
    let mut o = vec![tag];
    let n = telo.len();
    // 4-bajtová dĺžka (ako ffmpeg) – vždy platná
    o.extend([
        0x80 | ((n >> 21) & 0x7F) as u8,
        0x80 | ((n >> 14) & 0x7F) as u8,
        0x80 | ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ]);
    o.extend_from_slice(telo);
    o
}

fn esds(oti: u8, dsi: &[u8], id: u16) -> Vec<u8> {
    let mut dc = vec![oti, 0x15, 0, 0, 0];
    dc.extend(0u32.to_be_bytes());
    dc.extend(0u32.to_be_bytes());
    if !dsi.is_empty() {
        dc.extend(deskriptor(0x05, dsi));
    }
    let mut es = id.to_be_bytes().to_vec();
    es.push(0);
    es.extend(deskriptor(0x04, &dc));
    es.extend(deskriptor(0x06, &[0x02]));
    plny(b"esds", 0, 0, &deskriptor(0x03, &es))
}

fn vizualny(fourcc: &[u8; 4], s: u32, v: u32, deti: &[u8]) -> Vec<u8> {
    let mut t = vec![0u8; 6];
    t.extend(1u16.to_be_bytes()); // data_reference_index
    t.extend([0u8; 16]);
    t.extend((s as u16).to_be_bytes());
    t.extend((v as u16).to_be_bytes());
    t.extend(0x0048_0000u32.to_be_bytes());
    t.extend(0x0048_0000u32.to_be_bytes());
    t.extend(0u32.to_be_bytes());
    t.extend(1u16.to_be_bytes());
    t.extend([0u8; 32]);
    t.extend(0x0018u16.to_be_bytes());
    t.extend((-1i16).to_be_bytes());
    t.extend_from_slice(deti);
    bx(fourcc, &t)
}

fn zvukovy(fourcc: &[u8; 4], kanaly: u16, frekv: u32, deti: &[u8]) -> Vec<u8> {
    let mut t = vec![0u8; 6];
    t.extend(1u16.to_be_bytes());
    t.extend([0u8; 8]);
    t.extend(kanaly.to_be_bytes());
    t.extend(16u16.to_be_bytes());
    t.extend([0u8; 4]);
    t.extend(((frekv.min(65535)) << 16).to_be_bytes());
    t.extend_from_slice(deti);
    bx(fourcc, &t)
}

fn stsd(s: &StopaVystupu, id: u32) -> Vysledok<Vec<u8>> {
    let i = &s.info;
    let vstup = match i.kodek {
        Kodek::H264 => vizualny(b"avc1", i.sirka, i.vyska, &bx(b"avcC", &i.konfig)),
        Kodek::Hevc => vizualny(b"hvc1", i.sirka, i.vyska, &bx(b"hvcC", &i.konfig)),
        Kodek::Av1 => vizualny(b"av01", i.sirka, i.vyska, &bx(b"av1C", &i.konfig)),
        Kodek::Vp9 => {
            if i.konfig.is_empty() {
                return Err(Chyba::Nepodporovane("VP9 bez vpcC do MP4".into()));
            }
            vizualny(b"vp09", i.sirka, i.vyska, &bx(b"vpcC", &i.konfig))
        }
        Kodek::Aac => zvukovy(
            b"mp4a",
            i.kanaly.max(1),
            i.frekvencia,
            &esds(0x40, &i.konfig, id as u16),
        ),
        Kodek::Mp3 => zvukovy(
            b"mp4a",
            i.kanaly.max(1),
            i.frekvencia,
            &esds(0x6B, &[], id as u16),
        ),
        Kodek::Opus => {
            let kanaly = i.konfig.get(1).copied().unwrap_or(2) as u16;
            zvukovy(b"Opus", kanaly, 48_000, &bx(b"dOps", &i.konfig))
        }
        Kodek::Ac3 => zvukovy(
            b"ac-3",
            i.kanaly.max(2),
            i.frekvencia,
            &bx(b"dac3", &i.konfig),
        ),
        Kodek::Eac3 => zvukovy(
            b"ec-3",
            i.kanaly.max(2),
            i.frekvencia,
            &bx(b"dec3", &i.konfig),
        ),
        Kodek::Srt => return Err(Chyba::Nepodporovane("titulky v MP4".into())),
    };
    let mut t = 1u32.to_be_bytes().to_vec();
    t.extend(vstup);
    Ok(plny(b"stsd", 0, 0, &t))
}

fn mierka(s: &StopaVystupu) -> u32 {
    match s.info.kodek {
        Kodek::Opus => 48_000,
        _ if s.info.druh() == Druh::Zvuk && s.info.frekvencia > 0 => s.info.frekvencia,
        _ => 90_000,
    }
}

struct Tabulky {
    mierka: u32,
    trvanie: u64,
    stts: Vec<(u32, u32)>,
    ctts: Vec<(u32, i32)>,
    stss: Vec<u32>,
    stsz: Vec<u32>,
    stsc: Vec<(u32, u32)>,
    kusky: Vec<u64>,
    zaciatok_ns: i64,
    media_cas: i64,
}

fn tabulky(
    meta: &[VzorkaMeta],
    stopa: usize,
    s: &StopaVystupu,
    ofsety_kuskov: &[(usize, u64)],
) -> Tabulky {
    let m = mierka(s);
    let vz: Vec<&VzorkaMeta> = meta.iter().filter(|v| v.stopa as usize == stopa).collect();
    let prve_dts = vz.first().map(|v| v.dts).unwrap_or(0);
    let min_pts = vz.iter().map(|v| v.pts).min().unwrap_or(0);
    let dt: Vec<i64> = vz.iter().map(|v| z_ns(v.dts - prve_dts, m)).collect();
    let mut stts: Vec<(u32, u32)> = vec![];
    let mut ctts: Vec<(u32, i32)> = vec![];
    let mut trvanie = 0u64;
    for (k, v) in vz.iter().enumerate() {
        let d = match dt.get(k + 1) {
            Some(n) => (n - dt[k]).max(0),
            None => z_ns(v.trvanie, m).max(0),
        } as u32;
        trvanie += d as u64;
        match stts.last_mut() {
            Some((n, x)) if *x == d => *n += 1,
            _ => stts.push((1, d)),
        }
        let c = (z_ns(v.pts - prve_dts, m) - dt[k]) as i32;
        match ctts.last_mut() {
            Some((n, x)) if *x == c => *n += 1,
            _ => ctts.push((1, c)),
        }
    }
    if ctts.iter().all(|(_, c)| *c == 0) {
        ctts.clear();
    }
    let stss: Vec<u32> = if vz.iter().all(|v| v.kluc) {
        vec![]
    } else {
        vz.iter()
            .enumerate()
            .filter(|(_, v)| v.kluc)
            .map(|(k, _)| k as u32 + 1)
            .collect()
    };
    let stsz = vz.iter().map(|v| v.velkost).collect();
    // kúsky tejto stopy: (počet vzoriek, ofset)
    let mut stsc: Vec<(u32, u32)> = vec![];
    let mut kusky = vec![];
    for (k, &(pocet, ofset)) in ofsety_kuskov.iter().enumerate() {
        let _ = k;
        kusky.push(ofset);
        let cislo = kusky.len() as u32;
        match stsc.last() {
            Some((_, p)) if *p == pocet as u32 => {}
            _ => stsc.push((cislo, pocet as u32)),
        }
    }
    Tabulky {
        mierka: m,
        trvanie,
        stts,
        ctts,
        stss,
        stsz,
        stsc,
        kusky,
        zaciatok_ns: min_pts.max(0),
        media_cas: z_ns(min_pts - prve_dts, m).max(0),
    }
}

fn trak(
    id: u32,
    s: &StopaVystupu,
    t: &Tabulky,
    co64: bool,
    film_trvanie: u64,
) -> Vysledok<Vec<u8>> {
    let video = s.info.druh() == Druh::Video;
    let zapnuta = video || s.predvolena;
    let trvanie_ms = (t.trvanie as u128 * 1000 / t.mierka as u128) as u64;
    let oneskorenie_ms = (t.zaciatok_ns / 1_000_000) as u64;
    // tkhd
    let mut h = vec![0u8; 8];
    h.extend(id.to_be_bytes());
    h.extend([0u8; 4]);
    h.extend(((oneskorenie_ms + trvanie_ms).min(u32::MAX as u64) as u32).to_be_bytes());
    h.extend([0u8; 8]);
    h.extend(0u16.to_be_bytes()); // layer
    h.extend((if video { 0u16 } else { 1 }).to_be_bytes()); // alternate_group: zvuky sú alternatívy
    h.extend((if video { 0u16 } else { 0x0100 }).to_be_bytes());
    h.extend([0u8; 2]);
    for x in MATICA {
        h.extend(x.to_be_bytes());
    }
    h.extend((if video { s.info.sirka << 16 } else { 0 }).to_be_bytes());
    h.extend((if video { s.info.vyska << 16 } else { 0 }).to_be_bytes());
    let tkhd = plny(b"tkhd", 0, if zapnuta { 3 } else { 2 }, &h);
    // edts: oneskorenie začiatku a posun prezentácie
    let mut e = vec![];
    let mut pocet = 0u32;
    if oneskorenie_ms > 0 {
        e.extend((oneskorenie_ms as u32).to_be_bytes());
        e.extend((-1i32).to_be_bytes());
        e.extend(0x0001_0000u32.to_be_bytes());
        pocet += 1;
    }
    e.extend((trvanie_ms.min(u32::MAX as u64) as u32).to_be_bytes());
    e.extend((t.media_cas as i32).to_be_bytes());
    e.extend(0x0001_0000u32.to_be_bytes());
    pocet += 1;
    let mut elst = pocet.to_be_bytes().to_vec();
    elst.extend(e);
    let edts = bx(b"edts", &plny(b"elst", 0, 0, &elst));
    let _ = film_trvanie;
    // mdia
    let jazyk = s.jazyk.clone().or_else(|| s.info.jazyk.clone());
    let jazyk_b = jazyk
        .as_deref()
        .map(crate::spajanie::iso639_2)
        .unwrap_or_else(|| "und".into());
    let mut md = vec![0u8; 8];
    md.extend(t.mierka.to_be_bytes());
    md.extend((t.trvanie.min(u32::MAX as u64) as u32).to_be_bytes());
    md.extend(zbal_jazyk(&jazyk_t(&jazyk_b)).to_be_bytes());
    md.extend([0u8; 2]);
    let mdhd = plny(b"mdhd", 0, 0, &md);
    let elng = match &jazyk {
        Some(j) if j.contains(['-', '_']) => {
            let mut z = j.replace('_', "-").into_bytes();
            z.push(0);
            plny(b"elng", 0, 0, &z)
        }
        _ => vec![],
    };
    let meno = s
        .nazov
        .clone()
        .unwrap_or_else(|| if video { "Obraz".into() } else { "Zvuk".into() });
    let mut hd = vec![0u8; 4];
    hd.extend(if video { b"vide" } else { b"soun" });
    hd.extend([0u8; 12]);
    hd.extend(meno.as_bytes());
    hd.push(0);
    let hdlr = plny(b"hdlr", 0, 0, &hd);
    let hlavicka_media = if video {
        plny(b"vmhd", 0, 1, &[0u8; 8])
    } else {
        plny(b"smhd", 0, 0, &[0u8; 4])
    };
    let dref = plny(
        b"dref",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &plny(b"url ", 0, 1, &[])].concat(),
    );
    let dinf = bx(b"dinf", &dref);
    // stbl
    let mut stbl = stsd(s, id)?;
    let mut x = (t.stts.len() as u32).to_be_bytes().to_vec();
    for (n, d) in &t.stts {
        x.extend(n.to_be_bytes());
        x.extend(d.to_be_bytes());
    }
    stbl.extend(plny(b"stts", 0, 0, &x));
    if !t.ctts.is_empty() {
        let mut x = (t.ctts.len() as u32).to_be_bytes().to_vec();
        for (n, c) in &t.ctts {
            x.extend(n.to_be_bytes());
            x.extend(c.to_be_bytes());
        }
        let v1 = t.ctts.iter().any(|(_, c)| *c < 0);
        stbl.extend(plny(b"ctts", v1 as u8, 0, &x));
    }
    if !t.stss.is_empty() {
        let mut x = (t.stss.len() as u32).to_be_bytes().to_vec();
        for k in &t.stss {
            x.extend(k.to_be_bytes());
        }
        stbl.extend(plny(b"stss", 0, 0, &x));
    }
    let mut x = (t.stsc.len() as u32).to_be_bytes().to_vec();
    for (prvy, n) in &t.stsc {
        x.extend(prvy.to_be_bytes());
        x.extend(n.to_be_bytes());
        x.extend(1u32.to_be_bytes());
    }
    stbl.extend(plny(b"stsc", 0, 0, &x));
    let mut x = 0u32.to_be_bytes().to_vec();
    x.extend((t.stsz.len() as u32).to_be_bytes());
    for v in &t.stsz {
        x.extend(v.to_be_bytes());
    }
    stbl.extend(plny(b"stsz", 0, 0, &x));
    let mut x = (t.kusky.len() as u32).to_be_bytes().to_vec();
    for o in &t.kusky {
        if co64 {
            x.extend(o.to_be_bytes());
        } else {
            x.extend((*o as u32).to_be_bytes());
        }
    }
    stbl.extend(plny(if co64 { b"co64" } else { b"stco" }, 0, 0, &x));
    let minf = bx(
        b"minf",
        &[hlavicka_media, dinf, bx(b"stbl", &stbl)].concat(),
    );
    let mdia = bx(b"mdia", &[mdhd, elng, hdlr, minf].concat());
    Ok(bx(b"trak", &[tkhd, edts, mdia].concat()))
}

pub struct ZapisovacMp4 {
    f: BufWriter<File>,
    ocakavane: Vec<u32>,
    dalsia: usize,
    pub pocty: Vec<u64>,
}

impl ZapisovacMp4 {
    /// Zapíše ftyp + moov + hlavičku mdat; potom treba volať [`ZapisovacMp4::data`] pre každú vzorku v poradí `meta`.
    pub fn new(cesta: &Path, stopy: &[StopaVystupu], meta: &[VzorkaMeta]) -> Vysledok<Self> {
        let ma_avc = stopy.iter().any(|s| s.info.kodek == Kodek::H264);
        let mut ft = b"isom".to_vec();
        ft.extend(0x200u32.to_be_bytes());
        ft.extend(b"isomiso2");
        if ma_avc {
            ft.extend(b"avc1");
        }
        ft.extend(b"mp41");
        let ftyp = bx(b"ftyp", &ft);
        // kúsky: súvislé behy vzoriek tej istej stopy
        let mut behy: Vec<(usize, usize, u64)> = vec![]; // (stopa, počet, relatívny ofset)
        let mut rel = 0u64;
        for v in meta {
            match behy.last_mut() {
                Some((s, n, _)) if *s == v.stopa as usize && *n < 1024 => *n += 1,
                _ => behy.push((v.stopa as usize, 1, rel)),
            }
            rel += v.velkost as u64;
        }
        let spolu = rel;
        let postav = |zaklad: u64, co64: bool| -> Vysledok<Vec<u8>> {
            let mut traky = vec![];
            let mut film = 0u64;
            let mut tab = vec![];
            for (k, s) in stopy.iter().enumerate() {
                let ofs: Vec<(usize, u64)> = behy
                    .iter()
                    .filter(|b| b.0 == k)
                    .map(|b| (b.1, zaklad + b.2))
                    .collect();
                let t = tabulky(meta, k, s, &ofs);
                film = film.max(
                    (t.zaciatok_ns / 1_000_000) as u64
                        + (t.trvanie as u128 * 1000 / t.mierka as u128) as u64,
                );
                tab.push(t);
            }
            for (k, s) in stopy.iter().enumerate() {
                traky.extend(trak(k as u32 + 1, s, &tab[k], co64, film)?);
            }
            let mut mv = vec![0u8; 8];
            mv.extend(1000u32.to_be_bytes());
            mv.extend((film.min(u32::MAX as u64) as u32).to_be_bytes());
            mv.extend(0x0001_0000u32.to_be_bytes());
            mv.extend(0x0100u16.to_be_bytes());
            mv.extend([0u8; 10]);
            for x in MATICA {
                mv.extend(x.to_be_bytes());
            }
            mv.extend([0u8; 24]);
            mv.extend((stopy.len() as u32 + 1).to_be_bytes());
            let mvhd = plny(b"mvhd", 0, 0, &mv);
            Ok(bx(b"moov", &[mvhd, traky].concat()))
        };
        let skusobny = postav(0, false)?;
        let co64 = ftyp.len() as u64 + skusobny.len() as u64 * 2 + 16 + spolu > u32::MAX as u64;
        let dlzka_moov = postav(0, co64)?.len() as u64;
        let zaklad = ftyp.len() as u64 + dlzka_moov + 16;
        let moov = postav(zaklad, co64)?;
        if moov.len() as u64 != dlzka_moov {
            return Err(Chyba::Format("MP4: veľkosť hlavičky sa zmenila".into()));
        }
        let mut f = BufWriter::with_capacity(1 << 20, File::create(cesta)?);
        f.write_all(&ftyp)?;
        f.write_all(&moov)?;
        f.write_all(&1u32.to_be_bytes())?;
        f.write_all(b"mdat")?;
        f.write_all(&(spolu + 16).to_be_bytes())?;
        Ok(ZapisovacMp4 {
            f,
            ocakavane: meta.iter().map(|v| v.velkost).collect(),
            dalsia: 0,
            pocty: vec![0; stopy.len()],
        })
    }

    pub fn data(&mut self, stopa: usize, d: &[u8]) -> Vysledok<()> {
        match self.ocakavane.get(self.dalsia) {
            Some(&v) if v as usize == d.len() => {}
            _ => return Err(Chyba::Format("MP4: druhý prechod nesedí s prvým".into())),
        }
        self.dalsia += 1;
        self.pocty[stopa] += 1;
        self.f.write_all(d)?;
        Ok(())
    }

    pub fn dokonci(self) -> Vysledok<Vec<u64>> {
        if self.dalsia != self.ocakavane.len() {
            return Err(Chyba::Format(
                "MP4: chýbajú vzorky v druhom prechode".into(),
            ));
        }
        let f = self.f.into_inner().map_err(|e| Chyba::Io(e.into_error()))?;
        f.sync_all()?;
        Ok(self.pocty)
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn jazyk_mdhd() {
        assert_eq!(zbal_jazyk(&jazyk_t("cze")), (3 << 10) | (5 << 5) | 19);
        assert_eq!(jazyk_t("slo"), "slk");
        assert_eq!(zbal_jazyk("und"), 0x55C4);
        assert_eq!(zbal_jazyk("xx"), 0x55C4);
    }

    #[test]
    fn esds_obsahuje_asc() {
        let e = esds(0x40, &[0x11, 0x90], 1);
        assert_eq!(&e[4..8], b"esds");
        assert!(e
            .windows(7)
            .any(|w| w == [0x05, 0x80, 0x80, 0x80, 0x02, 0x11, 0x90]));
    }
}
