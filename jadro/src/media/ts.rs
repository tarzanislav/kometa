//! Čítanie MPEG-TS (ISO/IEC 13818-1) – kontajner kúskov HLS.
//!
//! PAT → PMT → elementárne prúdy; PES sa skladajú z 188-bajtových paketov. Podporované:
//! H.264 a AAC (ADTS) – to je takmer všetko HLS. Metadáta (ID3, SCTE-35, titulky) sa vynechajú;
//! iný obraz alebo zvuk vráti [`Chyba::Nepodporovane`], aby sa stopa nestratila potichu.

use super::{aac, h264, na_ns, Citac, InfoStopy, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const PAKET: usize = 188;
const HODINY: u32 = 90_000;
const OBVOD: i64 = 1 << 33;
/// Koľko paketov najviac prečítať, kým sa nájdu nastavenia všetkých stôp (≈ 19 MB).
const MAX_PRIESKUM: usize = 100_000;

struct Prud {
    kodek: Kodek,
    stopa: usize,
    pes: Vec<u8>,
    // H.264
    spsy: Vec<Vec<u8>>,
    ppsy: Vec<Vec<u8>>,
    // AAC
    zvysok: Vec<u8>,
    dalsi_cas: Option<i64>,
    // rozbalenie 33-bitových časov a nespojitosti
    posledny: Option<i64>,
    korekcia: i64,
    posledne_dts: Option<i64>,
}

pub struct CitacTs {
    vstup: BufReader<File>,
    /// bajty prečítané navyše pri hľadaní synchronizácie
    predbuf: Vec<u8>,
    pmt: Option<u16>,
    prudy: Vec<Prud>,
    stopy: Vec<InfoStopy>,
    pidy: HashMap<u16, usize>,
    fronta: VecDeque<Vzorka>,
    koniec: bool,
    pripravene: bool,
}

fn cas_pes(b: &[u8]) -> i64 {
    (((b[0] as i64 >> 1) & 7) << 30)
        | ((b[1] as i64) << 22)
        | ((b[2] as i64 >> 1) << 15)
        | ((b[3] as i64) << 7)
        | (b[4] as i64 >> 1)
}

impl CitacTs {
    pub fn otvor(cesta: &Path) -> Vysledok<Self> {
        let mut c = CitacTs {
            vstup: BufReader::with_capacity(1 << 20, File::open(cesta)?),
            predbuf: vec![],
            pmt: None,
            prudy: vec![],
            stopy: vec![],
            pidy: HashMap::new(),
            fronta: VecDeque::new(),
            koniec: false,
            pripravene: false,
        };
        // prieskum: PMT a nastavenia dekodérov (SPS/PPS, prvá hlavička ADTS)
        let mut n = 0;
        while !c.pripravene && !c.koniec {
            c.paket()?;
            n += 1;
            if c.pmt.is_some()
                && !c.prudy.is_empty()
                && c.stopy.iter().all(|s| !s.konfig.is_empty())
            {
                c.pripravene = true;
            }
            if n > MAX_PRIESKUM {
                break;
            }
        }
        if c.koniec && !c.pripravene {
            c.dokonci()?;
            c.pripravene = c.pmt.is_some() && c.stopy.iter().all(|s| !s.konfig.is_empty());
        }
        if !c.pripravene && c.pmt.is_some() {
            c.vynechaj_prazdne();
            c.pripravene = !c.prudy.is_empty() && c.stopy.iter().all(|s| !s.konfig.is_empty());
        }
        if c.pmt.is_none() {
            return Err(Chyba::Format("MPEG-TS bez tabuľky programu (PMT)".into()));
        }
        if c.prudy.is_empty() {
            return Err(Chyba::Format("MPEG-TS bez obrazu aj zvuku".into()));
        }
        if !c.pripravene {
            return Err(Chyba::Format(
                "v MPEG-TS chýbajú nastavenia dekodéra (SPS/PPS alebo ADTS)".into(),
            ));
        }
        Ok(c)
    }

    /// Doplní `okno` na `n` bajtov (najprv z `predbuf`); false = koniec súboru.
    fn dopln(&mut self, okno: &mut Vec<u8>, n: usize) -> Vysledok<bool> {
        if okno.len() < n && !self.predbuf.is_empty() {
            let k = (n - okno.len()).min(self.predbuf.len());
            okno.extend(self.predbuf.drain(..k));
        }
        if okno.len() < n {
            let zac = okno.len();
            okno.resize(n, 0);
            let mut precitane = zac;
            while precitane < n {
                let r = self.vstup.read(&mut okno[precitane..])?;
                if r == 0 {
                    okno.truncate(precitane);
                    return Ok(false);
                }
                precitane += r;
            }
        }
        Ok(true)
    }

    fn paket(&mut self) -> Vysledok<()> {
        let mut okno = Vec::with_capacity(PAKET + 1);
        if !self.dopln(&mut okno, PAKET)? {
            self.koniec = true;
            return Ok(());
        }
        // stratená synchronizácia: ďalší bajt 0x47, za ktorým o 188 bajtov nasleduje ďalší (alebo koniec)
        while okno[0] != 0x47 {
            match okno.iter().skip(1).position(|&b| b == 0x47) {
                Some(k) => {
                    okno.drain(..=k);
                }
                None => okno.clear(),
            }
            if !self.dopln(&mut okno, PAKET + 1)? && okno.len() < PAKET {
                self.koniec = true;
                return Ok(());
            }
            if okno[0] == 0x47 && okno.len() > PAKET && okno[PAKET] != 0x47 {
                okno[0] = 0; // falošný začiatok – hľadáme ďalej
            }
        }
        if okno.len() > PAKET {
            let navyse: Vec<u8> = okno.drain(PAKET..).collect();
            let mut nove = navyse;
            nove.append(&mut self.predbuf);
            self.predbuf = nove;
        }
        let mut p = [0u8; PAKET];
        p.copy_from_slice(&okno[..PAKET]);
        self.spracuj(&p)
    }

    fn spracuj(&mut self, p: &[u8; PAKET]) -> Vysledok<()> {
        let zaciatok = p[1] & 0x40 != 0;
        let pid = (((p[1] & 0x1F) as u16) << 8) | p[2] as u16;
        let adaptacia = (p[3] >> 4) & 3;
        let mut od = 4;
        if adaptacia & 2 != 0 {
            od += 1 + p[4] as usize;
        }
        if adaptacia & 1 == 0 || od >= PAKET {
            return Ok(());
        }
        let data = &p[od..];
        if pid == 0 {
            if zaciatok {
                self.pat(data);
            }
            return Ok(());
        }
        if Some(pid) == self.pmt {
            if zaciatok && self.prudy.is_empty() {
                self.pmt_tabulka(data)?;
            }
            return Ok(());
        }
        let Some(&i) = self.pidy.get(&pid) else {
            return Ok(());
        };
        if zaciatok && !self.prudy[i].pes.is_empty() {
            let pes = std::mem::take(&mut self.prudy[i].pes);
            self.pes(i, &pes)?;
        }
        if zaciatok || !self.prudy[i].pes.is_empty() {
            self.prudy[i].pes.extend_from_slice(data);
        }
        Ok(())
    }

    fn pat(&mut self, d: &[u8]) {
        let uk = d[0] as usize;
        let t = &d[(1 + uk).min(d.len())..];
        if t.len() < 12 || t[0] != 0 {
            return;
        }
        let dlzka = ((((t[1] & 0x0F) as usize) << 8) | t[2] as usize).min(t.len() - 3);
        if dlzka < 9 {
            return;
        }
        let mut i = 8;
        while i + 4 <= 3 + dlzka - 4 {
            let program = u16::from_be_bytes([t[i], t[i + 1]]);
            let pid = (((t[i + 2] & 0x1F) as u16) << 8) | t[i + 3] as u16;
            if program != 0 && self.pmt.is_none() {
                self.pmt = Some(pid);
            }
            i += 4;
        }
    }

    fn pmt_tabulka(&mut self, d: &[u8]) -> Vysledok<()> {
        let uk = d[0] as usize;
        let t = &d[(1 + uk).min(d.len())..];
        if t.len() < 16 || t[0] != 2 {
            return Ok(());
        }
        let dlzka = ((((t[1] & 0x0F) as usize) << 8) | t[2] as usize).min(t.len() - 3);
        if dlzka < 13 {
            return Ok(());
        }
        let koniec = 3 + dlzka - 4;
        let info = ((((t[10] & 0x0F) as usize) << 8) | t[11] as usize).min(koniec);
        let mut i = 12 + info;
        while i + 5 <= koniec {
            let typ = t[i];
            let pid = (((t[i + 1] & 0x1F) as u16) << 8) | t[i + 2] as u16;
            let es = (((t[i + 3] & 0x0F) as usize) << 8) | t[i + 4] as usize;
            let opisy = &t[(i + 5).min(koniec)..(i + 5 + es).min(koniec)];
            i += 5 + es;
            let jazyk = najdi_opis(opisy, 0x0A)
                .filter(|x| x.len() >= 3)
                .map(|x| String::from_utf8_lossy(&x[..3]).to_lowercase());
            let kodek = match typ {
                0x1B => Kodek::H264,
                0x0F => Kodek::Aac,
                0x24 => return Err(Chyba::Nepodporovane("HEVC v MPEG-TS".into())),
                0x03 | 0x04 => return Err(Chyba::Nepodporovane("MP3 v MPEG-TS".into())),
                0x81 => return Err(Chyba::Nepodporovane("AC-3 v MPEG-TS".into())),
                0x87 => return Err(Chyba::Nepodporovane("E-AC-3 v MPEG-TS".into())),
                0x11 => return Err(Chyba::Nepodporovane("AAC LATM v MPEG-TS".into())),
                0x02 | 0x01 | 0x10 | 0xEA => {
                    return Err(Chyba::Nepodporovane(format!(
                        "obraz typu 0x{typ:02X} v MPEG-TS"
                    )))
                }
                0x06 => {
                    if najdi_opis(opisy, 0x6A).is_some() || najdi_opis(opisy, 0x7A).is_some() {
                        return Err(Chyba::Nepodporovane("AC-3/E-AC-3 v MPEG-TS".into()));
                    }
                    if najdi_opis(opisy, 0x05).is_some_and(|r| r.starts_with(b"Opus")) {
                        return Err(Chyba::Nepodporovane("Opus v MPEG-TS".into()));
                    }
                    continue; // titulky, teletext, ID3 a iné súkromné údaje
                }
                _ => continue, // metadáta (ID3 0x15), SCTE-35 (0x86) a pod.
            };
            if self.pidy.contains_key(&pid) {
                continue;
            }
            let stopa = self.stopy.len();
            self.stopy.push(InfoStopy {
                kodek,
                konfig: vec![],
                sirka: 0,
                vyska: 0,
                frekvencia: 0,
                kanaly: 0,
                jazyk,
            });
            self.pidy.insert(pid, self.prudy.len());
            self.prudy.push(Prud {
                kodek,
                stopa,
                pes: vec![],
                spsy: vec![],
                ppsy: vec![],
                zvysok: vec![],
                dalsi_cas: None,
                posledny: None,
                korekcia: 0,
                posledne_dts: None,
            });
        }
        Ok(())
    }

    /// 33-bitový čas → súvislý čas v ns (pretečenie hodín a nespojitosti v HLS).
    fn cas(&mut self, i: usize, surovy: i64) -> i64 {
        let p = &mut self.prudy[i];
        let rozbaleny = match p.posledny {
            None => surovy,
            Some(posl) => {
                let zaklad = posl - posl.rem_euclid(OBVOD);
                [
                    zaklad - OBVOD + surovy,
                    zaklad + surovy,
                    zaklad + OBVOD + surovy,
                ]
                .into_iter()
                .min_by_key(|k| (k - posl).abs())
                .unwrap_or(surovy)
            }
        };
        p.posledny = Some(rozbaleny);
        na_ns(rozbaleny, HODINY) + p.korekcia
    }

    /// Ochrana pred skokom späť v čase (nespojitosť v HLS): čas pokračuje za predchádzajúcou vzorkou.
    fn sustav(&mut self, i: usize, dts: i64) -> i64 {
        let p = &mut self.prudy[i];
        if let Some(posl) = p.posledne_dts {
            if dts < posl - 1_000_000_000 || dts > posl + 60_000_000_000 {
                let posun = posl + 40_000_000 - dts;
                p.korekcia += posun;
                p.posledne_dts = Some(dts + posun);
                return posun;
            }
        }
        p.posledne_dts = Some(dts);
        0
    }

    fn pes(&mut self, i: usize, d: &[u8]) -> Vysledok<()> {
        if d.len() < 9 || d[0] != 0 || d[1] != 0 || d[2] != 1 {
            return Ok(());
        }
        let priznaky = d[7];
        let hlavicka = 9 + d[8] as usize;
        if hlavicka > d.len() {
            return Ok(());
        }
        let pts = (priznaky & 0x80 != 0 && d.len() >= 14).then(|| cas_pes(&d[9..14]));
        let dts = (priznaky & 0xC0 == 0xC0 && d.len() >= 19).then(|| cas_pes(&d[14..19]));
        let pts = pts.map(|v| self.cas(i, v));
        let dts = dts.map(|v| self.cas(i, v)).or(pts);
        let payload = &d[hlavicka..];
        match self.prudy[i].kodek {
            Kodek::H264 => self.video(i, payload, pts, dts),
            _ => self.zvuk(i, payload, pts),
        }
    }

    fn video(&mut self, i: usize, d: &[u8], pts: Option<i64>, dts: Option<i64>) -> Vysledok<()> {
        let (Some(mut pts), Some(mut dts)) = (pts, dts) else {
            return Ok(());
        };
        let p = &mut self.prudy[i];
        let s = h264::na_avcc(d, &mut p.spsy, &mut p.ppsy);
        let stopa = p.stopa;
        if self.stopy[stopa].konfig.is_empty() && !p.spsy.is_empty() && !p.ppsy.is_empty() {
            if let Some(c) = h264::avcc(&p.spsy, &p.ppsy) {
                let r = h264::sps(&p.spsy[0]).unwrap_or_default();
                let info = &mut self.stopy[stopa];
                info.konfig = c;
                info.sirka = r.sirka;
                info.vyska = r.vyska;
            }
        }
        if !s.ma_obraz {
            return Ok(());
        }
        let posun = self.sustav(i, dts);
        pts += posun;
        dts += posun;
        self.fronta.push_back(Vzorka {
            stopa,
            dts,
            pts,
            trvanie: 0,
            kluc: s.kluc,
            data: s.data,
        });
        Ok(())
    }

    fn zvuk(&mut self, i: usize, d: &[u8], pts: Option<i64>) -> Vysledok<()> {
        let stopa = self.prudy[i].stopa;
        let mut buf = std::mem::take(&mut self.prudy[i].zvysok);
        let pokracovanie = !buf.is_empty();
        buf.extend_from_slice(d);
        // čas prvého celého rámca: pokračovanie z predchádzajúceho PES, inak PTS tohto PES
        let mut cas = match (self.prudy[i].dalsi_cas, pts) {
            (Some(c), Some(p)) if pokracovanie || (c - p).abs() < 50_000_000 => c,
            (_, Some(p)) => p,
            (Some(c), None) => c,
            (None, None) => return Ok(()),
        };
        let mut od = 0;
        while od < buf.len() {
            let Some(h) = aac::adts(&buf[od..]) else {
                // stratená synchronizácia rámcov – hľadáme ďalší syncword
                match buf[od + 1..]
                    .windows(2)
                    .position(|w| w[0] == 0xFF && w[1] & 0xF6 == 0xF0)
                {
                    Some(k) => {
                        od += 1 + k;
                        continue;
                    }
                    None => {
                        od = buf.len();
                        break;
                    }
                }
            };
            if od + h.ramec > buf.len() {
                break;
            }
            let info = &mut self.stopy[stopa];
            if info.konfig.is_empty() {
                info.konfig = aac::asc(&h);
                info.frekvencia = h.frekvencia();
                info.kanaly = h.kanaly.max(1) as u16;
            }
            let trvanie = na_ns(1024, h.frekvencia());
            let posun = self.sustav(i, cas);
            cas += posun;
            self.fronta.push_back(Vzorka {
                stopa,
                dts: cas,
                pts: cas,
                trvanie,
                kluc: true,
                data: buf[od + h.hlavicka..od + h.ramec].to_vec(),
            });
            cas += trvanie;
            od += h.ramec;
        }
        self.prudy[i].dalsi_cas = Some(cas);
        self.prudy[i].zvysok = buf[od.min(buf.len())..].to_vec();
        Ok(())
    }

    /// Prúd ohlásený v PMT, ktorý neobsahuje ani jednu vzorku, nie je stopa – vynechá sa.
    fn vynechaj_prazdne(&mut self) {
        let prazdne: Vec<usize> = (0..self.stopy.len())
            .filter(|&k| {
                self.stopy[k].konfig.is_empty() && !self.fronta.iter().any(|v| v.stopa == k)
            })
            .collect();
        if prazdne.is_empty() {
            return;
        }
        let mut nove_cislo = vec![usize::MAX; self.stopy.len()];
        let mut n = 0;
        for (k, c) in nove_cislo.iter_mut().enumerate() {
            if !prazdne.contains(&k) {
                *c = n;
                n += 1;
            }
        }
        let mut k = 0;
        self.stopy.retain(|_| {
            let ponechat = !prazdne.contains(&k);
            k += 1;
            ponechat
        });
        let stare: Vec<Prud> = std::mem::take(&mut self.prudy);
        let mut mapa_prudov = HashMap::new();
        for (stary_index, mut p) in stare.into_iter().enumerate() {
            if nove_cislo[p.stopa] != usize::MAX {
                p.stopa = nove_cislo[p.stopa];
                mapa_prudov.insert(stary_index, self.prudy.len());
                self.prudy.push(p);
            }
        }
        self.pidy = self
            .pidy
            .iter()
            .filter_map(|(pid, i)| mapa_prudov.get(i).map(|n| (*pid, *n)))
            .collect();
        for v in self.fronta.iter_mut() {
            v.stopa = nove_cislo[v.stopa];
        }
    }

    fn dokonci(&mut self) -> Vysledok<()> {
        for i in 0..self.prudy.len() {
            let pes = std::mem::take(&mut self.prudy[i].pes);
            if !pes.is_empty() {
                self.pes(i, &pes)?;
            }
        }
        Ok(())
    }
}

fn najdi_opis(d: &[u8], tag: u8) -> Option<&[u8]> {
    let mut i = 0;
    while i + 2 <= d.len() {
        let (t, l) = (d[i], d[i + 1] as usize);
        let telo = d.get(i + 2..i + 2 + l)?;
        if t == tag {
            return Some(telo);
        }
        i += 2 + l;
    }
    None
}

impl Citac for CitacTs {
    fn stopy(&self) -> &[InfoStopy] {
        &self.stopy
    }

    fn dalsia(&mut self) -> Vysledok<Option<Vzorka>> {
        loop {
            if let Some(v) = self.fronta.pop_front() {
                return Ok(Some(v));
            }
            if self.koniec {
                if self.prudy.iter().any(|p| !p.pes.is_empty()) {
                    self.dokonci()?;
                    continue;
                }
                return Ok(None);
            }
            self.paket()?;
        }
    }
}

#[cfg(test)]
mod testy {
    use super::cas_pes;

    #[test]
    fn cas_v_hlavicke_pes() {
        // PTS = 0x1_2345_6789 (33 bitov) zakódovaný podľa normy
        let v: i64 = 0x1_2345_6789;
        let b = [
            0x21 | (((v >> 30) & 7) << 1) as u8,
            ((v >> 22) & 0xFF) as u8,
            ((((v >> 15) & 0x7F) << 1) | 1) as u8,
            ((v >> 7) & 0xFF) as u8,
            (((v & 0x7F) << 1) | 1) as u8,
        ];
        assert_eq!(cas_pes(&b), v);
    }
}
