//! Čítanie ISO BMFF (ISO/IEC 14496-12): bežné MP4 aj fragmentované MP4 (HLS fMP4, DASH).
//!
//! Hlavička `moov` sa načíta celá (je malá), vzorky sa čítajú prúdovo – zo súboru sa naraz
//! drží v pamäti len jeden fragment, nikdy celé video. Šifrované stopy (`encv`, `enca`,
//! `pssh`) vrátia [`Chyba::Drm`] – Kometa šifrovaný obsah nesťahuje ani nerozbaľuje.
//!
//! Textové stopy (3GPP Timed Text `tx3g`, QuickTime `text`) sa čítajú ako titulky: vzorka bez textu
//! (vyplnená medzera) sa preskočí. Kapitolová stopa (cieľ odkazu `tref/chap`) sa ako titulky nečíta.
//! [`prehlad`] vráti, čo o súbore ukážu prehrávače: stopy, kapitoly, údaje o videu a obrázok.

use super::bity::{be16, be32, be64};
use super::{aac, na_ns, Citac, InfoStopy, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Box: typ, začiatok obsahu a koniec (v bufferi).
#[derive(Clone, Copy, Debug)]
struct BoxInfo {
    typ: [u8; 4],
    obsah: u64,
    koniec: u64,
}

fn box_v_bufferi(d: &[u8], od: usize) -> Option<BoxInfo> {
    if od + 8 > d.len() {
        return None;
    }
    let mut velkost = be32(&d[od..]) as u64;
    let typ = [d[od + 4], d[od + 5], d[od + 6], d[od + 7]];
    let mut hlavicka = 8u64;
    if velkost == 1 {
        if od + 16 > d.len() {
            return None;
        }
        velkost = be64(&d[od + 8..]);
        hlavicka = 16;
    } else if velkost == 0 {
        velkost = (d.len() - od) as u64;
    }
    if velkost < hlavicka || od as u64 + velkost > d.len() as u64 {
        return None;
    }
    Some(BoxInfo {
        typ,
        obsah: od as u64 + hlavicka,
        koniec: od as u64 + velkost,
    })
}

/// Deti boxu v bufferi.
fn deti(d: &[u8], od: usize, koniec: usize) -> Vec<BoxInfo> {
    let mut out = vec![];
    let mut i = od;
    while let Some(b) = box_v_bufferi(&d[..koniec], i) {
        out.push(b);
        i = b.koniec as usize;
    }
    out
}

fn najdi<'a>(d: &'a [u8], od: usize, koniec: usize, typ: &[u8; 4]) -> Option<&'a [u8]> {
    deti(d, od, koniec)
        .into_iter()
        .find(|b| &b.typ == typ)
        .map(|b| &d[b.obsah as usize..b.koniec as usize])
}

fn vnorene<'a>(d: &'a [u8], typy: &[&[u8; 4]]) -> Option<&'a [u8]> {
    let mut aktualne = d;
    for t in typy {
        aktualne = najdi(aktualne, 0, aktualne.len(), t)?;
    }
    Some(aktualne)
}

/// Vzorka bežného MP4: (ofset, veľkosť, dts, posun prezentácie, kľúčová).
type ZaznamVzorky = (u64, u32, i64, i32, bool);

#[derive(Clone, Debug, Default)]
struct Trex {
    trvanie: u32,
    velkost: u32,
    priznaky: u32,
}

#[derive(Clone, Debug)]
struct Stopa {
    id: u32,
    mierka: u32,
    /// posun prezentačného času z edit listu (ns)
    posun: i64,
    trex: Trex,
    /// progresívne MP4: zoznam vzoriek (offset, veľkosť, dts, cto, kľúčová)
    vzorky: Vec<ZaznamVzorky>,
    /// koniec poslednej vzorky podľa stts (v mierke stopy) – jej trvanie
    koniec: i64,
    dalsia: usize,
    /// fragmentované: dts nasledujúcej vzorky (ak fragment nemá tfdt)
    dalsi_dts: i64,
    zvuk: bool,
    /// textová stopa (titulky)
    text: bool,
}

struct Polozka {
    stopa: usize,
    offset: u64,
    velkost: u32,
    dts: i64,
    pts: i64,
    trvanie: i64,
    kluc: bool,
}

pub struct CitacMp4 {
    f: File,
    dlzka_suboru: u64,
    stopy_info: Vec<InfoStopy>,
    stopy: Vec<Stopa>,
    fragmentovane: bool,
    /// pozícia ďalšieho boxu na najvyššej úrovni (fragmentované MP4)
    poz: u64,
    fronta: VecDeque<Polozka>,
}

fn dlzka_opisu(d: &[u8], i: &mut usize) -> Option<usize> {
    let mut n = 0usize;
    for _ in 0..4 {
        let b = *d.get(*i)?;
        *i += 1;
        n = (n << 7) | (b & 0x7F) as usize;
        if b & 0x80 == 0 {
            return Some(n);
        }
    }
    Some(n)
}

/// esds → (objectTypeIndication, DecoderSpecificInfo)
fn esds(d: &[u8]) -> Option<(u8, Vec<u8>)> {
    let mut i = 4; // verzia a príznaky
    if *d.get(i)? != 0x03 {
        return None;
    }
    i += 1;
    dlzka_opisu(d, &mut i)?;
    i += 2;
    let priznaky = *d.get(i)?;
    i += 1;
    if priznaky & 0x80 != 0 {
        i += 2;
    }
    if priznaky & 0x40 != 0 {
        i += 1 + *d.get(i)? as usize;
    }
    if priznaky & 0x20 != 0 {
        i += 2;
    }
    if *d.get(i)? != 0x04 {
        return None;
    }
    i += 1;
    let dlzka = dlzka_opisu(d, &mut i)?;
    let koniec = i + dlzka;
    let oti = *d.get(i)?;
    i += 13;
    let mut dsi = vec![];
    if i < koniec && d.get(i) == Some(&0x05) {
        i += 1;
        let n = dlzka_opisu(d, &mut i)?;
        dsi = d.get(i..i + n)?.to_vec();
    }
    Some((oti, dsi))
}

/// Jazyk z mdhd (zbalený ISO 639-2/T) → ISO 639-2/B, ako ho používa Kometa.
fn jazyk_mdhd(zbaleny: u16) -> Option<String> {
    if zbaleny < 0x400 || zbaleny == 0x7FFF {
        return None;
    }
    let p: String = (0..3)
        .map(|k| (((zbaleny >> (10 - 5 * k)) & 0x1F) as u8 + 0x60) as char)
        .collect();
    if !p.chars().all(|c| c.is_ascii_lowercase()) || p == "und" {
        return None;
    }
    Some(crate::spajanie::iso639_2(&p))
}

fn info_stopy(stsd: &[u8], tkhd: &[u8]) -> Vysledok<Option<InfoStopy>> {
    let vstup = deti(stsd, 8, stsd.len()).into_iter().next();
    let Some(z) = vstup else { return Ok(None) };
    let typ = &z.typ;
    if typ == b"encv" || typ == b"enca" {
        return Err(Chyba::Drm("MP4 CENC".into()));
    }
    if typ == b"tx3g" || typ == b"text" {
        return Ok(Some(InfoStopy {
            kodek: Kodek::Srt,
            konfig: vec![],
            sirka: 0,
            vyska: 0,
            frekvencia: 0,
            kanaly: 0,
            jazyk: None,
            nazov: None,
        }));
    }
    let telo = &stsd[z.obsah as usize..z.koniec as usize];
    let video = matches!(
        typ,
        b"avc1"
            | b"avc3"
            | b"hvc1"
            | b"hev1"
            | b"av01"
            | b"vp09"
            | b"vp08"
            | b"mp4v"
            | b"dvh1"
            | b"dvhe"
    );
    let zvuk = matches!(
        typ,
        b"mp4a"
            | b"Opus"
            | b"ac-3"
            | b"ec-3"
            | b".mp3"
            | b"fLaC"
            | b"alac"
            | b"samr"
            | b"sowt"
            | b"twos"
            | b"lpcm"
    );
    if !video && !zvuk {
        return Ok(None); // titulky, časový kód, metadáta
    }
    let (zaciatok_deti, sirka, vyska, mut frekvencia, mut kanaly) = if video {
        if telo.len() < 78 {
            return Err(Chyba::Format("poškodený opis obrazu v MP4".into()));
        }
        (78, be16(&telo[24..]) as u32, be16(&telo[26..]) as u32, 0, 0)
    } else {
        if telo.len() < 28 {
            return Err(Chyba::Format("poškodený opis zvuku v MP4".into()));
        }
        let verzia = be16(&telo[8..]);
        let navyse = match verzia {
            1 => 16,
            2 => 36,
            _ => 0,
        };
        (
            28 + navyse,
            0,
            0,
            be32(&telo[24..]) >> 16,
            be16(&telo[16..]),
        )
    };
    let deti_z = deti(telo, zaciatok_deti.min(telo.len()), telo.len());
    let obsah = |t: &[u8; 4]| {
        deti_z
            .iter()
            .find(|b| &b.typ == t)
            .map(|b| telo[b.obsah as usize..b.koniec as usize].to_vec())
    };
    let (kodek, konfig) = match typ {
        b"avc1" | b"avc3" => (Kodek::H264, obsah(b"avcC")),
        b"hvc1" | b"hev1" => (Kodek::Hevc, obsah(b"hvcC")),
        b"av01" => (Kodek::Av1, obsah(b"av1C")),
        b"vp09" => (Kodek::Vp9, obsah(b"vpcC")),
        b"Opus" => (Kodek::Opus, obsah(b"dOps")),
        b"ac-3" => (Kodek::Ac3, obsah(b"dac3")),
        b"ec-3" => (Kodek::Eac3, obsah(b"dec3")),
        b".mp3" => (Kodek::Mp3, Some(vec![])),
        b"mp4a" => {
            let (oti, dsi) = obsah(b"esds")
                .and_then(|e| esds(&e))
                .ok_or_else(|| Chyba::Format("MP4 zvuk bez esds".into()))?;
            match oti {
                0x40 | 0x66 | 0x67 | 0x68 => {
                    if let Some((f, k)) = aac::z_asc(&dsi) {
                        frekvencia = f;
                        if k > 0 {
                            kanaly = k;
                        }
                    }
                    (Kodek::Aac, Some(dsi))
                }
                0x69 | 0x6B => (Kodek::Mp3, Some(vec![])),
                x => return Err(Chyba::Nepodporovane(format!("zvuk MP4 typu 0x{x:02X}"))),
            }
        }
        t => {
            return Err(Chyba::Nepodporovane(format!(
                "kodek {} v MP4",
                String::from_utf8_lossy(t)
            )))
        }
    };
    let konfig = konfig.ok_or_else(|| Chyba::Format("MP4 stopa bez nastavení dekodéra".into()))?;
    // rozmery: z tkhd (16.16), ak opis obrazu nemá
    let (mut s, mut v) = (sirka, vyska);
    if (s == 0 || v == 0) && tkhd.len() >= 84 {
        let off = if tkhd[0] == 1 { 88 } else { 76 };
        if tkhd.len() >= off + 8 {
            s = be32(&tkhd[off..]) >> 16;
            v = be32(&tkhd[off + 4..]) >> 16;
        }
    }
    Ok(Some(InfoStopy {
        kodek,
        konfig,
        sirka: s,
        vyska: v,
        frekvencia,
        kanaly,
        jazyk: None,
        nazov: None,
    }))
}

/// ID stopy z tkhd.
fn id_stopy(tkhd: &[u8]) -> u32 {
    match tkhd.first() {
        Some(1) if tkhd.len() >= 24 => be32(&tkhd[20..]),
        Some(_) if tkhd.len() >= 16 => be32(&tkhd[12..]),
        _ => 0,
    }
}

/// Stopy, na ktoré táto stopa odkazuje ako na kapitoly (tref/chap).
fn odkazy_chap(trak: &[u8]) -> Vec<u32> {
    vnorene(trak, &[b"tref", b"chap"])
        .map(|c| c.chunks_exact(4).map(be32).collect())
        .unwrap_or_default()
}

/// Názov stopy z udta/name: holý text (ISO), alebo s 16-bitovou dĺžkou a jazykom (QuickTime).
fn nazov_stopy(d: &[u8]) -> Option<String> {
    let d = if d.len() >= 4 && be16(d) as usize == d.len() - 4 {
        &d[4..]
    } else {
        d
    };
    let t = String::from_utf8_lossy(d)
        .trim_matches(char::from(0))
        .trim()
        .to_string();
    (!t.is_empty()).then_some(t)
}

/// Vzorka tx3g/text → text titulku (UTF-8 alebo UTF-16 so značkou poradia); None = bez textu.
fn text_vzorky(d: &[u8]) -> Option<String> {
    let n = be16(d.get(..2)?) as usize;
    let t = &d[2..(2 + n).min(d.len())];
    let text = if t.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = t[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(t).into_owned()
    };
    let text = text.trim_matches(char::from(0)).trim_end().to_string();
    (!text.trim().is_empty()).then_some(text)
}

/// Stopa, ako ju uvidí prehrávač.
#[derive(Clone, Debug, Default)]
pub struct PrehladStopy {
    pub id: u32,
    /// vide, soun, sbtl, text…
    pub handler: String,
    /// opis vzorky: avc1, mp4a, tx3g…
    pub kodek: String,
    pub jazyk: Option<String>,
    /// udta/name, inak názov v hdlr
    pub nazov: Option<String>,
    pub zapnuta: bool,
    pub skupina: u16,
    /// roly MPEG-DASH z boxov kind
    pub roly: Vec<String>,
    /// ID kapitolovej stopy, na ktorú odkazuje
    pub kapitoly: Option<u32>,
}

/// Prehľad hlavičky MP4: stopy, kapitoly (chpl), údaje o videu (ilst) a obrázok.
#[derive(Clone, Debug, Default)]
pub struct Prehlad {
    pub stopy: Vec<PrehladStopy>,
    /// (začiatok v ms, názov)
    pub kapitoly: Vec<(i64, String)>,
    /// je tu kapitolová textová stopa, na ktorú niektorá stopa odkazuje
    pub kapitolova_stopa: bool,
    /// textové položky ilst podľa kľúča (©nam, ©ART, desc…)
    pub udaje: BTreeMap<String, String>,
    /// obrázok: (typ údajov 13 = JPEG / 14 = PNG, veľkosť)
    pub obal: Option<(u32, usize)>,
}

fn fourcc(t: &[u8]) -> String {
    t.iter().map(|&b| b as char).collect()
}

/// Prečíta hlavičku MP4 a vráti, čo z nej ukážu prehrávače.
pub fn prehlad(cesta: &Path) -> Vysledok<Prehlad> {
    let mut f = File::open(cesta)?;
    let dlzka = f.metadata()?.len();
    let mut poz = 0u64;
    let mut moov = None;
    while poz + 8 <= dlzka {
        let (typ, obsah, koniec) = CitacMp4::hlavicka_boxu(&mut f, poz, dlzka)?;
        if &typ == b"moov" {
            let mut b = vec![0u8; (koniec - obsah) as usize];
            f.seek(SeekFrom::Start(obsah))?;
            f.read_exact(&mut b)?;
            moov = Some(b);
            break;
        }
        poz = koniec;
    }
    let moov = moov.ok_or_else(|| Chyba::Format("MP4 bez hlavičky moov".into()))?;
    let mut p = Prehlad::default();
    for trak in deti(&moov, 0, moov.len())
        .into_iter()
        .filter(|b| &b.typ == b"trak")
    {
        let t = &moov[trak.obsah as usize..trak.koniec as usize];
        let tkhd = najdi(t, 0, t.len(), b"tkhd").unwrap_or(&[]);
        let mut s = PrehladStopy {
            id: id_stopy(tkhd),
            zapnuta: tkhd.len() >= 4 && be32(tkhd) & 1 == 1,
            kapitoly: odkazy_chap(t).first().copied(),
            ..Default::default()
        };
        let skupina_poz = if tkhd.first() == Some(&1) { 46 } else { 34 };
        if tkhd.len() >= skupina_poz + 2 {
            s.skupina = be16(&tkhd[skupina_poz..]);
        }
        if let Some(hdlr) = vnorene(t, &[b"mdia", b"hdlr"]).filter(|h| h.len() >= 24) {
            s.handler = fourcc(&hdlr[8..12]);
            let meno = String::from_utf8_lossy(&hdlr[24..])
                .trim_matches(char::from(0))
                .to_string();
            s.nazov = (!meno.is_empty()).then_some(meno);
        }
        if let Some(mdhd) = vnorene(t, &[b"mdia", b"mdhd"]) {
            let z = if mdhd[0] == 1 {
                be16(&mdhd[32..])
            } else {
                be16(&mdhd[20..])
            };
            s.jazyk = jazyk_mdhd(z);
        }
        if let Some(stsd) = vnorene(t, &[b"mdia", b"minf", b"stbl", b"stsd"]) {
            if stsd.len() >= 16 {
                s.kodek = fourcc(&stsd[12..16]);
            }
        }
        if let Some(udta) = najdi(t, 0, t.len(), b"udta") {
            if let Some(n) = najdi(udta, 0, udta.len(), b"name").and_then(nazov_stopy) {
                s.nazov = Some(n);
            }
            for k in deti(udta, 0, udta.len())
                .into_iter()
                .filter(|b| &b.typ == b"kind")
            {
                let d = &udta[k.obsah as usize + 4..k.koniec as usize];
                let mut casti = d.split(|&b| b == 0);
                if let (Some(_schema), Some(rola)) = (casti.next(), casti.next()) {
                    s.roly.push(String::from_utf8_lossy(rola).into_owned());
                }
            }
        }
        p.stopy.push(s);
    }
    let odkazovane: Vec<u32> = p.stopy.iter().filter_map(|s| s.kapitoly).collect();
    p.kapitolova_stopa = p
        .stopy
        .iter()
        .any(|s| odkazovane.contains(&s.id) && s.handler == "text");
    if let Some(udta) = najdi(&moov, 0, moov.len(), b"udta") {
        if let Some(c) = najdi(udta, 0, udta.len(), b"chpl") {
            let mut i = if c.first() == Some(&1) { 8 } else { 4 };
            let n = c.get(i).copied().unwrap_or(0) as usize;
            i += 1;
            for _ in 0..n {
                let (Some(cas), Some(&dl)) = (c.get(i..i + 8), c.get(i + 8)) else {
                    break;
                };
                let meno = c.get(i + 9..i + 9 + dl as usize).unwrap_or(&[]);
                p.kapitoly.push((
                    be64(cas) as i64 / 10_000,
                    String::from_utf8_lossy(meno).into_owned(),
                ));
                i += 9 + dl as usize;
            }
        }
        if let Some(meta) = najdi(udta, 0, udta.len(), b"meta") {
            // ISO meta je „plný“ box (verzia a príznaky), starší QuickTime nie
            let od = if meta.get(4..8) == Some(b"hdlr") {
                0
            } else {
                4
            };
            if let Some(ilst) = najdi(meta, od.min(meta.len()), meta.len(), b"ilst") {
                for pol in deti(ilst, 0, ilst.len()) {
                    let Some(data) = najdi(ilst, pol.obsah as usize, pol.koniec as usize, b"data")
                    else {
                        continue;
                    };
                    if data.len() < 8 {
                        continue;
                    }
                    let druh = be32(data) & 0xFF_FFFF;
                    let kluc = fourcc(&pol.typ);
                    if kluc == "covr" {
                        p.obal = Some((druh, data.len() - 8));
                    } else if druh == 1 {
                        p.udaje
                            .insert(kluc, String::from_utf8_lossy(&data[8..]).into_owned());
                    }
                }
            }
        }
    }
    Ok(p)
}

impl CitacMp4 {
    pub fn otvor(cesta: &Path) -> Vysledok<Self> {
        let mut f = File::open(cesta)?;
        let dlzka_suboru = f.metadata()?.len();
        // nájdi moov na najvyššej úrovni
        let mut poz = 0u64;
        let mut moov: Option<Vec<u8>> = None;
        let mut prvy_moof = None;
        while poz + 8 <= dlzka_suboru {
            let (typ, obsah, koniec) = Self::hlavicka_boxu(&mut f, poz, dlzka_suboru)?;
            if &typ == b"moov" {
                let mut b = vec![0u8; (koniec - obsah) as usize];
                f.seek(SeekFrom::Start(obsah))?;
                f.read_exact(&mut b)?;
                moov = Some(b);
            } else if &typ == b"moof" && prvy_moof.is_none() {
                prvy_moof = Some(poz);
            }
            if moov.is_some() && (prvy_moof.is_some() || &typ == b"mdat") {
                break;
            }
            poz = koniec;
        }
        let moov = moov.ok_or_else(|| Chyba::Format("MP4 bez hlavičky moov".into()))?;
        if najdi(&moov, 0, moov.len(), b"pssh").is_some() {
            return Err(Chyba::Drm("MP4 s kľúčmi DRM (pssh)".into()));
        }
        let mvhd_mierka = najdi(&moov, 0, moov.len(), b"mvhd")
            .map(|m| {
                if m[0] == 1 {
                    be32(&m[20..])
                } else {
                    be32(&m[12..])
                }
            })
            .unwrap_or(1000);
        let mut trex_zoznam = vec![];
        if let Some(mvex) = najdi(&moov, 0, moov.len(), b"mvex") {
            for b in deti(mvex, 0, mvex.len())
                .into_iter()
                .filter(|b| &b.typ == b"trex")
            {
                let t = &mvex[b.obsah as usize..b.koniec as usize];
                if t.len() >= 24 {
                    trex_zoznam.push((
                        be32(&t[4..]),
                        Trex {
                            trvanie: be32(&t[12..]),
                            velkost: be32(&t[16..]),
                            priznaky: be32(&t[20..]),
                        },
                    ));
                }
            }
        }
        let mut stopy_info = vec![];
        let mut stopy = vec![];
        let traky: Vec<&[u8]> = deti(&moov, 0, moov.len())
            .into_iter()
            .filter(|b| &b.typ == b"trak")
            .map(|b| &moov[b.obsah as usize..b.koniec as usize])
            .collect();
        let kapitolove: Vec<u32> = traky.iter().flat_map(|t| odkazy_chap(t)).collect();
        for t in traky {
            let tkhd = najdi(t, 0, t.len(), b"tkhd").unwrap_or(&[]);
            let id = id_stopy(tkhd);
            if kapitolove.contains(&id) {
                continue; // kapitoly nie sú titulky
            }
            let Some(mdia) = najdi(t, 0, t.len(), b"mdia") else {
                continue;
            };
            let Some(mdhd) = najdi(mdia, 0, mdia.len(), b"mdhd") else {
                continue;
            };
            let (mierka, jazyk) = if mdhd[0] == 1 {
                (be32(&mdhd[20..]), be16(&mdhd[32..]))
            } else {
                (be32(&mdhd[12..]), be16(&mdhd[20..]))
            };
            let Some(stbl) = vnorene(mdia, &[b"minf", b"stbl"]) else {
                continue;
            };
            let Some(stsd) = najdi(stbl, 0, stbl.len(), b"stsd") else {
                continue;
            };
            let Some(mut info) = info_stopy(stsd, tkhd)? else {
                continue;
            };
            info.jazyk = jazyk_mdhd(jazyk);
            info.nazov = vnorene(t, &[b"udta", b"name"]).and_then(nazov_stopy);
            if info.kodek == Kodek::Mp3 && info.frekvencia == 0 {
                info.frekvencia = 44100;
            }
            // edit list: prázdna úprava = oneskorenie, media_time = posun začiatku
            let mut posun = 0i64;
            if let Some(elst) = vnorene(t, &[b"edts", b"elst"]) {
                let v1 = elst[0] == 1;
                let n = be32(&elst[4..]) as usize;
                let dlzka_zaznamu = if v1 { 20 } else { 12 };
                for k in 0..n.min(8) {
                    let z = &elst[8 + k * dlzka_zaznamu..];
                    if z.len() < dlzka_zaznamu {
                        break;
                    }
                    let (trvanie, cas) = if v1 {
                        (be64(z) as i64, be64(&z[8..]) as i64)
                    } else {
                        (be32(z) as i64, be32(&z[4..]) as i32 as i64)
                    };
                    if cas == -1 {
                        posun += na_ns(trvanie, mvhd_mierka);
                    } else {
                        posun -= na_ns(cas, mierka);
                        break;
                    }
                }
            }
            let trex = trex_zoznam
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            let zvuk = info.kodek.druh() == super::Druh::Zvuk;
            let text = info.kodek == Kodek::Srt;
            let (vzorky, koniec) = Self::tabulky(stbl)?;
            stopy_info.push(info);
            stopy.push(Stopa {
                id,
                mierka,
                posun,
                trex,
                vzorky,
                koniec,
                dalsia: 0,
                dalsi_dts: 0,
                zvuk,
                text,
            });
        }
        if stopy.is_empty() {
            return Err(Chyba::Format("MP4 bez obrazu, zvuku aj titulkov".into()));
        }
        let fragmentovane = prvy_moof.is_some() || stopy.iter().all(|s| s.vzorky.is_empty());
        Ok(CitacMp4 {
            f,
            dlzka_suboru,
            stopy_info,
            stopy,
            fragmentovane,
            poz: prvy_moof.unwrap_or(dlzka_suboru),
            fronta: VecDeque::new(),
        })
    }

    fn hlavicka_boxu(f: &mut File, poz: u64, dlzka: u64) -> Vysledok<([u8; 4], u64, u64)> {
        let mut h = [0u8; 16];
        f.seek(SeekFrom::Start(poz))?;
        let n = f.read(&mut h)?;
        if n < 8 {
            return Err(Chyba::Format("useknutý box MP4".into()));
        }
        let mut velkost = be32(&h) as u64;
        let typ = [h[4], h[5], h[6], h[7]];
        let mut hl = 8;
        if velkost == 1 {
            if n < 16 {
                return Err(Chyba::Format("useknutý box MP4".into()));
            }
            velkost = be64(&h[8..]);
            hl = 16;
        } else if velkost == 0 {
            velkost = dlzka - poz;
        }
        if velkost < hl {
            return Err(Chyba::Format("poškodený box MP4".into()));
        }
        Ok((typ, poz + hl, (poz + velkost).min(dlzka)))
    }

    /// Tabuľky vzoriek bežného MP4 (stts, ctts, stsc, stsz, stco/co64, stss).
    fn tabulky(stbl: &[u8]) -> Vysledok<(Vec<ZaznamVzorky>, i64)> {
        let tab = |t: &[u8; 4]| najdi(stbl, 0, stbl.len(), t);
        let (Some(stts), Some(stsc)) = (tab(b"stts"), tab(b"stsc")) else {
            return Ok((vec![], 0));
        };
        let velkosti: Vec<u32> = if let Some(stsz) = tab(b"stsz") {
            let pevna = be32(&stsz[4..]);
            let n = be32(&stsz[8..]) as usize;
            if pevna != 0 {
                vec![pevna; n]
            } else {
                (0..n)
                    .filter_map(|k| stsz.get(12 + 4 * k..16 + 4 * k).map(be32))
                    .collect()
            }
        } else if let Some(stz2) = tab(b"stz2") {
            let bity = stz2[7];
            let n = be32(&stz2[8..]) as usize;
            (0..n)
                .map(|k| match bity {
                    4 => ((stz2[12 + k / 2] >> if k % 2 == 0 { 4 } else { 0 }) & 0xF) as u32,
                    8 => stz2[12 + k] as u32,
                    _ => be16(&stz2[12 + 2 * k..]) as u32,
                })
                .collect()
        } else {
            return Ok((vec![], 0));
        };
        let n = velkosti.len();
        if n == 0 {
            return Ok((vec![], 0));
        }
        let ofsety: Vec<u64> = if let Some(stco) = tab(b"stco") {
            (0..be32(&stco[4..]) as usize)
                .filter_map(|k| stco.get(8 + 4 * k..12 + 4 * k).map(|b| be32(b) as u64))
                .collect()
        } else if let Some(co64) = tab(b"co64") {
            (0..be32(&co64[4..]) as usize)
                .filter_map(|k| co64.get(8 + 8 * k..16 + 8 * k).map(be64))
                .collect()
        } else {
            return Err(Chyba::Format("MP4 bez tabuľky ofsetov".into()));
        };
        // časy
        let mut dts = Vec::with_capacity(n);
        let mut t = 0i64;
        for k in 0..be32(&stts[4..]) as usize {
            let z = stts
                .get(8 + 8 * k..16 + 8 * k)
                .ok_or_else(|| Chyba::Format("poškodené stts".into()))?;
            for _ in 0..be32(z) {
                dts.push(t);
                t += be32(&z[4..]) as i64;
            }
        }
        let mut cto = vec![0i32; n];
        if let Some(ctts) = tab(b"ctts") {
            let mut i = 0;
            for k in 0..be32(&ctts[4..]) as usize {
                let Some(z) = ctts.get(8 + 8 * k..16 + 8 * k) else {
                    break;
                };
                let posun = be32(&z[4..]) as i32;
                for _ in 0..be32(z) {
                    if i < n {
                        cto[i] = posun;
                    }
                    i += 1;
                }
            }
        }
        let kluc: Option<Vec<u32>> = tab(b"stss").map(|s| {
            (0..be32(&s[4..]) as usize)
                .filter_map(|k| s.get(8 + 4 * k..12 + 4 * k).map(be32))
                .collect()
        });
        // stsc: rozdelenie vzoriek do kúskov
        let zaznamy: Vec<(u32, u32)> = (0..be32(&stsc[4..]) as usize)
            .filter_map(|k| {
                stsc.get(8 + 12 * k..20 + 12 * k)
                    .map(|z| (be32(z), be32(&z[4..])))
            })
            .collect();
        let mut out = Vec::with_capacity(n);
        let mut vzorka = 0usize;
        for (ki, &ofset) in ofsety.iter().enumerate() {
            let cislo = ki as u32 + 1;
            let na_kusok = zaznamy
                .iter()
                .rev()
                .find(|(prvy, _)| *prvy <= cislo)
                .map(|(_, p)| *p)
                .unwrap_or(1);
            let mut o = ofset;
            for _ in 0..na_kusok {
                if vzorka >= n {
                    break;
                }
                let k = match &kluc {
                    Some(z) => z.binary_search(&(vzorka as u32 + 1)).is_ok(),
                    None => true,
                };
                out.push((
                    o,
                    velkosti[vzorka],
                    *dts.get(vzorka).unwrap_or(&t),
                    cto[vzorka],
                    k,
                ));
                o += velkosti[vzorka] as u64;
                vzorka += 1;
            }
        }
        Ok((out, t))
    }

    /// Ďalší fragment (moof) → položky do fronty.
    fn fragment(&mut self) -> Vysledok<bool> {
        while self.poz + 8 <= self.dlzka_suboru {
            let (typ, obsah, koniec) =
                Self::hlavicka_boxu(&mut self.f, self.poz, self.dlzka_suboru)?;
            let zaciatok = self.poz;
            self.poz = koniec;
            if &typ != b"moof" {
                continue;
            }
            let mut moof = vec![0u8; (koniec - obsah) as usize];
            self.f.seek(SeekFrom::Start(obsah))?;
            self.f.read_exact(&mut moof)?;
            let mut polozky = vec![];
            for traf in deti(&moof, 0, moof.len())
                .into_iter()
                .filter(|b| &b.typ == b"traf")
            {
                let t = &moof[traf.obsah as usize..traf.koniec as usize];
                let Some(tfhd) = najdi(t, 0, t.len(), b"tfhd") else {
                    continue;
                };
                let priznaky = be32(tfhd) & 0xFF_FFFF;
                let id = be32(&tfhd[4..]);
                let Some(si) = self.stopy.iter().position(|s| s.id == id) else {
                    continue;
                };
                if najdi(t, 0, t.len(), b"senc").is_some() {
                    return Err(Chyba::Drm("šifrované fragmenty MP4".into()));
                }
                let mut i = 8;
                let mut zaklad = zaciatok;
                if priznaky & 0x01 != 0 {
                    zaklad = be64(&tfhd[i..]);
                    i += 8;
                }
                if priznaky & 0x02 != 0 {
                    i += 4;
                }
                let st = &self.stopy[si];
                let mut predv_trvanie = st.trex.trvanie;
                let mut predv_velkost = st.trex.velkost;
                let mut predv_priznaky = st.trex.priznaky;
                if priznaky & 0x08 != 0 {
                    predv_trvanie = be32(&tfhd[i..]);
                    i += 4;
                }
                if priznaky & 0x10 != 0 {
                    predv_velkost = be32(&tfhd[i..]);
                    i += 4;
                }
                if priznaky & 0x20 != 0 {
                    predv_priznaky = be32(&tfhd[i..]);
                }
                let mut dts = match najdi(t, 0, t.len(), b"tfdt") {
                    Some(d) if d[0] == 1 => be64(&d[4..]) as i64,
                    Some(d) => be32(&d[4..]) as i64,
                    None => st.dalsi_dts,
                };
                let (mierka, posun, zvuk) = (st.mierka, st.posun, st.zvuk);
                let mut dalsi_ofset = zaklad;
                for trun in deti(t, 0, t.len())
                    .into_iter()
                    .filter(|b| &b.typ == b"trun")
                {
                    let r = &t[trun.obsah as usize..trun.koniec as usize];
                    let v1 = r[0] == 1;
                    let pr = be32(r) & 0xFF_FFFF;
                    let n = be32(&r[4..]) as usize;
                    let mut j = 8;
                    let mut ofset = dalsi_ofset;
                    if pr & 0x01 != 0 {
                        ofset = (zaklad as i64 + be32(&r[j..]) as i32 as i64) as u64;
                        j += 4;
                    }
                    let mut prve_priznaky = None;
                    if pr & 0x04 != 0 {
                        prve_priznaky = Some(be32(&r[j..]));
                        j += 4;
                    }
                    for k in 0..n {
                        let mut trv = predv_trvanie;
                        let mut vel = predv_velkost;
                        let mut prz = if k == 0 {
                            prve_priznaky.unwrap_or(predv_priznaky)
                        } else {
                            predv_priznaky
                        };
                        let mut cto = 0i64;
                        let mut citaj = |bit: u32| -> Option<u32> {
                            if pr & bit != 0 {
                                let v = r.get(j..j + 4).map(be32);
                                j += 4;
                                v
                            } else {
                                None
                            }
                        };
                        if let Some(v) = citaj(0x100) {
                            trv = v;
                        }
                        if let Some(v) = citaj(0x200) {
                            vel = v;
                        }
                        if let Some(v) = citaj(0x400) {
                            prz = v;
                        }
                        if let Some(v) = citaj(0x800) {
                            cto = if v1 { v as i32 as i64 } else { v as i64 };
                        }
                        let kluc = zvuk || (prz >> 16) & 1 == 0;
                        polozky.push(Polozka {
                            stopa: si,
                            offset: ofset,
                            velkost: vel,
                            dts: na_ns(dts, mierka) + posun,
                            pts: na_ns(dts + cto, mierka) + posun,
                            trvanie: na_ns(trv as i64, mierka),
                            kluc,
                        });
                        ofset += vel as u64;
                        dts += trv as i64;
                    }
                    dalsi_ofset = ofset;
                }
                self.stopy[si].dalsi_dts = dts;
            }
            polozky.sort_by_key(|p| (p.dts, p.stopa));
            self.fronta.extend(polozky);
            return Ok(true);
        }
        Ok(false)
    }

    fn precitaj(&mut self, p: &Polozka) -> Vysledok<Vzorka> {
        if p.offset + p.velkost as u64 > self.dlzka_suboru {
            return Err(Chyba::Format(
                "vzorka MP4 mimo súboru (useknutý súbor?)".into(),
            ));
        }
        let mut data = vec![0u8; p.velkost as usize];
        self.f.seek(SeekFrom::Start(p.offset))?;
        self.f.read_exact(&mut data)?;
        Ok(Vzorka {
            stopa: p.stopa,
            dts: p.dts,
            pts: p.pts,
            trvanie: p.trvanie,
            kluc: p.kluc,
            data,
        })
    }
}

impl Citac for CitacMp4 {
    fn stopy(&self) -> &[InfoStopy] {
        &self.stopy_info
    }

    fn dalsia(&mut self) -> Vysledok<Option<Vzorka>> {
        loop {
            let Some(mut v) = self.surova()? else {
                return Ok(None);
            };
            if !self.stopy[v.stopa].text {
                return Ok(Some(v));
            }
            if let Some(t) = text_vzorky(&v.data) {
                v.data = t.into_bytes();
                v.kluc = true;
                return Ok(Some(v));
            }
        }
    }
}

impl CitacMp4 {
    /// Ďalšia vzorka tak, ako je v súbore.
    fn surova(&mut self) -> Vysledok<Option<Vzorka>> {
        if self.fragmentovane {
            loop {
                if let Some(p) = self.fronta.pop_front() {
                    return self.precitaj(&p).map(Some);
                }
                if !self.fragment()? {
                    return Ok(None);
                }
            }
        }
        // bežné MP4: ďalšia vzorka s najmenším dts naprieč stopami
        let mut naj: Option<(usize, i64)> = None;
        for (i, s) in self.stopy.iter().enumerate() {
            if let Some(v) = s.vzorky.get(s.dalsia) {
                let d = na_ns(v.2, s.mierka) + s.posun;
                if naj.is_none_or_lt(d) {
                    naj = Some((i, d));
                }
            }
        }
        let Some((i, _)) = naj else { return Ok(None) };
        let s = &mut self.stopy[i];
        let (ofset, velkost, dts, cto, kluc) = s.vzorky[s.dalsia];
        let trvanie = s
            .vzorky
            .get(s.dalsia + 1)
            .map(|d| d.2 - dts)
            .unwrap_or_else(|| {
                if s.koniec > dts {
                    s.koniec - dts // posledná vzorka: trvanie zo stts
                } else if s.dalsia > 0 {
                    dts - s.vzorky[s.dalsia - 1].2
                } else {
                    0
                }
            });
        s.dalsia += 1;
        let p = Polozka {
            stopa: i,
            offset: ofset,
            velkost,
            dts: na_ns(dts, s.mierka) + s.posun,
            pts: na_ns(dts + cto as i64, s.mierka) + s.posun,
            trvanie: na_ns(trvanie, s.mierka),
            kluc: kluc || s.zvuk,
        };
        self.precitaj(&p).map(Some)
    }
}

trait MensieAko {
    fn is_none_or_lt(&self, d: i64) -> bool;
}

impl MensieAko for Option<(usize, i64)> {
    fn is_none_or_lt(&self, d: i64) -> bool {
        match self {
            None => true,
            Some((_, x)) => d < *x,
        }
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn esds_aac() {
        // esds z ffmpeg: AAC LC 48 kHz stereo
        let e = [
            0, 0, 0, 0, 0x03, 0x19, 0x00, 0x01, 0x00, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 1, 0xF4,
            0, 0, 1, 0xF4, 0, 0x05, 0x02, 0x11, 0x90, 0x06, 0x01, 0x02,
        ];
        assert_eq!(esds(&e), Some((0x40, vec![0x11, 0x90])));
    }

    #[test]
    fn text_titulkov() {
        assert_eq!(
            text_vzorky(b"\x00\x05Ahoj!\x00\x00\x00\x0cstyl").as_deref(),
            Some("Ahoj!")
        );
        assert_eq!(text_vzorky(&[0, 0]), None);
        assert_eq!(
            text_vzorky(&[0, 6, 0xFE, 0xFF, 0, b'O', 0, b'K']).as_deref(),
            Some("OK")
        );
        assert_eq!(nazov_stopy(b"\x00\x03\x15\xc7abc").as_deref(), Some("abc"));
        assert_eq!(
            nazov_stopy("Čeština".as_bytes()).as_deref(),
            Some("Čeština")
        );
    }

    #[test]
    fn jazyk_z_mdhd() {
        // „ces“ = (3,5,19) → 0x0CB3
        let z = (3u16 << 10) | (5 << 5) | 19;
        assert_eq!(jazyk_mdhd(z).as_deref(), Some("cze"));
        assert_eq!(jazyk_mdhd(0x55C4), None); // und
    }
}
