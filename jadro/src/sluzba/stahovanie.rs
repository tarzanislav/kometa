//! Stiahnutie jednej položky frontu.
//!
//! Poradie: Webshare/Fastshare → účty; služby, ktoré pozná len yt-dlp (YouTube…) → yt-dlp;
//! všetko ostatné vlastným jadrom (stránka → skener, prehrávač, stream, súbor). Keď jadro na stránke
//! nič nenájde alebo službu nepozná a yt-dlp je po ruke, skúsi sa yt-dlp.

use super::fronta::{self, Nova, Polozka, Stav};
use super::ytdlp::{self, FormatNa, Spolocne};
use super::zdroje::{self, bezpecny_nazov};
use super::Vnutro;
use crate::chyba::{Chyba, Vysledok};
use crate::http::{self, Moznosti};
use crate::klient::{self, s_priponou, Nastavenia};
use crate::spajanie::{self, Vstup};
use crate::stream;
use crate::udalosti::{Hlasic, Udalost};
use crate::web::riesenie::{self, Ciel};
use crate::web::vzory;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub enum Koniec {
    Subor(PathBuf),
    /// stránka mala viac videí – pridali sa do frontu
    Rozbalene(usize),
}

/// Služby, ktoré jadro nesťahuje samo – idú rovno do yt-dlp.
fn len_ytdlp(url: &str) -> bool {
    if let Some(p) = vzory::prehravac(url) {
        return !matches!(
            p.poskytovatel,
            "Vimeo" | "JW Player" | "Wistia" | "Cloudflare Stream" | "Bunny Stream"
        );
    }
    matches!(
        zdroje::nazov_zdroja(url),
        "YouTube" | "TikTok" | "Instagram" | "Facebook" | "X/Twitter"
    )
}

/// Úloha jednej položky: stiahne ju a zapíše výsledok do frontu.
pub async fn prace(v: Arc<Vnutro>, id: String, zrusenie: CancellationToken) {
    let Some(p) = v.fronta.lock().unwrap().najdi(&id).cloned() else {
        return;
    };
    let h = hlasic_polozky(&v, &id);
    let vysledok = stiahni(&v, &p, &zrusenie, &h).await;
    // skutočná veľkosť výsledku (pri streamoch sa počas sťahovania len odhaduje)
    let velkost = match &vysledok {
        Ok(Koniec::Subor(c)) => tokio::fs::metadata(c).await.ok().map(|m| m.len()),
        _ => None,
    };
    {
        let mut f = v.fronta.lock().unwrap();
        f.behy.remove(&id);
        f.uprav(&id, |x| {
            x.rychlost = 0.0;
            match vysledok {
                Ok(Koniec::Subor(ref cesta)) => {
                    x.stav = Stav::Hotovo;
                    x.subor = cesta.to_string_lossy().into_owned();
                    x.hotovo =
                        velkost.unwrap_or_else(|| x.celkom.unwrap_or(x.hotovo).max(x.hotovo));
                    x.celkom = Some(x.hotovo);
                    x.docasne.clear();
                    x.podiel = None;
                    x.krok.clear();
                    if x.nazov.is_empty() {
                        x.nazov = cesta
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                }
                Ok(Koniec::Rozbalene(n)) => {
                    x.stav = Stav::Rozbalene;
                    let stranka = if x.nazov.is_empty() {
                        x.url.clone()
                    } else {
                        x.nazov.clone()
                    };
                    x.nazov = format!("{stranka} – nájdené videá: {n}");
                }
                Err(Chyba::Pauza) => x.stav = Stav::Pauza,
                Err(Chyba::Heslo(ref t)) => {
                    x.stav = Stav::Chyba;
                    x.chyba = t.clone();
                    x.podrobnosti.clear();
                    x.treba_heslo = "subor".into();
                }
                Err(ref e) => {
                    let t = e.to_string();
                    let (kratka, podrobnosti) = match t.split_once("\n\nPodrobnosti: ") {
                        Some((a, b)) => (a.to_string(), b.to_string()),
                        None => (t, String::new()),
                    };
                    x.stav = Stav::Chyba;
                    x.chyba = kratka;
                    x.podrobnosti = podrobnosti;
                }
            }
        });
        f.uloz(true);
    }
    if let Ok(Koniec::Subor(_)) = &vysledok {
        skus_rozbalit(&v, &id, None, false);
    }
    match &vysledok {
        Ok(Koniec::Subor(c)) => v.zapis(format!(
            "✔ Hotovo: {}",
            c.file_name().unwrap_or_default().to_string_lossy()
        )),
        Err(Chyba::Pauza) | Ok(_) => {}
        Err(e) => v.zapis(format!(
            "✖ {}: {}",
            if p.nazov.is_empty() { &p.url } else { &p.nazov },
            e.to_string().lines().next().unwrap_or("")
        )),
    }
    v.naplanuj();
}

/// Priebeh viacerých stôp (obraz, zvuky) ako jeden: stopy idú za sebou, celková veľkosť sa odhadne.
struct Sucet {
    stop: usize,
    index: usize,
    zaklad: u64,
    cast: Option<String>,
    posledne: u64,
}

fn hlasic_polozky(v: &Arc<Vnutro>, id: &str) -> (Hlasic, Arc<Mutex<Sucet>>) {
    let sucet = Arc::new(Mutex::new(Sucet {
        stop: 1,
        index: 0,
        zaklad: 0,
        cast: None,
        posledne: 0,
    }));
    let (v2, id2, s2) = (Arc::downgrade(v), id.to_string(), sucet.clone());
    let h: Hlasic = Arc::new(move |u| {
        let Some(v) = v2.upgrade() else { return };
        match u {
            Udalost::Priebeh {
                stiahnute,
                celkom,
                rychlost,
                cast,
            } => {
                let (hotovo, spolu) = {
                    let mut s = s2.lock().unwrap();
                    if s.cast.is_some() && cast.is_some() && s.cast != cast {
                        s.zaklad += s.posledne;
                        s.index += 1;
                    }
                    s.cast = cast;
                    s.posledne = stiahnute;
                    let hotovo = s.zaklad + stiahnute;
                    let spolu = celkom.map(|c| {
                        let tu = s.zaklad + c;
                        if s.stop <= 1 || s.index + 1 >= s.stop {
                            tu
                        } else {
                            (tu as f64 * s.stop as f64 / (s.index + 1) as f64) as u64
                        }
                    });
                    (hotovo, spolu)
                };
                let mut f = v.fronta.lock().unwrap();
                f.uprav(&id2, |x| {
                    x.hotovo = hotovo;
                    x.celkom = spolu.map(|c| c.max(hotovo));
                    x.rychlost = rychlost;
                });
                f.uloz(false);
            }
            Udalost::Info { sprava } => v.zapis(sprava),
            Udalost::Varovanie { sprava } => v.zapis(format!("⚠ {sprava}")),
            _ => {}
        }
    });
    (h, sucet)
}

fn uprav(v: &Vnutro, id: &str, f: impl FnOnce(&mut Polozka)) {
    v.fronta.lock().unwrap().uprav(id, f);
}

/// Cieľ položky: pri pokračovaní ten istý, inak nový súbor bez prepísania existujúceho.
fn ciel(v: &Vnutro, p: &Polozka, meno: &str, pripona: &str) -> PathBuf {
    if !p.ciel.is_empty() {
        return PathBuf::from(&p.ciel);
    }
    let priecinok = PathBuf::from(&p.priecinok);
    let kmen = bezpecny_nazov(meno, 180);
    let kmen = kmen
        .strip_suffix(&format!(".{pripona}"))
        .unwrap_or(&kmen)
        .to_string();
    let obsadene = |c: &Path| {
        c.exists() || s_priponou(c, ".part").exists() || s_priponou(c, ".kometa").exists()
    };
    let mut c = priecinok.join(format!("{kmen}.{pripona}"));
    let mut n = 2;
    while obsadene(&c) {
        c = priecinok.join(format!("{kmen} ({n}).{pripona}"));
        n += 1;
    }
    let s = c.to_string_lossy().into_owned();
    uprav(v, &p.id, |x| x.ciel = s);
    c
}

/// Cieľ pre súbor s názvom aj príponou (Webshare, Fastshare, priamy súbor).
pub fn ciel_suboru(v: &Vnutro, p: &Polozka, meno: &str) -> PathBuf {
    match meno.rsplit_once('.') {
        Some((a, b)) if (1..=5).contains(&b.len()) && !a.is_empty() => ciel(v, p, a, b),
        _ => ciel(v, p, meno, "bin"),
    }
}

pub(crate) fn nazov_z_adresy(url: &str) -> String {
    let cesta = url.split(['?', '#']).next().unwrap_or("");
    let posledny = cesta.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let b = posledny.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(&posledny[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    if s.is_empty() || s.contains(':') {
        "kometa".into()
    } else {
        s
    }
}

async fn stiahni(
    v: &Arc<Vnutro>,
    p: &Polozka,
    z: &CancellationToken,
    h: &(Hlasic, Arc<Mutex<Sucet>>),
) -> Vysledok<Koniec> {
    if zdroje::ULOZISKA.contains(&p.zdroj.as_str()) {
        return super::ucty::stiahni(v, p, z, &h.0).await;
    }
    if let Some(o) = &p.jazyky {
        match super::jazyky::z_frontu(v, p, o, z).await {
            Err(Chyba::Nepodporovane(t)) => {
                v.zapis(format!(
                    "{}: {t} – sťahujem bežne",
                    if p.nazov.is_empty() { &p.url } else { &p.nazov }
                ));
            }
            r => return r.map(Koniec::Subor),
        }
    }
    let ytdlp = v.nastroje.ytdlp();
    if std::env::var("KOMETA_VZDY_YTDLP").is_ok_and(|x| x == "1") || len_ytdlp(&p.url) {
        let exe = ytdlp.ok_or_else(|| {
            Chyba::Ine(format!(
                "{} sťahuje pomocník yt-dlp, ktorý chýba – v Nastaveniach klikni na „Doinštalovať yt-dlp“.",
                zdroje::nazov_zdroja(&p.url).replace("Web", "Túto stránku")
            ))
        })?;
        return cez_ytdlp(v, p, z, h, &exe).await;
    }
    match nativne(v, p, z, h).await {
        Err(Chyba::Nepodporovane(t)) if !p.zo_skenu => match ytdlp {
            Some(exe) => {
                v.zapis(format!("Jadro: {t} – skúšam yt-dlp"));
                cez_ytdlp(v, p, z, h, &exe).await
            }
            None => Err(Chyba::Ine(format!(
                "{t}. Doinštaluj yt-dlp v Nastaveniach – pozná stovky ďalších služieb."
            ))),
        },
        Err(Chyba::Format(t)) if t.contains("nenašlo") && !p.zo_skenu => match ytdlp {
            Some(exe) => {
                v.zapis("Na stránke som nič nenašiel – skúšam yt-dlp");
                cez_ytdlp(v, p, z, h, &exe).await.map_err(|_| Chyba::Ine(t))
            }
            None => Err(Chyba::Ine(t)),
        },
        r => r,
    }
}

fn nastavenia_siete(v: &Vnutro, p: &Polozka) -> Nastavenia {
    let cookies = if p.prihlasenie {
        super::kurzy::cookies_pre(v, &p.url)
    } else {
        vec![]
    };
    let n = v.nastavenia.lock().unwrap();
    Nastavenia {
        cookies,
        referer: Some(p.referer.clone()).filter(|r| !r.is_empty()),
        hlavicky: vec![],
        spojenia: n.cislo("connections", 4).clamp(1, 16) as usize,
        max_vyska: match p.format.as_str() {
            "1080" => Some(1080),
            "720" => Some(720),
            _ => None,
        },
        jazyky: n.jazyky(),
    }
}

fn zvukovy(format: &str) -> bool {
    matches!(format, "audio" | "mp3")
}

/// Vlastné jadro: stránka, prehrávač, stream alebo súbor.
async fn nativne(
    v: &Arc<Vnutro>,
    p: &Polozka,
    z: &CancellationToken,
    (h, sucet): &(Hlasic, Arc<Mutex<Sucet>>),
) -> Vysledok<Koniec> {
    if p.format == "mp3" && v.nastroje.ffmpeg().is_none() {
        return Err(Chyba::Ine("Prevod do MP3 potrebuje FFmpeg – vyber „Zvuk – pôvodná kvalita“ alebo nainštaluj FFmpeg.".into()));
    }
    let n = nastavenia_siete(v, p);
    let k = klient::klient(&n)?;
    let r = riesenie::vyries(&k, &p.url, n.referer.as_deref(), h).await?;
    if !p.zo_skenu && !r.dalsie.is_empty() {
        let vsetky: Vec<_> = r.nalez.iter().chain(r.dalsie.iter()).collect();
        let stranka = vsetky
            .first()
            .map(|x| x.nazov.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Video".into());
        let nove: Vec<Nova> = vsetky
            .iter()
            .enumerate()
            .map(|(i, x)| Nova {
                url: x.url.clone(),
                format: p.format.clone(),
                priecinok: p.priecinok.clone(),
                nazov: format!(
                    "{} ({})",
                    if x.nazov.is_empty() {
                        &stranka
                    } else {
                        &x.nazov
                    },
                    i + 1
                ),
                zdroj: x.poskytovatel.clone(),
                referer: x.referer.clone(),
                heslo: p.heslo.clone(),
                zo_skenu: true,
                jazyky: None,
                prihlasenie: p.prihlasenie,
            })
            .collect();
        let pocet = nove.len();
        v.fronta.lock().unwrap().pridaj(nove);
        v.zapis(format!(
            "✔ Na stránke je {pocet} videí – pridané do frontu."
        ));
        return Ok(Koniec::Rozbalene(pocet));
    }
    match r.ciel {
        Ciel::Stream {
            url,
            referer,
            nazov,
            poskytovatel,
        } => {
            let mut n2 = n.clone();
            if referer.is_some() {
                n2.referer = referer;
            }
            let k2 = klient::klient(&n2)?;
            let mut plan = stream::naplanuj(&k2, &url, &n2).await?;
            if zvukovy(&p.format) {
                let zvuky: Vec<_> = plan
                    .stopy
                    .iter()
                    .filter(|s| s.druh == "zvuk")
                    .cloned()
                    .collect();
                if !zvuky.is_empty() {
                    plan.stopy = zvuky.into_iter().take(1).collect();
                }
            }
            let len_zvuk = plan.stopy.iter().all(|s| s.druh == "zvuk");
            let meno = Some(p.nazov.clone())
                .filter(|t| !t.is_empty())
                .or(nazov.clone())
                .unwrap_or_else(|| {
                    let m = nazov_z_adresy(&url);
                    m.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(m)
                });
            uprav(v, &p.id, |x| {
                if x.nazov.is_empty() {
                    x.nazov = meno.clone();
                }
                if x.zdroj == "Web" {
                    x.zdroj = poskytovatel.clone();
                }
            });
            sucet.lock().unwrap().stop = plan.stopy.len().max(1);
            let pripona = if len_zvuk {
                "m4a"
            } else if p.format == "mp4" {
                "mp4"
            } else {
                "mkv"
            };
            let c = ciel(v, p, &meno, pripona);
            let vysledok = stream::stiahni(&k2, &plan, &c, &meno, n2.spojenia, z, h).await?;
            if p.format == "mp3" {
                return na_mp3(v, &vysledok).await.map(Koniec::Subor);
            }
            Ok(Koniec::Subor(vysledok))
        }
        Ciel::Subor {
            url,
            referer,
            nazov,
            pripona,
            poskytovatel,
        } => {
            let mut n2 = n.clone();
            if referer.is_some() {
                n2.referer = referer;
            }
            let k2 = klient::klient(&n2)?;
            let z_adresy = nazov_z_adresy(&url);
            let meno = match Some(p.nazov.clone()).filter(|t| !t.is_empty()).or(nazov) {
                Some(m) => match &pripona {
                    Some(x) if !m.to_ascii_lowercase().ends_with(&format!(".{x}")) => {
                        format!("{m}.{x}")
                    }
                    _ => m,
                },
                None => z_adresy,
            };
            uprav(v, &p.id, |x| {
                if x.nazov.is_empty() {
                    x.nazov = meno.clone();
                }
                if x.zdroj == "Web" && poskytovatel != "súbor" {
                    x.zdroj = poskytovatel.clone();
                }
            });
            let c = ciel_suboru(v, p, &meno);
            let m = Moznosti {
                spojenia: n2.spojenia,
                ocakavana_velkost: None,
                zrusenie: z.clone(),
                hlasic: h.clone(),
            };
            Ok(Koniec::Subor(
                http::stiahni(&k2, http::pevna_url(url), &c, &m).await?,
            ))
        }
    }
}

async fn na_mp3(v: &Vnutro, zdroj: &Path) -> Vysledok<PathBuf> {
    let ff = v
        .nastroje
        .ffmpeg()
        .ok_or_else(|| Chyba::Ine("Prevod do MP3 potrebuje FFmpeg.".into()))?;
    let ciel = zdroj.with_extension("mp3");
    let mut c = tokio::process::Command::new(ff);
    c.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(zdroj)
        .args(["-vn", "-b:a", "192k"])
        .arg(&ciel);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    let o = c.output().await?;
    if !o.status.success() {
        return Err(Chyba::Ine(format!(
            "Prevod do MP3 zlyhal: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )));
    }
    let _ = tokio::fs::remove_file(zdroj).await;
    Ok(ciel)
}

fn pri_meta(v: &Arc<Vnutro>, id: &str) -> impl Fn(ytdlp::Meta) + Send + Sync {
    let (v, id) = (Arc::downgrade(v), id.to_string());
    move |m: ytdlp::Meta| {
        let Some(v) = v.upgrade() else { return };
        uprav(&v, &id, |x| {
            if x.nazov.is_empty() || x.zo_skenu {
                x.nazov = m.nazov.clone();
            }
            if !m.nahlad.is_empty() && x.nahlad.is_empty() {
                x.nahlad = m.nahlad.clone();
            }
            x.dlzka = m.dlzka.or(x.dlzka);
            if !m.autor.is_empty() {
                x.autor = m.autor.clone();
            }
        });
    }
}

/// yt-dlp len zistí formáty; vyberie, stiahne a spojí ich jadro vlastným kódom (bez FFmpeg).
/// Samotný yt-dlp s FFmpeg sťahuje len MP3 (treba prekódovať) a to, čo jadro nevie spojiť.
async fn cez_ytdlp(
    v: &Arc<Vnutro>,
    p: &Polozka,
    z: &CancellationToken,
    (h, sucet): &(Hlasic, Arc<Mutex<Sucet>>),
    exe: &Path,
) -> Vysledok<Koniec> {
    let ffmpeg = v.nastroje.ffmpeg();
    let (prehliadac, zvuk, spojenia, volby) = {
        let n = v.nastavenia.lock().unwrap();
        (
            n.text("cookies_browser"),
            n.text("audio_mode"),
            n.cislo("connections", 4).clamp(1, 16) as usize,
            super::doplnky::Volby::z_nastaveni(&n),
        )
    };
    let spolocne = Spolocne {
        prehliadac,
        referer: p.referer.clone(),
        heslo: p.heslo.clone(),
    };
    let priecinok = PathBuf::from(&p.priecinok);
    let meta = pri_meta(v, &p.id);
    if p.format == "mp3" && ffmpeg.is_none() {
        return Err(Chyba::Ine("Prevod do MP3 potrebuje FFmpeg – vyber „Zvuk – pôvodná kvalita“ alebo nainštaluj FFmpeg.".into()));
    }
    let plny = std::env::var("KOMETA_YTDLP_PLNY").is_ok_and(|x| x == "1");
    // vlastná cesta: formáty zistí yt-dlp, zvyšok jadro
    let (info, vyber) = if p.format == "mp3" || plny {
        (None, None)
    } else {
        let info = ytdlp::info(exe, &p.url, &spolocne, z).await?;
        let vyber = ytdlp::vyber_formaty(&info, &p.format, &zvuk);
        (Some(info), vyber)
    };
    if vyber.is_none() && (ffmpeg.is_some() || plny) {
        let (v2, id2) = (Arc::downgrade(v), p.id.clone());
        let docasny = move |t: String| {
            if let Some(v) = v2.upgrade() {
                uprav(&v, &id2, |x| {
                    if !x.docasne.contains(&t) {
                        x.docasne.push(t);
                    }
                });
            }
        };
        let u = ytdlp::Uloha {
            exe,
            ffmpeg: ffmpeg.as_deref(),
            url: &p.url,
            priecinok: &priecinok,
            format: &p.format,
            zvuk: &zvuk,
            spojenia,
            spolocne,
            doplnky: super::doplnky::argumenty_ytdlp(volby, &p.format),
        };
        return ytdlp::stiahni(&u, z, h, &meta, &docasny)
            .await
            .map(Koniec::Subor);
    }
    let (Some(info), Some(vyber)) = (info, vyber) else {
        return Err(Chyba::Ine(
            "Pre toto video sa nenašiel formát, ktorý Kometa spojí sama. Vyber iný formát alebo nainštaluj FFmpeg."
                .into(),
        ));
    };
    meta(ytdlp::meta_z(&info));
    let formaty = vyber.formaty;
    let titul = info
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("video")
        .to_string();
    let id_videa = info
        .get("id")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let pripona = vyber.pripona.as_str();
    let meno = if id_videa.is_empty() {
        titul.clone()
    } else {
        format!(
            "{} [{id_videa}]",
            titul.chars().take(120).collect::<String>()
        )
    };
    let c = ciel(v, p, &meno, pripona);
    {
        let mut s = sucet.lock().unwrap();
        s.stop = formaty.len();
    }
    let mut vstupy = vec![];
    for f in &formaty {
        let cast = s_priponou(
            &c,
            &format!(
                ".f{}.{}",
                bezpecny_nazov(&f.id, 40),
                if f.hls { "mp4" } else { &f.pripona }
            ),
        );
        stiahni_format(v, p, f, &cast, z, h).await?;
        vstupy.push(Vstup {
            cesta: cast,
            druh: if f.druh == "video+zvuk" {
                "video".into()
            } else {
                f.druh.into()
            },
            jazyk: f.jazyk.clone(),
            // AAC aj Opus v jednom súbore: v prehrávači ich rozlíši názov
            nazov: (formaty.iter().filter(|x| x.druh == "zvuk").count() > 1 && f.druh == "zvuk")
                .then(|| if f.pripona == "webm" { "Opus" } else { "AAC" }.to_string()),
            povodny: None,
        });
    }
    let mut doplnky = super::doplnky::z_info(&info, volby).await;
    if doplnky.titul.is_empty() {
        doplnky.titul = titul.clone();
    }
    if vstupy.len() == 1 && !formaty[0].hls && formaty[0].pripona == pripona {
        // jeden súbor: údaje, kapitoly a obrázok sa doňho vložia prebalením (ak to formát dovolí)
        let co_vlozit =
            doplnky.ma_udaje() || !doplnky.kapitoly.is_empty() || doplnky.obal.is_some();
        // priamy súbor z bežnej stránky (yt-dlp „Generic“) ostane presne taký, ako je na webe
        let genericky = info
            .get("extractor_key")
            .and_then(|e| e.as_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("generic"));
        let prebalit = co_vlozit && !genericky && matches!(pripona, "mp4" | "m4a");
        if prebalit && spajanie::spoj_s(&vstupy, &c, &doplnky).await.is_ok() {
            let _ = tokio::fs::remove_file(&vstupy[0].cesta).await;
            return Ok(Koniec::Subor(c));
        }
        tokio::fs::rename(&vstupy[0].cesta, &c).await?;
        return Ok(Koniec::Subor(c));
    }
    v.zapis("Spájam stopy…");
    let vysledok = spajanie::spoj_s(&vstupy, &c, &doplnky).await?;
    for x in &vstupy {
        let _ = tokio::fs::remove_file(&x.cesta).await;
    }
    Ok(Koniec::Subor(vysledok))
}

async fn stiahni_format(
    v: &Vnutro,
    p: &Polozka,
    f: &FormatNa,
    cast: &Path,
    z: &CancellationToken,
    h: &Hlasic,
) -> Vysledok<()> {
    if cast.exists() {
        return Ok(()); // stiahnutý pred pauzou
    }
    let id = f.id.clone();
    let h2 = h.clone();
    let hf: Hlasic = Arc::new(move |u| match u {
        Udalost::Priebeh {
            stiahnute,
            celkom,
            rychlost,
            ..
        } => h2(Udalost::Priebeh {
            stiahnute,
            celkom,
            rychlost,
            cast: Some(id.clone()),
        }),
        u => h2(u),
    });
    let mut n = nastavenia_siete(v, p);
    n.hlavicky = f.hlavicky.clone();
    n.referer = None;
    let k = klient::klient(&n)?;
    if f.hls {
        let plan = stream::naplanuj(&k, &f.url, &n).await?;
        stream::stiahni(&k, &plan, cast, &f.id, n.spojenia, z, &hf).await?;
    } else {
        let m = Moznosti {
            spojenia: n.spojenia,
            ocakavana_velkost: f.velkost,
            zrusenie: z.clone(),
            hlasic: hf,
        };
        http::stiahni(&k, http::pevna_url(f.url.clone()), cast, &m).await?;
    }
    Ok(())
}

/// Zmaže rozpracované súbory položky po tom, čo jej úloha skončí.
pub async fn uprac(p: Polozka, uloha: Option<tokio::task::JoinHandle<()>>) {
    if let Some(u) = uloha {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), u).await;
    }
    if p.stav != Stav::Hotovo {
        let _ = tokio::task::spawn_blocking(move || fronta::zmaz_rozpracovane(&p)).await;
    }
}

/// Po dokončení: ak je výsledok archív a rozbaľovanie je zapnuté (alebo `rucne`), rozbalí ho vedľa
/// neho. Pri rozdelenom archíve sa čaká, kým sú hotové všetky jeho časti vo fronte; rozbaľuje
/// položka s najnižším číslom časti. `heslo` = nové heslo zadané pri položke.
/// Heslá zadané pri iných položkách frontu (najnovšie prvé, bez opakovania), ktoré sa skúsia,
/// keď archív zadané heslo neprijme. Nanajvýš osem – rozbalenie s cudzím heslom sa rýchlo vzdá.
fn zname_hesla(f: &super::fronta::Fronta, okrem: &str) -> Vec<String> {
    let mut polozky: Vec<&super::fronta::Polozka> =
        f.polozky.iter().filter(|p| !p.heslo.is_empty()).collect();
    polozky.sort_by(|a, b| b.pridane.total_cmp(&a.pridane));
    let mut out: Vec<String> = vec![];
    for p in polozky {
        if p.heslo != okrem && !out.contains(&p.heslo) {
            out.push(p.heslo.clone());
        }
        if out.len() == 8 {
            break;
        }
    }
    out
}

pub fn skus_rozbalit(v: &Arc<Vnutro>, id: &str, heslo: Option<String>, rucne: bool) {
    use super::archivy;
    use super::fronta::Rozbalenie;
    let nastav = |v: &Vnutro, id: &str, stav: &str, text: String, ciel: String| {
        v.fronta.lock().unwrap().uprav(id, |x| {
            x.rozbalenie = Some(Rozbalenie {
                stav: stav.into(),
                text,
                ciel,
                podiel: None,
            })
        });
    };
    let (vlastnik, archiv, heslo, zname) = {
        let mut f = v.fronta.lock().unwrap();
        let Some(p) = f.najdi(id).cloned() else {
            return;
        };
        let cesta = PathBuf::from(&p.subor);
        let Some(c) = archivy::rozpoznaj(&cesta) else {
            return;
        };
        if !rucne && heslo.is_none() && !v.nastavenia.lock().unwrap().ano("rozbalovat", true) {
            f.uprav(id, |x| {
                x.rozbalenie = Some(Rozbalenie {
                    stav: "vypnute".into(),
                    ..Default::default()
                })
            });
            return;
        }
        // ostatné časti toho istého archívu vo fronte (rovnaký priečinok, meno a druh)
        let dir = cesta.parent().map(Path::to_path_buf).unwrap_or_default();
        let skupina: Vec<(u32, String, Stav, String)> = f
            .polozky
            .iter()
            .filter_map(|x| {
                let c2 = PathBuf::from(if x.subor.is_empty() {
                    &x.ciel
                } else {
                    &x.subor
                });
                let r = archivy::rozpoznaj(&c2)?;
                (r.druh == c.druh && r.meno == c.meno && c2.parent() == Some(dir.as_path()))
                    .then(|| (r.cislo, x.id.clone(), x.stav, x.heslo.clone()))
            })
            .collect();
        if skupina.iter().any(|x| x.2 != Stav::Hotovo) {
            let chyba = skupina.iter().filter(|x| x.2 != Stav::Hotovo).count();
            drop(f);
            nastav(
                v,
                id,
                "caka",
                format!("Rozbalí sa, keď budú stiahnuté všetky časti (chýba {chyba})."),
                String::new(),
            );
            return;
        }
        let vlastnik = skupina
            .iter()
            .min_by_key(|x| x.0)
            .map(|x| x.1.clone())
            .unwrap_or_else(|| id.to_string());
        let vp = f.najdi(&vlastnik).cloned().unwrap_or(p);
        if vp.rozbalenie.as_ref().is_some_and(|r| r.stav == "bezi") {
            return;
        }
        let heslo = heslo
            .or_else(|| skupina.iter().map(|x| x.3.clone()).find(|h| !h.is_empty()))
            .unwrap_or_default();
        for (_, i, _, _) in skupina.iter().filter(|x| x.1 != vlastnik) {
            f.uprav(i, |x| {
                x.rozbalenie = Some(Rozbalenie {
                    stav: "cast".into(),
                    text: format!("Časť archívu {} – rozbalí sa spolu s prvou časťou.", c.meno),
                    ..Default::default()
                })
            });
        }
        f.uprav(&vlastnik, |x| {
            x.heslo = heslo.clone();
            x.rozbalenie = Some(Rozbalenie {
                stav: "bezi".into(),
                text: "Rozbaľujem…".into(),
                ciel: String::new(),
                podiel: Some(0.0),
            })
        });
        (
            vlastnik,
            PathBuf::from(vp.subor),
            heslo.clone(),
            zname_hesla(&f, &heslo),
        )
    };
    let v2 = v.clone();
    v.rt_spawn(async move {
        let v = v2;
        let (v3, vl) = (v.clone(), vlastnik.clone());
        let a = archiv.clone();
        let r = tokio::task::spawn_blocking(move || {
            let posledne = std::cell::Cell::new(Instant::now());
            let priebeh = |hotovo: u64, celkom: u64| {
                if celkom > 0 && posledne.get().elapsed().as_millis() >= 200 {
                    posledne.set(Instant::now());
                    v3.fronta.lock().unwrap().uprav(&vl, |x| {
                        if let Some(r) = x.rozbalenie.as_mut() {
                            r.podiel = Some((hotovo as f64 / celkom as f64).min(1.0));
                        }
                    });
                }
            };
            let mut r = archivy::rozbal(&a, &heslo, &priebeh);
            // archív chce iné heslo: skúsiť tie, ktoré si zadal pri iných súboroch (často je
            // jedno heslo pre celú skupinu) – až potom sa opýtať
            let mut sedi = None;
            if matches!(r, Err(archivy::ChybaRozbalenia::Heslo(_))) {
                for h in &zname {
                    match archivy::rozbal(&a, h, &priebeh) {
                        Err(archivy::ChybaRozbalenia::Heslo(_)) => continue,
                        x => {
                            if x.is_ok() {
                                sedi = Some(h.clone());
                            }
                            r = x;
                            break;
                        }
                    }
                }
            }
            (r, sedi)
        })
        .await;
        let (r, sedi) = match r {
            Ok((r, sedi)) => (Ok(r), sedi),
            Err(e) => (Err(e), None),
        };
        if let Some(h) = &sedi {
            v.fronta
                .lock()
                .unwrap()
                .uprav(&vlastnik, |x| x.heslo = h.clone());
        }
        let meno = archiv
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match r {
            Ok(Ok((ciel, n))) => {
                let mut text = format!(
                    "Rozbalené: {n} {}",
                    match n {
                        1 => "súbor",
                        2..=4 => "súbory",
                        _ => "súborov",
                    }
                );
                if sedi.is_some() {
                    text.push_str(" – sedelo heslo z iného súboru");
                }
                if v.nastavenia.lock().unwrap().text("po_rozbaleni") == "kos" {
                    let casti = archivy::casti(&archiv);
                    let zmazane = tokio::task::spawn_blocking(move || {
                        casti.iter().filter(|c| trash::delete(c).is_ok()).count()
                    })
                    .await
                    .unwrap_or(0);
                    if zmazane > 0 {
                        text.push_str(" – archív je v Koši");
                    }
                }
                v.zapis(format!("📦 {meno}: {text}"));
                nastav(
                    &v,
                    &vlastnik,
                    "hotovo",
                    text,
                    ciel.to_string_lossy().into_owned(),
                );
            }
            Ok(Err(super::archivy::ChybaRozbalenia::Heslo(zle))) => {
                let t = super::archivy::ChybaRozbalenia::Heslo(zle).to_string();
                v.zapis(format!("📦 {meno}: {t}"));
                nastav(&v, &vlastnik, "heslo", t, String::new());
            }
            // rozdelený archív, ktorému chýba časť: počká na ďalšie časti vo fronte
            Ok(Err(_)) if archivy::je_delene(&archiv) => nastav(
                &v,
                &vlastnik,
                "caka",
                "Rozbalí sa, keď budú stiahnuté všetky časti archívu.".into(),
                String::new(),
            ),
            Ok(Err(e)) => {
                v.zapis(format!("📦 {meno}: {e}"));
                nastav(&v, &vlastnik, "chyba", e.to_string(), String::new());
            }
            Err(e) => nastav(
                &v,
                &vlastnik,
                "chyba",
                format!("Rozbaľovanie zlyhalo: {e}"),
                String::new(),
            ),
        }
        v.fronta.lock().unwrap().uloz(true);
    });
}

/// Stiahnuté súbory položiek (všetkých, ak `ids` je None), ktoré ešte sú na disku, a ich veľkosť.
pub fn stiahnute_subory(v: &Vnutro, ids: Option<&[String]>) -> (Vec<PathBuf>, u64) {
    let f = v.fronta.lock().unwrap();
    let mut out: Vec<PathBuf> = vec![];
    let mut spolu = 0;
    for p in &f.polozky {
        if ids.is_some_and(|ids| !ids.contains(&p.id))
            || p.stav != Stav::Hotovo
            || p.subor.is_empty()
        {
            continue;
        }
        let c = PathBuf::from(&p.subor);
        if let Ok(m) = std::fs::metadata(&c) {
            if m.is_file() && !out.contains(&c) {
                spolu += m.len();
                out.push(c);
            }
        }
    }
    (out, spolu)
}

/// Presunie stiahnuté súbory do Koša a položky odstráni z frontu (pri `ids` = None celý front).
/// Prázdne podpriečinky (kanál, playlist) v priečinku sťahovania sa zmažú. Iné súbory ostanú.
pub async fn zmaz_stiahnute(v: &Arc<Vnutro>, ids: Option<&[String]>) -> Value {
    let (subory, _) = stiahnute_subory(v, ids);
    let koren = PathBuf::from(v.priecinok_vystupu());
    let (zmazane, chyby) = tokio::task::spawn_blocking(move || {
        let mut zmazane = vec![];
        let mut chyby = vec![];
        for c in subory {
            match trash::delete(&c) {
                Ok(()) => zmazane.push(c),
                Err(e) => chyby.push(format!(
                    "{}: {e}",
                    c.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }
        // prázdne priečinky kanálov a playlistov (len vnútri priečinka sťahovania)
        for c in &zmazane {
            let mut d = c.parent();
            while let Some(p) = d {
                if p == koren || !p.starts_with(&koren) || std::fs::remove_dir(p).is_err() {
                    break;
                }
                d = p.parent();
            }
        }
        (zmazane, chyby)
    })
    .await
    .unwrap_or_default();
    let pouzite: Vec<String> = {
        let f = v.fronta.lock().unwrap();
        f.polozky
            .iter()
            .filter(|p| ids.map_or(true, |ids| ids.contains(&p.id)))
            // položku, ktorej súbor sa nepodarilo presunúť, nechať – nech sa dá skúsiť znova
            .filter(|p| {
                p.stav != Stav::Hotovo
                    || p.subor.is_empty()
                    || zmazane.iter().any(|z| z == Path::new(&p.subor))
                    || !Path::new(&p.subor).exists()
            })
            .map(|p| p.id.clone())
            .collect()
    };
    let odstranene = v.fronta.lock().unwrap().odstran(&pouzite);
    for (p, uloha) in odstranene {
        v.rt.spawn(uprac(p, uloha));
    }
    v.naplanuj();
    if !zmazane.is_empty() {
        v.zapis(format!("🗑 Do Koša: {} súborov", zmazane.len()));
    }
    for c in &chyby {
        v.zapis(format!("Nepodarilo sa presunúť do Koša – {c}"));
    }
    json!({"zmazane": zmazane.len(), "chyby": chyby})
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn co_ide_do_ytdlp() {
        assert!(len_ytdlp("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(len_ytdlp("https://www.tiktok.com/@a/video/1"));
        assert!(!len_ytdlp("https://player.vimeo.com/video/1"));
        assert!(!len_ytdlp("https://example.sk/clanok"));
        assert_eq!(
            nazov_z_adresy("https://a.sk/videa/M%C3%B4j%20film.mp4?x=1"),
            "Môj film.mp4"
        );
        assert_eq!(nazov_z_adresy("https://a.sk/"), "a.sk");
    }
}
