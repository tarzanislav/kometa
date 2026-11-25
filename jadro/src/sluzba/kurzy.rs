//! Kurzy po prihlásení (len aplikácia). Kometa otvorí vlastné okno prehliadača, v ktorom sa
//! prihlásiš a prejdeš na stránku kurzu. Potom z nej načíta zoznam lekcií (odkazy a nadpisy
//! kapitol tak, ako ich vidíš) a vybrané lekcie stiahne do priečinkov podľa kapitol.
//!
//! Prihlásenie (cookies) sa vezme z okna prehliadača, drží sa len v pamäti a posiela sa iba
//! tej stránke, ktorej patrí – nie vloženým prehrávačom iných služieb. Video chránené DRM
//! Kometa nestiahne (pri lekcii to napíše).

use super::fronta::Nova;
use super::{zdroje, Vnutro};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Skript, ktorý sa spustí na stránke kurzu: odkazy s textom, kapitola každého odkazu,
/// vložené prehrávače a videá. Nič nemení, len číta.
pub const ZBER_JS: &str = r#"(() => {
  const abs = (h) => { try { return new URL(h, location.href).href } catch (e) { return '' } };
  const txt = (el) => ((el && (el.innerText || el.textContent)) || '').replace(/\s+/g, ' ').trim();
  const kontajner = '[class*="section"],[class*="module"],[class*="chapter"],[class*="topic"],[class*="kapitol"],[class*="modul"],[class*="sekci"],[class*="curriculum-section"],[class*="lesson-group"],details';
  const nadpisy = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')];
  const pred = (el) => {
    let najdeny = '';
    for (const h of nadpisy) {
      if (h.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING) najdeny = h; else break;
    }
    return najdeny ? txt(najdeny) : '';
  };
  const kapitola = (a) => {
    const k = a.closest(kontajner);
    if (k) {
      const t = k.querySelector('h1,h2,h3,h4,h5,h6,summary,[class*="title"],[class*="nazov"],[class*="name"]');
      if (t && !t.contains(a) && !a.contains(t)) return txt(t);
    }
    return pred(a);
  };
  const odkazy = [];
  for (const a of document.querySelectorAll('a[href]')) {
    if (odkazy.length >= 3000) break;
    const href = abs(a.getAttribute('href'));
    if (!/^https?:/.test(href)) continue;
    const text = txt(a) || a.getAttribute('title') || a.getAttribute('aria-label') || '';
    odkazy.push({ href, text: text.slice(0, 200), kapitola: kapitola(a).slice(0, 200) });
  }
  const ramce = [...document.querySelectorAll('iframe[src]')].map((f) => abs(f.getAttribute('src'))).filter(Boolean);
  const videa = [...document.querySelectorAll('video[src],video source[src]')].map((v) => abs(v.getAttribute('src'))).filter(Boolean);
  const h1 = document.querySelector('h1');
  return { url: location.href, titulok: document.title, h1: h1 ? txt(h1) : '', odkazy, ramce, videa };
})()"#;

#[derive(Debug, Deserialize)]
pub struct Zber {
    pub url: String,
    #[serde(default)]
    pub titulok: String,
    #[serde(default)]
    pub h1: String,
    #[serde(default)]
    pub odkazy: Vec<OdkazZberu>,
    #[serde(default)]
    pub ramce: Vec<String>,
    #[serde(default)]
    pub videa: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct OdkazZberu {
    pub href: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kapitola: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lekcia {
    pub url: String,
    pub nazov: String,
    /// predvybraná (vyzerá ako lekcia tohto kurzu)
    #[serde(default)]
    pub odporucana: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Kapitola {
    pub nazov: String,
    pub lekcie: Vec<Lekcia>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Kurz {
    pub nazov: String,
    pub adresa: String,
    pub kapitoly: Vec<Kapitola>,
}

fn bez_hash(u: &str) -> &str {
    u.split('#').next().unwrap_or(u).trim_end_matches('/')
}

const NIE: [&str; 24] = [
    "login",
    "logout",
    "prihlas",
    "odhlas",
    "registr",
    "signup",
    "sign-up",
    "register",
    "cart",
    "kosik",
    "košík",
    "checkout",
    "account",
    "profil",
    "profile",
    "kontakt",
    "contact",
    "privacy",
    "gdpr",
    "cookies",
    "obchodne-podmienky",
    "terms",
    "wp-admin",
    "feed",
];
const ANO: [&str; 14] = [
    "lesson", "lekci", "lekce", "topic", "unit", "kapitol", "chapter", "lecture", "video", "modul",
    "module", "lekcia", "part", "diel",
];
const SUBORY: [&str; 12] = [
    ".jpg", ".jpeg", ".png", ".gif", ".svg", ".webp", ".css", ".js", ".ico", ".xml", ".pdf", ".zip",
];

/// Zo zberu stránky poskladá kurz: lekcie z tej istej stránky (domény), zoskupené podľa kapitol.
pub fn kurz_zo_zberu(z: &Zber) -> Kurz {
    let stranka = url::Url::parse(&z.url).ok();
    let host = stranka
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string();
    // „priečinok“ kurzu – lekcie často ležia pod jeho cestou
    let koren_cesty = stranka
        .as_ref()
        .map(|u| {
            let c = u.path().trim_end_matches('/');
            c.rsplit_once('/').map(|x| x.0).unwrap_or("").to_string()
        })
        .unwrap_or_default();
    let mut videne: HashSet<String> = HashSet::new();
    videne.insert(bez_hash(&z.url).to_string());
    let mut kapitoly: Vec<Kapitola> = vec![];
    // video priamo na stránke kurzu
    if !z.ramce.is_empty() || !z.videa.is_empty() {
        kapitoly.push(Kapitola {
            nazov: String::new(),
            lekcie: vec![Lekcia {
                url: bez_hash(&z.url).to_string(),
                nazov: if z.h1.is_empty() {
                    z.titulok.clone()
                } else {
                    z.h1.clone()
                },
                odporucana: true,
            }],
        });
    }
    for o in &z.odkazy {
        let Ok(u) = url::Url::parse(&o.href) else {
            continue;
        };
        let h = u.host_str().unwrap_or("").trim_start_matches("www.");
        if h != host && !h.ends_with(&format!(".{host}")) {
            continue;
        }
        let kluc = bez_hash(&o.href).to_string();
        let cesta = u.path().to_lowercase();
        let text = o.text.trim();
        if text.is_empty()
            || cesta.is_empty()
            || cesta == "/"
            || SUBORY.iter().any(|s| cesta.ends_with(s))
            || NIE.iter().any(|n| cesta.contains(n))
            || !videne.insert(kluc.clone())
        {
            continue;
        }
        let odporucana = ANO.iter().any(|a| cesta.contains(a))
            || (koren_cesty.len() > 1 && u.path().starts_with(&format!("{koren_cesty}/")));
        let nazov_kap = o.kapitola.trim().to_string();
        let k = match kapitoly.iter_mut().position(|k| k.nazov == nazov_kap) {
            Some(i) => &mut kapitoly[i],
            None => {
                kapitoly.push(Kapitola {
                    nazov: nazov_kap,
                    lekcie: vec![],
                });
                kapitoly.last_mut().unwrap()
            }
        };
        k.lekcie.push(Lekcia {
            url: kluc,
            nazov: text.chars().take(150).collect(),
            odporucana,
        });
    }
    // kapitoly s odporúčanými lekciami dopredu, ostatné (menu, päta) na koniec
    kapitoly.sort_by_key(|k| !k.lekcie.iter().any(|l| l.odporucana));
    let nazov = [&z.h1, &z.titulok]
        .iter()
        .map(|t| t.trim())
        .find(|t| !t.is_empty())
        .unwrap_or("Kurz")
        .to_string();
    Kurz {
        nazov,
        adresa: z.url.clone(),
        kapitoly,
    }
}

/// Cookies pre položku s prihlásením: uložené z prehliadača kurzov, inak (po reštarte) znova
/// z okna prehliadača, ak je otvorené.
pub fn cookies_pre(v: &Vnutro, url: &str) -> Vec<(String, String)> {
    let mut p = v.prihlasenia.lock().unwrap();
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    if !p.iter().any(|(u, _)| u.contains(&host)) {
        let nove = v.prostredie.prehliadac_cookies(url);
        p.extend(nove);
    }
    p.clone()
}

/// Položky frontu pre vybrané lekcie: priečinok kurzu, podpriečinky kapitol, čísla v názvoch.
pub fn polozky(kurz: &Kurz, vystup: &str, format: &str) -> Vec<Nova> {
    let koren = std::path::Path::new(vystup).join(zdroje::bezpecny_nazov(&kurz.nazov, 100));
    let kapitoly: Vec<&Kapitola> = kurz
        .kapitoly
        .iter()
        .filter(|k| !k.lekcie.is_empty())
        .collect();
    let jedna = kapitoly.len() <= 1;
    let mut out = vec![];
    for (ki, k) in kapitoly.iter().enumerate() {
        let priecinok = if jedna {
            koren.clone()
        } else {
            let meno = if k.nazov.trim().is_empty() {
                format!("{:02}", ki + 1)
            } else {
                format!("{:02} {}", ki + 1, k.nazov.trim())
            };
            koren.join(zdroje::bezpecny_nazov(&meno, 100))
        };
        for (li, l) in k.lekcie.iter().enumerate() {
            out.push(Nova {
                url: l.url.clone(),
                format: format.to_string(),
                priecinok: priecinok.to_string_lossy().into_owned(),
                nazov: format!("{:02} {}", li + 1, l.nazov.trim()),
                zdroj: "Kurz".into(),
                referer: kurz.adresa.clone(),
                prihlasenie: true,
                ..Default::default()
            });
        }
    }
    out
}

/// Prevedie výsledok skriptu (JSON text, niekedy ešte raz zabalený ako reťazec) na zber.
pub fn precitaj_zber(t: &str) -> Option<Zber> {
    let v: Value = serde_json::from_str(t).ok()?;
    let v = match v {
        Value::String(s) => serde_json::from_str(&s).ok()?,
        v => v,
    };
    serde_json::from_value(v).ok()
}

#[cfg(test)]
mod testy {
    use super::*;

    fn o(href: &str, text: &str, kap: &str) -> OdkazZberu {
        OdkazZberu {
            href: href.into(),
            text: text.into(),
            kapitola: kap.into(),
        }
    }

    #[test]
    fn kurz_z_odkazov() {
        let z = Zber {
            url: "https://www.vita.sk/online-kurz/ai-grafika/course-preview".into(),
            titulok: "Kurz | Vita".into(),
            h1: "AI grafika, dizajn a multimédiá".into(),
            odkazy: vec![
                o("https://www.vita.sk/", "Domov", ""),
                o("https://www.vita.sk/prihlasenie", "Prihlásiť", ""),
                o(
                    "https://www.vita.sk/online-kurz/ai-grafika/lekcia-1",
                    "Úvod do kurzu",
                    "Začíname",
                ),
                o(
                    "https://www.vita.sk/online-kurz/ai-grafika/lekcia-2",
                    "Ako písať zadania",
                    "Začíname",
                ),
                o(
                    "https://www.vita.sk/online-kurz/ai-grafika/lekcia-2#komentare",
                    "Komentáre",
                    "Začíname",
                ),
                o(
                    "https://www.vita.sk/online-kurz/ai-grafika/lekcia-3",
                    "Prvý obrázok",
                    "Generovanie",
                ),
                o("https://cdn.vita.sk/obrazok.png", "obrázok", ""),
                o("https://www.facebook.com/vita", "Facebook", ""),
                o("https://www.vita.sk/blog/clanok", "Blog", "Päta"),
            ],
            ramce: vec![],
            videa: vec![],
        };
        let k = kurz_zo_zberu(&z);
        assert_eq!(k.nazov, "AI grafika, dizajn a multimédiá");
        let mena: Vec<(&str, Vec<(&str, bool)>)> = k
            .kapitoly
            .iter()
            .map(|k| {
                (
                    k.nazov.as_str(),
                    k.lekcie
                        .iter()
                        .map(|l| (l.nazov.as_str(), l.odporucana))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            mena,
            vec![
                (
                    "Začíname",
                    vec![("Úvod do kurzu", true), ("Ako písať zadania", true)]
                ),
                ("Generovanie", vec![("Prvý obrázok", true)]),
                ("Päta", vec![("Blog", false)]),
            ]
        );
        let p = polozky(&k, "/tmp/out", "video");
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].nazov, "01 Úvod do kurzu");
        assert!(p[0]
            .priecinok
            .ends_with("AI grafika, dizajn a multimédiá/01 Začíname"));
        assert!(p[2].priecinok.ends_with("02 Generovanie") && p[2].nazov == "01 Prvý obrázok");
        assert!(p.iter().all(|x| x.prihlasenie && x.referer == z.url));
    }

    #[test]
    fn video_na_samotnej_stranke() {
        let z = Zber {
            url: "https://kurzy.example.com/lekcia/5".into(),
            titulok: "Lekcia 5".into(),
            h1: String::new(),
            odkazy: vec![],
            ramce: vec!["https://player.vimeo.com/video/1".into()],
            videa: vec![],
        };
        let k = kurz_zo_zberu(&z);
        assert_eq!(k.kapitoly.len(), 1);
        assert_eq!(
            k.kapitoly[0].lekcie[0].url,
            "https://kurzy.example.com/lekcia/5"
        );
        assert!(precitaj_zber("\"{\\\"url\\\":\\\"https://a.sk/\\\"}\"").is_some());
        assert!(precitaj_zber("{\"url\":\"https://a.sk/\",\"odkazy\":[]}").is_some());
    }
}
