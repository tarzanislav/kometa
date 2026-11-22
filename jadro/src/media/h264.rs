//! H.264 (AVC): rozdelenie Annex B na jednotky NAL, prevod na tvar s dĺžkami (AVCC),
//! zostavenie avcC a rozmery obrazu zo SPS.

use super::bity::Bity;

/// Jednotky NAL z prúdu Annex B (oddeľovače 00 00 01 / 00 00 00 01).
pub fn nal_jednotky(d: &[u8]) -> Vec<&[u8]> {
    let mut zaciatky = vec![];
    let mut i = 0;
    while i + 2 < d.len() {
        if d[i] == 0 && d[i + 1] == 0 && d[i + 2] == 1 {
            zaciatky.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(zaciatky.len());
    for (k, &z) in zaciatky.iter().enumerate() {
        let mut koniec = zaciatky.get(k + 1).map(|n| n - 3).unwrap_or(d.len());
        // nuly pred ďalším oddeľovačom (00 00 00 01) nepatria do NAL
        while koniec > z && d[koniec - 1] == 0 {
            koniec -= 1;
        }
        if koniec > z {
            out.push(&d[z..koniec]);
        }
    }
    out
}

/// RBSP bez bajtov zabraňujúcich emulácii (00 00 03 → 00 00).
pub fn rbsp(nal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(nal.len());
    let mut nuly = 0;
    for &b in nal {
        if nuly >= 2 && b == 3 {
            nuly = 0;
            continue;
        }
        nuly = if b == 0 { nuly + 1 } else { 0 };
        out.push(b);
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sps {
    pub profil: u8,
    pub kompatibilita: u8,
    pub uroven: u8,
    pub chroma: u32,
    pub hlbka_jas: u32,
    pub hlbka_farba: u32,
    pub sirka: u32,
    pub vyska: u32,
}

fn preskoc_skalovanie(b: &mut Bity, velkost: usize) -> Option<()> {
    let (mut posledna, mut dalsia) = (8i32, 8i32);
    for _ in 0..velkost {
        if dalsia != 0 {
            let delta = b.se()?;
            dalsia = (posledna + delta + 256) % 256;
        }
        posledna = if dalsia == 0 { posledna } else { dalsia };
    }
    Some(())
}

/// Rozmery a profil zo SPS (celá jednotka NAL vrátane prvého bajtu).
pub fn sps(nal: &[u8]) -> Option<Sps> {
    let r = rbsp(nal.get(1..)?);
    let mut b = Bity::new(&r);
    let profil = b.bity(8)? as u8;
    let kompatibilita = b.bity(8)? as u8;
    let uroven = b.bity(8)? as u8;
    b.ue()?;
    let (mut chroma, mut hj, mut hf) = (1, 0, 0);
    if matches!(
        profil,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma = b.ue()?;
        if chroma == 3 {
            b.bit()?;
        }
        hj = b.ue()?;
        hf = b.ue()?;
        b.bit()?;
        if b.bit()? == 1 {
            for i in 0..(if chroma == 3 { 12 } else { 8 }) {
                if b.bit()? == 1 {
                    preskoc_skalovanie(&mut b, if i < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    b.ue()?; // log2_max_frame_num_minus4
    match b.ue()? {
        0 => {
            b.ue()?;
        }
        1 => {
            b.bit()?;
            b.se()?;
            b.se()?;
            for _ in 0..b.ue()? {
                b.se()?;
            }
        }
        _ => {}
    }
    b.ue()?; // max_num_ref_frames
    b.bit()?;
    let sirka_mb = b.ue()? + 1;
    let vyska_mapa = b.ue()? + 1;
    let len_snimky = b.bit()?;
    if len_snimky == 0 {
        b.bit()?;
    }
    b.bit()?;
    let (mut l, mut p, mut h, mut d) = (0, 0, 0, 0);
    if b.bit()? == 1 {
        l = b.ue()?;
        p = b.ue()?;
        h = b.ue()?;
        d = b.ue()?;
    }
    let (ux, uy) = match chroma {
        0 | 3 => (1, 2 - len_snimky),
        2 => (2, 2 - len_snimky),
        _ => (2, 2 * (2 - len_snimky)),
    };
    let sirka = (sirka_mb * 16).checked_sub(ux * (l + p))?;
    let vyska = ((2 - len_snimky) * vyska_mapa * 16).checked_sub(uy * (h + d))?;
    Some(Sps {
        profil,
        kompatibilita,
        uroven,
        chroma,
        hlbka_jas: hj,
        hlbka_farba: hf,
        sirka,
        vyska,
    })
}

/// avcC (AVCDecoderConfigurationRecord) z jednotiek SPS a PPS.
pub fn avcc(spsy: &[Vec<u8>], ppsy: &[Vec<u8>]) -> Option<Vec<u8>> {
    let prvy = spsy.first()?;
    let s = sps(prvy)?;
    let mut o = vec![
        1,
        s.profil,
        s.kompatibilita,
        s.uroven,
        0xFF,
        0xE0 | spsy.len().min(31) as u8,
    ];
    for x in spsy.iter().take(31) {
        o.extend((x.len() as u16).to_be_bytes());
        o.extend(x);
    }
    o.push(ppsy.len().min(255) as u8);
    for x in ppsy.iter().take(255) {
        o.extend((x.len() as u16).to_be_bytes());
        o.extend(x);
    }
    if matches!(s.profil, 100 | 110 | 122 | 144) {
        o.extend([
            0xFC | (s.chroma as u8 & 3),
            0xF8 | (s.hlbka_jas as u8 & 7),
            0xF8 | (s.hlbka_farba as u8 & 7),
            0,
        ]);
    }
    Some(o)
}

/// Výsledok prevodu jednej prístupovej jednotky.
pub struct Snimka {
    pub data: Vec<u8>,
    pub kluc: bool,
    pub ma_obraz: bool,
}

/// Annex B → AVCC (4-bajtové dĺžky). Oddeľovače AUD a výplň sa vynechajú; SPS/PPS ostanú v prúde
/// (zbierajú sa aj do `spsy`/`ppsy` pre avcC).
pub fn na_avcc(d: &[u8], spsy: &mut Vec<Vec<u8>>, ppsy: &mut Vec<Vec<u8>>) -> Snimka {
    let mut data = Vec::with_capacity(d.len() + 16);
    let (mut idr, mut i_rez, mut zotavenie, mut ma_obraz) = (false, false, false, false);
    for nal in nal_jednotky(d) {
        let typ = nal[0] & 0x1F;
        match typ {
            9 | 12 => continue,
            7 => {
                if !spsy.iter().any(|x| x == nal) && spsy.len() < 8 {
                    spsy.push(nal.to_vec());
                }
            }
            8 => {
                if !ppsy.iter().any(|x| x == nal) && ppsy.len() < 32 {
                    ppsy.push(nal.to_vec());
                }
            }
            5 => {
                idr = true;
                ma_obraz = true;
            }
            1 => {
                ma_obraz = true;
                let r = rbsp(&nal[1..nal.len().min(16)]);
                let mut b = Bity::new(&r);
                if b.ue().is_some() {
                    if let Some(t) = b.ue() {
                        i_rez |= t % 5 == 2 || t % 5 == 4;
                    }
                }
            }
            6 => zotavenie |= nal.get(1) == Some(&6), // SEI recovery point
            _ => {}
        }
        data.extend((nal.len() as u32).to_be_bytes());
        data.extend(nal);
    }
    Snimka {
        data,
        kluc: idr || (i_rez && zotavenie),
        ma_obraz,
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    // skutočné SPS z libx264: 1280×720 High a 1278×718 Baseline (s orezaním)
    const SPS_720: [u8; 26] = [
        0x67, 0x64, 0x00, 0x1F, 0xAC, 0xD9, 0x40, 0x50, 0x05, 0xBB, 0x01, 0x10, 0x00, 0x00, 0x03,
        0x00, 0x10, 0x00, 0x00, 0x03, 0x03, 0x20, 0xF1, 0x83, 0x19, 0x60,
    ];
    const SPS_NEPARNE: [u8; 24] = [
        0x67, 0x42, 0xC0, 0x1F, 0xDA, 0x01, 0x40, 0x16, 0xFA, 0xAC, 0x04, 0x40, 0x00, 0x00, 0x03,
        0x00, 0x40, 0x00, 0x00, 0x0C, 0x83, 0xC6, 0x0C, 0xA8,
    ];

    #[test]
    fn rozdelenie_a_emulacia() {
        let d = [
            0, 0, 0, 1, 0x09, 0xF0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 0, 0, 3, 1,
        ];
        let n = nal_jednotky(&d);
        assert_eq!(
            n,
            vec![
                &[0x09, 0xF0][..],
                &[0x67, 1, 2][..],
                &[0x65, 0, 0, 3, 1][..]
            ]
        );
        assert_eq!(rbsp(n[2]), vec![0x65, 0, 0, 1]);
    }

    #[test]
    fn rozmery_zo_sps() {
        let s = sps(&SPS_720).unwrap();
        assert_eq!((s.sirka, s.vyska, s.profil, s.uroven), (1280, 720, 100, 31));
        let s = sps(&SPS_NEPARNE).unwrap();
        assert_eq!((s.sirka, s.vyska, s.profil), (1278, 718, 66));
    }

    #[test]
    fn prevod_snimky() {
        let mut d = vec![0, 0, 0, 1, 0x09, 0xF0];
        d.extend([0, 0, 0, 1]);
        d.extend(SPS_720);
        d.extend([0, 0, 0, 1, 0x68, 0xEF, 0x8F, 0xCB]);
        d.extend([0, 0, 1, 0x65, 0x88, 0x84]);
        let (mut s, mut p) = (vec![], vec![]);
        let v = na_avcc(&d, &mut s, &mut p);
        assert!(v.kluc && v.ma_obraz);
        assert_eq!((s.len(), p.len()), (1, 1));
        // AUD vynechaný: SPS(26) + PPS(4) + IDR(3), každý so 4-bajtovou dĺžkou
        assert_eq!(v.data.len(), 4 + 26 + 4 + 4 + 4 + 3);
        let c = avcc(&s, &p).unwrap();
        assert_eq!(&c[..6], &[1, 0x64, 0x00, 0x1F, 0xFF, 0xE1]);
        assert_eq!(c.len(), 6 + 2 + 26 + 1 + 2 + 4 + 4);
    }
}
