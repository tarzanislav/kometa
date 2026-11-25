//! Čítanie Matroska a WebM (MKV, MKA, WEBM) – opak zapisovača v [`super::mkv`].
//!
//! Číta sa postupne, bez načítania celého súboru: hlavička EBML, Segment, Info (mierka času),
//! Tracks a potom zhluky (Cluster) s blokmi (SimpleBlock, BlockGroup) – aj s nepoznanou dĺžkou
//! (živé WebM) a so všetkými druhmi zhlukovania rámcov (lacing: Xiph, pevné, EBML).
//!
//! Matroska ukladá len čas zobrazenia. Pre H.264 a HEVC (snímky B) sa čas dekódovania dopočíta:
//! bloky sú v poradí dekódovania, takže k-ty najmenší čas zobrazenia patrí k-temu bloku, posunutý
//! o oneskorenie zmerané na začiatku stopy. Ostatné kodeky (VP9, AV1, zvuk) poradie nemenia.

use super::{InfoStopy, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const TRACK_ENTRY: u32 = 0xAE;
const TIMESTAMP: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const BLOCK_DURATION: u32 = 0x9B;
const REFERENCE_BLOCK: u32 = 0xFB;
/// Prvky na úrovni Segmentu – ukončia zhluk s nepoznanou dĺžkou.
const NAJVYSSIE: [u32; 7] = [
    CLUSTER,
    0x1C53_BB6B,
    0x1254_C367,
    0x1043_A770,
    0x114D_9B74,
    INFO,
    TRACKS,
];
const NEZNAMA: u64 = u64::MAX;
/// Koľko blokov dopredu sa pozerá pri dopočítaní času dekódovania (hĺbka snímok B).
const OKNO: usize = 16;

fn chyba(t: impl Into<String>) -> Chyba {
    Chyba::Format(format!("Matroska: {}", t.into()))
}

/// Čítač s vlastnou pozíciou (aby sa dalo preskakovať bez otázok na systém).
struct Zdroj {
    r: BufReader<File>,
    poz: u64,
    dlzka: u64,
}

impl Zdroj {
    fn bajt(&mut self) -> Vysledok<Option<u8>> {
        let mut b = [0u8; 1];
        match self.r.read(&mut b)? {
            0 => Ok(None),
            _ => {
                self.poz += 1;
                Ok(Some(b[0]))
            }
        }
    }

    fn citaj(&mut self, n: usize) -> Vysledok<Vec<u8>> {
        if n as u64 > self.dlzka.saturating_sub(self.poz) {
            return Err(chyba("prvok siaha za koniec súboru"));
        }
        let mut v = vec![0u8; n];
        self.r.read_exact(&mut v)?;
        self.poz += n as u64;
        Ok(v)
    }

    fn preskoc(&mut self, n: u64) -> Vysledok<()> {
        self.r.seek_relative(n as i64)?;
        self.poz += n;
        Ok(())
    }

    fn na(&mut self, poz: u64) -> Vysledok<()> {
        self.r.seek(SeekFrom::Start(poz))?;
        self.poz = poz;
        Ok(())
    }

    /// ID prvku (so značkou dĺžky, ako sa píše v špecifikácii); `None` na konci súboru.
    fn id(&mut self) -> Vysledok<Option<u32>> {
        let Some(b) = self.bajt()? else {
            return Ok(None);
        };
        let n = b.leading_zeros() as usize + 1;
        if n > 4 {
            return Err(chyba("neplatné ID prvku"));
        }
        let mut v = b as u32;
        for _ in 1..n {
            v = v << 8 | self.bajt()?.ok_or_else(|| chyba("ID na konci súboru"))? as u32;
        }
        Ok(Some(v))
    }

    /// Dĺžka prvku; samé jednotky = nepoznaná.
    fn velkost(&mut self) -> Vysledok<u64> {
        let b = self.bajt()?.ok_or_else(|| chyba("dĺžka na konci súboru"))?;
        let n = b.leading_zeros() as usize + 1;
        if n > 8 {
            return Err(chyba("neplatná dĺžka prvku"));
        }
        let mut v = (b as u64) & (0xFF >> n);
        let mut jednotky = v == (0xFF >> n);
        for _ in 1..n {
            let x = self.bajt()?.ok_or_else(|| chyba("dĺžka na konci súboru"))?;
            jednotky &= x == 0xFF;
            v = v << 8 | x as u64;
        }
        Ok(if jednotky { NEZNAMA } else { v })
    }
}

/// Celé číslo bez znamienka z obsahu prvku.
fn uint(d: &[u8]) -> u64 {
    d.iter().take(8).fold(0, |a, &b| a << 8 | b as u64)
}

fn float(d: &[u8]) -> f64 {
    match d.len() {
        4 => f32::from_be_bytes([d[0], d[1], d[2], d[3]]) as f64,
        8 => f64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]),
        _ => 0.0,
    }
}

fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d)
        .trim_end_matches('\0')
        .to_string()
}

/// Podprvky v pamäti: (id, obsah).
fn prvky(mut d: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = vec![];
    let vint = |d: &[u8], s_znackou: bool| -> Option<(u64, usize)> {
        let b = *d.first()?;
        let n = b.leading_zeros() as usize + 1;
        if n > 8 || d.len() < n {
            return None;
        }
        let mut v = if s_znackou {
            b as u64
        } else {
            (b as u64) & (0xFF >> n)
        };
        for x in &d[1..n] {
            v = v << 8 | *x as u64;
        }
        Some((v, n))
    };
    while !d.is_empty() {
        let Some((id, a)) = vint(d, true) else { break };
        let Some((n, b)) = vint(&d[a..], false) else {
            break;
        };
        let zac = a + b;
        let kon = zac.saturating_add(n as usize).min(d.len());
        out.push((id as u32, &d[zac..kon]));
        d = &d[kon..];
    }
    out
}

/// OpusHead (Matroska, Ogg) → dOps (MP4) – rovnaký tvar, aký vracia čítač MP4.
pub fn dops_z_opushead(h: &[u8]) -> Option<Vec<u8>> {
    if h.len() < 19 || &h[..8] != b"OpusHead" {
        return None;
    }
    let kanaly = h[9];
    let rodina = h[18];
    let mut o = vec![0, kanaly];
    o.extend(u16::from_le_bytes([h[10], h[11]]).to_be_bytes());
    o.extend(u32::from_le_bytes([h[12], h[13], h[14], h[15]]).to_be_bytes());
    o.extend(i16::from_le_bytes([h[16], h[17]]).to_be_bytes());
    o.push(rodina);
    if rodina != 0 {
        o.extend(h.get(19..21 + kanaly as usize)?);
    }
    Some(o)
}

/// Obsah boxu vpcC (verzia 1) z hlavičky kľúčovej snímky VP9 – WebM ho zvyčajne neuvádza,
/// MP4 ho vyžaduje. Profil, bitová hĺbka a podvzorkovanie farieb sa čítajú priamo zo snímky.
pub fn vpcc_z_vp9(snimka: &[u8], sirka: u32, vyska: u32) -> Option<Vec<u8>> {
    let mut b = super::bity::Bity::new(snimka);
    if b.bity(2)? != 2 {
        return None; // frame_marker
    }
    let nizky = b.bity(1)?;
    let vysoky = b.bity(1)?;
    let profil = (vysoky << 1 | nizky) as u8;
    if profil == 3 {
        b.bity(1)?;
    }
    if b.bity(1)? == 1 {
        return None; // show_existing_frame
    }
    if b.bity(1)? != 0 {
        return None; // nie kľúčová snímka
    }
    b.bity(2)?; // show_frame, error_resilient_mode
    if b.bity(24)? != 0x49_8342 {
        return None; // sync code
    }
    let hlbka = if profil >= 2 {
        if b.bity(1)? == 1 {
            12
        } else {
            10
        }
    } else {
        8
    };
    let farby = b.bity(3)?;
    // podvzorkovanie podľa vpcC: 1 = 4:2:0 kolokované, 2 = 4:2:2, 3 = 4:4:4
    let (rozsah, podvz) = if farby == 7 {
        (1u8, 3u8) // RGB = 4:4:4, plný rozsah
    } else {
        let rozsah = b.bity(1)? as u8;
        let podvz = if profil == 1 || profil == 3 {
            match (b.bity(1)?, b.bity(1)?) {
                (1, 0) => 2,
                (0, 0) => 3,
                _ => 1,
            }
        } else {
            1
        };
        (rozsah, podvz)
    };
    // úroveň podľa rozlíšenia (VP9 levels): 720p → 3.1, 1080p → 4.1, 4K → 5.1, viac → 6.1
    let plocha = sirka as u64 * vyska as u64;
    let uroven = match plocha {
        0..=921_600 => 31,
        921_601..=2_228_224 => 41,
        2_228_225..=8_912_896 => 51,
        _ => 61,
    };
    // primárne farby/prenos/matica podľa farebného priestoru VP9 (BT.601, 709, 2020, sRGB)
    let (p, t, m) = match farby {
        1 => (6, 6, 6),
        2 | 5 => (1, 1, 1),
        3 => (6, 6, 6),
        4 => (9, 14, 9),
        7 => (1, 13, 0),
        _ => (2, 2, 2),
    };
    Some(vec![
        1,
        0,
        0,
        0,
        profil,
        uroven,
        (hlbka << 4) | (podvz << 1) | rozsah,
        p,
        t,
        m,
        0,
        0,
    ])
}

struct Stopa {
    cislo: u64,
    info: InfoStopy,
    /// posun začiatku (CodecDelay, Opus) v ns – odpočíta sa, ako to robí zoznam úprav v MP4
    posun: i64,
    predvolene_trvanie: i64,
    /// kodek mení poradie snímok (H.264, HEVC) → dopočítať čas dekódovania
    preusporiadanie: bool,
}

/// Dopočítanie času dekódovania pre stopu so snímkami B.
#[derive(Default)]
struct Dekodovanie {
    cakajuce: VecDeque<Vzorka>,
    casy: BinaryHeap<Reverse<i64>>,
    oneskorenie: Option<i64>,
    posledne: Option<i64>,
}

impl Dekodovanie {
    fn vloz(&mut self, s: Vzorka) {
        self.casy.push(Reverse(s.pts));
        self.cakajuce.push_back(s);
    }

    fn vydaj(&mut self, vsetko: bool) -> Option<Vzorka> {
        if self.cakajuce.is_empty() || (!vsetko && self.cakajuce.len() <= OKNO) {
            return None;
        }
        if self.oneskorenie.is_none() {
            // o koľko je k-ty najmenší čas neskôr než čas k-teho bloku – najviac na začiatku stopy
            let mut zoradene: Vec<i64> = self.cakajuce.iter().map(|s| s.pts).collect();
            zoradene.sort_unstable();
            let d = self
                .cakajuce
                .iter()
                .zip(&zoradene)
                .map(|(s, z)| z - s.pts)
                .max()
                .unwrap_or(0);
            self.oneskorenie = Some(d.max(0));
        }
        let mut s = self.cakajuce.pop_front()?;
        let Reverse(najmensi) = self.casy.pop()?;
        let mut dts = najmensi - self.oneskorenie.unwrap_or(0);
        if let Some(p) = self.posledne {
            dts = dts.max(p + 1);
        }
        s.dts = dts.min(s.pts.max(self.posledne.map_or(i64::MIN, |p| p + 1)));
        self.posledne = Some(s.dts);
        Some(s)
    }
}

pub struct CitacMkv {
    z: Zdroj,
    stopy: Vec<InfoStopy>,
    detaily: Vec<Stopa>,
    mierka: i64,
    koniec_segmentu: u64,
    /// koniec aktuálneho zhluku (`None` = mimo zhluku, NEZNAMA = do ďalšieho prvku najvyššej úrovne)
    zhluk: Option<u64>,
    cas_zhluku: i64,
    pripravene: Vec<VecDeque<Vzorka>>,
    dekodovanie: Vec<Dekodovanie>,
    koniec: bool,
}

fn kodek(id: &str) -> Option<Kodek> {
    Some(match id {
        "V_MPEG4/ISO/AVC" => Kodek::H264,
        "V_MPEGH/ISO/HEVC" => Kodek::Hevc,
        "V_AV1" => Kodek::Av1,
        "V_VP9" => Kodek::Vp9,
        "A_AAC" | "A_AAC/MPEG4/LC" | "A_AAC/MPEG2/LC" => Kodek::Aac,
        "A_OPUS" => Kodek::Opus,
        "A_MPEG/L3" => Kodek::Mp3,
        "A_AC3" => Kodek::Ac3,
        "A_EAC3" => Kodek::Eac3,
        "S_TEXT/UTF8" => Kodek::Srt,
        _ => return None,
    })
}

impl CitacMkv {
    pub fn otvor(cesta: &Path) -> Vysledok<Self> {
        let f = File::open(cesta)?;
        let dlzka = f.metadata()?.len();
        let mut z = Zdroj {
            r: BufReader::with_capacity(1 << 16, f),
            poz: 0,
            dlzka,
        };
        if z.id()? != Some(EBML) {
            return Err(chyba("súbor nezačína hlavičkou EBML"));
        }
        let n = z.velkost()?;
        let hlava = z.citaj(n as usize)?;
        let typ = prvky(&hlava)
            .iter()
            .find(|(i, _)| *i == 0x4282)
            .map(|(_, d)| text(d))
            .unwrap_or_default();
        if !matches!(typ.as_str(), "matroska" | "webm") {
            return Err(Chyba::Nepodporovane(format!("dokument EBML typu „{typ}“")));
        }
        if z.id()? != Some(SEGMENT) {
            return Err(chyba("chýba Segment"));
        }
        let n = z.velkost()?;
        let koniec_segmentu = if n == NEZNAMA {
            dlzka
        } else {
            (z.poz + n).min(dlzka)
        };
        let mut c = CitacMkv {
            z,
            stopy: vec![],
            detaily: vec![],
            mierka: 1_000_000,
            koniec_segmentu,
            zhluk: None,
            cas_zhluku: 0,
            pripravene: vec![],
            dekodovanie: vec![],
            koniec: false,
        };
        // hlavička segmentu až po prvý zhluk
        let mut prvy_zhluk = None;
        while c.z.poz < c.koniec_segmentu {
            let zac = c.z.poz;
            let Some(id) = c.z.id()? else { break };
            let n = c.z.velkost()?;
            match id {
                INFO if n != NEZNAMA => {
                    let d = c.z.citaj(n as usize)?;
                    if let Some((_, m)) = prvky(&d).into_iter().find(|(i, _)| *i == 0x2AD7B1) {
                        c.mierka = (uint(m) as i64).max(1);
                    }
                }
                TRACKS if n != NEZNAMA => {
                    let d = c.z.citaj(n as usize)?;
                    c.stopy_z(&d)?;
                }
                CLUSTER => {
                    prvy_zhluk = Some(zac);
                    break;
                }
                _ if n == NEZNAMA => return Err(chyba("prvok s nepoznanou dĺžkou pred zhlukmi")),
                _ => c.z.preskoc(n)?,
            }
        }
        if c.detaily.is_empty() {
            return Err(Chyba::Format(
                "v súbore nie je obraz ani zvuk, ktorý Kometa pozná".into(),
            ));
        }
        let start = prvy_zhluk.unwrap_or(c.koniec_segmentu);
        c.pripravene = (0..c.detaily.len()).map(|_| VecDeque::new()).collect();
        c.dekodovanie = (0..c.detaily.len())
            .map(|_| Dekodovanie::default())
            .collect();
        c.z.na(start)?;
        c.doplnit_vp9(start)?;
        c.stopy = c.detaily.iter().map(|s| s.info.clone()).collect();
        Ok(c)
    }

    fn stopy_z(&mut self, d: &[u8]) -> Vysledok<()> {
        for (id, t) in prvky(d) {
            if id != TRACK_ENTRY {
                continue;
            }
            let p = prvky(t);
            let najdi = |k: u32| p.iter().find(|(i, _)| *i == k).map(|(_, d)| *d);
            let Some(k) = najdi(0x86).map(text).as_deref().and_then(kodek) else {
                continue;
            };
            let cislo = najdi(0xD7).map(uint).unwrap_or(0);
            let mut info = InfoStopy {
                kodek: k,
                konfig: najdi(0x63A2).map(<[u8]>::to_vec).unwrap_or_default(),
                sirka: 0,
                vyska: 0,
                frekvencia: 0,
                kanaly: 0,
                jazyk: najdi(0x22B59C)
                    .map(text)
                    .filter(|j| !j.is_empty() && j != "und"),
                nazov: najdi(0x536E).map(text).filter(|n| !n.is_empty()),
            };
            if let Some(v) = najdi(0xE0) {
                for (i, x) in prvky(v) {
                    match i {
                        0xB0 => info.sirka = uint(x) as u32,
                        0xBA => info.vyska = uint(x) as u32,
                        _ => {}
                    }
                }
            }
            if let Some(a) = najdi(0xE1) {
                for (i, x) in prvky(a) {
                    match i {
                        0xB5 => info.frekvencia = float(x).round() as u32,
                        0x9F => info.kanaly = uint(x) as u16,
                        _ => {}
                    }
                }
            }
            if k == Kodek::Opus {
                info.konfig = dops_z_opushead(&info.konfig)
                    .ok_or_else(|| chyba("Opus bez platného OpusHead"))?;
                info.frekvencia = 48_000;
                info.kanaly = info.konfig[1] as u16;
            }
            if matches!(k, Kodek::H264 | Kodek::Hevc | Kodek::Aac) && info.konfig.is_empty() {
                return Err(chyba("stopa bez nastavení dekodéra (CodecPrivate)"));
            }
            self.detaily.push(Stopa {
                cislo,
                info,
                posun: najdi(0x56AA).map(uint).unwrap_or(0) as i64,
                predvolene_trvanie: najdi(0x23E383).map(uint).unwrap_or(0) as i64,
                preusporiadanie: matches!(k, Kodek::H264 | Kodek::Hevc),
            });
        }
        Ok(())
    }

    /// VP9 bez vpcC: dopočíta ho z prvej kľúčovej snímky (pozrie sa na začiatok zhlukov).
    fn doplnit_vp9(&mut self, start: u64) -> Vysledok<()> {
        if !self
            .detaily
            .iter()
            .any(|s| s.info.kodek == Kodek::Vp9 && s.info.konfig.is_empty())
        {
            return Ok(());
        }
        let mut videne = 0;
        while videne < 200 {
            let Some(s) = self.citaj_blok()? else { break };
            videne += 1;
            let i = s.stopa;
            let d = &mut self.detaily[i];
            if d.info.kodek == Kodek::Vp9 && d.info.konfig.is_empty() && s.kluc {
                if let Some(v) = vpcc_z_vp9(&s.data, d.info.sirka, d.info.vyska) {
                    d.info.konfig = v;
                }
            }
            if !self
                .detaily
                .iter()
                .any(|s| s.info.kodek == Kodek::Vp9 && s.info.konfig.is_empty())
            {
                break;
            }
        }
        // späť na začiatok
        for q in &mut self.pripravene {
            q.clear();
        }
        for d in &mut self.dekodovanie {
            *d = Dekodovanie::default();
        }
        self.zhluk = None;
        self.koniec = false;
        self.z.na(start)
    }

    /// Prečíta ďalší blok a jeho rámce uloží do `pripravene`; vráti prvý z nich (len na pozretie).
    fn citaj_blok(&mut self) -> Vysledok<Option<Vzorka>> {
        loop {
            if self.z.poz >= self.koniec_segmentu {
                return Ok(None);
            }
            if let Some(k) = self.zhluk {
                if k != NEZNAMA && self.z.poz >= k {
                    self.zhluk = None;
                    continue;
                }
            }
            let Some(id) = self.z.id()? else {
                return Ok(None);
            };
            let n = self.z.velkost()?;
            if id == CLUSTER {
                self.zhluk = Some(if n == NEZNAMA {
                    NEZNAMA
                } else {
                    self.z.poz + n
                });
                self.cas_zhluku = 0;
                continue;
            }
            if self.zhluk.is_none() || NAJVYSSIE.contains(&id) {
                // mimo zhluku (Cues, Tags…) alebo koniec zhluku s nepoznanou dĺžkou
                if n == NEZNAMA {
                    return Ok(None);
                }
                self.zhluk = None;
                self.z.preskoc(n)?;
                continue;
            }
            if n == NEZNAMA {
                return Err(chyba("prvok zhluku s nepoznanou dĺžkou"));
            }
            match id {
                TIMESTAMP => {
                    let d = self.z.citaj(n as usize)?;
                    self.cas_zhluku = uint(&d) as i64;
                }
                SIMPLE_BLOCK => {
                    let d = self.z.citaj(n as usize)?;
                    if let Some(v) = self.blok(&d, None, None)? {
                        return Ok(Some(v));
                    }
                }
                BLOCK_GROUP => {
                    let d = self.z.citaj(n as usize)?;
                    let p = prvky(&d);
                    let trvanie = p
                        .iter()
                        .find(|(i, _)| *i == BLOCK_DURATION)
                        .map(|(_, x)| uint(x) as i64);
                    let kluc = !p.iter().any(|(i, _)| *i == REFERENCE_BLOCK);
                    if let Some((_, b)) = p.iter().find(|(i, _)| *i == BLOCK) {
                        if let Some(v) = self.blok(b, Some(kluc), trvanie)? {
                            return Ok(Some(v));
                        }
                    }
                }
                _ => self.z.preskoc(n)?,
            }
        }
    }

    /// Rozloží blok na rámce (aj zhlukované) a vloží ich do frontu stopy.
    fn blok(
        &mut self,
        d: &[u8],
        kluc_skupiny: Option<bool>,
        trvanie: Option<i64>,
    ) -> Vysledok<Option<Vzorka>> {
        let b0 = *d.first().ok_or_else(|| chyba("prázdny blok"))?;
        let n = b0.leading_zeros() as usize + 1;
        if n > 8 || d.len() < n + 3 {
            return Err(chyba("poškodený blok"));
        }
        let cislo = d[1..n]
            .iter()
            .fold((b0 as u64) & (0xFF >> n), |a, &x| a << 8 | x as u64);
        let Some(i) = self.detaily.iter().position(|s| s.cislo == cislo) else {
            return Ok(None);
        };
        let rel = i16::from_be_bytes([d[n], d[n + 1]]) as i64;
        let priznaky = d[n + 2];
        let telo = &d[n + 3..];
        let ramce: Vec<&[u8]> = match (priznaky >> 1) & 3 {
            0 => vec![telo],
            druh => rozloz(telo, druh)?,
        };
        let s = &self.detaily[i];
        let kluc = match kluc_skupiny {
            Some(k) => k,
            None => priznaky & 0x80 != 0,
        } || s.info.kodek.druh() != super::Druh::Video;
        let zaklad = (self.cas_zhluku + rel) * self.mierka - s.posun;
        let krok = if s.predvolene_trvanie > 0 {
            s.predvolene_trvanie
        } else {
            trvanie
                .map(|t| t * self.mierka / ramce.len().max(1) as i64)
                .unwrap_or(0)
        };
        let prvy = Vzorka {
            stopa: i,
            dts: zaklad,
            pts: zaklad,
            trvanie: if ramce.len() == 1 {
                trvanie
                    .map(|t| t * self.mierka)
                    .unwrap_or(s.predvolene_trvanie)
            } else {
                krok
            },
            kluc,
            data: vec![],
        };
        let preusporiadanie = s.preusporiadanie;
        for (k, r) in ramce.iter().enumerate() {
            let t = zaklad + krok * k as i64;
            let v = Vzorka {
                dts: t,
                pts: t,
                data: r.to_vec(),
                ..prvy.clone()
            };
            // stopy so snímkami B najprv prejdú dopočítaním času dekódovania
            if preusporiadanie {
                self.dekodovanie[i].vloz(v);
            } else {
                self.pripravene[i].push_back(v);
            }
        }
        Ok(Some(Vzorka {
            data: ramce.first().map(|r| r.to_vec()).unwrap_or_default(),
            ..prvy
        }))
    }
}

/// Rámce zhlukovaného bloku: 1 = Xiph, 2 = pevná dĺžka, 3 = EBML.
fn rozloz(d: &[u8], druh: u8) -> Vysledok<Vec<&[u8]>> {
    let pocet = *d.first().ok_or_else(|| chyba("prázdny zhluk rámcov"))? as usize + 1;
    let mut p = 1;
    let mut dlzky: Vec<usize> = vec![];
    match druh {
        1 => {
            for _ in 0..pocet - 1 {
                let mut l = 0;
                loop {
                    let b = *d.get(p).ok_or_else(|| chyba("zhluk Xiph"))?;
                    p += 1;
                    l += b as usize;
                    if b != 255 {
                        break;
                    }
                }
                dlzky.push(l);
            }
        }
        2 => {
            let l = (d.len() - 1) / pocet;
            dlzky = vec![l; pocet - 1];
        }
        _ => {
            let vint = |d: &[u8], p: &mut usize, so_znamienkom: bool| -> Vysledok<i64> {
                let b = *d.get(*p).ok_or_else(|| chyba("zhluk EBML"))?;
                let n = b.leading_zeros() as usize + 1;
                if n > 8 || *p + n > d.len() {
                    return Err(chyba("zhluk EBML"));
                }
                let v = d[*p + 1..*p + n]
                    .iter()
                    .fold((b as u64) & (0xFF >> n), |a, &x| a << 8 | x as u64)
                    as i64;
                *p += n;
                Ok(if so_znamienkom {
                    v - ((1i64 << (7 * n - 1)) - 1)
                } else {
                    v
                })
            };
            let mut l = vint(d, &mut p, false)?;
            dlzky.push(l as usize);
            for _ in 1..pocet - 1 {
                l += vint(d, &mut p, true)?;
                dlzky.push(l.max(0) as usize);
            }
        }
    }
    let mut out = vec![];
    for l in dlzky {
        let r = d
            .get(p..p + l)
            .ok_or_else(|| chyba("rámec za koncom bloku"))?;
        out.push(r);
        p += l;
    }
    out.push(&d[p.min(d.len())..]);
    Ok(out)
}

impl super::Citac for CitacMkv {
    fn stopy(&self) -> &[InfoStopy] {
        &self.stopy
    }

    fn dalsia(&mut self) -> Vysledok<Option<Vzorka>> {
        loop {
            // ďalšia vzorka = najskoršia spomedzi stôp, keď má každá aktívna stopa aspoň jednu
            let hlava = |c: &mut CitacMkv, i: usize, vsetko: bool| -> Option<i64> {
                if c.detaily[i].preusporiadanie && c.pripravene[i].is_empty() {
                    if let Some(s) = c.dekodovanie[i].vydaj(vsetko) {
                        c.pripravene[i].push_back(s);
                    }
                }
                c.pripravene[i].front().map(|s| s.dts)
            };
            let koniec = self.koniec;
            let mut najlepsia: Option<(usize, i64)> = None;
            let mut chyba_niekde = false;
            for i in 0..self.detaily.len() {
                match hlava(self, i, koniec) {
                    Some(t) => {
                        if najlepsia.map_or(true, |(_, x)| t < x) {
                            najlepsia = Some((i, t));
                        }
                    }
                    None => chyba_niekde |= !koniec && self.ma_zasobu_na_cakanie(),
                }
            }
            if let Some((i, _)) = najlepsia {
                if self.koniec || !chyba_niekde {
                    return Ok(self.pripravene[i].pop_front());
                }
            } else if self.koniec {
                return Ok(None);
            }
            if self.citaj_blok()?.is_none() {
                self.koniec = true;
            }
        }
    }
}

impl CitacMkv {
    /// Keď niektorá stopa ešte nemá vzorku, oplatí sa čítať ďalej (aby sa nepredbehlo poradie),
    /// ale len kým zásoba ostatných nie je priveľká – stopa, ktorá skončila skôr, nič nezdrží.
    fn ma_zasobu_na_cakanie(&self) -> bool {
        let zasoba: usize = self.pripravene.iter().map(VecDeque::len).sum::<usize>()
            + self
                .dekodovanie
                .iter()
                .map(|d| d.cakajuce.len())
                .sum::<usize>();
        zasoba < 4096
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn opushead_na_dops() {
        let mut h = b"OpusHead".to_vec();
        h.extend([1, 2]);
        h.extend(312u16.to_le_bytes());
        h.extend(48_000u32.to_le_bytes());
        h.extend(0i16.to_le_bytes());
        h.push(0);
        let d = dops_z_opushead(&h).unwrap();
        assert_eq!(d, vec![0, 2, 0x01, 0x38, 0, 0, 0xBB, 0x80, 0, 0, 0]);
    }

    #[test]
    fn zhlukovanie_ramcov() {
        // Xiph: 3 rámce dĺžok 2, 300, zvyšok 1
        let mut d = vec![2u8, 2, 255, 45];
        d.extend([7u8; 2]);
        d.extend([8u8; 300]);
        d.push(9);
        let r = rozloz(&d, 1).unwrap();
        assert_eq!(
            r.iter().map(|x| x.len()).collect::<Vec<_>>(),
            vec![2, 300, 1]
        );
        // pevná dĺžka
        let r = rozloz(&[1, 1, 2, 3, 4], 2).unwrap();
        assert_eq!(r, vec![&[1u8, 2][..], &[3, 4][..]]);
        // EBML: dĺžky 3, 3+1=4, zvyšok
        let d = [2u8, 0x83, 0xC0, 1, 1, 1, 2, 2, 2, 2, 3];
        let r = rozloz(&d, 3).unwrap();
        assert_eq!(r.iter().map(|x| x.len()).collect::<Vec<_>>(), vec![3, 4, 1]);
    }

    #[test]
    fn cas_dekodovania_snimok_b() {
        // poradie dekódovania I P B B s časmi zobrazenia 0, 3, 1, 2 (po 40 ms)
        let mut d = Dekodovanie::default();
        let ms = 1_000_000;
        for (k, p) in [0, 3, 1, 2, 4, 7, 5, 6].iter().enumerate() {
            d.vloz(Vzorka {
                stopa: 0,
                dts: 0,
                pts: p * 40 * ms,
                trvanie: 0,
                kluc: k == 0,
                data: vec![],
            });
        }
        let mut out = vec![];
        while let Some(s) = d.vydaj(true) {
            out.push((s.dts, s.pts));
        }
        for w in out.windows(2) {
            assert!(w[0].0 < w[1].0, "{out:?}");
        }
        assert!(out.iter().all(|(dts, pts)| dts <= pts), "{out:?}");
        assert_eq!(out[0].0, -40 * ms);
    }
}
