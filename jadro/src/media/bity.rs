//! Čítanie po bitoch (pre hlavičky H.264/AAC) a pomocné funkcie pre bajty.

pub struct Bity<'a> {
    data: &'a [u8],
    poz: usize,
}

impl<'a> Bity<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Bity { data, poz: 0 }
    }

    pub fn bit(&mut self) -> Option<u32> {
        let b = *self.data.get(self.poz / 8)?;
        let v = (b >> (7 - (self.poz % 8))) & 1;
        self.poz += 1;
        Some(v as u32)
    }

    pub fn bity(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Some(v)
    }

    pub fn preskoc(&mut self, n: usize) -> Option<()> {
        self.poz += n;
        (self.poz <= self.data.len() * 8).then_some(())
    }

    /// Exp-Golomb bez znamienka (ue(v)).
    pub fn ue(&mut self) -> Option<u32> {
        let mut nuly = 0;
        while self.bit()? == 0 {
            nuly += 1;
            if nuly > 31 {
                return None;
            }
        }
        if nuly == 0 {
            return Some(0);
        }
        Some((1u32 << nuly) - 1 + self.bity(nuly)?)
    }

    /// Exp-Golomb so znamienkom (se(v)).
    pub fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        Some(if k % 2 == 1 {
            k.div_ceil(2) as i32
        } else {
            -((k / 2) as i32)
        })
    }
}

pub fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

pub fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

pub fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn exp_golomb() {
        // 1 | 010 | 011 | 00100 | 00101 → 0, 1, 2, 3, 4
        let d = [0b1010_0110, 0b0100_0010, 0b1000_0000];
        let mut b = Bity::new(&d);
        assert_eq!(
            [b.ue(), b.ue(), b.ue(), b.ue(), b.ue()],
            [Some(0), Some(1), Some(2), Some(3), Some(4)]
        );
        let d = [0b0100_1100, 0b1000_0000]; // 010 → +1, 011 → -1, 00100 → +2
        let mut b = Bity::new(&d);
        assert_eq!([b.se(), b.se(), b.se()], [Some(1), Some(-1), Some(2)]);
    }
}
