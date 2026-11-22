//! AAC: hlavičky ADTS (MPEG-TS) a AudioSpecificConfig (MP4/MKV).

use super::bity::Bity;

pub const FREKVENCIE: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adts {
    /// audioObjectType (1 = Main, 2 = LC, …)
    pub aot: u8,
    pub index_frekvencie: u8,
    pub kanaly: u8,
    pub hlavicka: usize,
    pub ramec: usize,
}

impl Adts {
    pub fn frekvencia(&self) -> u32 {
        FREKVENCIE
            .get(self.index_frekvencie as usize)
            .copied()
            .unwrap_or(48000)
    }
}

pub fn adts(d: &[u8]) -> Option<Adts> {
    if d.len() < 7 || d[0] != 0xFF || d[1] & 0xF6 != 0xF0 {
        return None;
    }
    let bez_crc = d[1] & 1 == 1;
    let ramec = (((d[3] & 3) as usize) << 11) | ((d[4] as usize) << 3) | ((d[5] >> 5) as usize);
    let hlavicka = if bez_crc { 7 } else { 9 };
    let idx = (d[2] >> 2) & 0xF;
    if ramec < hlavicka || idx > 12 {
        return None;
    }
    Some(Adts {
        aot: (d[2] >> 6) + 1,
        index_frekvencie: idx,
        kanaly: ((d[2] & 1) << 2) | (d[3] >> 6),
        hlavicka,
        ramec,
    })
}

/// AudioSpecificConfig (2 bajty) z údajov ADTS.
pub fn asc(a: &Adts) -> Vec<u8> {
    let v: u16 =
        ((a.aot as u16) << 11) | ((a.index_frekvencie as u16) << 7) | ((a.kanaly as u16) << 3);
    v.to_be_bytes().to_vec()
}

/// Frekvencia a počet kanálov z AudioSpecificConfig.
pub fn z_asc(c: &[u8]) -> Option<(u32, u16)> {
    let mut b = Bity::new(c);
    let mut aot = b.bity(5)?;
    if aot == 31 {
        aot = 32 + b.bity(6)?;
    }
    let _ = aot;
    let idx = b.bity(4)?;
    let frekvencia = if idx == 15 {
        b.bity(24)?
    } else {
        *FREKVENCIE.get(idx as usize)?
    };
    let kanaly = b.bity(4)? as u16;
    Some((frekvencia, kanaly))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn hlavicka_adts_a_asc() {
        // AAC LC, 44,1 kHz, stereo, rámec 371 bajtov, bez CRC
        let d = [0xFF, 0xF1, 0x50, 0x80, 0x2E, 0x7F, 0xFC];
        let a = adts(&d).unwrap();
        assert_eq!(
            (a.aot, a.frekvencia(), a.kanaly, a.hlavicka, a.ramec),
            (2, 44100, 2, 7, 371)
        );
        assert_eq!(asc(&a), vec![0x12, 0x10]);
        assert_eq!(z_asc(&[0x12, 0x10]), Some((44100, 2)));
        assert_eq!(z_asc(&[0x11, 0x90]), Some((48000, 2)));
        assert!(adts(&[0xFF, 0xF1, 0x50]).is_none());
    }
}
