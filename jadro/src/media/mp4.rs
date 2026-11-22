//! Čítanie ISO BMFF (ISO/IEC 14496-12): bežné MP4 aj fragmentované MP4 (HLS fMP4, DASH).
//!
//! Hlavička `moov` sa načíta celá (je malá), vzorky sa čítajú prúdovo – zo súboru sa naraz
//! drží v pamäti len jeden fragment, nikdy celé video. Šifrované stopy (`encv`, `enca`,
//! `pssh`) vrátia [`Chyba::Drm`] – Kometa šifrovaný obsah nesťahuje ani nerozbaľuje.

use super::bity::{be16, be32, be64};
use super::{aac, na_ns, Citac, InfoStopy, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use std::collections::VecDeque;
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
    dalsia: usize,
    /// fragmentované: dts nasledujúcej vzorky (ak fragment nemá tfdt)
    dalsi_dts: i64,
    zvuk: bool,
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
    }))
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
        for trak in deti(&moov, 0, moov.len())
            .into_iter()
            .filter(|b| &b.typ == b"trak")
        {
            let t = &moov[trak.obsah as usize..trak.koniec as usize];
            let tkhd = najdi(t, 0, t.len(), b"tkhd").unwrap_or(&[]);
            let id = if tkhd.len() >= 24 {
                if tkhd[0] == 1 {
                    be32(&tkhd[20..])
                } else {
                    be32(&tkhd[12..])
                }
            } else {
                0
            };
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
            let vzorky = Self::tabulky(stbl)?;
            stopy_info.push(info);
            stopy.push(Stopa {
                id,
                mierka,
                posun,
                trex,
                vzorky,
                dalsia: 0,
                dalsi_dts: 0,
                zvuk,
            });
        }
        if stopy.is_empty() {
            return Err(Chyba::Format("MP4 bez obrazu aj zvuku".into()));
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
    fn tabulky(stbl: &[u8]) -> Vysledok<Vec<ZaznamVzorky>> {
        let tab = |t: &[u8; 4]| najdi(stbl, 0, stbl.len(), t);
        let (Some(stts), Some(stsc)) = (tab(b"stts"), tab(b"stsc")) else {
            return Ok(vec![]);
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
            return Ok(vec![]);
        };
        let n = velkosti.len();
        if n == 0 {
            return Ok(vec![]);
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
        Ok(out)
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
                if s.dalsia > 0 {
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
    fn jazyk_z_mdhd() {
        // „ces“ = (3,5,19) → 0x0CB3
        let z = (3u16 << 10) | (5 << 5) | 19;
        assert_eq!(jazyk_mdhd(z).as_deref(), Some("cze"));
        assert_eq!(jazyk_mdhd(0x55C4), None); // und
    }
}
