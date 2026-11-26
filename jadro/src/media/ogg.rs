//! Zápis zvuku Opus do súboru `.opus` (Ogg, RFC 3533 a RFC 7845) – bez FFmpeg a bez prekódovania.
//!
//! Stránka 1: hlavička `OpusHead`, stránka 2: `OpusTags` (názov, autor, dátum, popis, zdroj,
//! kapitoly a obrázok ako `METADATA_BLOCK_PICTURE`), potom pakety zvuku. Pozícia na stránke
//! (granule) je počet vzoriek pri 48 kHz vrátane úvodného preskočenia (pre-skip) – dĺžku každého
//! paketu určuje jeho prvý bajt (TOC, RFC 6716, 3.1).

use super::{Doplnky, NS};
use crate::chyba::{Chyba, Vysledok};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// CRC-32 stránky Ogg: polynóm 0x04C11DB7, bez odrazu, začiatok 0.
fn crc_ogg(data: &[u8]) -> u32 {
    static TAB: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TAB.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, x) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 {
                    (r << 1) ^ 0x04C1_1DB7
                } else {
                    r << 1
                };
            }
            *x = r;
        }
        t
    });
    data.iter()
        .fold(0u32, |c, &b| (c << 8) ^ t[((c >> 24) as u8 ^ b) as usize])
}

/// Počet vzoriek (48 kHz) v pakete Opus podľa jeho TOC.
pub fn vzorky_paketu(p: &[u8]) -> u64 {
    let Some(&toc) = p.first() else { return 0 };
    let konfig = toc >> 3;
    // dĺžka rámca v desatinách milisekundy
    let ramec = match konfig {
        0..=11 => [100, 200, 400, 600][(konfig % 4) as usize],
        12..=15 => [100, 200][(konfig % 2) as usize],
        _ => [25, 50, 100, 200][(konfig % 4) as usize],
    };
    let ramcov = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => p.get(1).map(|b| (b & 0x3F) as u64).unwrap_or(0),
    };
    ramcov * ramec * 48 / 10
}

/// dOps (MP4, tak ho dáva čítač) alebo OpusHead → OpusHead pre Ogg. Vráti aj pre-skip.
pub fn opus_head(konfig: &[u8], kanaly: u16) -> (Vec<u8>, u16) {
    if konfig.starts_with(b"OpusHead") && konfig.len() >= 19 {
        let pre = u16::from_le_bytes([konfig[10], konfig[11]]);
        return (konfig.to_vec(), pre);
    }
    let mut h = b"OpusHead".to_vec();
    h.push(1);
    if konfig.len() >= 11 {
        // dOps: verzia, kanály, pre-skip (BE), frekvencia (BE), zisk (BE), mapovanie [+ tabuľka]
        let pre = u16::from_be_bytes([konfig[2], konfig[3]]);
        h.push(konfig[1]);
        h.extend(pre.to_le_bytes());
        h.extend(u32::from_be_bytes(konfig[4..8].try_into().unwrap()).to_le_bytes());
        h.extend(i16::from_be_bytes([konfig[8], konfig[9]]).to_le_bytes());
        h.extend(&konfig[10..]);
        (h, pre)
    } else {
        let k = kanaly.clamp(1, 2) as u8;
        h.push(k);
        h.extend(312u16.to_le_bytes()); // bežné úvodné preskočenie kódovača
        h.extend(48_000u32.to_le_bytes());
        h.extend(0i16.to_le_bytes());
        h.push(0);
        (h, 312)
    }
}

fn base64(d: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                o.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

/// OpusTags: dodávateľ a poznámky KĽÚČ=hodnota (Vorbis comment), vrátane kapitol a obrázka.
pub fn opus_tags(d: &Doplnky, koniec_ns: i64) -> Vec<u8> {
    let mut pozn: Vec<String> = vec![];
    let mut pridaj = |k: &str, v: Option<&str>| {
        if let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) {
            pozn.push(format!("{k}={v}"));
        }
    };
    pridaj("TITLE", Some(&d.titul));
    pridaj("ARTIST", d.autor.as_deref());
    pridaj("DATE", d.datum.as_deref());
    pridaj("DESCRIPTION", d.popis.as_deref());
    pridaj("CONTACT", d.zdroj.as_deref());
    for (i, k) in d.kapitoly_do(koniec_ns).iter().enumerate() {
        let ms = k.od / 1_000_000;
        let cas = format!(
            "{:02}:{:02}:{:02}.{:03}",
            ms / 3_600_000,
            ms / 60_000 % 60,
            ms / 1000 % 60,
            ms % 1000
        );
        pozn.push(format!("CHAPTER{:03}={cas}", i + 1));
        pozn.push(format!("CHAPTER{:03}NAME={}", i + 1, k.nazov));
    }
    if let Some(o) = &d.obal {
        // blok obrázka FLAC: typ 3 (predná strana obalu), MIME, bez popisu, rozmery neuvedené
        let mut b = 3u32.to_be_bytes().to_vec();
        b.extend((o.mime().len() as u32).to_be_bytes());
        b.extend(o.mime().as_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend([0u8; 16]);
        b.extend((o.data.len() as u32).to_be_bytes());
        b.extend(&o.data);
        pozn.push(format!("METADATA_BLOCK_PICTURE={}", base64(&b)));
    }
    let dodavatel = format!("Kometa {}", crate::VERZIA);
    let mut t = b"OpusTags".to_vec();
    t.extend((dodavatel.len() as u32).to_le_bytes());
    t.extend(dodavatel.as_bytes());
    t.extend((pozn.len() as u32).to_le_bytes());
    for p in pozn {
        t.extend((p.len() as u32).to_le_bytes());
        t.extend(p.as_bytes());
    }
    t
}

/// Zapisovač stránok Ogg pre jeden logický prúd.
pub struct ZapisovacOgg {
    f: BufWriter<File>,
    seriove: u32,
    stranka: u32,
    /// pakety čakajúce na stránku a pozícia na konci posledného z nich
    cakajuce: Vec<Vec<u8>>,
    granule: u64,
    pocet: u64,
}

impl ZapisovacOgg {
    pub fn new(cesta: &Path, head: &[u8], tags: &[u8]) -> Vysledok<Self> {
        let mut z = ZapisovacOgg {
            f: BufWriter::new(File::create(cesta)?),
            seriove: 0x4B4F_4D54, // „KOMT“
            stranka: 0,
            cakajuce: vec![],
            granule: 0,
            pocet: 0,
        };
        z.stranka_s(&[head], 0, 0x02)?;
        // OpusTags môže byť dlhší (obrázok) – rozloží sa na viac stránok
        z.dlhy_paket(tags)?;
        Ok(z)
    }

    /// Zapíše jednu stránku s celými paketmi.
    fn stranka_s(&mut self, pakety: &[&[u8]], granule: u64, typ: u8) -> Vysledok<()> {
        let mut segmenty = vec![];
        for p in pakety {
            let mut n = p.len();
            while n >= 255 {
                segmenty.push(255u8);
                n -= 255;
            }
            segmenty.push(n as u8);
        }
        self.zapis_stranku(&segmenty, &pakety.concat(), granule, typ)
    }

    fn zapis_stranku(
        &mut self,
        segmenty: &[u8],
        data: &[u8],
        granule: u64,
        typ: u8,
    ) -> Vysledok<()> {
        let mut s = b"OggS".to_vec();
        s.push(0);
        s.push(typ);
        s.extend(granule.to_le_bytes());
        s.extend(self.seriove.to_le_bytes());
        s.extend(self.stranka.to_le_bytes());
        s.extend([0u8; 4]);
        s.push(segmenty.len() as u8);
        s.extend(segmenty);
        s.extend(data);
        let c = crc_ogg(&s);
        s[22..26].copy_from_slice(&c.to_le_bytes());
        self.f.write_all(&s)?;
        self.stranka += 1;
        Ok(())
    }

    /// Paket dlhší ako jedna stránka (najviac 255 segmentov) – pokračovacie stránky.
    fn dlhy_paket(&mut self, p: &[u8]) -> Vysledok<()> {
        let mut segmenty: Vec<u8> = vec![255; p.len() / 255];
        segmenty.push((p.len() % 255) as u8);
        let mut poz = 0usize;
        let mut prva = true;
        for kus in segmenty.chunks(255) {
            let dlzka: usize = kus.iter().map(|&x| x as usize).sum();
            let posledny = poz + dlzka == p.len();
            // granule -1 (žiadny paket sa na stránke nekončí) okrem stránky, kde paket skončí
            let g = if posledny { 0 } else { u64::MAX };
            self.zapis_stranku(kus, &p[poz..poz + dlzka], g, if prva { 0 } else { 1 })?;
            poz += dlzka;
            prva = false;
        }
        Ok(())
    }

    /// Pridá paket zvuku; `granule` = pozícia na jeho konci.
    pub fn paket(&mut self, p: &[u8], granule: u64) -> Vysledok<()> {
        if p.len() > 255 * 254 {
            return Err(Chyba::Format("Opus: paket je príliš veľký".into()));
        }
        let segmentov: usize = self.cakajuce.iter().map(|x| x.len() / 255 + 1).sum();
        if segmentov + p.len() / 255 + 1 > 255 || self.cakajuce.len() >= 50 {
            self.vyprazdni(0)?;
        }
        self.cakajuce.push(p.to_vec());
        self.granule = granule;
        self.pocet += 1;
        Ok(())
    }

    fn vyprazdni(&mut self, typ: u8) -> Vysledok<()> {
        if self.cakajuce.is_empty() {
            return Ok(());
        }
        let p = std::mem::take(&mut self.cakajuce);
        let r: Vec<&[u8]> = p.iter().map(Vec::as_slice).collect();
        self.stranka_s(&r, self.granule, typ)
    }

    /// Posledná stránka (koniec prúdu). Vráti počet paketov.
    pub fn dokonci(mut self) -> Vysledok<u64> {
        self.vyprazdni(0x04)?;
        let f = self.f.into_inner().map_err(|e| Chyba::Io(e.into_error()))?;
        f.sync_all()?;
        Ok(self.pocet)
    }
}

/// Prehľad súboru Ogg Opus: (hlavička je OpusHead, počet paketov zvuku, posledná pozícia, CRC v poriadku).
pub fn over(cesta: &Path) -> Vysledok<(bool, u64, u64)> {
    let d = std::fs::read(cesta)?;
    let mut i = 0;
    let mut paketov = 0u64;
    let mut granule = 0u64;
    let mut stranka = 0;
    let mut head = false;
    let mut hlavicky = 0; // pakety OpusHead a OpusTags sa nerátajú
    while i + 27 <= d.len() {
        if &d[i..i + 4] != b"OggS" {
            return Err(Chyba::Format("Ogg: poškodená stránka".into()));
        }
        let n = d[i + 26] as usize;
        let seg = &d[i + 27..i + 27 + n];
        let dlzka: usize = seg.iter().map(|&x| x as usize).sum();
        let koniec = i + 27 + n + dlzka;
        if koniec > d.len() {
            return Err(Chyba::Format("Ogg: neúplná stránka".into()));
        }
        let mut kopia = d[i..koniec].to_vec();
        let ulozene = u32::from_le_bytes(kopia[22..26].try_into().unwrap());
        kopia[22..26].fill(0);
        if crc_ogg(&kopia) != ulozene {
            return Err(Chyba::Format("Ogg: nesedí CRC stránky".into()));
        }
        if stranka == 0 {
            head = d[i + 27 + n..].starts_with(b"OpusHead");
        }
        let g = u64::from_le_bytes(d[i + 6..i + 14].try_into().unwrap());
        let konciacich = seg.iter().filter(|&&x| x < 255).count() as u64;
        if hlavicky < 2 {
            let z = konciacich.min(2 - hlavicky);
            hlavicky += z;
            paketov += konciacich - z;
        } else {
            paketov += konciacich;
        }
        if g != u64::MAX && stranka > 0 {
            granule = granule.max(g);
        }
        stranka += 1;
        i = koniec;
    }
    Ok((head, paketov, granule))
}

/// Dĺžka v ns podľa pozície (bez úvodného preskočenia).
pub fn trvanie_ns(granule: u64, pre_skip: u16) -> i64 {
    (granule.saturating_sub(pre_skip as u64) as i128 * NS as i128 / 48_000) as i64
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn crc_a_dlzky() {
        // známa hodnota: CRC Ogg prázdnych dát je 0
        assert_eq!(crc_ogg(&[]), 0);
        assert_eq!(base64(b"Kometa"), "S29tZXRh");
        assert_eq!(base64(b"ab"), "YWI=");
        // CELT 20 ms, 1 rámec → 960 vzoriek; SILK 60 ms, 2 rámce → 5760; kód 3 s 4 rámcami 2,5 ms
        assert_eq!(vzorky_paketu(&[31 << 3]), 960);
        assert_eq!(vzorky_paketu(&[(3 << 3) | 1]), 5760);
        assert_eq!(vzorky_paketu(&[(16 << 3) | 3, 4]), 480);
        // dOps → OpusHead
        let dops = [0u8, 2, 0x01, 0x38, 0, 0, 0xBB, 0x80, 0, 0, 0];
        let (h, pre) = opus_head(&dops, 2);
        assert_eq!(pre, 312);
        assert_eq!(&h[..10], b"OpusHead\x01\x02");
        assert_eq!(&h[10..12], &312u16.to_le_bytes());
        assert_eq!(h.len(), 19);
    }
}
