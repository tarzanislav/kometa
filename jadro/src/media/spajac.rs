//! Spojenie stôp z viacerých súborov do jedného MKV/MKA alebo MP4/M4A – bez FFmpeg.
//! Titulky (SRT) idú do Matroska ako S_TEXT/UTF8, do MP4 ako 3GPP Timed Text (tx3g).
//! Spolu so stopami sa zapíšu údaje o videu, kapitoly a obrázok ([`Doplnky`]).
//!
//! Vzorky sa zlučujú podľa času dekódovania, všetky stopy sa posunú na spoločný začiatok 0
//! (vzájomný posun obrazu a zvuku ostane zachovaný). Výsledok sa zapíše do dočasného súboru,
//! skontroluje a až potom premenuje na konečný názov.

use super::mkv::{self, StopaVystupu, ZapisovacMkv};
use super::mp4zapis::{tx3g_vzorka, VzorkaMeta, ZapisovacMp4, TX3G_PRAZDNA};
use super::{otvor, Citac, Doplnky, Druh, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use crate::klient::s_priponou;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Vstupný súbor so stopami, ktoré sa z neho vezmú.
#[derive(Clone, Debug)]
pub struct Vstup {
    pub cesta: PathBuf,
    /// "video", "zvuk" alebo "video+zvuk"
    pub druh: String,
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
    /// stopa v pôvodnom jazyku videa (Some(true)) alebo preklad/dabing (Some(false))
    pub povodny: Option<bool>,
}

struct Vyber {
    citace: Vec<Box<dyn Citac>>,
    /// vstup → stopa čítača → stopa výsledku
    mapa: Vec<Vec<Option<usize>>>,
    stopy: Vec<StopaVystupu>,
}

fn vyber(vstupy: &[Vstup]) -> Vysledok<Vyber> {
    let mut citace = vec![];
    for v in vstupy {
        citace.push(otvor(&v.cesta)?);
    }
    let mut mapa: Vec<Vec<Option<usize>>> =
        citace.iter().map(|c| vec![None; c.stopy().len()]).collect();
    let mut stopy = vec![];
    // najprv obraz (len prvý), potom zvuky v poradí vstupov (= poradie jazykov)
    for druh in [Druh::Video, Druh::Zvuk, Druh::Titulky] {
        for (i, (c, v)) in citace.iter().zip(vstupy).enumerate() {
            let chcem = match druh {
                Druh::Video => v.druh.contains("video"),
                Druh::Zvuk => v.druh.contains("zvuk"),
                Druh::Titulky => v.druh.contains("titulky"),
            };
            if !chcem {
                continue;
            }
            for (k, info) in c.stopy().iter().enumerate() {
                if info.druh() != druh {
                    continue;
                }
                if druh == Druh::Video
                    && stopy
                        .iter()
                        .any(|s: &StopaVystupu| s.info.druh() == Druh::Video)
                {
                    continue;
                }
                let prvy_zvuk = druh == Druh::Zvuk
                    && !stopy
                        .iter()
                        .any(|s: &StopaVystupu| s.info.druh() == Druh::Zvuk);
                mapa[i][k] = Some(stopy.len());
                stopy.push(StopaVystupu {
                    info: info.clone(),
                    jazyk: if druh != Druh::Video {
                        v.jazyk.clone().or_else(|| info.jazyk.clone())
                    } else {
                        None
                    },
                    nazov: if druh != Druh::Video {
                        v.nazov.clone().or_else(|| info.nazov.clone())
                    } else {
                        None
                    },
                    predvolena: druh == Druh::Video || prvy_zvuk,
                    // titulky v inom jazyku nie sú „dabing“ – pri nich sa označí len pôvodný jazyk
                    povodna: match druh {
                        Druh::Video => None,
                        Druh::Zvuk => v.povodny,
                        Druh::Titulky => v.povodny.filter(|&p| p),
                    },
                });
            }
        }
    }
    if stopy.is_empty() {
        return Err(Chyba::Format("v súboroch nie je obraz ani zvuk".into()));
    }
    Ok(Vyber {
        citace,
        mapa,
        stopy,
    })
}

#[allow(clippy::needless_range_loop)] // indexy vstupov spájajú viac polí (buf, mapa, koniec)
/// Prejde všetky vzorky v poradí dekódovania; doplní chýbajúce trvania a posunie časy o `nula`.
/// Vráti použitú nulu (pri `None` ju určí z najmenšieho prezentačného času na začiatku).
fn prejdi(
    v: &mut Vyber,
    nula: Option<i64>,
    mut f: impl FnMut(usize, Vzorka) -> Vysledok<()>,
) -> Vysledok<i64> {
    let n = v.citace.len();
    let mut buf: Vec<VecDeque<(usize, Vzorka)>> = (0..n).map(|_| VecDeque::new()).collect();
    let mut koniec = vec![false; n];
    let dalsia =
        |v: &mut Vyber, i: usize, koniec: &mut [bool]| -> Vysledok<Option<(usize, Vzorka)>> {
            while !koniec[i] {
                match v.citace[i].dalsia()? {
                    Some(s) => {
                        if let Some(Some(o)) = v.mapa[i].get(s.stopa) {
                            return Ok(Some((*o, s)));
                        }
                    }
                    None => koniec[i] = true,
                }
            }
            Ok(None)
        };
    // nula: najmenší čas medzi prvými vzorkami (pozrieme sa dopredu, kým nemá každá stopa vzorku)
    let nula = match nula {
        Some(z) => z,
        None => {
            let mut min = i64::MAX;
            for i in 0..n {
                let stopy_vstupu: Vec<usize> = v.mapa[i].iter().flatten().copied().collect();
                let mut videne = vec![];
                while buf[i].len() < 4000 && !stopy_vstupu.iter().all(|s| videne.contains(s)) {
                    match dalsia(v, i, &mut koniec)? {
                        Some((o, s)) => {
                            if !videne.contains(&o) {
                                videne.push(o);
                            }
                            buf[i].push_back((o, s));
                        }
                        None => break,
                    }
                }
                for (_, s) in &buf[i] {
                    min = min.min(s.pts);
                }
            }
            if min == i64::MAX {
                0
            } else {
                min
            }
        }
    };
    let pocet = v.stopy.len();
    let mut cakajuca: Vec<Option<Vzorka>> = (0..pocet).map(|_| None).collect();
    let mut posledne_trvanie = vec![0i64; pocet];
    let mut vydaj = |o: usize,
                     mut s: Vzorka,
                     cakajuca: &mut Vec<Option<Vzorka>>,
                     posl: &mut Vec<i64>|
     -> Vysledok<()> {
        s.pts -= nula;
        s.dts -= nula;
        if let Some(mut p) = cakajuca[o].take() {
            if p.trvanie <= 0 {
                let d = s.dts - p.dts;
                p.trvanie = if d > 0 { d } else { posl[o] };
            }
            posl[o] = p.trvanie;
            f(o, p)?;
        }
        cakajuca[o] = Some(s);
        Ok(())
    };
    loop {
        for i in 0..n {
            if buf[i].is_empty() && !koniec[i] {
                if let Some(x) = dalsia(v, i, &mut koniec)? {
                    buf[i].push_back(x);
                }
            }
        }
        let naj = (0..n)
            .filter(|&i| !buf[i].is_empty())
            .min_by_key(|&i| buf[i][0].1.dts);
        let Some(i) = naj else { break };
        let (o, s) = buf[i].pop_front().unwrap_or_else(|| unreachable!());
        vydaj(o, s, &mut cakajuca, &mut posledne_trvanie)?;
    }
    for o in 0..pocet {
        if let Some(mut p) = cakajuca[o].take() {
            if p.trvanie <= 0 {
                p.trvanie = posledne_trvanie[o];
            }
            f(o, p)?;
        }
    }
    Ok(nula)
}

/// Spojí vstupy do `ciel`; formát podľa prípony (.mp4/.m4a = MP4, inak Matroska).
/// Vráti počty vzoriek v jednotlivých stopách výsledku.
pub fn spoj(vstupy: &[Vstup], ciel: &Path, titul: &str) -> Vysledok<Vec<u64>> {
    spoj_s(vstupy, ciel, &Doplnky::s_titulom(titul))
}

/// Ako [`spoj`], navyše zapíše údaje o videu, kapitoly a obrázok.
pub fn spoj_s(vstupy: &[Vstup], ciel: &Path, doplnky: &Doplnky) -> Vysledok<Vec<u64>> {
    let mp4 = matches!(
        ciel.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp4") | Some("m4a") | Some("m4v")
    );
    let docasny = s_priponou(ciel, ".spajanie");
    let opus = ciel
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("opus"));
    let vysledok = if opus {
        spoj_opus(vstupy, &docasny, doplnky)
    } else if mp4 {
        spoj_mp4(vstupy, &docasny, doplnky)
    } else {
        spoj_mkv(vstupy, &docasny, doplnky)
    };
    match vysledok {
        Ok(pocty) => {
            std::fs::rename(&docasny, ciel)?;
            Ok(pocty)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&docasny);
            Err(e)
        }
    }
}

/// Jedna stopa Opus → súbor `.opus` (Ogg) bez prekódovania.
fn spoj_opus(vstupy: &[Vstup], docasny: &Path, doplnky: &Doplnky) -> Vysledok<Vec<u64>> {
    let mut v = vyber(vstupy)?;
    if v.stopy.len() != 1 || v.stopy[0].info.kodek != Kodek::Opus {
        return Err(Chyba::Nepodporovane(
            "do súboru .opus ide len jedna zvuková stopa Opus".into(),
        ));
    }
    let info = v.stopy[0].info.clone();
    let (head, pre) = super::ogg::opus_head(&info.konfig, info.kanaly as u16);
    let tags = super::ogg::opus_tags(doplnky, doplnky.trvanie.unwrap_or(0));
    let mut z = super::ogg::ZapisovacOgg::new(docasny, &head, &tags)?;
    let mut g = pre as u64;
    prejdi(&mut v, None, |_, s| {
        g += super::ogg::vzorky_paketu(&s.data);
        z.paket(&s.data, g)
    })?;
    let n = z.dokonci()?;
    let (ok, prec, _) = super::ogg::over(docasny)?;
    if !ok || prec != n {
        return Err(Chyba::Format(format!(
            "kontrola Opus: zapísané {n} paketov, prečítané {prec}"
        )));
    }
    Ok(vec![n])
}

fn spoj_mkv(vstupy: &[Vstup], docasny: &Path, doplnky: &Doplnky) -> Vysledok<Vec<u64>> {
    let mut v = vyber(vstupy)?;
    let mut z = ZapisovacMkv::new(docasny, &v.stopy, doplnky)?;
    prejdi(&mut v, None, |o, s| {
        z.zapis(o, s.pts, s.trvanie, s.kluc, &s.data)
    })?;
    let (pocty, kapitol) = z.dokonci_s()?;
    let k = mkv::over_vsetko(docasny, v.stopy.len())?;
    if k.pocty != pocty || k.kapitoly != kapitol || k.prilohy != doplnky.obal.is_some() as usize {
        return Err(Chyba::Format(format!(
            "kontrola MKV: zapísané {pocty:?}, prečítané {:?} (kapitoly {}, prílohy {})",
            k.pocty, k.kapitoly, k.prilohy
        )));
    }
    Ok(pocty)
}

/// Titulky v MP4 (tx3g) platia od svojho začiatku po ďalšiu vzorku, preto čas bez titulku treba
/// vyplniť prázdnou vzorkou a prekryvy orezať. Ostatné stopy prejdú bez zmeny.
struct Medzery {
    /// pri textovej stope: koniec poslednej zapísanej vzorky (ns)
    koniec: Vec<Option<i64>>,
}

impl Medzery {
    fn new(v: &Vyber) -> Self {
        Medzery {
            koniec: v
                .stopy
                .iter()
                .map(|s| (s.info.kodek == Kodek::Srt).then_some(0))
                .collect(),
        }
    }

    fn vydaj(
        &mut self,
        o: usize,
        s: Vzorka,
        f: &mut impl FnMut(usize, Vzorka) -> Vysledok<()>,
    ) -> Vysledok<()> {
        let Some(koniec) = self.koniec[o] else {
            return f(o, s);
        };
        let od = s.pts.max(koniec);
        let do_ = s.pts + s.trvanie.max(0);
        let text = String::from_utf8_lossy(&s.data).into_owned();
        if do_ <= od || text.trim().is_empty() {
            return Ok(()); // titulok úplne prekrytý predošlým alebo prázdny
        }
        if od > koniec {
            f(
                o,
                Vzorka {
                    dts: koniec,
                    pts: koniec,
                    trvanie: od - koniec,
                    kluc: true,
                    data: TX3G_PRAZDNA.to_vec(),
                    ..s.clone()
                },
            )?;
        }
        self.koniec[o] = Some(do_);
        f(
            o,
            Vzorka {
                dts: od,
                pts: od,
                trvanie: do_ - od,
                kluc: true,
                data: tx3g_vzorka(&text),
                ..s
            },
        )
    }
}

fn spoj_mp4(vstupy: &[Vstup], docasny: &Path, doplnky: &Doplnky) -> Vysledok<Vec<u64>> {
    // 1. prechod: veľkosti a časy
    let mut v = vyber(vstupy)?;
    let mut meta = vec![];
    let mut medzery = Medzery::new(&v);
    let nula = prejdi(&mut v, None, |o, s| {
        medzery.vydaj(o, s, &mut |o, s| {
            meta.push(VzorkaMeta {
                stopa: o as u16,
                velkost: s.data.len() as u32,
                dts: s.dts,
                pts: s.pts,
                trvanie: s.trvanie,
                kluc: s.kluc,
            });
            Ok(())
        })
    })?;
    let stopy = v.stopy.clone();
    drop(v);
    let mut z = ZapisovacMp4::new(docasny, &stopy, &meta, doplnky)?;
    let kapitol = z.kapitoly;
    // 2. prechod: samotné vzorky v rovnakom poradí
    let mut v = vyber(vstupy)?;
    let mut medzery = Medzery::new(&v);
    prejdi(&mut v, Some(nula), |o, s| {
        medzery.vydaj(o, s, &mut |o, s| z.data(o, &s.data))
    })?;
    let pocty = z.dokonci()?;
    // kontrola: súbor sa dá otvoriť vlastným čítačom a sedia počty vzoriek
    let mut c = super::mp4::CitacMp4::otvor(docasny)?;
    let mut prec = vec![0u64; stopy.len()];
    while let Some(s) = c.dalsia()? {
        if let Some(x) = prec.get_mut(s.stopa) {
            *x += 1;
        }
    }
    if prec != pocty {
        return Err(Chyba::Format(format!(
            "kontrola MP4: zapísané {pocty:?}, prečítané {prec:?}"
        )));
    }
    // a hlavička: stopy, kapitoly, údaje a obrázok tak, ako ich uvidia prehrávače
    let p = super::mp4::prehlad(docasny)?;
    let stop = p.stopy.iter().filter(|s| s.handler != "text").count();
    let zle = |co: &str| Err(Chyba::Format(format!("kontrola MP4: {co}")));
    if stop != stopy.len() {
        return zle("nesedí počet stôp");
    }
    if p.kapitoly.len() != kapitol.min(255) || p.kapitolova_stopa != (kapitol > 0) {
        return zle("kapitoly");
    }
    if !doplnky.titul.trim().is_empty() && !p.udaje.contains_key("\u{a9}nam") {
        return zle("chýba názov videa");
    }
    if p.obal.is_some() != doplnky.obal.is_some() {
        return zle("obrázok videa");
    }
    Ok(pocty)
}
