//! Jazykové stopy z YouTube: video, vybrané jazyky v zvolenom poradí (CZ → SK → ďalšie)
//! a originál v jednom súbore. yt-dlp len zistí formáty; sťahuje jadro po úsekoch
//! a spája vlastné spájanie – aj zvuk Opus z WebM, bez FFmpeg.
//!
//! Rovnaké voľby a tvary dát ako `kometa/jazyky.py` v Pythone.

use super::ytdlp::{self, FormatNa, Spolocne};
use super::zdroje::bezpecny_nazov;
use super::Vnutro;
use crate::chyba::{Chyba, Vysledok};
use crate::http::{self, Moznosti};
use crate::klient::{self, Nastavenia};
use crate::spajanie::{self, Vstup};
use crate::stream;
use crate::udalosti::{Hlasic, Udalost};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// Slovenské názvy jazykov podľa kódu (ako v Pythone).
const NAZVY: &[(&str, &str)] = &[
    ("en", "Angličtina"),
    ("en-us", "Angličtina (USA)"),
    ("en-gb", "Angličtina (Británia)"),
    ("en-in", "Angličtina (India)"),
    ("en-au", "Angličtina (Austrália)"),
    ("en-ca", "Angličtina (Kanada)"),
    ("cs", "Čeština"),
    ("sk", "Slovenčina"),
    ("pl", "Poľština"),
    ("hu", "Maďarčina"),
    ("ru", "Ruština"),
    ("uk", "Ukrajinčina"),
    ("be", "Bieloruština"),
    ("de", "Nemčina"),
    ("fr", "Francúzština"),
    ("fr-ca", "Francúzština (Kanada)"),
    ("es", "Španielčina"),
    ("es-419", "Španielčina (Latinská Amerika)"),
    ("es-us", "Španielčina (USA)"),
    ("es-es", "Španielčina (Španielsko)"),
    ("es-mx", "Španielčina (Mexiko)"),
    ("it", "Taliančina"),
    ("pt", "Portugalčina"),
    ("pt-br", "Portugalčina (Brazília)"),
    ("pt-pt", "Portugalčina (Portugalsko)"),
    ("nl", "Holandčina"),
    ("ro", "Rumunčina"),
    ("bg", "Bulharčina"),
    ("hr", "Chorvátčina"),
    ("sr", "Srbčina"),
    ("sl", "Slovinčina"),
    ("el", "Gréčtina"),
    ("tr", "Turečtina"),
    ("ar", "Arabčina"),
    ("he", "Hebrejčina"),
    ("fa", "Perzština"),
    ("hi", "Hindčina"),
    ("bn", "Bengálčina"),
    ("ur", "Urdčina"),
    ("ta", "Tamilčina"),
    ("te", "Telugčina"),
    ("ml", "Malajálamčina"),
    ("mr", "Maráthčina"),
    ("id", "Indonézština"),
    ("ms", "Malajčina"),
    ("fil", "Filipínčina"),
    ("th", "Thajčina"),
    ("vi", "Vietnamčina"),
    ("ja", "Japončina"),
    ("ko", "Kórejčina"),
    ("zh", "Čínština"),
    ("zh-hans", "Čínština (zjednodušená)"),
    ("zh-hant", "Čínština (tradičná)"),
    ("zh-cn", "Čínština (Čína)"),
    ("zh-tw", "Čínština (Taiwan)"),
    ("sv", "Švédčina"),
    ("da", "Dánčina"),
    ("no", "Nórčina"),
    ("nb", "Nórčina"),
    ("fi", "Fínčina"),
    ("lt", "Litovčina"),
    ("lv", "Lotyština"),
    ("et", "Estónčina"),
    ("kk", "Kazaština"),
];

fn nazov_kodu(k: &str) -> Option<&'static str> {
    NAZVY.iter().find(|(x, _)| *x == k).map(|(_, n)| *n)
}

/// Pekný slovenský názov jazyka (napr. „en-US“ → „Angličtina (USA)“).
pub fn nazov_jazyka(kod: &str, zo_zdroja: &str) -> String {
    if kod.is_empty() {
        return if zo_zdroja.is_empty() {
            "Pôvodná stopa".into()
        } else {
            zo_zdroja.into()
        };
    }
    let mut c = kod.to_lowercase();
    let mut pripona = "";
    if let Some(z) = c.strip_suffix("-desc") {
        c = z.to_string();
        pripona = " – zvukový popis";
    }
    let meno = nazov_kodu(&c).map(str::to_string).or_else(|| {
        let (zaklad, region) = c.split_once('-')?;
        Some(format!(
            "{} ({})",
            nazov_kodu(zaklad)?,
            region.to_uppercase()
        ))
    });
    meno.unwrap_or_else(|| {
        if zo_zdroja.is_empty() {
            kod.to_string()
        } else {
            zo_zdroja.to_string()
        }
    }) + pripona
}

/// Jedna jazyková stopa videa.
#[derive(Clone, Debug)]
pub struct Stopa {
    pub kod: String,
    pub nazov: String,
    pub original: bool,
    pub najlepsi: Value,
    pub aac: Option<Value>,
    pub opus: Option<Value>,
}

fn s<'a>(f: &'a Value, k: &str) -> &'a str {
    f.get(k).and_then(Value::as_str).unwrap_or("")
}

fn n(f: &Value, k: &str) -> f64 {
    f.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn len_zvuk(f: &Value) -> bool {
    s(f, "vcodec") == "none" && !matches!(s(f, "acodec"), "" | "none")
}

/// Poradie kvality: bez kompresie dynamiky (DRC), priamy súbor, vyšší dátový tok, Opus.
fn poradie(f: &Value) -> (bool, bool, i64, bool) {
    let drc =
        s(f, "format_note").to_lowercase().contains("drc") || s(f, "format_id").contains("drc");
    (
        !drc,
        matches!(
            f.get("protocol").and_then(Value::as_str).unwrap_or("https"),
            "https" | "http"
        ),
        (n(f, "abr").max(n(f, "tbr")) * 1000.0) as i64,
        s(f, "acodec").starts_with("opus"),
    )
}

fn je_aac(f: &Value) -> bool {
    s(f, "acodec").starts_with("mp4a") || s(f, "ext") == "m4a"
}

fn cisty_nazov(poznamka: &str) -> String {
    let prvy = poznamka.split(',').next().unwrap_or("");
    let mut t = prvy.replace("(default)", "").replace("(original)", "");
    for x in ["original", "Original"] {
        t = t.replace(x, "");
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn bez_diakritiky(t: &str) -> String {
    t.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'ä' => 'a',
            'č' => 'c',
            'ď' => 'd',
            'é' | 'ě' => 'e',
            'í' => 'i',
            'ĺ' | 'ľ' => 'l',
            'ň' => 'n',
            'ó' | 'ô' => 'o',
            'ŕ' | 'ř' => 'r',
            'š' => 's',
            'ť' => 't',
            'ú' | 'ů' => 'u',
            'ý' => 'y',
            'ž' => 'z',
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// Jazykové stopy z údajov yt-dlp: originál prvý, ostatné abecedne (bez ohľadu na diakritiku).
pub fn stopy(info: &Value) -> Vec<Stopa> {
    let mut skupiny: Vec<(String, Vec<&Value>)> = vec![];
    for f in info
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !len_zvuk(f) {
            continue;
        }
        let k = s(f, "language").to_string();
        match skupiny.iter_mut().find(|(x, _)| *x == k) {
            Some((_, v)) => v.push(f),
            None => skupiny.push((k, vec![f])),
        }
    }
    let najlepsi = |v: &[&Value]| v.iter().copied().max_by_key(|f| poradie(f)).cloned();
    let mut out: Vec<(Stopa, f64)> = skupiny
        .iter()
        .map(|(kod, f)| {
            let naj = najlepsi(f).unwrap_or(Value::Null);
            let aac: Vec<&Value> = f.iter().copied().filter(|x| je_aac(x)).collect();
            let opus: Vec<&Value> = f
                .iter()
                .copied()
                .filter(|x| s(x, "acodec").starts_with("opus"))
                .collect();
            let original = f.iter().any(|x| {
                x.get("language_preference").and_then(Value::as_i64) == Some(10)
                    || s(x, "format_note").to_lowercase().contains("original")
            });
            let pref = f
                .iter()
                .map(|x| {
                    x.get("language_preference")
                        .and_then(Value::as_f64)
                        .unwrap_or(-1.0)
                })
                .fold(-1.0, f64::max);
            (
                Stopa {
                    kod: kod.clone(),
                    nazov: nazov_jazyka(kod, &cisty_nazov(s(&naj, "format_note"))),
                    original,
                    aac: najlepsi(&aac),
                    opus: najlepsi(&opus),
                    najlepsi: naj,
                },
                pref,
            )
        })
        .collect();
    if !out.is_empty() && !out.iter().any(|(s, _)| s.original) {
        let i = (0..out.len())
            .max_by(|&a, &b| {
                out[a]
                    .1
                    .partial_cmp(&out[b].1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        if out.len() == 1 || out[i].1 > 0.0 {
            out[i].0.original = true;
        }
    }
    let mut v: Vec<Stopa> = out.into_iter().map(|(s, _)| s).collect();
    v.sort_by_key(|s| (!s.original, bez_diakritiky(&s.nazov)));
    v
}

/// Stopy podľa želaných kódov v zadanom poradí („cs“ nájde aj „cs-CZ“). Vráti (vybrané, nenájdené).
pub fn vyber(stopy: &[Stopa], chcene: &[String]) -> (Vec<Stopa>, Vec<String>) {
    let chcene: Vec<String> = chcene
        .iter()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    let (mut vybrane, mut chyba): (Vec<Stopa>, Vec<String>) = (vec![], vec![]);
    for w in &chcene {
        let presna = stopy.iter().find(|s| s.kod.to_lowercase() == *w);
        let t = presna.or_else(|| {
            let zaklad = w.split('-').next().unwrap_or("");
            stopy.iter().find(|s| {
                let k = s.kod.to_lowercase();
                k.split('-').next() == Some(zaklad) && !chcene.contains(&k) && !k.ends_with("-desc")
            })
        });
        match t {
            None => chyba.push(w.clone()),
            Some(t) if !vybrane.iter().any(|x| x.kod == t.kod) => vybrane.push(t.clone()),
            _ => {}
        }
    }
    (vybrane, chyba)
}

/// Najlepší samotný obraz (do zvolenej výšky; pre MP4 prednostne H.264).
pub fn obraz(info: &Value, vyska: Option<u32>, h264: bool) -> Option<Value> {
    let formaty: Vec<&Value> = info
        .get("formats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| s(f, "acodec") == "none" && !matches!(s(f, "vcodec"), "" | "none"))
        .collect();
    let max = vyska.map(|v| v as f64).unwrap_or(f64::INFINITY);
    let kluc = |f: &&Value| {
        (
            (n(f, "height") * 1000.0) as i64,
            (n(f, "tbr") * 1000.0) as i64,
        )
    };
    let s_kodekom = |k: &str| {
        formaty
            .iter()
            .copied()
            .filter(|f| s(f, "vcodec").starts_with(k))
            .collect::<Vec<_>>()
    };
    let mut poradie: Vec<Vec<&Value>> = vec![];
    if h264 {
        poradie.push(
            s_kodekom("avc1")
                .into_iter()
                .filter(|f| n(f, "height") <= max)
                .collect(),
        );
        poradie.push(s_kodekom("avc1"));
    }
    poradie.push(
        formaty
            .iter()
            .copied()
            .filter(|f| n(f, "height") <= max)
            .collect(),
    );
    poradie.push(formaty.clone());
    poradie
        .into_iter()
        .find_map(|v| v.into_iter().max_by_key(kluc).cloned())
}

// ---------------------------------------------------------------------------------------------
// Stav pre okno a úloha
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Jazyky {
    stav: Mutex<StavJazykov>,
    nacitane: Mutex<Option<(Value, Vec<Stopa>)>>,
    zrusenie: Mutex<CancellationToken>,
}

#[derive(Default, Clone)]
struct StavJazykov {
    busy: bool,
    frac: f64,
    text: String,
    log: Vec<String>,
    file: String,
    error: String,
    rev: u64,
}

impl Jazyky {
    pub fn stav(&self) -> Value {
        let s = self.stav.lock().unwrap();
        let od = s.log.len().saturating_sub(80);
        json!({"busy": s.busy, "frac": s.frac, "text": s.text, "log": &s.log[od..], "file": s.file,
               "error": s.error, "rev": s.rev})
    }

    fn uprav(&self, f: impl FnOnce(&mut StavJazykov)) {
        f(&mut self.stav.lock().unwrap());
    }

    fn zapis(&self, t: impl Into<String>) {
        self.uprav(|s| s.log.push(t.into()));
    }

    pub fn zrus(&self) {
        self.zrusenie.lock().unwrap().cancel();
        self.uprav(|s| s.text = "Ruším…".into());
    }
}

fn je_youtube(url: &str) -> bool {
    matches!(super::zdroje::nazov_zdroja(url), "YouTube")
}

fn spolocne(v: &Vnutro) -> Spolocne {
    Spolocne {
        prehliadac: v.nastavenia.lock().unwrap().text("cookies_browser"),
        ..Default::default()
    }
}

pub async fn nacitaj(v: &Arc<Vnutro>, url: &str) -> Vysledok<Value> {
    let url = url.trim();
    if !je_youtube(url) {
        return Err(Chyba::Ine(
            "Vlož odkaz na video z YouTube, napríklad https://www.youtube.com/watch?v=…".into(),
        ));
    }
    let exe = v.nastroje.ytdlp().ok_or_else(|| {
        Chyba::Ine(
            "Jazyky videa zistí pomocník yt-dlp – v Nastaveniach klikni na „Doinštalovať yt-dlp“."
                .into(),
        )
    })?;
    let info = ytdlp::info(&exe, url, &spolocne(v), &CancellationToken::new()).await?;
    if info.get("formats").is_none() {
        return Err(Chyba::Ine(
            "Zadaj odkaz na jedno konkrétne video (nie na playlist alebo kanál).".into(),
        ));
    }
    let st = stopy(&info);
    let (vybrane, _) = vyber(&st, &v.nastavenia.lock().unwrap().jazyky());
    let orig = st.iter().find(|s| s.original);
    let autor = [s(&info, "uploader"), s(&info, "channel")]
        .into_iter()
        .find(|x| !x.is_empty())
        .unwrap_or("")
        .to_string();
    let nahlad = Some(super::zdroje::nahlad_youtube(url))
        .filter(|x| !x.is_empty())
        .unwrap_or_else(|| s(&info, "thumbnail").to_string());
    let out = json!({
        "nazov": info.get("title").or(info.get("id")),
        "autor": autor,
        "dlzka": info.get("duration"),
        "nahlad": nahlad,
        "original": orig.map(|o| o.nazov.clone()).unwrap_or_default(),
        "stopy": st.iter().map(|t| json!({"kod": t.kod, "nazov": t.nazov, "original": t.original,
            "aac": t.aac.is_some(), "kbps": n(&t.najlepsi, "abr").round() as i64})).collect::<Vec<_>>(),
        "vybrane": vybrane.iter().filter(|t| !t.original).map(|t| t.kod.clone()).collect::<Vec<_>>(),
    });
    *v.jazyky.nacitane.lock().unwrap() = Some((info, st));
    Ok(out)
}

/// Voľby z okna (rovnaké kľúče ako v Pythone).
struct Uloha {
    info: Value,
    stopy: Vec<Stopa>,
    original: Option<Stopa>,
    rezim: String,
    existujuce: Option<PathBuf>,
    pridat_original: bool,
    samostatne: bool,
    vyska: Option<u32>,
    zvuk: String,
    mp4: bool,
    priecinok: PathBuf,
}

pub fn stiahni(v: &Arc<Vnutro>, volby: &Map<String, Value>) -> Vysledok<()> {
    if v.jazyky.stav.lock().unwrap().busy {
        return Err(Chyba::Ine("Sťahovanie jazykov už beží.".into()));
    }
    let (info, vsetky) = v
        .jazyky
        .nacitane
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| Chyba::Ine("Najprv načítaj jazyky videa.".into()))?;
    let t = |k: &str| {
        volby
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let b = |k: &str| volby.get(k).and_then(Value::as_bool).unwrap_or(false);
    let poradie: Vec<String> = volby
        .get("poradie")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect();
    let stopy: Vec<Stopa> = poradie
        .iter()
        .filter_map(|k| vsetky.iter().find(|s| s.kod == *k).cloned())
        .collect();
    let rezim = Some(t("rezim"))
        .filter(|r| matches!(r.as_str(), "video" | "existing" | "audio"))
        .unwrap_or_else(|| "video".into());
    if stopy.is_empty() && !b("original") {
        return Err(Chyba::Ine("Vyber aspoň jeden jazyk.".into()));
    }
    let existujuce = Some(PathBuf::from(t("subor").trim()))
        .filter(|p| rezim == "existing" && !p.as_os_str().is_empty());
    if rezim == "existing" && !existujuce.as_ref().is_some_and(|p| p.is_file()) {
        return Err(Chyba::Ine("Vyber svoje stiahnuté video.".into()));
    }
    let kvalita = Some(t("kvalita"))
        .filter(|k| !k.is_empty())
        .unwrap_or_else(|| "Najlepšia".into());
    let zvuk = Some(t("zvuk"))
        .filter(|z| matches!(z.as_str(), "oboje" | "aac" | "opus"))
        .unwrap_or_else(|| "oboje".into());
    let mp4 = t("kontajner") == "mp4";
    {
        let mut n = v.nastavenia.lock().unwrap();
        let jazyky: Vec<String> = stopy
            .iter()
            .filter(|s| !s.kod.is_empty() && !s.original)
            .map(|s| s.kod.clone())
            .collect();
        if !jazyky.is_empty() {
            n.nastav("langs", json!(jazyky));
        }
        let mut z = Map::new();
        z.insert("mode".into(), json!(rezim));
        z.insert("include_original".into(), json!(b("original")));
        z.insert("keep_separate".into(), json!(b("samostatne")));
        z.insert("max_height".into(), json!(kvalita));
        z.insert("audio_mode".into(), json!(zvuk));
        z.insert("kontajner".into(), json!(if mp4 { "mp4" } else { "mkv" }));
        n.zmen(&z);
    }
    let uloha = Uloha {
        original: vsetky.iter().find(|s| s.original).cloned(),
        info,
        stopy,
        rezim,
        existujuce,
        pridat_original: b("original"),
        samostatne: b("samostatne"),
        vyska: kvalita.strip_suffix('p').and_then(|x| x.parse().ok()),
        zvuk: if mp4 { "aac".into() } else { zvuk }, // MP4 = najširšia podpora: H.264 a AAC
        mp4,
        priecinok: PathBuf::from(v.nastavenia.lock().unwrap().text("outdir")),
    };
    let z = CancellationToken::new();
    *v.jazyky.zrusenie.lock().unwrap() = z.clone();
    v.jazyky.uprav(|s| {
        *s = StavJazykov {
            busy: true,
            text: "Pripravujem…".into(),
            rev: s.rev + 1,
            ..Default::default()
        };
    });
    let v2 = v.clone();
    v.rt_spawn(async move {
        let r = prace(&v2, uloha, &z).await;
        v2.jazyky.uprav(|s| {
            s.busy = false;
            match &r {
                Ok(p) => {
                    s.frac = 1.0;
                    s.text = "Hotovo".into();
                    s.file = p.to_string_lossy().into_owned();
                    s.log.push(format!("✔ Uložené: {}", p.display()));
                }
                Err(_) if z.is_cancelled() => s.text = "Zrušené".into(),
                Err(e) => {
                    s.text = "Chyba".into();
                    s.error = e.to_string().lines().next().unwrap_or("").to_string();
                }
            }
            s.rev += 1;
        });
    });
    Ok(())
}

/// Priebeh celej úlohy: kroky (sťahovania + spojenie) a podiel v aktuálnom kroku.
struct Priebeh {
    krokov: f64,
    krok: f64,
    text: String,
}

fn hlasic(v: &Arc<Vnutro>, p: &Arc<Mutex<Priebeh>>) -> Hlasic {
    let (v, p) = (Arc::downgrade(v), p.clone());
    Arc::new(move |u| {
        let Some(v) = v.upgrade() else { return };
        if let Udalost::Priebeh {
            stiahnute, celkom, ..
        } = u
        {
            let p = p.lock().unwrap();
            let cast = celkom
                .map(|c| stiahnute as f64 / c.max(1) as f64)
                .unwrap_or(0.0)
                .min(1.0);
            let frac = ((p.krok + cast) / p.krokov).min(1.0);
            let text = p.text.clone();
            v.jazyky.uprav(|s| {
                s.frac = frac;
                s.text = text;
            });
        }
    })
}

fn krok(v: &Vnutro, p: &Arc<Mutex<Priebeh>>, text: String) {
    let (frac, t) = {
        let mut p = p.lock().unwrap();
        p.text = text;
        (p.krok / p.krokov, p.text.clone())
    };
    v.jazyky.uprav(|s| {
        s.frac = frac;
        s.text = t;
    });
}

fn dalsi(p: &Arc<Mutex<Priebeh>>) {
    p.lock().unwrap().krok += 1.0;
}

async fn stiahni_format(
    v: &Vnutro,
    f: &FormatNa,
    ciel: &Path,
    z: &CancellationToken,
    h: &Hlasic,
) -> Vysledok<PathBuf> {
    let n = Nastavenia {
        referer: None,
        hlavicky: f.hlavicky.clone(),
        spojenia: v
            .nastavenia
            .lock()
            .unwrap()
            .cislo("connections", 4)
            .clamp(1, 16) as usize,
        max_vyska: None,
        jazyky: vec![],
    };
    let k = klient::klient(&n)?;
    if f.hls {
        let plan = stream::naplanuj(&k, &f.url, &n).await?;
        let c = ciel.with_extension("mp4");
        return stream::stiahni(&k, &plan, &c, &f.id, n.spojenia, z, h).await;
    }
    let c = ciel.with_extension(if f.pripona.is_empty() {
        "bin"
    } else {
        &f.pripona
    });
    let m = Moznosti {
        spojenia: n.spojenia,
        ocakavana_velkost: f.velkost,
        zrusenie: z.clone(),
        hlasic: h.clone(),
    };
    http::stiahni(&k, http::pevna_url(f.url.clone()), &c, &m).await
}

/// Formát z údajov yt-dlp → čo stiahne jadro.
fn na(f: &Value, druh: &'static str) -> Vysledok<FormatNa> {
    ytdlp::format_na(f, druh).ok_or_else(|| {
        Chyba::Nepodporovane(format!(
            "formát {} sa nedá stiahnuť priamo",
            s(f, "format_id")
        ))
    })
}

fn jedinecna(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let kmen = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pr = p
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (2..)
        .map(|i| p.with_file_name(format!("{kmen} ({i}).{pr}")))
        .find(|c| !c.exists())
        .unwrap_or(p)
}

async fn na_aac(v: &Vnutro, zdroj: &Path, ciel: &Path) -> Option<PathBuf> {
    let ff = v.nastroje.ffmpeg()?;
    let mut c = tokio::process::Command::new(ff);
    c.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(zdroj)
        .args(["-vn", "-map", "0:a:0", "-c:a", "aac", "-b:a", "192k"])
        .arg(ciel);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    let o = c.output().await.ok()?;
    (o.status.success() && ciel.exists()).then(|| ciel.to_path_buf())
}

async fn prace(v: &Arc<Vnutro>, u: Uloha, z: &CancellationToken) -> Vysledok<PathBuf> {
    tokio::fs::create_dir_all(&u.priecinok).await?;
    let titul = [s(&u.info, "title"), s(&u.info, "id")]
        .into_iter()
        .find(|x| !x.is_empty())
        .unwrap_or("video")
        .to_string();
    let mut stopy = u.stopy.clone();
    // originál (napr. angličtina) až na koniec – prvé sú jazyky v poradí, ako si ich vybral
    if let (true, Some(o)) = (u.pridat_original, &u.original) {
        if !stopy.iter().any(|t| t.kod == o.kod) {
            stopy.push(o.clone());
        }
    }
    let oboje = u.zvuk == "oboje";
    let krokov = stopy.len() as f64 * if oboje { 2.0 } else { 1.0 }
        + if u.rezim == "video" { 1.0 } else { 0.0 }
        + 1.0;
    let p = Arc::new(Mutex::new(Priebeh {
        krokov,
        krok: 0.0,
        text: String::new(),
    }));
    let h = hlasic(v, &p);
    let tmp = u
        .priecinok
        .join(format!(".kometa_jazyky_{}", super::server::novy_kluc()));
    tokio::fs::create_dir_all(&tmp).await?;
    let vysledok = async {
        let mut video = None;
        if u.rezim == "video" {
            let f = obraz(&u.info, u.vyska, u.mp4)
                .ok_or_else(|| Chyba::Ine("Video nemá samostatný obraz na stiahnutie.".into()))?;
            let popis = format!(
                "⬇ Video{}{}",
                u.vyska
                    .map(|x| format!(" (najviac {x}p)"))
                    .unwrap_or_else(|| " (najlepšia kvalita)".into()),
                if u.mp4 && s(&f, "vcodec").starts_with("avc1") {
                    " – H.264 pre MP4"
                } else {
                    ""
                }
            );
            v.jazyky.zapis(popis);
            krok(v, &p, "Sťahujem video…".into());
            video = Some(stiahni_format(v, &na(&f, "video")?, &tmp.join("video"), z, &h).await?);
            dalsi(&p);
        }
        let mut hlavne: Vec<(Stopa, PathBuf, &str)> = vec![];
        let mut druhe: Vec<(Stopa, PathBuf)> = vec![];
        for (i, t) in stopy.iter().enumerate() {
            if z.is_cancelled() {
                return Err(Chyba::Pauza);
            }
            krok(
                v,
                &p,
                format!("Sťahujem: {} ({}/{})…", t.nazov, i + 1, stopy.len()),
            );
            v.jazyky.zapis(format!(
                "⬇ {}  [{}]",
                t.nazov,
                if t.kod.is_empty() { "—" } else { &t.kod }
            ));
            let meno = |druh: &str| {
                tmp.join(format!(
                    "a{:02}_{}_{druh}",
                    i + 1,
                    bezpecny_nazov(if t.kod.is_empty() { "orig" } else { &t.kod }, 20)
                ))
            };
            let opus_f = t.opus.clone().unwrap_or_else(|| t.najlepsi.clone());
            if u.zvuk == "opus" {
                hlavne.push((
                    t.clone(),
                    stiahni_format(v, &na(&opus_f, "zvuk")?, &meno("opus"), z, &h).await?,
                    "opus",
                ));
                dalsi(&p);
                continue;
            }
            let mut opus_cesta = None;
            let (hlavna, kodek) = match &t.aac {
                Some(a) => (
                    stiahni_format(v, &na(a, "zvuk")?, &meno("aac"), z, &h).await?,
                    "aac",
                ),
                None => {
                    let o = stiahni_format(v, &na(&opus_f, "zvuk")?, &meno("opus"), z, &h).await?;
                    opus_cesta = Some(o.clone());
                    match na_aac(v, &o, &meno("aac").with_extension("m4a")).await {
                        Some(a) => {
                            v.jazyky.zapis(format!(
                                "↻ {} prevedené do AAC (zdroj ho neponúka)",
                                t.nazov
                            ));
                            (a, "aac")
                        }
                        None => {
                            v.jazyky.zapis(format!(
                                "ℹ {}: zdroj nemá AAC a bez FFmpeg ostane Opus",
                                t.nazov
                            ));
                            (o, "opus")
                        }
                    }
                }
            };
            hlavne.push((t.clone(), hlavna, kodek));
            dalsi(&p);
            if oboje {
                if let Some(o) = &t.opus {
                    if kodek == "aac" {
                        let cesta = match opus_cesta {
                            Some(c) => c,
                            None => {
                                stiahni_format(v, &na(o, "zvuk")?, &meno("opus"), z, &h).await?
                            }
                        };
                        druhe.push((t.clone(), cesta));
                    }
                }
                dalsi(&p);
            }
        }
        if z.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        krok(v, &p, "Spájam stopy do jedného súboru…".into());
        v.jazyky.zapis("⇒ Spájam stopy…");
        let kody: Vec<String> = hlavne
            .iter()
            .map(|(t, _, _)| {
                if t.kod.is_empty() {
                    "orig".into()
                } else {
                    t.kod.clone()
                }
            })
            .collect();
        let znacka = if kody.len() <= 8 {
            kody.join("+")
        } else {
            format!("{} jazykov", kody.len())
        };
        let pripona = match (&u.rezim[..], u.mp4) {
            ("audio", true) => "m4a",
            ("audio", false) => "mka",
            (_, true) => "mp4",
            _ => "mkv",
        };
        let mut vstupy: Vec<Vstup> = vec![];
        let ciel = match (&u.rezim[..], &u.existujuce, &video) {
            ("existing", Some(e), _) => {
                vstupy.push(Vstup {
                    cesta: e.clone(),
                    druh: "video".into(),
                    jazyk: None,
                    nazov: None,
                });
                let kmen = e
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                u.priecinok.join(format!(
                    "{} [{znacka}].{pripona}",
                    bezpecny_nazov(&kmen, 140)
                ))
            }
            (_, _, Some(vid)) => {
                vstupy.push(Vstup {
                    cesta: vid.clone(),
                    druh: "video".into(),
                    jazyk: None,
                    nazov: None,
                });
                u.priecinok.join(format!(
                    "{} [{znacka}].{pripona}",
                    bezpecny_nazov(&titul, 140)
                ))
            }
            _ => u.priecinok.join(format!(
                "{} [{znacka}].{pripona}",
                bezpecny_nazov(&titul, 140)
            )),
        };
        // najprv hlavné stopy v poradí jazykov (prvá = predvolená), potom tie isté jazyky v Opus
        for (t, cesta, kodek) in &hlavne {
            let opus_navyse = *kodek == "opus" && oboje;
            vstupy.push(Vstup {
                cesta: cesta.clone(),
                druh: "zvuk".into(),
                jazyk: Some(t.kod.clone()).filter(|k| !k.is_empty()),
                nazov: Some(format!(
                    "{}{}{}",
                    t.nazov,
                    if t.original { " – originál" } else { "" },
                    if opus_navyse { " – Opus" } else { "" }
                )),
            });
        }
        for (t, cesta) in &druhe {
            vstupy.push(Vstup {
                cesta: cesta.clone(),
                druh: "zvuk".into(),
                jazyk: Some(t.kod.clone()).filter(|k| !k.is_empty()),
                nazov: Some(format!(
                    "{}{} – Opus (vyššia kvalita)",
                    t.nazov,
                    if t.original { " – originál" } else { "" }
                )),
            });
        }
        if let ("existing", Some(e)) = (&u.rezim[..], &u.existujuce) {
            // pôvodný zvuk vlastného videa na koniec; pomenovať len stopu bez názvu
            let e2 = e.clone();
            let povodne = tokio::task::spawn_blocking(move || {
                crate::media::otvor(&e2).map(|c| c.stopy().to_vec())
            })
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
            let zvuky: Vec<_> = povodne
                .iter()
                .filter(|s| s.druh() == crate::media::Druh::Zvuk)
                .collect();
            let bez_nazvu = zvuky.len() == 1 && zvuky[0].nazov.is_none();
            vstupy.push(Vstup {
                cesta: e.clone(),
                druh: "zvuk".into(),
                jazyk: if bez_nazvu && zvuky[0].jazyk.is_none() {
                    u.original
                        .as_ref()
                        .map(|o| o.kod.clone())
                        .filter(|k| !k.is_empty())
                } else {
                    None
                },
                nazov: bez_nazvu.then(|| {
                    u.original
                        .as_ref()
                        .map(|o| format!("{} – originál", o.nazov))
                        .unwrap_or_else(|| "Pôvodný zvuk".into())
                }),
            });
        }
        let ciel = jedinecna(ciel);
        let out = spajanie::spoj(&vstupy, &ciel, &titul).await?;
        if u.samostatne {
            for (t, cesta, kodek) in &hlavne {
                let p = if *kodek == "aac" { "m4a" } else { "mka" };
                let c = jedinecna(u.priecinok.join(format!(
                    "{} [{}].{p}",
                    bezpecny_nazov(&titul, 140),
                    if t.kod.is_empty() { "orig" } else { &t.kod }
                )));
                let vstup = Vstup {
                    cesta: cesta.clone(),
                    druh: "zvuk".into(),
                    jazyk: Some(t.kod.clone()).filter(|k| !k.is_empty()),
                    nazov: Some(t.nazov.clone()),
                };
                if let Ok(x) = spajanie::spoj(std::slice::from_ref(&vstup), &c, &titul).await {
                    v.jazyky.zapis(format!(
                        "   ↳ {}",
                        x.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            }
        }
        Ok(out)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&tmp).await;
    vysledok
}

#[cfg(test)]
mod testy {
    use super::*;

    fn f(id: &str, jazyk: &str, kodek: &str, abr: f64, poznamka: &str, pref: i64) -> Value {
        json!({"format_id": id, "language": jazyk, "vcodec": "none", "acodec": kodek, "abr": abr,
               "ext": if kodek.starts_with("mp4a") { "m4a" } else { "webm" }, "protocol": "https",
               "format_note": poznamka, "language_preference": pref, "url": format!("https://cdn/{id}")})
    }

    #[test]
    fn stopy_a_vyber() {
        let info = json!({"formats": [
            f("140-0", "en", "mp4a.40.2", 129.0, "English original (default), medium", 10),
            f("251-0", "en", "opus", 140.0, "English original (default), medium", 10),
            f("251-1", "cs", "opus", 130.0, "Czech, medium", -1),
            f("140-1", "cs", "mp4a.40.2", 129.0, "Czech, medium", -1),
            f("251-2", "sk", "opus", 128.0, "Slovak, medium", -1),
            f("251-3", "de", "opus", 128.0, "German, medium", -1),
            f("251-drc", "de", "opus", 150.0, "German, medium, DRC", -1),
            {"format_id": "137", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "tbr": 4000, "ext": "mp4"},
            {"format_id": "248", "vcodec": "vp9", "acodec": "none", "height": 1080, "tbr": 2500, "ext": "webm"},
            {"format_id": "313", "vcodec": "vp9", "acodec": "none", "height": 2160, "tbr": 12000, "ext": "webm"},
        ]});
        let st = stopy(&info);
        let nazvy: Vec<&str> = st.iter().map(|s| s.nazov.as_str()).collect();
        assert_eq!(
            nazvy,
            vec!["Angličtina", "Čeština", "Nemčina", "Slovenčina"]
        );
        assert!(st[0].original && st[0].aac.is_some());
        assert_eq!(s(&st[2].najlepsi, "format_id"), "251-3", "DRC až nakoniec");
        assert!(st[3].aac.is_none());
        let (v, chyba) = vyber(&st, &["cs".into(), "sk".into(), "pl".into()]);
        assert_eq!(
            v.iter().map(|x| x.kod.as_str()).collect::<Vec<_>>(),
            vec!["cs", "sk"]
        );
        assert_eq!(chyba, vec!["pl"]);
        assert_eq!(s(&obraz(&info, None, false).unwrap(), "format_id"), "313");
        assert_eq!(
            s(&obraz(&info, Some(1080), false).unwrap(), "format_id"),
            "137"
        );
        assert_eq!(s(&obraz(&info, None, true).unwrap(), "format_id"), "137");
    }

    #[test]
    fn nazvy_jazykov() {
        assert_eq!(nazov_jazyka("en-US", ""), "Angličtina (USA)");
        assert_eq!(nazov_jazyka("cs-CZ", ""), "Čeština (CZ)");
        assert_eq!(nazov_jazyka("en-desc", ""), "Angličtina – zvukový popis");
        assert_eq!(nazov_jazyka("xx", "Klingon"), "Klingon");
        assert_eq!(nazov_jazyka("", ""), "Pôvodná stopa");
    }
}
