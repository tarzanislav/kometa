//! Zápis MP4 (ISO BMFF) – vlastný zapisovač s hlavičkou na začiatku („faststart“).
//!
//! Dvojprechodový: najprv sa zozbierajú údaje o vzorkách (veľkosti, časy), z nich sa postaví
//! `moov` a zapíše sa pred `mdat`; v druhom prechode sa do `mdat` zapíšu samotné vzorky
//! v rovnakom poradí. Jazyk ide do `mdhd` (ISO 639-2/T) a pri regióne aj do `elng` (BCP 47),
//! názov stopy do `hdlr` aj `udta/name`; predvolená zvuková stopa je jediná zapnutá v skupine zvukov.
//!
//! Titulky: 3GPP Timed Text (`tx3g`, 3GPP TS 26.245) v stope `sbtl` – každý jazyk vlastná stopa,
//! predvolene vypnutá; čas bez titulku vyplnia prázdne vzorky, lebo text platí až po ďalšiu vzorku.
//! Roly stôp (pôvodný jazyk, dabing) ako `kind` s rolami MPEG-DASH (ISO/IEC 14496-12, 8.10.4).
//! Kapitoly dvakrát: zoznam `chpl` v `moov/udta` a kapitolová textová stopa, na ktorú odkazuje
//! prvá stopa cez `tref/chap` – prvý spôsob čítajú prehrávače na počítači, druhý zariadenia Apple.
//! Údaje o videu (názov, autor, dátum, zdroj, popis, obrázok) v `moov/udta/meta/ilst`.

use super::mkv::StopaVystupu;
use super::{orez_utf8, z_ns, Doplnky, Druh, Kapitola, Kodek, NS};
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

/// Vzorka tx3g bez textu – úsek, keď sa nič nezobrazuje.
pub const TX3G_PRAZDNA: [u8; 2] = [0, 0];

/// Text titulku → vzorka tx3g: 16-bitová dĺžka a text v UTF-8 (riadky oddelené LF).
pub fn tx3g_vzorka(text: &str) -> Vec<u8> {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    let t = orez_utf8(t.trim_matches('\n'), u16::MAX as usize);
    let mut o = (t.len() as u16).to_be_bytes().to_vec();
    o.extend_from_slice(t.as_bytes());
    o
}

/// Opis vzorky `tx3g`: biele písmo bez pozadia, v strede pri spodnom okraji. Textové pole je celý
/// obraz (ak ho poznáme) a veľkosť písma asi dvadsatina jeho výšky – čitateľný riadok titulkov.
fn tx3g_opis(sirka: u32, vyska: u32) -> Vec<u8> {
    let mut t = vec![0u8; 6];
    t.extend(1u16.to_be_bytes()); // data_reference_index
    t.extend(0u32.to_be_bytes()); // displayFlags
    t.push(1); // vodorovne: stred
    t.push(0xFF); // zvisle: dole (−1)
    t.extend([0u8; 4]); // pozadie RGBA: priehľadné
    let (s, v) = (
        sirka.min(i16::MAX as u32) as i16,
        vyska.min(i16::MAX as u32) as i16,
    );
    for x in [0i16, 0, v, s] {
        t.extend(x.to_be_bytes()); // BoxRecord: hore, vľavo, dole, vpravo
    }
    t.extend(0u16.to_be_bytes()); // StyleRecord: startChar
    t.extend(0u16.to_be_bytes()); // endChar
    t.extend(1u16.to_be_bytes()); // font-ID
    t.push(0); // bez tučného/kurzívy
    t.push(if vyska > 0 {
        (vyska / 20).clamp(12, 255) as u8
    } else {
        18
    });
    t.extend([0xFF; 4]); // biela, nepriehľadná
    let pismo = b"Sans-Serif";
    let mut ft = 1u16.to_be_bytes().to_vec(); // jedno písmo
    ft.extend(1u16.to_be_bytes());
    ft.push(pismo.len() as u8);
    ft.extend(pismo);
    t.extend(bx(b"ftab", &ft));
    bx(b"tx3g", &t)
}

/// Položka iTunes zoznamu `ilst`: box s jedným `data` (typ 1 = UTF-8, 13 = JPEG, 14 = PNG).
fn polozka_ilst(typ: &[u8; 4], druh: u32, obsah: &[u8]) -> Vec<u8> {
    let mut d = druh.to_be_bytes().to_vec();
    d.extend(0u32.to_be_bytes()); // jazyk/krajina: žiadne
    d.extend_from_slice(obsah);
    bx(typ, &bx(b"data", &d))
}

/// `moov/udta`: kapitoly (`chpl`) a údaje o videu (`meta` s `ilst`).
fn udta_filmu(d: &Doplnky, kapitoly: &[Kapitola]) -> Vec<u8> {
    let mut u = vec![];
    if !kapitoly.is_empty() {
        // chpl verzie 1: 4 rezervované bajty, počet (najviac 255), potom začiatok v 100 ns a názov
        let mut t = vec![0u8; 4];
        let n = kapitoly.len().min(255);
        t.push(n as u8);
        for k in &kapitoly[..n] {
            t.extend(((k.od.max(0) / 100) as u64).to_be_bytes());
            let meno = orez_utf8(&k.nazov, 255);
            t.push(meno.len() as u8);
            t.extend(meno.as_bytes());
        }
        u.extend(plny(b"chpl", 1, 0, &t));
    }
    let mut ilst = vec![];
    let mut text = |kluc: &[u8; 4], hodnota: Option<&str>| {
        if let Some(h) = hodnota.map(str::trim).filter(|h| !h.is_empty()) {
            ilst.extend(polozka_ilst(kluc, 1, h.as_bytes()));
        }
    };
    text(b"\xA9nam", Some(&d.titul));
    text(b"\xA9ART", d.autor.as_deref());
    text(b"\xA9day", d.datum.as_deref());
    text(b"\xA9cmt", d.zdroj.as_deref());
    // krátky popis (iTunes ho obmedzuje na 255 znakov), celý ide do „ldes“
    if let Some(p) = d.popis.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        let kratky: String = p.chars().take(255).collect();
        text(b"desc", Some(&kratky));
        if kratky.len() < p.len() {
            text(b"ldes", Some(p));
        }
    }
    text(b"\xA9too", Some(&format!("Kometa {}", crate::VERZIA)));
    if let Some(o) = &d.obal {
        ilst.extend(polozka_ilst(
            b"covr",
            if o.png() { 14 } else { 13 },
            &o.data,
        ));
    }
    let mut hd = 0u32.to_be_bytes().to_vec(); // pre_defined
    hd.extend(b"mdir");
    hd.extend(b"appl");
    hd.extend([0u8; 8]);
    hd.push(0); // prázdny názov
    let meta = plny(
        b"meta",
        0,
        0,
        &[plny(b"hdlr", 0, 0, &hd), bx(b"ilst", &ilst)].concat(),
    );
    u.extend(meta);
    bx(b"udta", &u)
}

/// Druh stopy v MP4 (titulky a kapitoly sú obe textové, ale s inou úlohou).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Uloha {
    Obraz,
    Zvuk,
    Titulky,
    Kapitoly,
}

/// Čo treba vedieť na zápis jednej stopy `trak`.
struct OpisStopy {
    uloha: Uloha,
    stsd: Vec<u8>,
    jazyk: Option<String>,
    nazov: String,
    zapnuta: bool,
    sirka: u32,
    vyska: u32,
    /// roly MPEG-DASH (urn:mpeg:dash:role:2011), napr. „main“, „dub“
    roly: Vec<&'static str>,
    /// ID kapitolovej stopy (tref/chap)
    kapitoly: Option<u32>,
}

const SCHEMA_ROL: &str = "urn:mpeg:dash:role:2011";

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

fn stsd(s: &StopaVystupu, id: u32, ramec: (u32, u32)) -> Vysledok<Vec<u8>> {
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
        Kodek::Srt => tx3g_opis(ramec.0, ramec.1),
    };
    Ok(stsd_z(vstup))
}

fn stsd_z(vstup: Vec<u8>) -> Vec<u8> {
    let mut t = 1u32.to_be_bytes().to_vec();
    t.extend(vstup);
    plny(b"stsd", 0, 0, &t)
}

fn mierka(s: &StopaVystupu) -> u32 {
    match s.info.kodek {
        Kodek::Opus => 48_000,
        Kodek::Srt => 1000,
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

fn tabulky(meta: &[VzorkaMeta], stopa: usize, m: u32, ofsety_kuskov: &[(usize, u64)]) -> Tabulky {
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

/// Opis stopy z výsledku spájania; `ramec` = rozmer obrazu (pre textové pole titulkov).
fn opis(
    s: &StopaVystupu,
    id: u32,
    ramec: (u32, u32),
    kapitoly: Option<u32>,
) -> Vysledok<OpisStopy> {
    let uloha = match s.info.druh() {
        Druh::Video => Uloha::Obraz,
        Druh::Zvuk => Uloha::Zvuk,
        Druh::Titulky => Uloha::Titulky,
    };
    let (sirka, vyska) = match uloha {
        Uloha::Obraz => (s.info.sirka, s.info.vyska),
        Uloha::Titulky => ramec,
        _ => (0, 0),
    };
    let roly = match (uloha, s.povodna) {
        (Uloha::Zvuk, Some(true)) => vec!["main"],
        (Uloha::Zvuk, Some(false)) => vec!["dub"],
        _ => vec![],
    };
    Ok(OpisStopy {
        uloha,
        stsd: stsd(s, id, ramec)?,
        jazyk: s.jazyk.clone().or_else(|| s.info.jazyk.clone()),
        nazov: s.nazov.clone().unwrap_or_else(|| {
            match uloha {
                Uloha::Obraz => "Obraz",
                Uloha::Zvuk => "Zvuk",
                _ => "Titulky",
            }
            .into()
        }),
        // titulky predvolene vypnuté – zapneš ich v prehrávači
        zapnuta: uloha == Uloha::Obraz || (uloha == Uloha::Zvuk && s.predvolena),
        sirka,
        vyska,
        roly,
        kapitoly,
    })
}

fn trak(id: u32, o: &OpisStopy, t: &Tabulky, co64: bool) -> Vec<u8> {
    let trvanie_ms = (t.trvanie as u128 * 1000 / t.mierka as u128) as u64;
    let oneskorenie_ms = (t.zaciatok_ns / 1_000_000) as u64;
    // tkhd
    let mut h = vec![0u8; 8];
    h.extend(id.to_be_bytes());
    h.extend([0u8; 4]);
    h.extend(((oneskorenie_ms + trvanie_ms).min(u32::MAX as u64) as u32).to_be_bytes());
    h.extend([0u8; 8]);
    // vrstva: titulky pred obrazom (−1); skupina: zvuky (1) aj titulky (2) sú alternatívy navzájom
    let (vrstva, skupina): (i16, u16) = match o.uloha {
        Uloha::Obraz | Uloha::Kapitoly => (0, 0),
        Uloha::Zvuk => (0, 1),
        Uloha::Titulky => (-1, 2),
    };
    h.extend(vrstva.to_be_bytes());
    h.extend(skupina.to_be_bytes());
    h.extend((if o.uloha == Uloha::Zvuk { 0x0100u16 } else { 0 }).to_be_bytes());
    h.extend([0u8; 2]);
    for x in MATICA {
        h.extend(x.to_be_bytes());
    }
    h.extend((o.sirka << 16).to_be_bytes());
    h.extend((o.vyska << 16).to_be_bytes());
    // príznaky: 1 = zapnutá, 2 = súčasť prezentácie (kapitoly nie sú ani jedno)
    let priznaky = match (o.uloha, o.zapnuta) {
        (Uloha::Kapitoly, _) => 0,
        (_, true) => 3,
        (_, false) => 2,
    };
    let tkhd = plny(b"tkhd", 0, priznaky, &h);
    let tref = match o.kapitoly {
        Some(k) => bx(b"tref", &bx(b"chap", &k.to_be_bytes())),
        None => vec![],
    };
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
    // mdia
    let jazyk_b = o
        .jazyk
        .as_deref()
        .map(crate::spajanie::iso639_2)
        .unwrap_or_else(|| "und".into());
    let mut md = vec![0u8; 8];
    md.extend(t.mierka.to_be_bytes());
    md.extend((t.trvanie.min(u32::MAX as u64) as u32).to_be_bytes());
    md.extend(zbal_jazyk(&jazyk_t(&jazyk_b)).to_be_bytes());
    md.extend([0u8; 2]);
    let mdhd = plny(b"mdhd", 0, 0, &md);
    let elng = match &o.jazyk {
        Some(j) if j.contains(['-', '_']) => {
            let mut z = j.replace('_', "-").into_bytes();
            z.push(0);
            plny(b"elng", 0, 0, &z)
        }
        _ => vec![],
    };
    let mut hd = vec![0u8; 4];
    hd.extend(match o.uloha {
        Uloha::Obraz => b"vide",
        Uloha::Zvuk => b"soun",
        Uloha::Titulky => b"sbtl",
        Uloha::Kapitoly => b"text",
    });
    hd.extend([0u8; 12]);
    hd.extend(o.nazov.as_bytes());
    hd.push(0);
    let hdlr = plny(b"hdlr", 0, 0, &hd);
    let hlavicka_media = match o.uloha {
        Uloha::Obraz => plny(b"vmhd", 0, 1, &[0u8; 8]),
        Uloha::Zvuk => plny(b"smhd", 0, 0, &[0u8; 4]),
        _ => plny(b"nmhd", 0, 0, &[]),
    };
    let dref = plny(
        b"dref",
        0,
        0,
        &[&1u32.to_be_bytes()[..], &plny(b"url ", 0, 1, &[])].concat(),
    );
    let dinf = bx(b"dinf", &dref);
    // stbl
    let mut stbl = o.stsd.clone();
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
    // udta stopy: názov (ako ho čítajú prehrávače a FFmpeg) a roly
    let mut u = bx(b"name", o.nazov.as_bytes());
    for r in &o.roly {
        let mut k = SCHEMA_ROL.as_bytes().to_vec();
        k.push(0);
        k.extend(r.as_bytes());
        k.push(0);
        u.extend(plny(b"kind", 0, 0, &k));
    }
    let udta = bx(b"udta", &u);
    bx(b"trak", &[tkhd, tref, edts, mdia, udta].concat())
}

pub struct ZapisovacMp4 {
    f: BufWriter<File>,
    ocakavane: Vec<u32>,
    dalsia: usize,
    /// textové stopy: počítajú sa len vzorky s textom (prázdne vyplňujú medzery)
    textove: Vec<bool>,
    pub pocty: Vec<u64>,
    /// koľko kapitol sa zapísalo (chpl aj kapitolová stopa)
    pub kapitoly: usize,
}

impl ZapisovacMp4 {
    /// Zapíše ftyp + moov + hlavičku mdat (a kapitoly na jej začiatok); potom treba volať
    /// [`ZapisovacMp4::data`] pre každú vzorku v poradí `meta`.
    pub fn new(
        cesta: &Path,
        stopy: &[StopaVystupu],
        meta: &[VzorkaMeta],
        doplnky: &Doplnky,
    ) -> Vysledok<Self> {
        let ma_avc = stopy.iter().any(|s| s.info.kodek == Kodek::H264);
        let mut ft = b"isom".to_vec();
        ft.extend(0x200u32.to_be_bytes());
        ft.extend(b"isomiso2");
        if ma_avc {
            ft.extend(b"avc1");
        }
        ft.extend(b"mp41");
        let ftyp = bx(b"ftyp", &ft);
        let ramec = stopy
            .iter()
            .find(|s| s.info.druh() == Druh::Video)
            .map(|s| (s.info.sirka, s.info.vyska))
            .unwrap_or((0, 0));
        // dĺžka filmu (ms) z tabuliek vzoriek – potrebná pre koniec poslednej kapitoly
        let mut film_ms = 0u64;
        for (k, s) in stopy.iter().enumerate() {
            let t = tabulky(meta, k, mierka(s), &[]);
            film_ms = film_ms.max(
                (t.zaciatok_ns / 1_000_000) as u64
                    + (t.trvanie as u128 * 1000 / t.mierka as u128) as u64,
            );
        }
        // kapitoly: vzorky textovej stopy na začiatku mdat (jeden kúsok)
        let kapitoly = doplnky.kapitoly_do(film_ms as i64 * 1_000_000);
        let kap_data: Vec<Vec<u8>> = kapitoly.iter().map(|k| tx3g_vzorka(&k.nazov)).collect();
        let n_kap = stopy.len();
        let kap_meta: Vec<VzorkaMeta> = kapitoly
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let do_ = kapitoly
                    .get(i + 1)
                    .map(|n| n.od)
                    .unwrap_or(film_ms as i64 * 1_000_000)
                    .max(k.od + NS / 1000);
                VzorkaMeta {
                    stopa: n_kap as u16,
                    velkost: kap_data[i].len() as u32,
                    dts: k.od,
                    pts: k.od,
                    trvanie: do_ - k.od,
                    kluc: true,
                }
            })
            .collect();
        let kap_bajty: u64 = kap_data.iter().map(|d| d.len() as u64).sum();
        let id_kapitol = (!kapitoly.is_empty()).then_some(stopy.len() as u32 + 1);
        // kúsky: súvislé behy vzoriek tej istej stopy (za kapitolami)
        let mut behy: Vec<(usize, usize, u64)> = vec![]; // (stopa, počet, relatívny ofset)
        let mut rel = kap_bajty;
        for v in meta {
            match behy.last_mut() {
                Some((s, n, _)) if *s == v.stopa as usize && *n < 1024 => *n += 1,
                _ => behy.push((v.stopa as usize, 1, rel)),
            }
            rel += v.velkost as u64;
        }
        let spolu = rel;
        let udta = udta_filmu(doplnky, &kapitoly);
        let postav = |zaklad: u64, co64: bool| -> Vysledok<Vec<u8>> {
            let mut traky = vec![];
            let mut film = 0u64;
            for (k, s) in stopy.iter().enumerate() {
                let ofs: Vec<(usize, u64)> = behy
                    .iter()
                    .filter(|b| b.0 == k)
                    .map(|b| (b.1, zaklad + b.2))
                    .collect();
                let t = tabulky(meta, k, mierka(s), &ofs);
                film = film.max(
                    (t.zaciatok_ns / 1_000_000) as u64
                        + (t.trvanie as u128 * 1000 / t.mierka as u128) as u64,
                );
                // na kapitoly odkazuje prvá stopa (obraz, alebo prvý zvuk)
                let o = opis(s, k as u32 + 1, ramec, id_kapitol.filter(|_| k == 0))?;
                traky.extend(trak(k as u32 + 1, &o, &t, co64));
            }
            if let Some(id) = id_kapitol {
                let t = tabulky(&kap_meta, n_kap, 1000, &[(kapitoly.len(), zaklad)]);
                let o = OpisStopy {
                    uloha: Uloha::Kapitoly,
                    stsd: stsd_z(tx3g_opis(0, 0)),
                    jazyk: None,
                    nazov: "Kapitoly".into(),
                    zapnuta: false,
                    sirka: 0,
                    vyska: 0,
                    roly: vec![],
                    kapitoly: None,
                };
                traky.extend(trak(id, &o, &t, co64));
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
            let dalsie_id = stopy.len() as u32 + 1 + id_kapitol.is_some() as u32;
            mv.extend(dalsie_id.to_be_bytes());
            let mvhd = plny(b"mvhd", 0, 0, &mv);
            Ok(bx(b"moov", &[mvhd, traky, udta.clone()].concat()))
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
        for d in &kap_data {
            f.write_all(d)?;
        }
        Ok(ZapisovacMp4 {
            f,
            ocakavane: meta.iter().map(|v| v.velkost).collect(),
            dalsia: 0,
            textove: stopy.iter().map(|s| s.info.kodek == Kodek::Srt).collect(),
            pocty: vec![0; stopy.len()],
            kapitoly: kapitoly.len(),
        })
    }

    pub fn data(&mut self, stopa: usize, d: &[u8]) -> Vysledok<()> {
        match self.ocakavane.get(self.dalsia) {
            Some(&v) if v as usize == d.len() => {}
            _ => return Err(Chyba::Format("MP4: druhý prechod nesedí s prvým".into())),
        }
        self.dalsia += 1;
        if !(self.textove[stopa] && d.len() <= 2) {
            self.pocty[stopa] += 1;
        }
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
    fn tx3g_vzorky_a_opis() {
        assert_eq!(
            tx3g_vzorka("Ahoj\r\nsvet\n"),
            b"\x00\x09Ahoj\nsvet".to_vec()
        );
        assert_eq!(tx3g_vzorka(""), TX3G_PRAZDNA.to_vec());
        let o = tx3g_opis(1920, 1080);
        assert_eq!(&o[4..8], b"tx3g");
        // 8 hlavička + 8 SampleEntry + 4 displayFlags + 2 zarovnanie + 4 pozadie
        assert_eq!(&o[26..34], &[0, 0, 0, 0, 0x04, 0x38, 0x07, 0x80]);
        assert_eq!(&o[38..42], &[0, 1, 0, 54]); // písmo č. 1, obyčajné, 1080 / 20
        assert_eq!(&o[o.len() - 19..o.len() - 15], b"ftab"); // ftab: 23 B s písmom Sans-Serif
        assert_eq!(
            u32::from_be_bytes(o[..4].try_into().unwrap()) as usize,
            o.len()
        );
    }

    #[test]
    fn udaje_a_kapitoly() {
        let d = Doplnky {
            titul: "Film".into(),
            autor: Some("Kanál".into()),
            kapitoly: vec![
                Kapitola {
                    od: 0,
                    nazov: "Začiatok".into(),
                },
                Kapitola {
                    od: 61 * NS,
                    nazov: "Druhá".into(),
                },
            ],
            ..Default::default()
        };
        let u = udta_filmu(&d, &d.kapitoly_do(0));
        let hladaj = |vzor: &[u8]| u.windows(vzor.len()).position(|w| w == vzor);
        let c = hladaj(b"chpl").unwrap();
        assert_eq!(u[c + 4], 1); // verzia 1
        assert_eq!(u[c + 12], 2); // dve kapitoly
        assert_eq!(&u[c + 13..c + 22], &[0, 0, 0, 0, 0, 0, 0, 0, 9]); // prvá od 0, názov 9 B
        let druha = c + 13 + 8 + 1 + "Začiatok".len();
        assert_eq!(
            u64::from_be_bytes(u[druha..druha + 8].try_into().unwrap()),
            610_000_000 // 61 s v 100 ns
        );
        assert!(hladaj(b"\xA9namdata").is_none()); // medzi nimi je veľkosť
        let n = hladaj(b"\xA9nam").unwrap();
        assert_eq!(&u[n + 8..n + 12], b"data");
        assert_eq!(&u[n + 12..n + 16], &[0, 0, 0, 1]);
        assert_eq!(&u[n + 20..n + 24], b"Film");
        assert!(hladaj(b"mdirappl").is_some());
        assert!(hladaj(b"covr").is_none());
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
