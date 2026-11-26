//! Zápis Matroska (MKV/MKA) – vlastný zapisovač EBML – a kontrola zapísaného súboru.
//!
//! Štruktúra: EBML hlavička, Segment { SeekHead (v rezervovanom mieste), Info, Tracks, Tags,
//! Attachments (obrázok videa), Cluster…, Chapters, Cues }. Kapitoly idú až za clustre, lebo
//! koniec poslednej kapitoly je koniec videa, ktorý sa zistí až pri zápise.
//! Clustre začínajú na kľúčových snímkach obrazu, aby sa dalo rýchlo pretáčať. Veľkosti, trvanie
//! a odkazy sa po zápise doplnia na svoje miesta.

use super::{Doplnky, Druh, InfoStopy, Kapitola, Kodek};
use crate::chyba::{Chyba, Vysledok};
use crate::spajanie::iso639_2;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

// identifikátory prvkov
const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEKHEAD: u32 = 0x114D_9B74;
const SEEK: u32 = 0x4DBB;
const SEEKID: u32 = 0x53AB;
const SEEKPOS: u32 = 0x53AC;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const CUES: u32 = 0x1C53_BB6B;
const VOID: u32 = 0xEC;
const TIMESTAMP: u32 = 0xE7;
const SIMPLEBLOCK: u32 = 0xA3;
const CHAPTERS: u32 = 0x1043_A770;
const ATTACHMENTS: u32 = 0x1941_A469;
const TAGS: u32 = 0x1254_C367;

/// Stopa výsledného súboru: čo o nej vie čítač + jazyk, názov a predvolenosť.
#[derive(Clone, Debug)]
pub struct StopaVystupu {
    pub info: InfoStopy,
    /// BCP 47 alebo ISO 639 (napr. „cs“, „en-US“, „slo“)
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
    pub predvolena: bool,
    /// v pôvodnom jazyku videa (Some(true)), preklad/dabing (Some(false)), nevie sa (None)
    pub povodna: Option<bool>,
}

fn id_bajty(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    let n = if id >= 0x100_0000 {
        4
    } else if id >= 0x1_0000 {
        3
    } else if id >= 0x100 {
        2
    } else {
        1
    };
    b[4 - n..].to_vec()
}

fn vint(n: u64) -> Vec<u8> {
    for dlzka in 1..=8u32 {
        if n < (1u64 << (7 * dlzka)) - 1 {
            let mut v = n | (1u64 << (7 * dlzka));
            let mut out = vec![0u8; dlzka as usize];
            for i in (0..dlzka as usize).rev() {
                out[i] = v as u8;
                v >>= 8;
            }
            return out;
        }
    }
    vint8(n).to_vec()
}

fn vint8(n: u64) -> [u8; 8] {
    let mut b = n.to_be_bytes();
    b[0] = 0x01;
    b
}

fn prvok(id: u32, telo: &[u8]) -> Vec<u8> {
    let mut o = id_bajty(id);
    o.extend(vint(telo.len() as u64));
    o.extend_from_slice(telo);
    o
}

fn uint(id: u32, v: u64) -> Vec<u8> {
    let b = v.to_be_bytes();
    let z = b.iter().position(|&x| x != 0).unwrap_or(7);
    prvok(id, &b[z..])
}

fn float(id: u32, v: f64) -> Vec<u8> {
    prvok(id, &v.to_be_bytes())
}

fn text(id: u32, s: &str) -> Vec<u8> {
    prvok(id, s.as_bytes())
}

fn void(dlzka: usize) -> Vec<u8> {
    // celková dĺžka presne `dlzka` (aspoň 2 bajty)
    let mut o = vec![VOID as u8];
    if dlzka <= 128 {
        o.extend(vint8((dlzka - 2) as u64)[7..].iter().map(|b| b | 0x80));
        o.resize(dlzka, 0);
    } else {
        o.extend(vint8((dlzka - 9) as u64));
        o.resize(dlzka, 0);
    }
    o
}

/// dOps (MP4) → OpusHead (Matroska CodecPrivate).
fn opus_head(dops: &[u8]) -> Option<(Vec<u8>, u16)> {
    if dops.len() < 11 {
        return None;
    }
    let kanaly = dops[1];
    let preskip = u16::from_be_bytes([dops[2], dops[3]]);
    let frekv = u32::from_be_bytes([dops[4], dops[5], dops[6], dops[7]]);
    let zisk = i16::from_be_bytes([dops[8], dops[9]]);
    let rodina = dops[10];
    let mut o = b"OpusHead".to_vec();
    o.push(1);
    o.push(kanaly);
    o.extend(preskip.to_le_bytes());
    o.extend(frekv.to_le_bytes());
    o.extend(zisk.to_le_bytes());
    o.push(rodina);
    if rodina != 0 {
        o.extend(dops.get(11..11 + 2 + kanaly as usize)?);
    }
    Some((o, preskip))
}

fn kodek_id(k: Kodek) -> &'static str {
    match k {
        Kodek::H264 => "V_MPEG4/ISO/AVC",
        Kodek::Hevc => "V_MPEGH/ISO/HEVC",
        Kodek::Av1 => "V_AV1",
        Kodek::Vp9 => "V_VP9",
        Kodek::Aac => "A_AAC",
        Kodek::Opus => "A_OPUS",
        Kodek::Mp3 => "A_MPEG/L3",
        Kodek::Ac3 => "A_AC3",
        Kodek::Eac3 => "A_EAC3",
        Kodek::Srt => "S_TEXT/UTF8",
    }
}

/// Jednoduchý jedinečný identifikátor (čas + poradie), bez cudzích knižníc.
fn uid(seme: u64) -> u64 {
    let cas = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // splitmix64
    let mut z = cas ^ seme.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) | 1
}

fn stopa_prvok(cislo: usize, s: &StopaVystupu) -> Vysledok<Vec<u8>> {
    let i = &s.info;
    let mut t = vec![];
    t.extend(uint(0xD7, cislo as u64 + 1));
    t.extend(uint(0x73C5, uid(cislo as u64 + 7)));
    t.extend(uint(
        0x83,
        match i.druh() {
            Druh::Video => 1,
            Druh::Zvuk => 2,
            Druh::Titulky => 0x11,
        },
    ));
    t.extend(uint(0xB9, 1)); // FlagEnabled
    t.extend(uint(0x88, s.predvolena as u64)); // FlagDefault (predvolená hodnota je 1 – zapisuje sa vždy)
    t.extend(uint(0x55AA, 0)); // FlagForced
    if let Some(p) = s.povodna {
        t.extend(uint(0x55AE, p as u64)); // FlagOriginal (Matroska v4)
    }
    t.extend(uint(0x9C, 0)); // FlagLacing
    let jazyk = s.jazyk.clone().or_else(|| i.jazyk.clone());
    match &jazyk {
        Some(j) => {
            t.extend(text(0x22B59C, &iso639_2(j)));
            let bcp = j.replace('_', "-");
            if bcp.len() <= 12 && bcp.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                t.extend(text(0x22B59D, &bcp));
            }
        }
        None => t.extend(text(0x22B59C, "und")),
    }
    if let Some(n) = &s.nazov {
        t.extend(text(0x536E, n));
    }
    t.extend(text(0x86, kodek_id(i.kodek)));
    match i.kodek {
        Kodek::H264 | Kodek::Hevc | Kodek::Aac => {
            if i.konfig.is_empty() {
                return Err(Chyba::Format(format!(
                    "stopa {} bez nastavení dekodéra",
                    kodek_id(i.kodek)
                )));
            }
            t.extend(prvok(0x63A2, &i.konfig));
        }
        Kodek::Av1 => {
            if !i.konfig.is_empty() {
                t.extend(prvok(0x63A2, &i.konfig));
            }
        }
        Kodek::Opus => {
            let (hlava, preskip) = opus_head(&i.konfig)
                .ok_or_else(|| Chyba::Format("Opus bez platného dOps".into()))?;
            t.extend(prvok(0x63A2, &hlava));
            // CodecDelay sa nezapisuje: časy vzoriek už majú posun zo zdroja (edit list), takže by sa
            // odpočítal dvakrát a zvuk by predbiehal obraz. Úvodné vzorky dekodéra sú len ticho.
            let _ = preskip;
            t.extend(uint(0x56BB, 80_000_000)); // SeekPreRoll
        }
        _ => {}
    }
    if i.druh() == Druh::Titulky {
        // text nemá nastavenia dekodéra ani obraz/zvuk
    } else if i.druh() == Druh::Video {
        let mut v = vec![];
        if i.sirka > 0 && i.vyska > 0 {
            v.extend(uint(0xB0, i.sirka as u64));
            v.extend(uint(0xBA, i.vyska as u64));
        }
        t.extend(prvok(0xE0, &v));
    } else {
        let mut a = vec![];
        let f = if i.kodek == Kodek::Opus {
            48_000
        } else {
            i.frekvencia.max(1)
        };
        a.extend(float(0xB5, f as f64));
        a.extend(uint(0x9F, i.kanaly.max(1) as u64));
        t.extend(prvok(0xE1, &a));
    }
    Ok(prvok(0xAE, &t))
}

const REZERVA_SEEKHEAD: usize = 256;

/// Tags: údaje o celom videu (cieľ 50 = film/epizóda) – názvy podľa zoznamu značiek Matroska.
fn znacky(d: &Doplnky) -> Vec<u8> {
    let mut polozky: Vec<(&str, &str)> = vec![];
    if !d.titul.is_empty() {
        polozky.push(("TITLE", &d.titul));
    }
    for (meno, hodnota) in [
        ("ARTIST", &d.autor),
        ("DATE_RELEASED", &d.datum),
        ("DESCRIPTION", &d.popis),
        ("URL", &d.zdroj),
    ] {
        if let Some(h) = hodnota.as_deref().filter(|h| !h.trim().is_empty()) {
            polozky.push((meno, h));
        }
    }
    if polozky.is_empty() {
        return vec![];
    }
    // len TargetTypeValue 50 (bez názvu cieľa) – prehrávače to berú ako údaje celého súboru
    let mut tag = prvok(0x63C0, &uint(0x68CA, 50));
    for (meno, hodnota) in polozky {
        let mut st = text(0x45A3, meno);
        st.extend(text(0x447A, "und"));
        st.extend(uint(0x4484, 1));
        st.extend(text(0x4487, hodnota));
        tag.extend(prvok(0x67C8, &st));
    }
    prvok(TAGS, &prvok(0x7373, &tag))
}

/// Attachments: obrázok videa ako „cover.jpg/png“ – tak ho hľadajú prehrávače a knižnice médií.
fn prilohy(d: &Doplnky) -> Vec<u8> {
    let Some(o) = &d.obal else { return vec![] };
    let mut f = text(0x467E, "Obrázok videa");
    f.extend(text(0x466E, &format!("cover.{}", o.pripona())));
    f.extend(text(0x4660, o.mime()));
    f.extend(prvok(0x465C, &o.data));
    f.extend(uint(0x46AE, uid(0xC0FE)));
    prvok(ATTACHMENTS, &prvok(0x61A7, &f))
}

/// Chapters: jedno vydanie, každá kapitola od svojho začiatku po začiatok ďalšej (posledná po koniec).
fn kapitoly(k: &[Kapitola], koniec_ns: i64) -> Vec<u8> {
    if k.is_empty() {
        return vec![];
    }
    let mut ed = uint(0x45BC, uid(0xED17));
    ed.extend(uint(0x45BD, 0)); // EditionFlagHidden
    ed.extend(uint(0x45DB, 1)); // EditionFlagDefault
    for (i, x) in k.iter().enumerate() {
        let do_ = k.get(i + 1).map(|n| n.od).unwrap_or(koniec_ns).max(x.od);
        let mut a = uint(0x73C4, uid(0xCA00 + i as u64));
        a.extend(uint(0x91, x.od.max(0) as u64));
        a.extend(uint(0x92, do_.max(0) as u64));
        a.extend(uint(0x98, 0)); // ChapterFlagHidden
        a.extend(uint(0x4598, 1)); // ChapterFlagEnabled
        let mut zobraz = text(0x85, &x.nazov);
        zobraz.extend(text(0x437C, "und"));
        a.extend(prvok(0x80, &zobraz));
        ed.extend(prvok(0xB6, &a));
    }
    prvok(CHAPTERS, &prvok(0x45B9, &ed))
}

pub struct ZapisovacMkv {
    f: BufWriter<File>,
    seg_data: u64,
    seekhead_poz: u64,
    trvanie_poz: u64,
    info_poz: u64,
    tracks_poz: u64,
    /// ďalšie prvky pred clustrami (Tags, Attachments): (ID, pozícia v Segmente)
    dalsie_poz: Vec<(u32, u64)>,
    kapitoly: Vec<Kapitola>,
    pocet_stop: usize,
    video: Option<usize>,
    klaster: Vec<u8>,
    klaster_cas: i64,
    klaster_otvoreny: bool,
    klaster_cue: Option<(i64, usize)>,
    cues: Vec<(i64, usize, u64)>,
    pocty: Vec<u64>,
    koniec_ns: i64,
    /// stopy titulkov – zapisujú sa ako BlockGroup s BlockDuration
    titulky: Vec<bool>,
}

impl ZapisovacMkv {
    pub fn new(cesta: &Path, stopy: &[StopaVystupu], doplnky: &Doplnky) -> Vysledok<Self> {
        let titul = doplnky.titul.as_str();
        if stopy.is_empty() || stopy.len() > 126 {
            return Err(Chyba::Format("nesprávny počet stôp pre MKV".into()));
        }
        let mut f = BufWriter::with_capacity(1 << 20, File::create(cesta)?);
        // EBML hlavička
        let mut h = vec![];
        h.extend(uint(0x4286, 1));
        h.extend(uint(0x42F7, 1));
        h.extend(uint(0x42F2, 4));
        h.extend(uint(0x42F3, 8));
        h.extend(text(0x4282, "matroska"));
        h.extend(uint(0x4287, 4));
        h.extend(uint(0x4285, 2));
        f.write_all(&prvok(EBML, &h))?;
        // Segment s neznámou veľkosťou (doplní sa)
        f.write_all(&id_bajty(SEGMENT))?;
        f.write_all(&vint8(0))?;
        let seg_data = f.stream_position()?;
        let seekhead_poz = seg_data;
        f.write_all(&void(REZERVA_SEEKHEAD))?;
        // Info
        let info_poz = f.stream_position()? - seg_data;
        let mut info = vec![];
        info.extend(uint(0x2AD7B1, 1_000_000)); // TimestampScale: 1 ms
        let app = format!("Kometa {}", crate::VERZIA);
        info.extend(text(0x4D80, &app));
        info.extend(text(0x5741, &app));
        if !titul.is_empty() {
            info.extend(text(0x7BA9, titul));
        }
        let mut uid_segmentu = uid(99).to_be_bytes().to_vec();
        uid_segmentu.extend(uid(101).to_be_bytes());
        info.extend(prvok(0x73A4, &uid_segmentu));
        let pred_trvanim = info.len();
        info.extend(float(0x4489, 0.0)); // Duration – doplní sa
        f.write_all(&id_bajty(INFO))?;
        f.write_all(&vint(info.len() as u64))?;
        let telo_info = f.stream_position()?;
        f.write_all(&info)?;
        let trvanie_poz = telo_info + pred_trvanim as u64 + 3; // ID (2 B) + veľkosť (1 B)
                                                               // Tracks
        let tracks_poz = f.stream_position()? - seg_data;
        let mut t = vec![];
        for (k, s) in stopy.iter().enumerate() {
            t.extend(stopa_prvok(k, s)?);
        }
        f.write_all(&prvok(TRACKS, &t))?;
        let mut dalsie_poz = vec![];
        for (id, telo) in [(TAGS, znacky(doplnky)), (ATTACHMENTS, prilohy(doplnky))] {
            if !telo.is_empty() {
                dalsie_poz.push((id, f.stream_position()? - seg_data));
                f.write_all(&telo)?;
            }
        }
        Ok(ZapisovacMkv {
            f,
            seg_data,
            seekhead_poz,
            trvanie_poz,
            info_poz,
            tracks_poz,
            dalsie_poz,
            kapitoly: doplnky.kapitoly.clone(),
            pocet_stop: stopy.len(),
            video: stopy.iter().position(|s| s.info.druh() == Druh::Video),
            klaster: vec![],
            klaster_cas: 0,
            klaster_otvoreny: false,
            klaster_cue: None,
            cues: vec![],
            pocty: vec![0; stopy.len()],
            koniec_ns: 0,
            titulky: stopy
                .iter()
                .map(|s| s.info.druh() == Druh::Titulky)
                .collect(),
        })
    }

    fn zavri_klaster(&mut self) -> Vysledok<()> {
        if !self.klaster_otvoreny {
            return Ok(());
        }
        let poz = self.f.stream_position()? - self.seg_data;
        self.f.write_all(&id_bajty(CLUSTER))?;
        self.f.write_all(&vint(self.klaster.len() as u64))?;
        self.f.write_all(&self.klaster)?;
        if let Some((cas, stopa)) = self.klaster_cue.take() {
            self.cues.push((cas, stopa, poz));
        }
        self.klaster.clear();
        self.klaster_otvoreny = false;
        Ok(())
    }

    /// Zapíše vzorku. `pts` v ns od začiatku súboru (≥ 0).
    pub fn zapis(
        &mut self,
        stopa: usize,
        pts: i64,
        trvanie: i64,
        kluc: bool,
        data: &[u8],
    ) -> Vysledok<()> {
        let ms = (pts.max(0) + 500_000) / 1_000_000;
        let je_video = Some(stopa) == self.video;
        let rel = ms - self.klaster_cas;
        let novy = !self.klaster_otvoreny
            || (je_video && kluc && rel >= 1000)
            || (self.video.is_none() && rel >= 5000)
            || !(-30_000..=30_000).contains(&rel)
            || self.klaster.len() > 16 << 20;
        if novy {
            self.zavri_klaster()?;
            self.klaster_cas = ms;
            self.klaster_otvoreny = true;
            self.klaster.extend(uint(TIMESTAMP, ms as u64));
            // bod na pretáčanie: kľúčová snímka obrazu (alebo prvá stopa pri zvuku bez obrazu)
            let cue_stopa = self.video.unwrap_or(0);
            if stopa == cue_stopa && kluc {
                self.klaster_cue = Some((ms, stopa));
            }
        }
        let rel = (ms - self.klaster_cas) as i16;
        let mut blok = Vec::with_capacity(data.len() + 4);
        blok.push(0x80 | (stopa as u8 + 1));
        blok.extend(rel.to_be_bytes());
        if self.titulky[stopa] {
            // titulok: Block + BlockDuration (ako dlho ostane na obrazovke)
            blok.push(0);
            blok.extend_from_slice(data);
            let mut g = prvok(0xA1, &blok);
            g.extend(uint(0x9B, ((trvanie.max(0) + 500_000) / 1_000_000) as u64));
            self.klaster.extend(prvok(0xA0, &g));
        } else {
            blok.push(if kluc { 0x80 } else { 0 });
            blok.extend_from_slice(data);
            self.klaster.extend(id_bajty(SIMPLEBLOCK));
            self.klaster.extend(vint(blok.len() as u64));
            self.klaster.extend(blok);
        }
        self.pocty[stopa] += 1;
        if !self.titulky[stopa] {
            self.koniec_ns = self.koniec_ns.max(pts + trvanie.max(0));
        }
        Ok(())
    }

    /// Dokončí súbor (Cues, SeekHead, trvanie, veľkosť) a vráti počty vzoriek podľa stôp.
    pub fn dokonci(self) -> Vysledok<Vec<u64>> {
        self.dokonci_s().map(|(p, _)| p)
    }

    /// Ako [`ZapisovacMkv::dokonci`], navyše vráti počet zapísaných kapitol.
    pub fn dokonci_s(mut self) -> Vysledok<(Vec<u64>, usize)> {
        self.zavri_klaster()?;
        let mut ciele = vec![(INFO, self.info_poz), (TRACKS, self.tracks_poz)];
        ciele.extend(self.dalsie_poz.iter().copied());
        let kap = Doplnky {
            kapitoly: std::mem::take(&mut self.kapitoly),
            ..Default::default()
        }
        .kapitoly_do(self.koniec_ns);
        let k = kapitoly(&kap, self.koniec_ns);
        if !k.is_empty() {
            ciele.push((CHAPTERS, self.f.stream_position()? - self.seg_data));
            self.f.write_all(&k)?;
        }
        let cues_poz = self.f.stream_position()? - self.seg_data;
        let mut c = vec![];
        for (cas, stopa, poz) in &self.cues {
            let mut tp = vec![];
            tp.extend(uint(0xF7, *stopa as u64 + 1));
            tp.extend(uint(0xF1, *poz));
            let mut cp = uint(0xB3, *cas as u64);
            cp.extend(prvok(0xB7, &tp));
            c.extend(prvok(0xBB, &cp));
        }
        if !c.is_empty() {
            self.f.write_all(&prvok(CUES, &c))?;
        }
        let koniec = self.f.stream_position()?;
        // SeekHead do rezervovaného miesta
        let mut sh = vec![];
        if !c.is_empty() {
            ciele.push((CUES, cues_poz));
        }
        for (id, poz) in ciele {
            let mut s = prvok(SEEKID, &id_bajty(id));
            s.extend(uint(SEEKPOS, poz));
            sh.extend(prvok(SEEK, &s));
        }
        let mut sh = prvok(SEEKHEAD, &sh);
        let zvysok = REZERVA_SEEKHEAD - sh.len();
        sh.extend(void(zvysok));
        self.f.seek(SeekFrom::Start(self.seekhead_poz))?;
        self.f.write_all(&sh)?;
        // trvanie (ms) a veľkosť Segmentu
        self.f.seek(SeekFrom::Start(self.trvanie_poz))?;
        self.f
            .write_all(&(self.koniec_ns as f64 / 1e6).to_be_bytes())?;
        self.f.seek(SeekFrom::Start(self.seg_data - 8))?;
        self.f.write_all(&vint8(koniec - self.seg_data))?;
        self.f.flush()?;
        let f = self.f.into_inner().map_err(|e| Chyba::Io(e.into_error()))?;
        f.sync_all()?;
        let _ = self.pocet_stop;
        Ok((self.pocty, kap.len()))
    }
}

// ------------------------------------------------------------------------------------------
// kontrola zapísaného súboru

fn citaj_vint(d: &[u8], i: &mut usize, id: bool) -> Option<u64> {
    let prvy = *d.get(*i)?;
    let dlzka = prvy.leading_zeros() as usize + 1;
    if dlzka > 8 {
        return None;
    }
    let mut v = if id {
        prvy as u64
    } else {
        (prvy as u64) & ((1u64 << (8 - dlzka)) - 1)
    };
    for k in 1..dlzka {
        v = (v << 8) | *d.get(*i + k)? as u64;
    }
    *i += dlzka;
    Some(v)
}

/// Čo našla kontrola v zapísanom MKV.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct KontrolaMkv {
    pub pocty: Vec<u64>,
    pub kapitoly: usize,
    pub prilohy: usize,
    pub znacky: usize,
}

/// Overí štruktúru súboru a počty blokov podľa stôp. Vráti počty blokov.
pub fn over(cesta: &Path, pocet_stop: usize) -> Vysledok<Vec<u64>> {
    over_vsetko(cesta, pocet_stop).map(|k| k.pocty)
}

/// Počet detí s daným ID (o jednu úroveň nižšie) v tele prvku.
fn pocet_deti(telo: &[u8], hladane: u32, vnorit: &[u32]) -> usize {
    let mut q = 0;
    let mut n = 0;
    while q < telo.len() {
        let (Some(id), Some(vel)) = (
            citaj_vint(telo, &mut q, true),
            citaj_vint(telo, &mut q, false),
        ) else {
            break;
        };
        let koniec = (q + vel as usize).min(telo.len());
        if id as u32 == hladane {
            n += 1;
        } else if let Some((prvy, zvysok)) = vnorit.split_first() {
            if id as u32 == *prvy {
                n += pocet_deti(&telo[q..koniec], hladane, zvysok);
            }
        }
        q = koniec;
    }
    n
}

/// Overí štruktúru, počty blokov podľa stôp a spočíta kapitoly, prílohy a značky.
pub fn over_vsetko(cesta: &Path, pocet_stop: usize) -> Vysledok<KontrolaMkv> {
    let zle = |s: &str| Chyba::Format(format!("kontrola MKV: {s}"));
    let mut f = File::open(cesta)?;
    let dlzka = f.metadata()?.len();
    let mut hlava = vec![0u8; 64.min(dlzka as usize)];
    f.read_exact(&mut hlava)?;
    let mut i = 0;
    if citaj_vint(&hlava, &mut i, true) != Some(EBML as u64) {
        return Err(zle("chýba hlavička EBML"));
    }
    let h = citaj_vint(&hlava, &mut i, false).ok_or_else(|| zle("hlavička"))? as usize;
    if !hlava[i..(i + h).min(hlava.len())]
        .windows(8)
        .any(|w| w == b"matroska")
    {
        return Err(zle("nie je to Matroska"));
    }
    i += h;
    if citaj_vint(&hlava, &mut i, true) != Some(SEGMENT as u64) {
        return Err(zle("chýba Segment"));
    }
    let velkost = citaj_vint(&hlava, &mut i, false).ok_or_else(|| zle("veľkosť Segmentu"))?;
    let seg = i as u64;
    if seg + velkost != dlzka {
        return Err(zle("veľkosť Segmentu nesedí s dĺžkou súboru"));
    }
    let mut pocty = vec![0u64; pocet_stop];
    let (mut kap, mut pril, mut zn) = (0usize, 0usize, 0usize);
    let (mut ma_info, mut ma_tracks) = (false, false);
    let mut poz = seg;
    let mut hl = [0u8; 16];
    while poz < dlzka {
        f.seek(SeekFrom::Start(poz))?;
        let n = f.read(&mut hl)?;
        let mut j = 0;
        let id = citaj_vint(&hl[..n], &mut j, true).ok_or_else(|| zle("poškodený prvok"))?;
        let vel = citaj_vint(&hl[..n], &mut j, false).ok_or_else(|| zle("poškodený prvok"))?;
        let telo = poz + j as u64;
        if telo + vel > dlzka {
            return Err(zle("prvok presahuje koniec súboru"));
        }
        match id as u32 {
            INFO => ma_info = true,
            TRACKS => ma_tracks = true,
            CHAPTERS | ATTACHMENTS | TAGS if vel < 64 << 20 => {
                let mut b = vec![0u8; vel as usize];
                f.seek(SeekFrom::Start(telo))?;
                f.read_exact(&mut b)?;
                match id as u32 {
                    CHAPTERS => kap += pocet_deti(&b, 0xB6, &[0x45B9]),
                    ATTACHMENTS => pril += pocet_deti(&b, 0x61A7, &[]),
                    _ => zn += pocet_deti(&b, 0x67C8, &[0x7373]),
                }
            }
            CLUSTER => {
                let mut k = vec![0u8; vel as usize];
                f.seek(SeekFrom::Start(telo))?;
                f.read_exact(&mut k)?;
                let mut q = 0;
                while q < k.len() {
                    let cid =
                        citaj_vint(&k, &mut q, true).ok_or_else(|| zle("poškodený cluster"))?;
                    let cvel = citaj_vint(&k, &mut q, false)
                        .ok_or_else(|| zle("poškodený cluster"))?
                        as usize;
                    if q + cvel > k.len() {
                        return Err(zle("blok presahuje cluster"));
                    }
                    // SimpleBlock, alebo BlockGroup (titulky) s Block vnútri
                    let blok = match cid as u32 {
                        SIMPLEBLOCK => Some(q),
                        0xA0 => {
                            let mut r = q;
                            let bid = citaj_vint(&k, &mut r, true);
                            let _ = citaj_vint(&k, &mut r, false);
                            (bid == Some(0xA1)).then_some(r)
                        }
                        _ => None,
                    };
                    if let Some(mut r) = blok {
                        let stopa =
                            citaj_vint(&k, &mut r, false).ok_or_else(|| zle("blok"))? as usize;
                        if stopa == 0 || stopa > pocet_stop {
                            return Err(zle("blok neznámej stopy"));
                        }
                        pocty[stopa - 1] += 1;
                    }
                    q += cvel;
                }
            }
            _ => {}
        }
        poz = telo + vel;
    }
    if !ma_info || !ma_tracks {
        return Err(zle("chýba Info alebo Tracks"));
    }
    Ok(KontrolaMkv {
        pocty,
        kapitoly: kap,
        prilohy: pril,
        znacky: zn,
    })
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn kodovanie_ebml() {
        assert_eq!(vint(0), vec![0x80]);
        assert_eq!(vint(126), vec![0xFE]);
        assert_eq!(vint(127), vec![0x40, 0x7F]);
        assert_eq!(vint(16_382), vec![0x7F, 0xFE]);
        assert_eq!(id_bajty(EBML), vec![0x1A, 0x45, 0xDF, 0xA3]);
        assert_eq!(id_bajty(0xA3), vec![0xA3]);
        assert_eq!(uint(0xD7, 1), vec![0xD7, 0x81, 1]);
        assert_eq!(uint(0x83, 0), vec![0x83, 0x81, 0]);
        for n in [2usize, 3, 10, 127, 128, 129, 200] {
            let v = void(n);
            assert_eq!(v.len(), n);
            let mut i = 0;
            assert_eq!(citaj_vint(&v, &mut i, true), Some(VOID as u64));
            let vel = citaj_vint(&v, &mut i, false).unwrap() as usize;
            assert_eq!(i + vel, n, "void {n}");
        }
    }

    #[test]
    fn opus_hlavicka() {
        let dops = [0, 2, 0x01, 0x38, 0, 0, 0xBB, 0x80, 0, 0, 0];
        let (h, preskip) = opus_head(&dops).unwrap();
        assert_eq!(preskip, 312);
        assert_eq!(&h[..8], b"OpusHead");
        assert_eq!(&h[8..], &[1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
    }
}
