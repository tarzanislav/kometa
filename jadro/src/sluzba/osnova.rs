//! Prechod osnovy kurzu: pre portály, kde lekcie nie sú odkazy na samostatné stránky, ale
//! prepínajú sa kliknutím na tej istej stránke (napr. „course-progress“).
//!
//! Kometa v okne prehliadača postupne klikne na každú lekciu osnovy – riadok s dĺžkou („26:40“,
//! „20 minút“) – počká, kým sa obsah lekcie zmení, a zapíše si, čo sa ukázalo: prehrávač videa,
//! text lekcie, test s otázkami a materiály (PDF…). Klikne len na riadky osnovy, nič iné.
//! Texty a testy sa uložia ako stránky HTML, videá idú do sťahovania cez adresu prehrávača.

use super::kurzy::{Kapitola, Kurz, Lekcia};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// Pomocné funkcie spoločné pre všetky skripty prechodu (nájdenie riadkov osnovy).
const NAJDI_JS: &str = r##"/* kometaOsnova */
const kTxt = (el) => ((el && (el.innerText || el.textContent)) || '').replace(/\s+/g, ' ').trim();
const kCas = /^(?:\d{1,2}:\d{2}(?::\d{2})?|\d{1,3} ?(?:min(?:út|úty|uta|uty|ut)?|mins?|minutes?)\.?)$/i;
const kNadpisy = 'h1,h2,h3,h4,h5,h6';
// bunky s dĺžkou: najmenšie prvky, ktorých celý text je dĺžka
const kBunky = () => {
  const vsetky = [...document.querySelectorAll('body *')].filter((el) => {
    if (el.closest('script,style,noscript,footer')) return false;
    const t = (el.textContent || '').replace(/\s+/g, ' ').trim();
    return t.length <= 24 && kCas.test(t);
  });
  return vsetky.filter((el) => !vsetky.some((x) => x !== el && el.contains(x)));
};
// riadok lekcie: najväčší obal okolo dĺžky, v ktorom nie je iná dĺžka ani nadpis kapitoly
const kRiadky = () => {
  const bunky = kBunky();
  const out = [];
  for (const b of bunky) {
    let r = b;
    while (r.parentElement && r.parentElement !== document.body) {
      const p = r.parentElement;
      if (bunky.filter((x) => p.contains(x)).length > 1) break;
      if (p.querySelector(kNadpisy) && !r.querySelector(kNadpisy)) break;
      r = p;
    }
    if (r !== b && !out.some((x) => x.r === r)) out.push({ r, b });
  }
  return out;
};
const kSum = /^(videné|vid[ée]n[ée]|hotovo|dokončené|zobrazené|viewed|completed|done|náhľad|preview|zamknuté|locked)$/i;
const kNazov = (r, b) => kTxt(r).replace(kTxt(b), ' ').split(' ').filter((w) => w && !kSum.test(w))
  .join(' ').trim().slice(0, 200);
const kKapitola = (r) => {
  let najdeny = null;
  for (const h of document.querySelectorAll(kNadpisy)) {
    if (h.compareDocumentPosition(r) & Node.DOCUMENT_POSITION_FOLLOWING) najdeny = h; else break;
  }
  return najdeny ? kTxt(najdeny).slice(0, 200) : '';
};
const kCiel = (r) => r.querySelector('a[href]:not([href="#"]):not([href^="javascript"])')
  || r.querySelector('a,button,[role="button"],[onclick],[data-href],[data-url],[data-lesson],[data-id]') || r;
const kAktivna = (r) => !!r.closest('[aria-current]') || r.querySelector('[aria-current]') !== null
  || [r, ...r.querySelectorAll('*')].some((e) => /\b(active|current|selected|playing|aktiv)/i.test(e.getAttribute('class') || ''));
"##;

/// Zoznam riadkov osnovy (každý dostane značku `data-kometa-lekcia`).
const OSNOVA: &str = r#"return kRiadky().map(({ r, b }, i) => {
  r.setAttribute('data-kometa-lekcia', String(i));
  const c = kCiel(r);
  return { i, nazov: kNazov(r, b), kapitola: kKapitola(r), dlzka: kTxt(b), aktivna: kAktivna(r),
           odkaz: c.tagName === 'A' && /^https?:/.test(c.href) ? c.href : '' };
});"#;

/// Obsah práve otvorenej lekcie (mimo osnovy).
const OBSAH: &str = r#"const riadky = kRiadky().map((x) => x.r);
let osnova = riadky[0] || null;
while (osnova && !riadky.every((r) => osnova.contains(r))) osnova = osnova.parentElement;
const mimo = (el) => !(osnova && (osnova.contains(el) || el.contains(osnova)));
const abs = (u) => { try { return new URL(u, location.href).href } catch (e) { return '' } };
const vidno = (el) => { const b = el.getBoundingClientRect(); return b.width >= 160 && b.height >= 90; };
const ramce = [...document.querySelectorAll('iframe')].filter((f) => mimo(f) && vidno(f))
  .map((f) => abs(f.getAttribute('src') || '')).filter((u) => /^https?:/.test(u));
const videa = [...document.querySelectorAll('video')].filter(mimo)
  .map((v) => v.currentSrc || v.getAttribute('src') || ((v.querySelector('source') || {}).src) || '')
  .map(abs).filter((u) => /^https?:/.test(u));
const kand = [...document.querySelectorAll('main,article,[role="main"],[class*="content"],[class*="lesson"],[class*="unit"],[class*="entry"],[class*="topic"],[id*="content"]')]
  .filter((el) => mimo(el) && !el.closest('header,footer,nav'));
const dl = (el) => kTxt(el).length;
const hlavny = kand.sort((a, b) => dl(b) - dl(a))[0] || null;
let obsah = '';
if (hlavny) {
  const k = hlavny.cloneNode(true);
  k.querySelectorAll('script,style,noscript,iframe,object,embed,link,meta,[data-kometa-lekcia]').forEach((e) => e.remove());
  k.querySelectorAll('*').forEach((e) => {
    for (const a of [...e.attributes]) if (/^on/i.test(a.name)) e.removeAttribute(a.name);
    if (e.hasAttribute('src')) e.setAttribute('src', abs(e.getAttribute('src')));
    if (e.hasAttribute('href')) e.setAttribute('href', abs(e.getAttribute('href')));
    if (e.matches('input,textarea,select,button')) e.setAttribute('disabled', '');
  });
  obsah = k.innerHTML.slice(0, 1500000);
}
const nadpis = kTxt((hlavny && hlavny.querySelector('h1,h2,h3')) || document.querySelector('h1'));
const materialy = hlavny ? [...hlavny.querySelectorAll('a[href]')]
  .map((a) => ({ href: abs(a.getAttribute('href')), text: kTxt(a) }))
  .filter((x) => /\.(pdf|docx?|pptx?|ppsx?|xlsx?|odt|ods|odp|rtf|txt|epub|zip|rar|7z|mp3|m4a|wav)(\?|#|$)/i.test(x.href)) : [];
const formular = !!(hlavny && hlavny.querySelector('input[type="radio"],input[type="checkbox"],textarea,select'));
return { url: location.href, nadpis, ramce, videa, obsah, text: hlavny ? dl(hlavny) : 0, formular, materialy,
         podpis: ramce.join('|') + '#' + videa.join('|') + '#' + nadpis + '#' + (hlavny ? kTxt(hlavny).slice(0, 400) : '') };"#;

fn skript(telo: &str) -> String {
    format!("(() => {{\n{NAJDI_JS}\n{telo}\n}})()")
}

/// Skript, ktorý zoznam riadkov osnovy vráti (aj ich počet sa hodí pri bežnom načítaní).
pub fn skript_osnovy() -> String {
    skript(OSNOVA)
}

fn skript_obsahu() -> String {
    skript(OBSAH)
}

fn skript_kliku(i: usize) -> String {
    skript(&format!(
        "const x = kRiadky()[{i}];\nif (!x) return {{ ok: false }};\nconst c = kCiel(x.r);\nc.scrollIntoView({{ block: 'center' }});\nc.click();\nreturn {{ ok: true }};"
    ))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Riadok {
    #[serde(default)]
    pub nazov: String,
    #[serde(default)]
    pub kapitola: String,
    #[serde(default)]
    pub dlzka: String,
    #[serde(default)]
    pub aktivna: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Material {
    href: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Obsah {
    #[serde(default)]
    url: String,
    #[serde(default)]
    nadpis: String,
    #[serde(default)]
    ramce: Vec<String>,
    #[serde(default)]
    videa: Vec<String>,
    #[serde(default)]
    obsah: String,
    #[serde(default)]
    text: usize,
    #[serde(default)]
    formular: bool,
    #[serde(default)]
    materialy: Vec<Material>,
    #[serde(default)]
    podpis: String,
}

/// Výsledok skriptu (JSON alebo JSON zabalený v reťazci) → typ.
fn citaj<T: serde::de::DeserializeOwned>(t: &str) -> Option<T> {
    let v: Value = serde_json::from_str(t).ok()?;
    let v = match v {
        Value::String(s) => serde_json::from_str(&s).ok()?,
        v => v,
    };
    serde_json::from_value(v).ok()
}

/// Prehrávače videa, ktoré Kometa pozná (rámec s nimi = video lekcia).
const PREHRAVACE: [&str; 16] = [
    "vimeo.com",
    "youtube.com",
    "youtube-nocookie.com",
    "youtu.be",
    "wistia",
    "mediadelivery.net",
    "jwplayer",
    "jwplatform",
    "brightcove",
    "vidyard",
    "loom.com",
    "cloudflarestream",
    "dailymotion",
    "kaltura",
    "spotlightr",
    "sproutvideo",
];

// ---- stav prechodu (jeden naraz), zrušenie a texty lekcií ----

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stav {
    pub bezi: bool,
    pub hotovo: usize,
    pub celkom: usize,
    pub lekcia: String,
    pub chyba: String,
    pub hotovy: bool,
}

static STAV: LazyLock<Mutex<Stav>> = LazyLock::new(Default::default);
static VYSLEDOK: LazyLock<Mutex<Option<Kurz>>> = LazyLock::new(Default::default);
static ZRUSIT: AtomicBool = AtomicBool::new(false);
/// Texty a testy lekcií (HTML) podľa `obsah_id` – zapíšu sa pri stiahnutí.
static OBSAHY: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

pub fn stav() -> Stav {
    STAV.lock().unwrap().clone()
}

pub fn zastav() {
    ZRUSIT.store(true, Ordering::SeqCst);
}

pub fn vysledok() -> Option<Kurz> {
    VYSLEDOK.lock().unwrap().clone()
}

pub fn obsah(id: &str) -> Option<String> {
    OBSAHY.lock().unwrap().get(id).cloned()
}

/// Začne prechod (vráti `false`, ak už jeden beží).
pub fn zacni() -> bool {
    let mut s = STAV.lock().unwrap();
    if s.bezi {
        return false;
    }
    *s = Stav {
        bezi: true,
        ..Default::default()
    };
    ZRUSIT.store(false, Ordering::SeqCst);
    *VYSLEDOK.lock().unwrap() = None;
    true
}

/// Dokončí prechod: výsledok alebo chyba.
pub fn skonci(r: Result<Kurz, String>) {
    let mut s = STAV.lock().unwrap();
    s.bezi = false;
    match r {
        Ok(k) => {
            s.hotovy = true;
            *VYSLEDOK.lock().unwrap() = Some(k);
        }
        Err(e) => s.chyba = e,
    }
}

/// Prejde osnovu: `spusti` vykoná skript v okne prehliadača, `cakaj` počká. Vráti kurz s
/// lekciami (video so zdrojom, text a test s uloženým obsahom, materiály).
pub fn prejdi(
    spusti: &dyn Fn(&str) -> Result<String, String>,
    cakaj: &dyn Fn(Duration),
    nazov_kurzu: &str,
) -> Result<Kurz, String> {
    let riadky: Vec<Riadok> = citaj(&spusti(&skript_osnovy())?)
        .ok_or("Stránka nevrátila osnovu – počkaj, kým sa načíta, a skús znova.")?;
    if riadky.is_empty() {
        return Err(
            "Na stránke nie je osnova s lekciami (riadky s dĺžkou ako 26:40 alebo 20 minút)."
                .into(),
        );
    }
    let precitaj = || -> Result<Obsah, String> {
        citaj(&spusti(&skript_obsahu())?)
            .ok_or_else(|| "Stránka nevrátila obsah lekcie.".to_string())
    };
    let mut predosly = precitaj()?;
    let aktivna = riadky.iter().position(|r| r.aktivna);
    let mut kliknute = false;
    let mut kapitoly: Vec<Kapitola> = vec![];
    let mut adresy: HashMap<String, usize> = HashMap::new();
    let mut materialy: std::collections::HashSet<String> = Default::default();
    let id_prechodu = super::fronta::nove_id();
    STAV.lock().unwrap().celkom = riadky.len();
    for (i, r) in riadky.iter().enumerate() {
        if ZRUSIT.load(Ordering::SeqCst) {
            break;
        }
        {
            let mut s = STAV.lock().unwrap();
            s.hotovo = i;
            s.lekcia = r.nazov.clone();
        }
        // otvorená lekcia na začiatku sa nečíta znova; ostatné: klik a čakanie na zmenu obsahu
        let obsah = if aktivna == Some(i) && !kliknute {
            Some(predosly.clone())
        } else {
            let _ = spusti(&skript_kliku(i))?;
            kliknute = true;
            let mut novy = None;
            for _ in 0..24 {
                cakaj(Duration::from_millis(500));
                let o = precitaj().unwrap_or_default();
                if !o.podpis.is_empty() && o.podpis != predosly.podpis {
                    // ešte chvíľa, kým sa doplní aj prehrávač
                    cakaj(Duration::from_millis(700));
                    novy = Some(precitaj().unwrap_or(o));
                    break;
                }
            }
            novy
        };
        let kap = r.kapitola.trim().to_string();
        let k = match kapitoly.iter().position(|k| k.nazov == kap) {
            Some(x) => x,
            None => {
                kapitoly.push(Kapitola {
                    nazov: kap,
                    lekcie: vec![],
                    menu: false,
                });
                kapitoly.len() - 1
            }
        };
        let Some(o) = obsah else {
            // obsah sa nezmenil – lekcia sa nedá otvoriť (zamknutá?); ostane, ale neoznačená
            kapitoly[k].lekcie.push(Lekcia {
                url: format!(
                    "{}#kometa-lekcia-{i}",
                    predosly.url.split('#').next().unwrap_or("")
                ),
                nazov: r.nazov.clone(),
                odporucana: false,
                material: false,
                dlzka: r.dlzka.clone(),
                druh: "nezistene".into(),
                zdroj: String::new(),
                obsah_id: String::new(),
            });
            continue;
        };
        predosly = o.clone();
        // jedinečná adresa lekcie (pri stránkach, ktoré lekcie prepínajú na mieste, je rovnaká)
        let zaklad = o.url.split('#').next().unwrap_or("").to_string();
        let n = adresy.entry(zaklad.clone()).or_default();
        *n += 1;
        let url = if *n == 1 {
            zaklad
        } else {
            format!("{zaklad}#kometa-lekcia-{i}")
        };
        let prehravac = o
            .ramce
            .iter()
            .find(|u| PREHRAVACE.iter().any(|p| u.contains(p)))
            .cloned()
            .or_else(|| o.videa.iter().find(|u| !u.starts_with("blob:")).cloned());
        let (druh, zdroj, obsah_id) = match prehravac {
            Some(z) => ("video", z, String::new()),
            None if o.formular || o.text > 0 => {
                let id = format!("{id_prechodu}-{i}");
                OBSAHY
                    .lock()
                    .unwrap()
                    .insert(id.clone(), dokument(&r.nazov, &o.url, &o.obsah));
                (if o.formular { "test" } else { "text" }, String::new(), id)
            }
            None => ("nezistene", String::new(), String::new()),
        };
        kapitoly[k].lekcie.push(Lekcia {
            url,
            nazov: if r.nazov.is_empty() {
                o.nadpis.clone()
            } else {
                r.nazov.clone()
            },
            odporucana: druh != "nezistene",
            material: false,
            dlzka: r.dlzka.clone(),
            druh: druh.into(),
            zdroj,
            obsah_id,
        });
        for m in &o.materialy {
            if materialy.insert(m.href.clone()) {
                let nazov = if m.text.trim().is_empty() {
                    super::stahovanie::nazov_z_adresy(&m.href)
                } else {
                    m.text.trim().chars().take(150).collect()
                };
                kapitoly[k].lekcie.push(Lekcia {
                    url: m.href.clone(),
                    nazov,
                    odporucana: true,
                    material: true,
                    dlzka: String::new(),
                    druh: "material".into(),
                    zdroj: String::new(),
                    obsah_id: String::new(),
                });
            }
        }
    }
    STAV.lock().unwrap().hotovo = riadky.len();
    Ok(Kurz {
        nazov: nazov_kurzu.to_string(),
        adresa: predosly.url.split('#').next().unwrap_or("").to_string(),
        kapitoly,
        riadkov_osnovy: riadky.len(),
    })
}

/// Samostatná stránka HTML s textom lekcie (obrázky a odkazy vedú na pôvodnú stránku).
fn dokument(nazov: &str, url: &str, obsah: &str) -> String {
    let h = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    format!(
        "<!doctype html>\n<html lang=\"sk\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{t}</title>\n<style>\nbody {{ font-family: system-ui, sans-serif; max-width: 46rem; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: #1d2433; }}\nimg, video {{ max-width: 100%; height: auto; }}\ntable {{ border-collapse: collapse; }} td, th {{ border: 1px solid #ccd; padding: .3rem .5rem; }}\n.zdroj {{ margin-top: 3rem; font-size: .85rem; color: #667; }}\n</style>\n</head>\n<body>\n<h1>{t}</h1>\n{obsah}\n<p class=\"zdroj\">Zdroj: <a href=\"{u}\">{u}</a> – uložené Kometou</p>\n</body>\n</html>\n",
        t = h(nazov),
        u = h(url),
    )
}

#[cfg(test)]
mod skusky {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Simulovaná stránka: lekcie sa prepínajú kliknutím, adresa sa nemení.
    fn stranka() -> (Vec<Value>, Vec<Value>) {
        let riadky = vec![
            serde_json::json!({"nazov": "Ako používať AI nástroje", "kapitola": "I. Prehľad", "dlzka": "26:40", "aktivna": true}),
            serde_json::json!({"nazov": "Ktoré chatovacie nástroje", "kapitola": "I. Prehľad", "dlzka": "20 minút"}),
            serde_json::json!({"nazov": "Ako začať s Gemini", "kapitola": "II. Gemini", "dlzka": "44:11"}),
            serde_json::json!({"nazov": "Priebežný test", "kapitola": "II. Gemini", "dlzka": "20 minút"}),
            serde_json::json!({"nazov": "Zamknutá lekcia", "kapitola": "II. Gemini", "dlzka": "10:00"}),
        ];
        let u = "https://www.vita.sk/online-kurz/ai/course-progress";
        let obsahy = vec![
            serde_json::json!({"url": u, "nadpis": "Ako používať", "ramce": ["https://player.vimeo.com/video/1?h=a"], "text": 50, "podpis": "1"}),
            serde_json::json!({"url": u, "nadpis": "Chatovacie", "text": 900, "obsah": "<p>Text <b>lekcie</b></p>",
                               "materialy": [{"href": "https://www.vita.sk/wp-content/uploads/zoznam.pdf", "text": "Zoznam nástrojov"}], "podpis": "2"}),
            serde_json::json!({"url": u, "nadpis": "Gemini", "ramce": ["https://www.googletagmanager.com/x", "https://player.vimeo.com/video/3?h=c"], "podpis": "3"}),
            serde_json::json!({"url": u, "nadpis": "Test", "text": 300, "formular": true, "obsah": "<form><input type=radio> A</form>", "podpis": "4"}),
        ];
        (riadky, obsahy)
    }

    #[test]
    fn prechod_osnovy_na_jednej_stranke() {
        let (riadky, obsahy) = stranka();
        let otvorena = Cell::new(0usize);
        let kliky = RefCell::new(vec![]);
        let spusti = |js: &str| -> Result<String, String> {
            if js.contains("c.click()") {
                let i: usize = js
                    .split("kRiadky()[")
                    .nth(1)
                    .unwrap()
                    .split(']')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                kliky.borrow_mut().push(i);
                if i < obsahy.len() {
                    otvorena.set(i); // zamknutá lekcia (4) sa neotvorí
                }
                return Ok("{\"ok\":true}".into());
            }
            if js.contains("data-kometa-lekcia', String(i)") {
                return Ok(Value::Array(riadky.clone()).to_string());
            }
            Ok(obsahy[otvorena.get()].to_string())
        };
        let k = prejdi(&spusti, &|_| {}, "Kurz AI").unwrap();
        // otvorená prvá lekcia sa nekliká
        assert_eq!(*kliky.borrow(), vec![1, 2, 3, 4]);
        type Kap<'a> = (&'a str, Vec<(&'a str, &'a str, bool)>);
        let prehlad: Vec<Kap> = k
            .kapitoly
            .iter()
            .map(|x| {
                (
                    x.nazov.as_str(),
                    x.lekcie
                        .iter()
                        .map(|l| (l.nazov.as_str(), l.druh.as_str(), l.odporucana))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            prehlad,
            vec![
                (
                    "I. Prehľad",
                    vec![
                        ("Ako používať AI nástroje", "video", true),
                        ("Ktoré chatovacie nástroje", "text", true),
                        ("Zoznam nástrojov", "material", true),
                    ]
                ),
                (
                    "II. Gemini",
                    vec![
                        ("Ako začať s Gemini", "video", true),
                        ("Priebežný test", "test", true),
                        ("Zamknutá lekcia", "nezistene", false),
                    ]
                ),
            ]
        );
        let l = &k.kapitoly;
        // video: adresa prehrávača (nie meracieho skriptu), adresy lekcií sú jedinečné
        assert_eq!(l[0].lekcie[0].zdroj, "https://player.vimeo.com/video/1?h=a");
        assert_eq!(l[1].lekcie[0].zdroj, "https://player.vimeo.com/video/3?h=c");
        let mut adresy: Vec<&str> = l
            .iter()
            .flat_map(|x| x.lekcie.iter().map(|y| y.url.as_str()))
            .collect();
        adresy.sort();
        adresy.dedup();
        assert_eq!(adresy.len(), 6);
        // text a test sú uložené ako stránky HTML
        let t = obsah(&l[0].lekcie[1].obsah_id).unwrap();
        assert!(
            t.contains("<title>Ktoré chatovacie nástroje</title>")
                && t.contains("<p>Text <b>lekcie</b></p>")
        );
        assert!(obsah(&l[1].lekcie[1].obsah_id).unwrap().contains("<form>"));
        assert_eq!(k.riadkov_osnovy, 5);
    }

    #[test]
    fn bez_osnovy_chyba() {
        let e = prejdi(&|_| Ok("[]".into()), &|_| {}, "x").unwrap_err();
        assert!(e.contains("osnova"), "{e}");
    }

    #[test]
    fn dokument_bez_vlozenia_kodu_do_nazvu() {
        let d = dokument(
            "<script>x</script> & ja",
            "https://a.sk/?a=1&b=\"2\"",
            "<p>ok</p>",
        );
        assert!(d.contains("<title>&lt;script&gt;x&lt;/script&gt; &amp; ja</title>"));
        assert!(d.contains("href=\"https://a.sk/?a=1&amp;b=&quot;2&quot;\""));
    }

    #[test]
    fn video_z_prechodu_ide_cez_prehravac_text_nejde_do_frontu() {
        let u = "https://www.vita.sk/kurz/course-progress";
        let k = Kurz {
            nazov: "Kurz".into(),
            adresa: u.into(),
            kapitoly: vec![Kapitola {
                nazov: "I.".into(),
                lekcie: vec![
                    Lekcia {
                        url: format!("{u}#kometa-lekcia-0"),
                        nazov: "Video".into(),
                        druh: "video".into(),
                        zdroj: "https://player.vimeo.com/video/1?h=a".into(),
                        ..Default::default()
                    },
                    Lekcia {
                        url: format!("{u}#kometa-lekcia-1"),
                        nazov: "Text".into(),
                        druh: "text".into(),
                        obsah_id: "x".into(),
                        ..Default::default()
                    },
                ],
                menu: false,
            }],
            riadkov_osnovy: 2,
        };
        let p = super::super::kurzy::polozky(&k, "/tmp/out", "video");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].url, "https://player.vimeo.com/video/1?h=a");
        assert_eq!(p[0].referer, u);
        assert_eq!(p[0].nazov, "01 Video");
    }
}
