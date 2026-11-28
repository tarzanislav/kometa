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
/// vložené prehrávače a videá. Pri každom odkaze aj to, či je v menu/päte stránky, dĺžka
/// v jeho riadku („23:10“, „20 minút“ – tak sa poznajú lekcie v osnove), číslo zoznamu, v ktorom
/// leží, a či vedie na práve otvorenú stránku. Nič nemení, len číta.
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
  const menu = 'header,footer,[role="banner"],[role="contentinfo"],[class*="menu"],[id*="menu"],[class*="navbar"],[class*="footer"],[class*="header"],[class*="breadcrumb"],[class*="cookie"]';
  const cas = /(?:^|[^\d:])(\d{1,2}:\d{2}(?::\d{2})?)(?![\d:])|(\d{1,3})\s*(?:min(?:út|úty|uta|uty|ut)?|mins?|minutes?)\b/i;
  const zoznamy = new Map();
  const zoznam = (a) => {
    let el = a.parentElement;
    for (let i = 0; el && i < 8; i++, el = el.parentElement) {
      if (el.querySelectorAll('a[href]').length >= 3) {
        if (!zoznamy.has(el)) zoznamy.set(el, zoznamy.size + 1);
        return zoznamy.get(el);
      }
    }
    return 0;
  };
  // riadok odkazu: najväčší obal, v ktorom nie je odkaz nikam inam (obrázok aj názov lekcie
  // často vedú na tú istú adresu)
  const riadok = (a) => {
    let el = a;
    for (let i = 0; el.parentElement && i < 5; i++) {
      const ciele = new Set([...el.parentElement.querySelectorAll('a[href]')].map((x) => x.href.split('#')[0]));
      if (ciele.size > 1) break;
      el = el.parentElement;
    }
    return el;
  };
  const bez = (u) => u.split('#')[0].replace(/\/$/, '');
  const tu = bez(location.href);
  const odkazy = [];
  for (const a of document.querySelectorAll('a[href]')) {
    if (odkazy.length >= 3000) break;
    const href = abs(a.getAttribute('href'));
    if (!/^https?:/.test(href)) continue;
    const text = txt(a) || a.getAttribute('title') || a.getAttribute('aria-label') || '';
    const m = cas.exec(txt(riadok(a)));
    odkazy.push({
      href, text: text.slice(0, 200), kapitola: kapitola(a).slice(0, 200),
      menu: !!a.closest(menu), dlzka: m ? (m[1] || m[2] + ' min') : '', zoznam: zoznam(a), aktivna: bez(href) === tu,
    });
  }
  const ramce = [...document.querySelectorAll('iframe[src]')].map((f) => abs(f.getAttribute('src'))).filter(Boolean);
  const videa = [...document.querySelectorAll('video[src],video source[src]')].map((v) => abs(v.getAttribute('src'))).filter(Boolean);
  const h1 = document.querySelector('h1');
  const kc = /^(?:\d{1,2}:\d{2}(?::\d{2})?|\d{1,3} ?(?:min(?:út|úty|uta|uty|ut)?|mins?|minutes?)\.?)$/i;
  const bunky = [...document.querySelectorAll('body *')].filter((el) => !el.closest('script,style,noscript,footer')
    && kc.test((el.textContent || '').replace(/\s+/g, ' ').trim()));
  const dlzok = bunky.filter((el) => !bunky.some((x) => x !== el && el.contains(x))).length;
  return { url: location.href, titulok: document.title, h1: h1 ? txt(h1) : '', odkazy, ramce, videa, dlzok };
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
    /// počet dĺžok lekcií na stránke („26:40“, „20 minút“)
    #[serde(default)]
    pub dlzok: usize,
}

#[derive(Debug, Default, Deserialize)]
pub struct OdkazZberu {
    pub href: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kapitola: String,
    /// v menu, hlavičke alebo päte stránky
    #[serde(default)]
    pub menu: bool,
    /// dĺžka v riadku odkazu („23:10“, „20 min“)
    #[serde(default)]
    pub dlzka: String,
    /// číslo zoznamu odkazov, v ktorom leží (0 = žiadny)
    #[serde(default)]
    pub zoznam: u32,
    /// vedie na práve otvorenú stránku
    #[serde(default)]
    pub aktivna: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Lekcia {
    pub url: String,
    pub nazov: String,
    /// predvybraná (vyzerá ako lekcia tohto kurzu)
    #[serde(default)]
    pub odporucana: bool,
    /// materiál na stiahnutie (PDF, prezentácia, archív…), nie stránka s videom
    #[serde(default)]
    pub material: bool,
    /// dĺžka lekcie, ako ju ukazuje stránka
    #[serde(default)]
    pub dlzka: String,
    /// z prechodu osnovy: „video“, „text“, „test“, „material“, „nezistene“ (prázdne = stránka)
    #[serde(default)]
    pub druh: String,
    /// adresa prehrávača videa (z prechodu osnovy); prázdna = sťahuje sa stránka lekcie
    #[serde(default)]
    pub zdroj: String,
    /// text alebo test lekcie uložený pri prechode (zapíše sa ako stránka HTML)
    #[serde(default)]
    pub obsah_id: String,
    /// poradie lekcie v kapitole na stránke (od 1) – číslo v názve súboru sedí, aj keď sa
    /// sťahuje len časť kapitoly; 0 = podľa poradia vo výbere
    #[serde(default, skip_serializing_if = "je_nula")]
    pub cislo: usize,
}

fn je_nula(n: &usize) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Kapitola {
    pub nazov: String,
    pub lekcie: Vec<Lekcia>,
    /// odkazy z menu a päty stránky – nie sú súčasťou kurzu, nikdy sa neoznačia samy
    #[serde(default)]
    pub menu: bool,
    /// poradie kapitoly v kurze (od 1); 0 = podľa poradia vo výbere
    #[serde(default, skip_serializing_if = "je_nula")]
    pub cislo: usize,
}

/// Názov časti s odkazmi z menu a päty stránky.
pub const KAPITOLA_MENU: &str = "Odkazy z menu a päty stránky";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Kurz {
    pub nazov: String,
    pub adresa: String,
    pub kapitoly: Vec<Kapitola>,
    /// koľko riadkov s dĺžkou (lekcií osnovy) je na stránke – keď ich je viac než načítaných
    /// lekcií, lekcie sa prepínajú kliknutím a treba prechod osnovy
    #[serde(default)]
    pub riadkov_osnovy: usize,
    /// koľko kapitol má celý kurz (pri sťahovaní časti kurzu: kapitola dostane vlastný
    /// priečinok, aj keď sa práve sťahuje len jedna); 0 = podľa výberu
    #[serde(default, skip_serializing_if = "je_nula")]
    pub kapitol: usize,
}

/// Lišta prehliadača kurzov (Späť, Dopredu, Obnoviť, adresa) – skript, ktorý aplikácia pridá
/// do každej stránky v okne prehliadača kurzov. V stránke nemá žiadny prístup ku Komete.
pub const LISTA_PREHLIADACA: &str = include_str!("lista_prehliadaca.js");

/// Priečinok so všetkými kurzami v priečinku na sťahovanie.
pub const PRIECINOK_KURZOV: &str = "KURZY";
/// Priečinok kurzu na materiály s odkazom (.txt) a súbory stiahnuté v prehliadači kurzov.
pub const PRIECINOK_MATERIALOV: &str = "MATERIALY";

/// Voľný názov súboru v priečinku: keď už súbor s tým menom je (alebo sa práve sťahuje),
/// dostane na koniec číslo – „Materiály.zip“, „Materiály (2).zip“…
pub fn volny_subor(
    priecinok: &std::path::Path,
    navrh: &str,
    obsadeny: &dyn Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let meno = zdroje::bezpecny_nazov(navrh, 180);
    let meno = if meno.is_empty() {
        "stiahnuté".to_string()
    } else {
        meno
    };
    let (kmen, pripona) = match meno.rsplit_once('.') {
        Some((k, p)) if !k.is_empty() && (1..=8).contains(&p.len()) && !p.contains(' ') => {
            (k.to_string(), format!(".{p}"))
        }
        _ => (meno.clone(), String::new()),
    };
    let mut c = priecinok.join(&meno);
    let mut n = 2;
    while c.exists() || obsadeny(&c) {
        c = priecinok.join(format!("{kmen} ({n}){pripona}"));
        n += 1;
    }
    c
}

/// Odkazy (http/https) v textovom súbore – materiál „odkaz vo vnútri súboru“.
pub fn odkazy_v_texte(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for slovo in text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')) {
        let slovo = slovo.trim_end_matches(['.', ',', ';', ')', ']']);
        if (slovo.starts_with("https://") || slovo.starts_with("http://"))
            && url::Url::parse(slovo).is_ok()
            && !out.iter().any(|x| x == slovo)
        {
            out.push(slovo.to_string());
        }
    }
    out
}

/// Názov kurzu bez mena portálu na konci („Excel pre pokročilých | Vita“ → „Excel pre
/// pokročilých“). Koniec sa odreže, len keď v ňom je meno stránky – „Python – základy“ ostane.
pub fn cisty_nazov(nazov: &str, adresa: &str) -> String {
    let nazov = nazov.trim();
    // mená stránky z adresy: „kurzy.vita.sk“ → „kurzy“, „vita“ (bez koncovky domény)
    let host = hostitel(adresa).unwrap_or_default();
    let casti: Vec<&str> = host.split('.').collect();
    let mena: Vec<&str> = casti[..casti.len().saturating_sub(1)]
        .iter()
        .copied()
        .filter(|m| m.len() >= 3)
        .collect();
    if mena.is_empty() {
        return nazov.to_string();
    }
    for odd in [" | ", " – ", " — ", " - ", " :: "] {
        if let Some((zaciatok, koniec)) = nazov.rsplit_once(odd) {
            let k = koniec.to_lowercase();
            if !zaciatok.trim().is_empty()
                && mena.iter().any(|m| k.contains(m))
                && k.chars().count() <= 40
            {
                return cisty_nazov(zaciatok, adresa);
            }
        }
    }
    nazov.to_string()
}

/// Priečinok kurzu: `<sťahovanie>/KURZY/<názov kurzu>`.
pub fn koren_kurzu(kurz: &Kurz, vystup: &str) -> std::path::PathBuf {
    let nazov = cisty_nazov(&kurz.nazov, &kurz.adresa);
    let nazov = if nazov.is_empty() {
        "Kurz".to_string()
    } else {
        nazov
    };
    std::path::Path::new(vystup)
        .join(PRIECINOK_KURZOV)
        .join(zdroje::bezpecny_nazov(&nazov, 100))
}

/// Miesto lekcie na disku: priečinok kapitoly, názov súboru (bez prípony; materiál s ňou)
/// a kľúč lekcie v prehľade kurzu.
pub struct Miesto<'a> {
    pub kapitola: &'a Kapitola,
    pub cislo_kapitoly: usize,
    pub priecinok: std::path::PathBuf,
    /// priečinok kapitoly voči priečinku kurzu („“ = priamo v ňom)
    pub relativny: String,
    pub lekcia: &'a Lekcia,
    pub cislo: usize,
    /// „03 Premenné“, materiál „04 Pracovný list.pdf“
    pub nazov: String,
    /// „02 Základy/03 Premenné“ – jednoznačný v rámci kurzu
    pub kluc: String,
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
const SUBORY: [&str; 10] = [
    ".jpg", ".jpeg", ".png", ".gif", ".svg", ".webp", ".css", ".js", ".ico", ".xml",
];
/// Materiály kurzu (súbory na stiahnutie).
const MATERIALY: [&str; 21] = [
    "pdf", "doc", "docx", "ppt", "pptx", "pps", "ppsx", "xls", "xlsx", "odt", "ods", "odp", "rtf",
    "txt", "epub", "zip", "rar", "7z", "mp3", "m4a", "wav",
];

/// Prípona materiálu z cesty odkazu („…/pracovny-list.pdf“ → „pdf“).
pub fn pripona_materialu(u: &url::Url) -> Option<&'static str> {
    let posledny = u.path().rsplit('/').next()?.to_ascii_lowercase();
    let (_, p) = posledny.rsplit_once('.')?;
    MATERIALY.iter().find(|m| **m == p).copied()
}

/// Zo zberu stránky poskladá kurz: lekcie a materiály zoskupené podľa kapitol.
///
/// Čo je lekcia: najsilnejší znak je **osnova** – zoznam odkazov, pri ktorých stránka ukazuje
/// dĺžku („23:10“, „20 minút“), alebo zoznam s práve otvorenou lekciou. Keď osnova je, odporúčané
/// sú len jej lekcie. Inak rozhodnú slová v adrese (lekcia, kapitola…) a cesta pod kurzom.
/// Odkazy z menu a päty stránky (ostatné kurzy, archívy, bonusy) idú do zvláštnej časti, ktorá
/// sa nikdy neoznačí sama. Materiály (PDF, prezentácie, archívy) môžu byť aj na inej doméne.
pub fn kurz_zo_zberu(z: &Zber) -> Kurz {
    let stranka = url::Url::parse(&z.url).ok();
    let host = stranka
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string();
    // „priečinok“ kurzu – lekcie často ležia pod jeho cestou; jeden diel cesty („/kurzy“) je
    // zvyčajne katalóg všetkých kurzov, ten za koreň nestačí
    let koren_cesty = stranka
        .as_ref()
        .map(|u| {
            let c = u.path().trim_end_matches('/');
            let rodic = c.rsplit_once('/').map(|x| x.0).unwrap_or("");
            if rodic.matches('/').count() >= 2 {
                rodic.to_string()
            } else {
                c.to_string()
            }
        })
        .unwrap_or_default();
    // osnova: zoznamy s dĺžkami lekcií alebo s práve otvorenou lekciou (nie v menu)
    let mut dlzky: std::collections::HashMap<u32, usize> = Default::default();
    let mut osnova: HashSet<u32> = HashSet::new();
    for o in z.odkazy.iter().filter(|o| o.zoznam > 0) {
        if !o.dlzka.is_empty() {
            *dlzky.entry(o.zoznam).or_default() += 1;
        }
        if o.aktivna && !o.menu {
            osnova.insert(o.zoznam);
        }
    }
    osnova.extend(dlzky.iter().filter(|(_, n)| **n >= 2).map(|(z, _)| *z));
    // ten istý odkaz býva v menu aj v obsahu – berie sa výskyt mimo menu, s dĺžkou, najskôr prvý
    let mut najlepsi: std::collections::HashMap<String, usize> = Default::default();
    for (i, o) in z.odkazy.iter().enumerate() {
        let kluc = bez_hash(&o.href).to_string();
        let skore = |o: &OdkazZberu| (!o.menu, !o.dlzka.is_empty());
        match najlepsi.get(&kluc) {
            Some(&j) if skore(&z.odkazy[j]) >= skore(o) => {}
            _ => {
                najlepsi.insert(kluc, i);
            }
        }
    }
    let mut videne: HashSet<String> = HashSet::new();
    let mut kapitoly: Vec<Kapitola> = vec![];
    // otvorená stránka je lekcia z osnovy – pôjde s ostatnými na svojom mieste
    let tu_v_osnove = z
        .odkazy
        .iter()
        .any(|o| o.aktivna && o.zoznam > 0 && osnova.contains(&o.zoznam));
    if !tu_v_osnove {
        videne.insert(bez_hash(&z.url).to_string());
    }
    // video priamo na stránke kurzu
    if !tu_v_osnove && (!z.ramce.is_empty() || !z.videa.is_empty()) {
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
                ..Default::default()
            }],
            menu: false,
            cislo: 0,
        });
    }
    for (i, o) in z.odkazy.iter().enumerate() {
        let Ok(u) = url::Url::parse(&o.href) else {
            continue;
        };
        let kluc = bez_hash(&o.href).to_string();
        if najlepsi.get(&kluc) != Some(&i) || !videne.insert(kluc.clone()) {
            continue;
        }
        let h = u.host_str().unwrap_or("").trim_start_matches("www.");
        let doma = h == host || h.ends_with(&format!(".{host}"));
        let cesta = u.path().to_lowercase();
        let pripona = pripona_materialu(&u);
        let material = pripona.is_some();
        // materiál môže byť na úložisku inej domény, lekcia nie
        if (!doma && !material)
            || cesta.is_empty()
            || cesta == "/"
            || SUBORY.iter().any(|s| cesta.ends_with(s))
            || NIE.iter().any(|n| cesta.contains(n))
        {
            continue;
        }
        let mut text = o.text.trim().to_string();
        if text.is_empty() && material {
            text = super::stahovanie::nazov_z_adresy(&o.href);
        }
        if text.is_empty() {
            continue;
        }
        let v_osnove = o.zoznam > 0 && osnova.contains(&o.zoznam);
        let menu = o.menu && !v_osnove && o.dlzka.is_empty();
        let odporucana = !menu
            && if material {
                true
            } else if !osnova.is_empty() {
                v_osnove || !o.dlzka.is_empty()
            } else {
                ANO.iter().any(|a| cesta.contains(a))
                    || (koren_cesty.len() > 1 && u.path().starts_with(&format!("{koren_cesty}/")))
            };
        let nazov_kap = if menu {
            KAPITOLA_MENU.to_string()
        } else {
            o.kapitola.trim().to_string()
        };
        let k = match kapitoly
            .iter_mut()
            .position(|k| k.nazov == nazov_kap && k.menu == menu)
        {
            Some(i) => &mut kapitoly[i],
            None => {
                kapitoly.push(Kapitola {
                    nazov: nazov_kap,
                    lekcie: vec![],
                    menu,
                    cislo: 0,
                });
                kapitoly.last_mut().unwrap()
            }
        };
        k.lekcie.push(Lekcia {
            url: kluc,
            nazov: text.chars().take(150).collect(),
            odporucana,
            material,
            dlzka: o.dlzka.chars().take(20).collect(),
            ..Default::default()
        });
    }
    // kapitoly s odporúčanými lekciami dopredu, ostatné potom, menu a päta úplne na koniec
    kapitoly.sort_by_key(|k| {
        (
            k.menu,
            !k.lekcie.iter().any(|l| l.odporucana),
            k.lekcie.iter().all(|l| l.material), // materiály za lekciami
        )
    });
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
        riadkov_osnovy: z.dlzok,
        kapitol: 0,
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

/// Kde bude každá vybraná lekcia: priečinok kurzu, podpriečinky kapitol (pri kurze s jednou
/// kapitolou rovno priečinok kurzu), čísla podľa poradia na stránke.
pub fn plan<'a>(kurz: &'a Kurz, vystup: &str) -> (std::path::PathBuf, Vec<Miesto<'a>>) {
    let koren = koren_kurzu(kurz, vystup);
    let kapitoly: Vec<&Kapitola> = kurz
        .kapitoly
        .iter()
        .filter(|k| !k.lekcie.is_empty())
        .collect();
    let jedna = if kurz.kapitol > 0 {
        kurz.kapitol <= 1
    } else {
        kapitoly.len() <= 1
    };
    let mut out = vec![];
    for (ki, k) in kapitoly.into_iter().enumerate() {
        let cislo_kapitoly = if k.cislo > 0 { k.cislo } else { ki + 1 };
        let relativny = if jedna {
            String::new()
        } else {
            let meno = if k.nazov.trim().is_empty() {
                format!("{cislo_kapitoly:02}")
            } else {
                format!("{cislo_kapitoly:02} {}", k.nazov.trim())
            };
            zdroje::bezpecny_nazov(&meno, 100)
        };
        let priecinok = if relativny.is_empty() {
            koren.clone()
        } else {
            koren.join(&relativny)
        };
        for (li, l) in k.lekcie.iter().enumerate() {
            let cislo = if l.cislo > 0 { l.cislo } else { li + 1 };
            let mut nazov = format!("{cislo:02} {}", l.nazov.trim());
            // materiál si nechá svoju príponu („03 Pracovný list.pdf“)
            let pripona = url::Url::parse(&l.url)
                .ok()
                .as_ref()
                .and_then(pripona_materialu);
            if let Some(p) = pripona {
                if !nazov.to_ascii_lowercase().ends_with(&format!(".{p}")) {
                    nazov = format!("{nazov}.{p}");
                }
            }
            // textový súbor s odkazom na zdieľaný priečinok: do MATERIALY kurzu
            let priecinok = if pripona == Some("txt") {
                koren.join(PRIECINOK_MATERIALOV)
            } else {
                priecinok.clone()
            };
            // kľúč = cesta k súboru bez prípony (názov ako na disku, bez „/“ a pod.)
            let kmen = zdroje::bezpecny_nazov(&nazov, 180);
            let kluc = if relativny.is_empty() {
                kmen
            } else {
                format!("{relativny}/{kmen}")
            };
            out.push(Miesto {
                kapitola: k,
                cislo_kapitoly,
                priecinok,
                relativny: relativny.clone(),
                lekcia: l,
                cislo,
                nazov,
                kluc,
            });
        }
    }
    (koren, out)
}

/// Text alebo test lekcie z prechodu osnovy (zapíše sa ako stránka HTML, nejde do frontu).
pub fn je_text(l: &Lekcia) -> bool {
    !l.obsah_id.is_empty()
}

/// Uloží texty a testy lekcií ako stránky HTML do priečinkov kapitol. Vráti kľúč lekcie a súbor.
pub fn uloz_texty(miesta: &[Miesto]) -> Vec<(String, std::path::PathBuf)> {
    let mut out = vec![];
    for m in miesta.iter().filter(|m| je_text(m.lekcia)) {
        let Some(html) = super::osnova::obsah(&m.lekcia.obsah_id) else {
            continue;
        };
        let meno = zdroje::bezpecny_nazov(&m.nazov, 150);
        // ten istý text pri opakovanom sťahovaní sa prepíše, nie zdvojí
        let cesta = m.priecinok.join(format!("{meno}.html"));
        if std::fs::create_dir_all(&m.priecinok).is_ok() && std::fs::write(&cesta, html).is_ok() {
            out.push((m.kluc.clone(), cesta));
        }
    }
    out
}

/// Položky frontu pre vybrané lekcie (bez textov – tie sa uložia hneď).
pub fn polozky(miesta: &[Miesto], kurz: &Kurz, format: &str, setrne: bool) -> Vec<Nova> {
    miesta
        .iter()
        .filter(|m| !je_text(m.lekcia))
        .map(|m| {
            let l = m.lekcia;
            // video z prechodu osnovy: priamo prehrávač, stránka lekcie ako odkiaľ prišiel
            let (url, referer) = if l.zdroj.is_empty() {
                (l.url.clone(), kurz.adresa.clone())
            } else {
                (l.zdroj.clone(), bez_hash(&l.url).to_string())
            };
            // materiál (PDF, odkaz v .txt…) sa stiahne ako súbor, nehľadá sa v ňom video
            let material = l.material
                || url::Url::parse(&l.url)
                    .ok()
                    .as_ref()
                    .and_then(pripona_materialu)
                    .is_some();
            Nova {
                url,
                format: if material && l.zdroj.is_empty() {
                    "subor".to_string()
                } else {
                    format.to_string()
                },
                priecinok: m.priecinok.to_string_lossy().into_owned(),
                nazov: m.nazov.clone(),
                zdroj: "Kurz".into(),
                referer,
                prihlasenie: true,
                setrne,
                lekcia: m.kluc.clone(),
                ..Default::default()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Moje stránky s kurzami: záložky a prihlásenie uložené v trezore systému
// ---------------------------------------------------------------------------------------------

/// Stránka, na ktorej máš kurzy. Heslo nie je tu, ale v trezore systému (Správca poverení
/// vo Windows, Kľúčenka v macOS, Secret Service v Linuxe); bez trezoru v `ucty.dat`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Zalozka {
    #[serde(default)]
    pub id: String,
    pub nazov: String,
    pub adresa: String,
    /// prihlasovacie meno alebo e-mail
    #[serde(default)]
    pub meno: String,
}

fn subor_zaloziek() -> std::path::PathBuf {
    super::nastavenia::priecinok_nastaveni().join("kurzy.json")
}

/// Hostiteľ stránky bez „www.“ – podľa neho sa záložka spáruje s otvorenou stránkou.
pub fn hostitel(adresa: &str) -> Option<String> {
    let u = url::Url::parse(adresa).ok()?;
    let h = u
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    (!h.is_empty()).then_some(h)
}

/// Patrí stránka (hostiteľ) k záložke – tá istá doména alebo jej poddoména (napr. prihlásenie
/// na „konto.portal.sk“ pri záložke „portal.sk“)?
pub fn patri(stranka: &str, zalozka: &str) -> bool {
    stranka == zalozka || stranka.ends_with(&format!(".{zalozka}"))
}

/// Názov v trezore pre heslo k stránke.
pub fn sluzba_hesla(host: &str) -> String {
    format!("kurz:{host}")
}

/// `kurzy.json`: „zalozky“ (stránky s kurzami) a „vybrane“ (konkrétne kurzy).
fn citaj_kurzy_json() -> Value {
    std::fs::read_to_string(subor_zaloziek())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Zapíše jeden kľúč `kurzy.json`, ostatné nechá.
fn zapis_kurzy_json(kluc: &str, hodnota: Value) -> Result<(), String> {
    let mut v = citaj_kurzy_json();
    v[kluc] = hodnota;
    let t = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    let cesta = subor_zaloziek();
    if let Some(d) = cesta.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let docasny = cesta.with_extension("json.novy");
    std::fs::write(&docasny, t).map_err(|e| format!("Záložky sa nepodarilo uložiť: {e}"))?;
    std::fs::rename(&docasny, &cesta).map_err(|e| format!("Záložky sa nepodarilo uložiť: {e}"))
}

pub fn zalozky() -> Vec<Zalozka> {
    serde_json::from_value(citaj_kurzy_json()["zalozky"].clone()).unwrap_or_default()
}

fn uloz_zalozky(z: &[Zalozka]) -> Result<(), String> {
    zapis_kurzy_json("zalozky", serde_json::json!(z))
}

// ---------------------------------------------------------------------------------------------
// Vybrané kurzy: konkrétne stránky kurzov (osnova), aby sa nemuselo preklikávať
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VybranyKurz {
    pub id: String,
    pub nazov: String,
    pub adresa: String,
    #[serde(default)]
    pub pridane: f64,
}

pub fn vybrane() -> Vec<VybranyKurz> {
    serde_json::from_value(citaj_kurzy_json()["vybrane"].clone()).unwrap_or_default()
}

/// Adresa kurzu na porovnanie: bez vnútornej značky lekcie a bez „/“ na konci.
fn adresa_kurzu(a: &str) -> String {
    a.trim()
        .split("#kometa-lekcia-")
        .next()
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string()
}

/// Pridá kurz do Vybraných (ten istý kurz sa nezdvojí – len sa premenuje a posunie navrch).
pub fn uloz_vybrany(nazov: &str, adresa: &str) -> Result<(), String> {
    let adresa = adresa_kurzu(adresa);
    let host = hostitel(&adresa)
        .filter(|_| adresa.starts_with("http://") || adresa.starts_with("https://"))
        .ok_or("Kurz sa dá uložiť, keď je v prehliadači otvorená stránka kurzu.")?;
    let mut vsetky = vybrane();
    let stary = vsetky
        .iter()
        .position(|k| adresa_kurzu(&k.adresa) == adresa)
        .map(|i| vsetky.remove(i));
    if stary.is_none() && vsetky.len() >= 300 {
        return Err("Vybraných kurzov je už priveľa (300) – niektoré odstráň.".into());
    }
    let nazov: String = match cisty_nazov(nazov, &adresa).as_str() {
        "" => host,
        n => n.chars().take(150).collect(),
    };
    vsetky.insert(
        0,
        VybranyKurz {
            id: stary
                .as_ref()
                .map(|k| k.id.clone())
                .unwrap_or_else(super::fronta::nove_id),
            nazov,
            adresa,
            pridane: stary
                .map(|k| k.pridane)
                .unwrap_or_else(super::fronta::teraz),
        },
    );
    zapis_kurzy_json("vybrane", serde_json::json!(vsetky))
}

/// Odstráni vybrané kurzy podľa id (`None` = všetky).
pub fn zmaz_vybrane(ids: Option<&[String]>) -> Result<(), String> {
    let zvysne: Vec<VybranyKurz> = match ids {
        None => vec![],
        Some(ids) => vybrane()
            .into_iter()
            .filter(|k| !ids.contains(&k.id))
            .collect(),
    };
    zapis_kurzy_json("vybrane", serde_json::json!(zvysne))
}

pub fn vybrane_pre_okno() -> Value {
    let zalozky = zalozky();
    Value::Array(
        vybrane()
            .into_iter()
            .map(|k| {
                let host = hostitel(&k.adresa);
                // stránka s uloženým prihlásením, ku ktorej kurz patrí
                let stranka = host.as_ref().and_then(|h| {
                    zalozky
                        .iter()
                        .filter(|z| hostitel(&z.adresa).is_some_and(|zh| patri(h, &zh)))
                        .max_by_key(|z| hostitel(&z.adresa).map_or(0, |x| x.len()))
                });
                let s_heslom = stranka.is_some_and(|z| {
                    !z.meno.is_empty()
                        && hostitel(&z.adresa).is_some_and(|zh| {
                            super::ucty::heslo(&sluzba_hesla(&zh), &z.meno).is_some()
                        })
                });
                serde_json::json!({
                    "id": k.id, "nazov": k.nazov, "adresa": k.adresa, "host": host,
                    "stranka": stranka.map(|z| z.nazov.clone()), "s_heslom": s_heslom,
                })
            })
            .collect(),
    )
}

/// Stiahnuté kurzy v priečinku KURZY (podľa `kometa-kurz.json`), naposledy zmenené prvé.
pub fn stiahnute(vystup: &str) -> Value {
    let koren = std::path::Path::new(vystup).join(PRIECINOK_KURZOV);
    let mut out: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(&koren)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let d = e.path();
            let manifest = d.join(super::prehlad::MANIFEST);
            let cas = std::fs::metadata(&manifest)
                .and_then(|m| m.modified())
                .ok()?;
            let m: super::prehlad::Manifest =
                serde_json::from_str(&std::fs::read_to_string(&manifest).ok()?).ok()?;
            let lekcie: Vec<&super::prehlad::MLekcia> =
                m.kapitoly.iter().flat_map(|k| k.lekcie.iter()).collect();
            let stranka = d.join(super::prehlad::STRANKA);
            Some((
                cas,
                serde_json::json!({
                    "nazov": m.nazov, "adresa": m.adresa,
                    "lekcii": lekcie.len(),
                    "stiahnutych": lekcie.iter().filter(|l| !l.subor.is_empty()).count(),
                    "priecinok": d, "prehlad": stranka.is_file().then_some(stranka),
                }),
            ))
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.0));
    Value::Array(out.into_iter().take(50).map(|x| x.1).collect())
}

/// Záložky pre okno: bez hesiel, len či je heslo uložené.
pub fn zalozky_pre_okno() -> Value {
    Value::Array(
        zalozky()
            .into_iter()
            .map(|z| {
                let s_heslom = !z.meno.is_empty()
                    && hostitel(&z.adresa)
                        .is_some_and(|h| super::ucty::heslo(&sluzba_hesla(&h), &z.meno).is_some());
                serde_json::json!({"id": z.id, "nazov": z.nazov, "adresa": z.adresa, "meno": z.meno,
                                   "host": hostitel(&z.adresa), "s_heslom": s_heslom})
            })
            .collect(),
    )
}

/// Pridá alebo upraví záložku. `heslo`: `None` = nechať, prázdne = zmazať, inak uložiť do trezoru.
pub fn uloz_zalozku(nova: Zalozka, heslo: Option<&str>) -> Result<(), String> {
    let adresa = nova.adresa.trim();
    let adresa = if adresa.starts_with("http://") || adresa.starts_with("https://") {
        adresa.to_string()
    } else {
        format!("https://{adresa}")
    };
    let host = hostitel(&adresa).ok_or("Zadaj adresu stránky, napríklad https://www.vita.sk")?;
    let mut vsetky = zalozky();
    if vsetky.len() >= 200 && nova.id.is_empty() {
        return Err("Záložiek je už priveľa (200).".into());
    }
    let nazov: String = match nova.nazov.trim() {
        "" => host.clone(),
        n => n.chars().take(100).collect(),
    };
    let meno: String = nova.meno.trim().chars().take(200).collect();
    let mut z = Zalozka {
        id: nova.id.clone(),
        nazov,
        adresa,
        meno,
    };
    // pôvodné heslo ide s ňou, keď sa zmení meno alebo stránka
    let povodna = vsetky.iter().position(|x| !z.id.is_empty() && x.id == z.id);
    if let Some(i) = povodna {
        let stara = vsetky[i].clone();
        let stary_host = hostitel(&stara.adresa).unwrap_or_default();
        if (stara.meno != z.meno || stary_host != host) && !stara.meno.is_empty() {
            let stare = super::ucty::heslo(&sluzba_hesla(&stary_host), &stara.meno);
            super::ucty::zmaz_heslo(&sluzba_hesla(&stary_host), &stara.meno);
            if let (Some(h), None, false) = (stare, heslo, z.meno.is_empty()) {
                super::ucty::uloz_heslo(&sluzba_hesla(&host), &z.meno, &h);
            }
        }
        vsetky[i] = z.clone();
    } else {
        z.id = super::fronta::nove_id();
        vsetky.push(z.clone());
    }
    match heslo {
        Some(h) if !h.is_empty() && !z.meno.is_empty() => {
            super::ucty::uloz_heslo(&sluzba_hesla(&host), &z.meno, h)
        }
        Some(_) if !z.meno.is_empty() => super::ucty::zmaz_heslo(&sluzba_hesla(&host), &z.meno),
        _ => {}
    }
    uloz_zalozky(&vsetky)
}

/// Zmaže záložku aj jej heslo z trezoru.
pub fn zmaz_zalozku(id: &str) -> Result<(), String> {
    let mut vsetky = zalozky();
    if let Some(i) = vsetky.iter().position(|z| z.id == id) {
        let z = vsetky.remove(i);
        if let (Some(h), false) = (hostitel(&z.adresa), z.meno.is_empty()) {
            super::ucty::zmaz_heslo(&sluzba_hesla(&h), &z.meno);
        }
    }
    uloz_zalozky(&vsetky)
}

/// Záložka, ku ktorej patrí otvorená stránka.
pub fn zalozka_pre_stranku(adresa: &str) -> Option<Zalozka> {
    let h = hostitel(adresa)?;
    zalozky()
        .into_iter()
        .filter(|z| hostitel(&z.adresa).is_some_and(|zh| patri(&h, &zh)))
        // najpresnejšia (najdlhšia) doména vyhráva
        .max_by_key(|z| hostitel(&z.adresa).map_or(0, |x| x.len()))
}

/// Skript, ktorý vyplní prihlasovací formulár na otvorenej stránke – len keď stránka patrí
/// k záložke (kontroluje to ešte raz priamo v stránke). Nič neodošle: prihlásiš sa ty.
const PRIHLAS_JS: &str = r#"((host, meno, heslo) => {
  /* kometaPrihlasenie */
  const h = location.hostname.replace(/^www\./, '').toLowerCase();
  if (!(h === host || h.endsWith('.' + host))) return { meno: false, heslo: false, chyba: 'ina-stranka' };
  const vidno = (el) => !!(el.offsetWidth || el.offsetHeight || el.getClientRects().length) && !el.disabled && !el.readOnly;
  const nastav = (el, v) => {
    const zapis = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    el.focus();
    zapis.call(el, v);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const pole = [...document.querySelectorAll('input[type="password"]')].find(vidno);
  const okruh = (pole && pole.form) || document;
  const textove = [...okruh.querySelectorAll('input')].filter((i) => vidno(i) && /^(email|text|tel|)$/i.test(i.getAttribute('type') || ''));
  const pred = pole ? textove.filter((i) => i.compareDocumentPosition(pole) & Node.DOCUMENT_POSITION_FOLLOWING) : textove;
  const vzor = /mail|user|login|meno|name|uzivatel|užívateľ|account|prihlas/i;
  const kandidati = pred.slice().reverse();
  const menoPole = kandidati.find((i) => vzor.test([i.name, i.id, i.getAttribute('autocomplete'), i.placeholder, i.type].join(' '))) || kandidati[0];
  if (menoPole && meno) nastav(menoPole, meno);
  if (pole) nastav(pole, heslo);
  if (pole) pole.focus();
  return { meno: !!(menoPole && meno), heslo: !!pole };
})(HOST, MENO, HESLO)"#;

/// Skript prihlásenia s hodnotami vloženými ako reťazce JSON (žiadny text z nich nie je kód).
pub fn skript_prihlasenia(host: &str, meno: &str, heslo: &str) -> String {
    let j = |t: &str| serde_json::to_string(t).unwrap_or_else(|_| "\"\"".into());
    PRIHLAS_JS.replacen(
        "HOST, MENO, HESLO",
        &format!("{}, {}, {}", j(host), j(meno), j(heslo)),
        1,
    )
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
            ..Default::default()
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
            dlzok: 0,
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
        let (koren, m) = plan(&k, "/tmp/out");
        assert!(koren.starts_with("/tmp/out/KURZY"));
        let p = polozky(&m, &k, "video", true);
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|x| x.setrne));
        assert_eq!(p[2].lekcia, "02 Generovanie/01 Prvý obrázok");
        assert_eq!(p[0].nazov, "01 Úvod do kurzu");
        let cesta = |x: &Nova| std::path::PathBuf::from(&x.priecinok);
        let kurz = std::path::Path::new("AI grafika, dizajn a multimédiá");
        assert!(cesta(&p[0]).ends_with(kurz.join("01 Začíname")));
        assert!(cesta(&p[2]).ends_with("02 Generovanie") && p[2].nazov == "01 Prvý obrázok");
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
            dlzok: 0,
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

    /// Stránka lekcie v LMS: osnova v bočnom paneli s dĺžkami, menu s inými kurzami a archívmi,
    /// materiál na inej doméne, odkaz „Pokračovať“ na ďalšiu lekciu.
    #[test]
    fn osnova_s_dlzkami_a_menu() {
        let zaklad = "https://www.akademia.sk/kurzy/ai-grafika";
        let menu = |href: &str, text: &str| OdkazZberu {
            href: href.into(),
            text: text.into(),
            menu: true,
            zoznam: 1,
            ..Default::default()
        };
        let lekcia = |cast: &str, text: &str, kap: &str, dlzka: &str, zoznam: u32| OdkazZberu {
            href: format!("{zaklad}/lekcia/{cast}"),
            text: text.into(),
            kapitola: kap.into(),
            dlzka: dlzka.into(),
            zoznam,
            aktivna: cast == "ako-funguje",
            ..Default::default()
        };
        let z = Zber {
            url: format!("{zaklad}/lekcia/ako-funguje"),
            titulok: "Online kurz AI Grafika".into(),
            h1: "Online kurz AI Grafika, Dizajn a Multimédia".into(),
            odkazy: vec![
                menu("https://www.akademia.sk/", "Home"),
                menu("https://www.akademia.sk/moje-bonusy", "Moje bonusy"),
                menu(
                    "https://www.akademia.sk/kategoria/tituly/",
                    "Online Profesijné Tituly archívy",
                ),
                menu(
                    "https://www.akademia.sk/kategoria/administracia/",
                    "Administrácia IT archívy",
                ),
                menu(&format!("{zaklad}/lekcia/ako-pouzivat-firefly"), "Firefly"),
                lekcia(
                    "ako-funguje",
                    "Ako funguje AI grafika",
                    "I. Základy",
                    "23:10",
                    7,
                ),
                lekcia(
                    "ai-v-adobe",
                    "AI grafika v Adobe",
                    "I. Základy",
                    "20 min",
                    7,
                ),
                lekcia(
                    "ako-pouzivat-firefly",
                    "Ako používať Adobe Firefly",
                    "I. Základy",
                    "02:14:15",
                    7,
                ),
                lekcia(
                    "zorientovat",
                    "Ako sa zorientovať",
                    "II. AI nástroje",
                    "01:03:14",
                    8,
                ),
                lekcia("dalle", "Obrázky v DALL·E", "II. AI nástroje", "35:57", 8),
                OdkazZberu {
                    href: format!("{zaklad}/lekcia/ai-v-adobe"),
                    text: "Pokračovať na ďalšiu časť".into(),
                    ..Default::default()
                },
                OdkazZberu {
                    href: format!("{zaklad}/materialy"),
                    text: "Materiály".into(),
                    kapitola: "O tomto kurze".into(),
                    zoznam: 9,
                    ..Default::default()
                },
                OdkazZberu {
                    href: "https://cdn.akademia.sk/subory/klavesove-skratky.pdf".into(),
                    text: String::new(),
                    kapitola: "Materiály".into(),
                    ..Default::default()
                },
                OdkazZberu {
                    href: "https://www.akademia.sk/subory/vop.pdf".into(),
                    text: "Podmienky".into(),
                    menu: true,
                    ..Default::default()
                },
            ],
            ramce: vec![],
            videa: vec![],
            dlzok: 0,
        };
        let k = kurz_zo_zberu(&z);
        type Riadok<'a> = (&'a str, bool, Vec<(&'a str, bool, bool)>);
        let prehlad: Vec<Riadok> = k
            .kapitoly
            .iter()
            .map(|x| {
                (
                    x.nazov.as_str(),
                    x.menu,
                    x.lekcie
                        .iter()
                        .map(|l| (l.nazov.as_str(), l.odporucana, l.material))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            prehlad,
            vec![
                (
                    "I. Základy",
                    false,
                    vec![
                        ("Ako funguje AI grafika", true, false),
                        ("AI grafika v Adobe", true, false),
                        ("Ako používať Adobe Firefly", true, false),
                    ]
                ),
                (
                    "II. AI nástroje",
                    false,
                    vec![
                        ("Ako sa zorientovať", true, false),
                        ("Obrázky v DALL·E", true, false)
                    ]
                ),
                (
                    "Materiály",
                    false,
                    vec![("klavesove-skratky.pdf", true, true)]
                ),
                ("O tomto kurze", false, vec![("Materiály", false, false)]),
                (
                    KAPITOLA_MENU,
                    true,
                    vec![
                        ("Moje bonusy", false, false),
                        ("Online Profesijné Tituly archívy", false, false),
                        ("Administrácia IT archívy", false, false),
                        ("Podmienky", false, true),
                    ]
                ),
            ]
        );
        assert_eq!(k.kapitoly[0].lekcie[0].dlzka, "23:10");
        // materiál si nechá príponu, lekcie nie
        let mut vyber = k.clone();
        vyber.kapitoly.truncate(3);
        let (_, m) = plan(&vyber, "/tmp/out");
        let p = polozky(&m, &vyber, "video", false);
        assert_eq!(p.last().unwrap().nazov, "01 klavesove-skratky.pdf");
        assert_eq!(p[0].nazov, "01 Ako funguje AI grafika");
    }

    #[test]
    fn volny_nazov_a_odkazy_v_texte() {
        let d = std::env::temp_dir().join(format!("kometa-volny-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Materiály.zip"), b"x").unwrap();
        let nic = |_: &std::path::Path| false;
        assert_eq!(
            volny_subor(&d, "Materiály.zip", &nic),
            d.join("Materiály (2).zip")
        );
        assert_eq!(volny_subor(&d, "nový.pdf", &nic), d.join("nový.pdf"));
        let obsadeny = |c: &std::path::Path| c.ends_with("Materiály (2).zip");
        assert_eq!(
            volny_subor(&d, "Materiály.zip", &obsadeny),
            d.join("Materiály (3).zip")
        );
        assert_eq!(volny_subor(&d, "a/b:c.zip", &nic), d.join("a-b -c.zip"));
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(
            odkazy_v_texte("Materiály (Odkaz vo vnútri súboru)\r\nhttps://a.example.com/:f:/g/x?e=1.\nznova https://a.example.com/:f:/g/x?e=1 a <http://b.example.com/y>"),
            ["https://a.example.com/:f:/g/x?e=1", "http://b.example.com/y"]
        );
        assert!(odkazy_v_texte("žiadny odkaz, ftp://x").is_empty());
    }

    #[test]
    fn material_s_odkazom_ide_do_materialov_a_stiahne_sa_ako_subor() {
        let u = "https://kurzy.example.com";
        let k = Kurz {
            nazov: "Kurz".into(),
            adresa: format!("{u}/kurz"),
            kapitoly: vec![Kapitola {
                nazov: "Úvod".into(),
                lekcie: vec![
                    Lekcia {
                        url: format!("{u}/l/1"),
                        nazov: "Video".into(),
                        ..Default::default()
                    },
                    Lekcia {
                        url: format!("{u}/m/odkaz.txt"),
                        nazov: "Materiály (odkaz)".into(),
                        material: true,
                        ..Default::default()
                    },
                    Lekcia {
                        url: format!("{u}/m/list.pdf"),
                        nazov: "List".into(),
                        material: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }],
            kapitol: 2,
            ..Default::default()
        };
        let (koren, m) = plan(&k, "/tmp/out");
        let p = polozky(&m, &k, "fhd", false);
        let kde: Vec<(&str, &str, String)> = p
            .iter()
            .map(|x| {
                (
                    x.nazov.as_str(),
                    x.format.as_str(),
                    std::path::Path::new(&x.priecinok)
                        .strip_prefix(&koren)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect();
        assert_eq!(
            kde,
            [
                ("01 Video", "fhd", "01 Úvod".to_string()),
                ("02 Materiály (odkaz).txt", "subor", "MATERIALY".to_string()),
                ("03 List.pdf", "subor", "01 Úvod".to_string()),
            ]
        );
    }

    #[test]
    fn prihlasenie_len_pre_svoju_stranku() {
        assert_eq!(
            hostitel("https://www.Vita.sk/kurz").as_deref(),
            Some("vita.sk")
        );
        assert!(patri("vita.sk", "vita.sk") && patri("konto.vita.sk", "vita.sk"));
        assert!(!patri("zlavita.sk", "vita.sk") && !patri("vita.sk.zle.com", "vita.sk"));
        // hodnoty sú v skripte ako reťazce JSON – úvodzovky ani </script> z nich kód neurobia
        let js = skript_prihlasenia("vita.sk", "ja@example.sk", "he\"slo');alert(1)//</script>");
        assert!(
            js.ends_with(r#"("vita.sk", "ja@example.sk", "he\"slo');alert(1)//</script>")"#),
            "{js}"
        );
        assert!(js.contains("kometaPrihlasenie") && !js.contains("HOST, MENO, HESLO"));
    }
}
