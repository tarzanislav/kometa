//! Služba pre okno Komety: to isté rozhranie (metódy a tvary dát), aké ponúka okno v Pythone
//! (`kometa/webapp.py`), ale celé v Ruste. Používa ho aplikácia Tauri aj `kometa okno`.
//!
//! Každé volanie vráti `{"ok": true, "data": …}` alebo `{"ok": false, "chyba": "…", "podrobnosti": "…"}`.

pub mod aktualizacie;
pub mod archivy;
pub mod doplnky;
pub mod fronta;
pub mod jazyky;
pub mod kurzy;
pub mod nastavenia;
pub mod nastroje;
pub mod prostredie;
pub mod server;
pub mod stahovanie;
pub mod strazca;
pub mod ucty;
pub mod ytdlp;
pub mod zdroje;

use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Udalost};
use crate::web::{skener::Skener, vzory};
use fronta::{Fronta, Nova, Polozka, Stav};
use nastavenia::Nastavenia;
use nastroje::Nastroje;
use prostredie::Prostredie;
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const VERZIA: &str = env!("CARGO_PKG_VERSION");

/// Metódy, ktoré smie volať okno (rovnaké mená ako v Pythone + doinštalovanie yt-dlp).
pub const METODY: &[&str] = &[
    "init",
    "stav",
    "aktualizacia_skontroluj",
    "aktualizacia_instaluj",
    "aktualizacia_pri_starte",
    "pridat",
    "rozpoznat",
    "kolekcia",
    "pridat_vybrane",
    "skenovat",
    "pridat_zo_skenu",
    "pozastavit",
    "spustit",
    "odstranit",
    "vycistit_hotove",
    "vycistit_front",
    "zadat_heslo",
    "rozbalit",
    "schranka_stiahnut",
    "schranka_zahodit",
    "kurz_otvorit",
    "kurz_nacitat",
    "kurz_stiahnut",
    "subory_na_zmazanie",
    "zmazat_subory",
    "pozastavit_vsetko",
    "spustit_vsetko",
    "otvorit",
    "otvorit_subor",
    "schranka",
    "jazyky_nacitat",
    "jazyky_zistit",
    "jazyky_stiahnut",
    "jazyky_zrusit",
    "vybrat_priecinok",
    "vybrat_video",
    "prihlasit",
    "odhlasit",
    "ulozit_nastavenia",
    "doinstalovat_ytdlp",
    "doinstalovat_ffmpeg",
];

/// Graf rýchlosti: jeden bod za sekundu, posledných 90 s.
struct Graf {
    body: VecDeque<f64>,
    posledny: Instant,
}

#[derive(Default, Clone)]
struct Instalacia {
    /// čo sa inštaluje: „yt-dlp“ alebo „FFmpeg“
    co: &'static str,
    bezi: bool,
    text: String,
    podiel: f64,
}

pub struct Vnutro {
    pub priecinok: PathBuf,
    pub nastavenia: Mutex<Nastavenia>,
    pub fronta: Mutex<Fronta>,
    pub prostredie: Box<dyn Prostredie>,
    pub nastroje: Nastroje,
    pub ucty: ucty::Ucty,
    pub jazyky: jazyky::Jazyky,
    zaznam: Mutex<VecDeque<String>>,
    graf: Mutex<Graf>,
    posledne_volanie: Mutex<Instant>,
    instalacia: Mutex<Instalacia>,
    /// strážca schránky: zachytené odkazy
    pub strazca: Mutex<strazca::Strazca>,
    /// prihlásenie z prehliadača kurzov (cookies) – len v pamäti
    pub prihlasenia: Mutex<Vec<(String, String)>>,
    /// nová verzia Komety: kontrola, stiahnutie, inštalácia
    pub aktualizacia: Mutex<aktualizacie::StavAktualizacie>,
    rt: tokio::runtime::Handle,
    /// prostredie okna pre UI: "aplikacia" (Tauri) alebo "prehliadac"
    pub okno: &'static str,
    /// verzia programu, ktorú ukáže okno (aplikácia má vlastnú, `kometa okno` verziu jadra)
    verzia: &'static str,
}

impl Vnutro {
    pub fn zapis(&self, s: impl Into<String>) {
        let mut z = self.zaznam.lock().unwrap();
        if z.len() >= 300 {
            z.pop_front();
        }
        z.push_back(s.into());
    }

    pub fn rt_spawn<F>(&self, f: F)
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.rt.spawn(f);
    }

    /// Spustí čakajúce položky do limitu súbežných sťahovaní.
    pub fn naplanuj(self: &Arc<Self>) {
        let limit = self
            .nastavenia
            .lock()
            .unwrap()
            .cislo("parallel", 2)
            .clamp(1, 6) as usize;
        let mut f = self.fronta.lock().unwrap();
        for (id, z) in f.na_spustenie(limit) {
            let uloha = self
                .rt
                .spawn(stahovanie::prace(self.clone(), id.clone(), z));
            if let Some(b) = f.behy.get_mut(&id) {
                b.uloha = Some(uloha);
            }
        }
        f.uloz(true);
    }

    fn priecinok_vystupu(&self) -> String {
        let n = self.nastavenia.lock().unwrap();
        let mut o = n.text("outdir");
        if o.is_empty() {
            o = nastavenia::predvoleny_vystup()
                .to_string_lossy()
                .into_owned();
        }
        let _ = std::fs::create_dir_all(&o);
        o
    }
}

/// Formát položky podľa stránky: z úložiska sa stiahne hotový súbor („subor“),
/// „Video s jazykmi“ ide len pri YouTube – ostatné stránky dostanú bežné video.
fn format_pre(url: &str, preset: &str) -> String {
    if zdroje::je_ulozisko(url) {
        "subor".into()
    } else if preset == "jazyky" && zdroje::nazov_zdroja(url) != "YouTube" {
        "video".into()
    } else {
        preset.into()
    }
}

#[derive(Clone)]
pub struct Sluzba {
    v: Arc<Vnutro>,
}

impl From<Arc<Vnutro>> for Sluzba {
    fn from(v: Arc<Vnutro>) -> Self {
        Sluzba { v }
    }
}

fn chyba(t: impl Into<String>) -> Chyba {
    Chyba::Ine(t.into())
}

/// Argument `i` z poľa argumentov okna; chýbajúci = predvolená hodnota typu.
fn arg<T: DeserializeOwned + Default>(args: &[Value], i: usize) -> Vysledok<T> {
    match args.get(i) {
        None | Some(Value::Null) => Ok(T::default()),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| chyba("Neplatné údaje z okna.")),
    }
}

impl Sluzba {
    /// `rt` = runtime, na ktorom bežia sťahovania. `okno` = "aplikacia" alebo "prehliadac".
    pub fn new(
        prostredie: Box<dyn Prostredie>,
        rt: tokio::runtime::Handle,
        okno: &'static str,
        verzia: &'static str,
    ) -> Self {
        let priecinok = nastavenia::priecinok_nastaveni();
        let n = Nastavenia::nacitaj(&priecinok);
        let pokracovat = n.ano("autoresume", true);
        let v = Arc::new(Vnutro {
            fronta: Mutex::new(Fronta::nacitaj(&priecinok, pokracovat)),
            nastroje: Nastroje::new(&priecinok),
            ucty: ucty::Ucty::new(),
            jazyky: jazyky::Jazyky::default(),
            nastavenia: Mutex::new(n),
            priecinok,
            prostredie,
            zaznam: Mutex::new(VecDeque::new()),
            graf: Mutex::new(Graf {
                body: VecDeque::from(vec![0.0; 90]),
                posledny: Instant::now(),
            }),
            posledne_volanie: Mutex::new(Instant::now()),
            instalacia: Mutex::new(Instalacia::default()),
            strazca: Mutex::new(strazca::Strazca::default()),
            prihlasenia: Mutex::new(vec![]),
            aktualizacia: Mutex::new(aktualizacie::StavAktualizacie::default()),
            rt,
            okno,
            verzia,
        });
        let s = Sluzba { v };
        let oprava = s.v.nastavenia.lock().unwrap().opraveny_priecinok.clone();
        if let Some((stary, novy)) = oprava {
            s.v.zapis(format!(
                "Sťahujem do {novy} (systémový priečinok Videá). Skôr stiahnuté súbory ostali v {stary}."
            ));
        }
        s.v.naplanuj();
        ucty::over_pri_starte(&s.v);
        strazca::spusti(&s.v);
        aktualizacie::spusti(&s.v);
        s
    }

    pub fn vnutro(&self) -> &Arc<Vnutro> {
        &self.v
    }

    /// Koľko sekúnd sa okno neozvalo (okno v prehliadači sa pýta na stav každú pol sekundu).
    pub fn ticho_sekund(&self) -> f64 {
        self.v
            .posledne_volanie
            .lock()
            .unwrap()
            .elapsed()
            .as_secs_f64()
    }

    /// Pri zatváraní: zastaví sťahovania tak, aby sa pri ďalšom spustení pokračovalo.
    pub async fn zatvor(&self) {
        let (ulohy, bezalo) = {
            let mut f = self.v.fronta.lock().unwrap();
            f.zatvara = true;
            let bezalo: Vec<String> = f
                .polozky
                .iter()
                .filter(|p| p.stav == Stav::Stahuje)
                .map(|p| p.id.clone())
                .collect();
            let ulohy: Vec<_> = f
                .behy
                .values_mut()
                .filter_map(|b| {
                    b.zrusenie.cancel();
                    b.uloha.take()
                })
                .collect();
            (ulohy, bezalo)
        };
        for u in ulohy {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(8), u).await;
        }
        let pokracovat = self.v.nastavenia.lock().unwrap().ano("autoresume", true);
        let mut f = self.v.fronta.lock().unwrap();
        for id in bezalo {
            f.uprav(&id, |p| {
                if matches!(p.stav, Stav::Pauza | Stav::Stahuje) {
                    p.stav = if pokracovat { Stav::Caka } else { Stav::Pauza };
                }
            });
        }
        f.uloz(true);
    }

    /// Jednotná odpoveď pre okno.
    pub async fn volaj(&self, metoda: &str, args: Vec<Value>) -> Value {
        *self.v.posledne_volanie.lock().unwrap() = Instant::now();
        if !METODY.contains(&metoda) {
            return json!({"ok": false, "chyba": "Neznáma akcia."});
        }
        match self.vykonaj(metoda, &args).await {
            Ok(data) => json!({"ok": true, "data": data}),
            Err(e) => {
                let t = e.to_string();
                let (veta, podrobnosti) = match t.split_once("\n\nPodrobnosti: ") {
                    Some((a, b)) => (a.to_string(), b.to_string()),
                    None => (t, String::new()),
                };
                json!({"ok": false, "chyba": veta, "podrobnosti": podrobnosti})
            }
        }
    }

    async fn vykonaj(&self, m: &str, a: &[Value]) -> Vysledok<Value> {
        let v = &self.v;
        Ok(match m {
            "init" => self.init(),
            "stav" => self.stav(),
            "pridat" => self.pridat(&arg::<String>(a, 0)?, &arg::<String>(a, 1)?, &arg::<String>(a, 2)?)?,
            "zadat_heslo" => {
                let (id, heslo) = (arg::<String>(a, 0)?, arg::<String>(a, 1)?);
                let rozbalit = {
                    let mut f = v.fronta.lock().unwrap();
                    let Some(p) = f.najdi(&id).cloned() else {
                        return Err(chyba("Položka už nie je vo fronte."));
                    };
                    let archiv = p.stav == Stav::Hotovo;
                    f.uprav(&id, |x| x.heslo = heslo.clone());
                    if !archiv {
                        f.spusti(std::slice::from_ref(&id));
                    }
                    archiv
                };
                if rozbalit {
                    stahovanie::skus_rozbalit(v, &id, Some(heslo), true);
                } else {
                    v.naplanuj();
                }
                Value::Null
            }
            "rozbalit" => {
                stahovanie::skus_rozbalit(v, &arg::<String>(a, 0)?, None, true);
                Value::Null
            }
            "schranka_stiahnut" => {
                let ids = arg::<Option<Vec<String>>>(a, 0)?;
                let preset = self.preset(&arg::<String>(a, 1)?);
                let vybrane = strazca::vyber(v, ids.as_deref());
                let (kolekcie, ostatne): (Vec<_>, Vec<_>) = vybrane.into_iter().partition(|z| z.kolekcia);
                let urls: Vec<String> = ostatne.into_iter().map(|z| z.url).collect();
                let n = self.pridat_odkazy(&urls, &preset, "");
                json!({"pridane": n, "kolekcie": kolekcie.into_iter().map(|z| z.url).collect::<Vec<_>>()})
            }
            "schranka_zahodit" => {
                strazca::vyber(v, arg::<Option<Vec<String>>>(a, 0)?.as_deref());
                Value::Null
            }
            "kurz_otvorit" => {
                let url = arg::<String>(a, 0)?;
                let url = url.trim();
                let url = if url.starts_with("http://") || url.starts_with("https://") {
                    url.to_string()
                } else {
                    format!("https://{url}")
                };
                if url::Url::parse(&url).is_err() {
                    return Err(chyba("Zadaj adresu stránky, napríklad https://www.vita.sk"));
                }
                v.prostredie.prehliadac_otvor(&url).map_err(chyba)?;
                Value::Null
            }
            "kurz_nacitat" => {
                let v2 = v.clone();
                let t = tokio::task::spawn_blocking(move || v2.prostredie.prehliadac_skript(kurzy::ZBER_JS))
                    .await
                    .map_err(|_| chyba("Stránku sa nepodarilo prečítať."))?
                    .map_err(chyba)?;
                let z = kurzy::precitaj_zber(&t)
                    .ok_or_else(|| chyba("Stránku sa nepodarilo prečítať – počkaj, kým sa načíta, a skús znova."))?;
                let k = kurzy::kurz_zo_zberu(&z);
                if k.kapitoly.is_empty() {
                    return Err(chyba(
                        "Na tejto stránke som nenašiel lekcie ani video. Prejdi na stránku s obsahom kurzu (zoznam lekcií).",
                    ));
                }
                json!(k)
            }
            "kurz_stiahnut" => {
                let k: kurzy::Kurz = arg(a, 0)?;
                let preset = self.preset(&arg::<String>(a, 1)?);
                let v2 = v.clone();
                let adresy: Vec<String> = std::iter::once(k.adresa.clone())
                    .chain(k.kapitoly.iter().flat_map(|x| x.lekcie.iter().map(|l| l.url.clone())))
                    .collect();
                // prihlásenie z okna prehliadača (pre každú doménu lekcií)
                let cookies = tokio::task::spawn_blocking(move || {
                    let mut hosty = std::collections::HashSet::new();
                    let mut out = vec![];
                    for u in adresy {
                        let h = url::Url::parse(&u).ok().and_then(|x| x.host_str().map(str::to_string));
                        if h.is_some_and(|h| hosty.insert(h)) {
                            out.extend(v2.prostredie.prehliadac_cookies(&u));
                        }
                    }
                    out
                })
                .await
                .unwrap_or_default();
                if !cookies.is_empty() {
                    *v.prihlasenia.lock().unwrap() = cookies;
                }
                let nove = kurzy::polozky(&k, &v.priecinok_vystupu(), &preset);
                let pocet = nove.len();
                let n = v.fronta.lock().unwrap().pridaj(nove).len();
                v.naplanuj();
                v.zapis(format!("🎓 Kurz „{}“: {n} lekcií do sťahovania", k.nazov));
                json!({"pridane": n, "uz_vo_fronte": pocet - n})
            }
            "rozpoznat" => Value::Array(
                zdroje::odkazy(&arg::<String>(a, 0)?)
                    .into_iter()
                    .take(50)
                    .map(|u| json!({"zdroj": zdroje::nazov_zdroja(&u), "kolekcia": zdroje::je_kolekcia(&u), "ulozisko": zdroje::je_ulozisko(&u), "url": u}))
                    .collect(),
            ),
            "kolekcia" => self.kolekcia(&arg::<String>(a, 0)?, &arg::<String>(a, 1)?).await?,
            "pridat_vybrane" => self.pridat_vybrane(
                &arg::<Vec<Value>>(a, 0)?,
                &arg::<String>(a, 1)?,
                &arg::<String>(a, 2)?,
                arg::<Option<fronta::VolbyJazykov>>(a, 3)?,
            )?,
            "skenovat" => self.skenovat(&arg::<String>(a, 0)?).await?,
            "pridat_zo_skenu" => self.pridat_zo_skenu(&arg::<Vec<Value>>(a, 0)?, &arg::<String>(a, 1)?, &arg::<String>(a, 2)?),
            "pozastavit" => {
                v.fronta.lock().unwrap().pozastav(&arg::<Vec<String>>(a, 0)?);
                Value::Null
            }
            "spustit" => {
                v.fronta.lock().unwrap().spusti(&arg::<Vec<String>>(a, 0)?);
                v.naplanuj();
                Value::Null
            }
            "odstranit" => {
                let ids = arg::<Vec<String>>(a, 0)?;
                let odstranene = v.fronta.lock().unwrap().odstran(&ids);
                for (p, uloha) in odstranene {
                    v.rt.spawn(stahovanie::uprac(p, uloha));
                }
                v.naplanuj();
                Value::Null
            }
            "vycistit_hotove" => {
                v.fronta.lock().unwrap().vycisti_hotove();
                Value::Null
            }
            "vycistit_front" => {
                // všetko z frontu preč; stiahnuté súbory ostanú, rozpracované sa uprace
                let mut f = v.fronta.lock().unwrap();
                let ids = f.ids();
                for (p, uloha) in f.odstran(&ids) {
                    v.rt.spawn(stahovanie::uprac(p, uloha));
                }
                Value::Null
            }
            "subory_na_zmazanie" => {
                let ids = arg::<Option<Vec<String>>>(a, 0)?;
                let (subory, velkost) = stahovanie::stiahnute_subory(v, ids.as_deref());
                json!({"pocet": subory.len(), "velkost": velkost})
            }
            "zmazat_subory" => {
                let ids = arg::<Option<Vec<String>>>(a, 0)?;
                stahovanie::zmaz_stiahnute(v, ids.as_deref()).await
            }
            "pozastavit_vsetko" => {
                let mut f = v.fronta.lock().unwrap();
                let ids = f.ids();
                f.pozastav(&ids);
                Value::Null
            }
            "spustit_vsetko" => {
                {
                    let mut f = v.fronta.lock().unwrap();
                    let ids = f.ids();
                    f.spusti(&ids);
                }
                v.naplanuj();
                Value::Null
            }
            "otvorit" => {
                let id = arg::<String>(a, 0)?;
                let p = v.fronta.lock().unwrap().najdi(&id).cloned();
                let (priecinok, oznac) = match p {
                    Some(p) if !p.subor.is_empty() && Path::new(&p.subor).exists() => {
                        let s = PathBuf::from(&p.subor);
                        (s.parent().map(Path::to_path_buf).unwrap_or_default(), Some(s))
                    }
                    Some(p) => (PathBuf::from(&p.priecinok), None),
                    None => (PathBuf::from(v.priecinok_vystupu()), None),
                };
                self.blokujuco(move |v| v.prostredie.otvor(&priecinok, oznac.as_deref())).await;
                Value::Null
            }
            "otvorit_subor" => {
                let s = PathBuf::from(arg::<String>(a, 0)?);
                if s.exists() {
                    let dir = s.parent().map(Path::to_path_buf).unwrap_or_default();
                    self.blokujuco(move |v| v.prostredie.otvor(&dir, Some(&s))).await;
                }
                Value::Null
            }
            "schranka" => json!(self.blokujuco(|v| v.prostredie.schranka()).await),
            "jazyky_zistit" => jazyky::zisti(v, arg::<Vec<String>>(a, 0)?).await?,
            "aktualizacia_skontroluj" => aktualizacie::skontroluj(v).await,
            "aktualizacia_instaluj" => aktualizacie::instaluj(v)?,
            "aktualizacia_pri_starte" => aktualizacie::pri_starte(v),
            "jazyky_nacitat" => jazyky::nacitaj(v, &arg::<String>(a, 0)?).await?,
            "jazyky_stiahnut" => {
                jazyky::stiahni(v, &arg::<Map<String, Value>>(a, 0)?)?;
                Value::Null
            }
            "jazyky_zrusit" => {
                v.jazyky.zrus();
                Value::Null
            }
            "vybrat_priecinok" => {
                let start = v.priecinok_vystupu();
                let cesta = self.blokujuco(move |v| v.prostredie.vyber_priecinok(&start)).await;
                if !cesta.is_empty() {
                    let mut z = Map::new();
                    z.insert("outdir".into(), json!(cesta));
                    v.nastavenia.lock().unwrap().zmen(&z);
                }
                json!(cesta)
            }
            "vybrat_video" => {
                let start = v.priecinok_vystupu();
                json!(self.blokujuco(move |v| v.prostredie.vyber_video(&start)).await)
            }
            "prihlasit" => ucty::prihlas(v, &arg::<String>(a, 0)?, &arg::<String>(a, 1)?, &arg::<String>(a, 2)?).await?,
            "odhlasit" => {
                ucty::odhlas(v, &arg::<String>(a, 0)?);
                Value::Null
            }
            "ulozit_nastavenia" => {
                let z = arg::<Map<String, Value>>(a, 0)?;
                let mut n = v.nastavenia.lock().unwrap();
                n.zmen(&z);
                let out = n.pre_okno();
                drop(n);
                v.naplanuj();
                out
            }
            "doinstalovat_ffmpeg" => {
                self.doinstalovat("FFmpeg");
                Value::Null
            }
            "doinstalovat_ytdlp" => {
                self.doinstalovat("yt-dlp");
                Value::Null
            }
            _ => return Err(chyba("Neznáma akcia.")),
        })
    }

    /// Volanie systému (dialóg, schránka), ktoré blokuje – mimo vlákien sťahovania.
    async fn blokujuco<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Vnutro) -> T + Send + 'static,
    ) -> T {
        let v = self.v.clone();
        tokio::task::spawn_blocking(move || f(&v))
            .await
            .expect("úloha okna")
    }

    fn init(&self) -> Value {
        let n = self.v.nastavenia.lock().unwrap();
        let ytdlp = self.v.nastroje.ytdlp().is_some();
        json!({
            "verzia": self.v.verzia,
            "verzia_jadra": VERZIA,
            "platforma": if cfg!(windows) { "win32" } else if cfg!(target_os = "macos") { "darwin" } else { "linux" },
            "nastavenia": n.pre_okno(),
            "jazyky_poradie": n.jazyky(),
            "formaty": nastavenia::FORMATY.iter().map(|(id, nazov)| json!({"id": id, "nazov": nazov})).collect::<Vec<_>>(),
            "prehliadace": nastavenia::PREHLIADACE,
            "zvuk": nastavenia::ZVUK.iter().map(|(k, t)| (k.to_string(), json!(t))).collect::<Map<String, Value>>(),
            "chyba_ffmpeg": self.v.nastroje.ffmpeg().is_none(),
            "chyba_ytdlp": !ytdlp,
            "jadro": "rust",
            "okno": self.v.okno,
            "funkcie": {
                "jazyky": true,
                "ucty": true,
                "kolekcie": ytdlp,
                "doinstalovat_ytdlp": true,
                "doinstalovat_ffmpeg": nastroje::ffmpeg_doinstalovatelny(),
                "archivy": true,
                "strazca": self.v.prostredie.sleduje_schranku(),
                "kurzy": self.v.prostredie.ma_prehliadac(),
                "ffmpeg_volitelny": true,
            },
        })
    }

    fn stav(&self) -> Value {
        let (polozky, rychlost) = {
            let f = self.v.fronta.lock().unwrap();
            (
                f.polozky.iter().map(polozka_pre_okno).collect::<Vec<_>>(),
                f.rychlost(),
            )
        };
        let graf = {
            let mut g = self.v.graf.lock().unwrap();
            if g.posledny.elapsed().as_secs_f64() >= 1.0 {
                if g.body.len() >= 90 {
                    g.body.pop_front();
                }
                g.body.push_back(rychlost);
                g.posledny = Instant::now();
            }
            g.body.iter().copied().collect::<Vec<_>>()
        };
        let i = self.v.instalacia.lock().unwrap().clone();
        json!({
            "polozky": polozky,
            "rychlost": rychlost,
            "graf": graf,
            "jazyky": self.v.jazyky.stav(),
            "ucty": self.v.ucty.stav(),
            "aktualizacia": *self.v.aktualizacia.lock().unwrap(),
            "nastroje": {
                "ytdlp": self.v.nastroje.ytdlp().is_some(),
                "ffmpeg": self.v.nastroje.ffmpeg().is_some(),
                "instaluje": i.bezi,
                "co": i.co,
                "text": i.text,
                "podiel": i.podiel,
            },
            "zaznam": self.v.zaznam.lock().unwrap().iter().rev().take(40).rev().cloned().collect::<Vec<_>>(),
            "zachytene": strazca::stav(&self.v),
            "prehliadac": self.v.prostredie.prehliadac_stranka().map(|(adresa, titulok)| json!({"adresa": adresa, "titulok": titulok})),
        })
    }

    fn pridat(&self, text: &str, preset: &str, heslo: &str) -> Vysledok<Value> {
        let urls = zdroje::odkazy(text);
        if urls.is_empty() {
            return Err(chyba(
                "Vlož aspoň jeden odkaz – začína na http:// alebo https://.",
            ));
        }
        let preset = self.preset(preset);
        let mut z = Map::new();
        z.insert("dl_preset".into(), json!(preset));
        self.v.nastavenia.lock().unwrap().zmen(&z);
        let (kolekcie, jednotlive): (Vec<String>, Vec<String>) =
            urls.into_iter().partition(|u| zdroje::je_kolekcia(u));
        let pocet = jednotlive.len();
        let n = self.pridat_odkazy(&jednotlive, &preset, heslo);
        Ok(json!({"pridane": n, "uz_vo_fronte": pocet - n, "kolekcie": kolekcie}))
    }

    /// Pridá jednotlivé odkazy (nie playlisty) do frontu; vráti počet nových položiek.
    pub fn pridat_odkazy(&self, jednotlive: &[String], preset: &str, heslo: &str) -> usize {
        let priecinok = self.v.priecinok_vystupu();
        let preset = preset.to_string();
        let jazyky = (preset == "jazyky").then(|| self.jazyky_z_nastaveni());
        let nove = self.v.fronta.lock().unwrap().pridaj(
            jednotlive
                .iter()
                .cloned()
                .map(|url| Nova {
                    format: format_pre(&url, &preset),
                    jazyky: jazyky
                        .clone()
                        .filter(|_| zdroje::nazov_zdroja(&url) == "YouTube"),
                    nazov: ucty::nazov_z_odkazu_fastshare(&url)
                        .filter(|_| zdroje::nazov_zdroja(&url) == "Fastshare")
                        .unwrap_or_default(),
                    url,
                    priecinok: priecinok.clone(),
                    heslo: heslo.trim().to_string(),
                    ..Default::default()
                })
                .collect(),
        );
        self.v.naplanuj();
        nove.len()
    }

    /// Jazyky podľa nastavení (karta Jazykové stopy si ich pamätá).
    fn jazyky_z_nastaveni(&self) -> fronta::VolbyJazykov {
        let n = self.v.nastavenia.lock().unwrap();
        fronta::VolbyJazykov {
            poradie: n.jazyky(),
            original: n.ano("include_original", true),
            zvuk: n.text("audio_mode"),
            zvuky: Default::default(),
            kontajner: n.text("kontajner"),
            kvalita: n.text("max_height"),
            rezim: "video".into(),
            bez_jazykov: "povodny".into(),
            titulky: n.text("titulky"),
            titulky_auto: n.ano("titulky_auto", false),
        }
    }

    fn preset(&self, p: &str) -> String {
        if nastavenia::FORMATY.iter().any(|(k, _)| *k == p) {
            p.to_string()
        } else {
            Some(self.v.nastavenia.lock().unwrap().text("dl_preset"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "video".into())
        }
    }

    async fn kolekcia(&self, url: &str, karta: &str) -> Vysledok<Value> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(chyba("Zadaj odkaz na playlist alebo kanál."));
        }
        let exe = self.v.nastroje.ytdlp().ok_or_else(|| chyba("Playlisty a kanály načíta pomocník yt-dlp – v Nastaveniach klikni na „Doinštalovať yt-dlp“."))?;
        let karta = if karta.is_empty() { "videos" } else { karta };
        let spol = ytdlp::Spolocne {
            prehliadac: self.v.nastavenia.lock().unwrap().text("cookies_browser"),
            ..Default::default()
        };
        let mut d = ytdlp::zoznam(&exe, &zdroje::adresa_kolekcie(url, karta), &spol).await?;
        if let Some(e) = d.get_mut("entries").and_then(Value::as_array_mut) {
            for x in e {
                let n = zdroje::nahlad_youtube(x["url"].as_str().unwrap_or(""));
                x["nahlad"] = json!(n);
            }
        }
        d["kanal"] = json!(zdroje::je_kanal(url));
        Ok(d)
    }

    fn pridat_vybrane(
        &self,
        polozky: &[Value],
        preset: &str,
        priecinok: &str,
        jazyky: Option<fronta::VolbyJazykov>,
    ) -> Vysledok<Value> {
        if let Some(o) = &jazyky {
            if o.poradie.is_empty() && !o.original {
                return Err(chyba("Vyber aspoň jeden jazyk."));
            }
        }
        let preset = if jazyky.is_some() {
            "jazyky".to_string()
        } else {
            self.preset(preset)
        };
        let mut out = PathBuf::from(self.v.priecinok_vystupu());
        if !priecinok.trim().is_empty() {
            out = out.join(zdroje::bezpecny_nazov(priecinok, 100));
        }
        let out = out.to_string_lossy().into_owned();
        let nove: Vec<Nova> = polozky
            .iter()
            .filter_map(|p| {
                let url = p.get("url")?.as_str()?.to_string();
                let youtube = zdroje::nazov_zdroja(&url) == "YouTube";
                Some(Nova {
                    nazov: p
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    format: format_pre(&url, &preset),
                    jazyky: jazyky.clone().filter(|_| youtube),
                    url,
                    priecinok: out.clone(),
                    ..Default::default()
                })
            })
            .collect();
        let pocet = nove.len();
        let pridane = self.v.fronta.lock().unwrap().pridaj(nove).len();
        self.v.naplanuj();
        Ok(json!({"pridane": pridane, "uz_vo_fronte": pocet - pridane}))
    }

    async fn skenovat(&self, url: &str) -> Vysledok<Value> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(chyba(
                "Zadaj adresu stránky – začína na http:// alebo https://.",
            ));
        }
        let k = crate::klient::klient(&crate::klient::Nastavenia::default())?;
        let h = udalosti::tichy();
        let nalezy = Skener::new(&k, &h).prehladaj(url, None).await?;
        let ytdlp = self.v.nastroje.ytdlp().is_some();
        Ok(Value::Array(
            nalezy
                .iter()
                .map(|n| {
                    let druh = match n.druh {
                        vzory::Druh::Prehravac => "embed",
                        vzory::Druh::Zvuk => "audio",
                        vzory::Druh::Stream => "stream",
                        vzory::Druh::Video => "video",
                        vzory::Druh::Subor => "subor",
                        vzory::Druh::Titulky => "titulky",
                    };
                    let nativny = matches!(n.poskytovatel.as_str(), "Vimeo" | "JW Player" | "Wistia" | "Cloudflare Stream" | "Bunny Stream");
                    let da_sa = n.druh.da_sa_stiahnut() && (n.druh != vzory::Druh::Prehravac || nativny || ytdlp);
                    json!({"url": n.url, "druh": druh, "druh_text": n.druh_text, "poskytovatel": n.poskytovatel,
                           "nazov": n.nazov, "poznamka": n.poznamka, "skore": n.skore, "referer": n.referer,
                           "odporucane": n.odporucane && da_sa, "da_sa": da_sa})
                })
                .collect(),
        ))
    }

    fn pridat_zo_skenu(&self, polozky: &[Value], preset: &str, heslo: &str) -> Value {
        let preset = self.preset(preset);
        let priecinok = self.v.priecinok_vystupu();
        let s = |p: &Value, k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let nove: Vec<Nova> = polozky
            .iter()
            .filter(|p| !s(p, "url").is_empty())
            .map(|p| Nova {
                url: s(p, "url"),
                format: preset.clone(),
                priecinok: priecinok.clone(),
                nazov: s(p, "nazov"),
                zdroj: s(p, "poskytovatel"),
                referer: s(p, "referer"),
                heslo: heslo.to_string(),
                zo_skenu: true,
                jazyky: None,
                prihlasenie: false,
            })
            .collect();
        let n = self.v.fronta.lock().unwrap().pridaj(nove).len();
        self.v.naplanuj();
        json!({"pridane": n})
    }

    /// Doinštaluje pomocníka („yt-dlp“ alebo „FFmpeg“) na pozadí; naraz beží len jedna inštalácia.
    fn doinstalovat(&self, co: &'static str) {
        {
            let mut i = self.v.instalacia.lock().unwrap();
            if i.bezi {
                return;
            }
            *i = Instalacia {
                co,
                bezi: true,
                text: format!("Sťahujem {co}…"),
                podiel: 0.0,
            };
        }
        let v = self.v.clone();
        self.v.rt.spawn(async move {
            let v2 = Arc::downgrade(&v);
            let h: udalosti::Hlasic = Arc::new(move |u| {
                if let (
                    Some(v),
                    Udalost::Priebeh {
                        stiahnute,
                        celkom: Some(c),
                        ..
                    },
                ) = (v2.upgrade(), u)
                {
                    v.instalacia.lock().unwrap().podiel = stiahnute as f64 / c.max(1) as f64;
                }
            });
            let k = crate::klient::klient(&crate::klient::Nastavenia::default());
            let r = match k {
                Ok(k) if co == "FFmpeg" => v.nastroje.doinstaluj_ffmpeg(&k, &h).await,
                Ok(k) => v.nastroje.doinstaluj_ytdlp(&k, &h).await,
                Err(e) => Err(e),
            };
            let mut i = v.instalacia.lock().unwrap();
            i.bezi = false;
            i.podiel = 1.0;
            i.text = match r {
                Ok(_) => format!("{co} je pripravený."),
                Err(e) => format!("{co} sa nepodarilo doinštalovať: {e}"),
            };
            drop(i);
            v.zapis(v.instalacia.lock().unwrap().text.clone());
        });
    }
}

fn polozka_pre_okno(p: &Polozka) -> Value {
    let zostava = match (p.celkom, p.rychlost) {
        (Some(c), r) if r > 0.0 && c > p.hotovo => Some((c - p.hotovo) as f64 / r),
        _ => None,
    };
    json!({
        "id": p.id, "url": p.url, "zdroj": p.zdroj, "format": p.format, "nazov": p.nazov, "stav": p.stav,
        "hotovo": p.hotovo, "celkom": p.celkom, "rychlost": p.rychlost, "zostava": zostava,
        "subor": Path::new(&p.subor).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "chyba": p.chyba, "podrobnosti": p.podrobnosti, "nahlad": p.nahlad, "dlzka": p.dlzka,
        "autor": p.autor, "pridane": p.pridane,
        "podiel": p.podiel, "krok": p.krok,
        "jazyky": p.jazyky.as_ref().map(jazyky::popis_volieb),
        "treba_heslo": p.treba_heslo,
        "rozbalenie": p.rozbalenie.as_ref().map(|r| json!({
            "stav": r.stav, "text": r.text, "podiel": r.podiel,
            "ciel": Path::new(&r.ciel).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        })),
    })
}
