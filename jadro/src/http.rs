//! Sťahovanie súboru cez HTTP s pokračovaním.
//!
//! * jedno spojenie: `súbor.part` + Range od miesta, kde sa skončilo,
//! * viac spojení: súbor sa rozdelí na úseky; voľné spojenie prevezme polovicu
//!   najväčšieho zvyšku (nikto nečaká na posledného), a keď server ďalšie spojenia
//!   odmieta (429/503 alebo stránka namiesto dát), ich počet sa sám zníži,
//! * stav úsekov sa ukladá do `súbor.kometa`, takže po prerušení sa pokračuje,
//! * keď dočasný odkaz vyprší (401/403/404/410), jadro si vypýta nový.

use crate::chyba::{Chyba, Vysledok};
use crate::klient::{celkom_z_rozsahu, je_html, s_priponou, volna_cesta, zaciatok_z_rozsahu};
use crate::udalosti::{varovanie, Hlasic, Udalost};
use futures_util::future::BoxFuture;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub const MIN_USEK: u64 = 1 << 20;
pub const MIN_DELENE: u64 = 8 << 20;
const MAX_POKUSOV: u32 = 12;

/// Zdroj adresy: `false` = daj odkaz, `true` = starý prestal fungovať, daj nový.
pub type ZdrojUrl = Arc<dyn Fn(bool) -> BoxFuture<'static, Vysledok<String>> + Send + Sync>;

pub fn pevna_url(u: impl Into<String>) -> ZdrojUrl {
    let u: String = u.into();
    Arc::new(move |_| {
        let u = u.clone();
        Box::pin(async move { Ok(u) })
    })
}

#[derive(Clone)]
pub struct Moznosti {
    pub spojenia: usize,
    pub ocakavana_velkost: Option<u64>,
    pub zrusenie: CancellationToken,
    pub hlasic: Hlasic,
}

pub fn part_cesta(ciel: &Path) -> PathBuf {
    s_priponou(ciel, ".part")
}

pub fn stav_cesta(ciel: &Path) -> PathBuf {
    s_priponou(ciel, ".kometa")
}

async fn spi(z: &CancellationToken, s: Duration) -> Vysledok<()> {
    tokio::select! {
        _ = z.cancelled() => Err(Chyba::Pauza),
        _ = tokio::time::sleep(s) => Ok(()),
    }
}

fn caka(pokus: u32) -> Duration {
    Duration::from_secs((1u64 << pokus.min(5)).min(30))
}

/// Hlavná funkcia: vyberie viac spojení (veľký súbor, server podporuje Range) alebo jedno.
pub async fn stiahni(k: &Client, zdroj: ZdrojUrl, ciel: &Path, m: &Moznosti) -> Vysledok<PathBuf> {
    if let Some(p) = ciel.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let part = part_cesta(ciel);
    let stav = stav_cesta(ciel);
    let rozpracovane_jednym = part.exists() && !stav.exists();
    if m.spojenia > 1 && !rozpracovane_jednym {
        let celkom = if stav.exists() {
            StavUsekov::nacitaj(&stav).map(|s| s.celkom)
        } else {
            zisti_velkost(k, &zdroj).await
        };
        if let Some(celkom) = celkom.filter(|c| *c >= MIN_DELENE) {
            match Useky::new(k.clone(), zdroj.clone(), ciel, celkom, m)
                .spusti()
                .await
            {
                Err(Chyba::Ine(s)) if s == BEZ_RANGE => {
                    varovanie(
                        &m.hlasic,
                        "Server nepodporuje sťahovanie po častiach – pokračujem jedným spojením.",
                    );
                    let _ = tokio::fs::remove_file(&part).await;
                    let _ = tokio::fs::remove_file(&stav).await;
                }
                vysledok => return vysledok,
            }
        }
    }
    jedno_spojenie(k, zdroj, ciel, m).await
}

/// Overí podporu Range a zistí veľkosť (Range: bytes=0-0 → 206 + Content-Range).
async fn zisti_velkost(k: &Client, zdroj: &ZdrojUrl) -> Option<u64> {
    let mut url = zdroj(false).await.ok()?;
    for pokus in 0..2 {
        let r = k
            .get(&url)
            .header(RANGE, "bytes=0-0")
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .ok()?;
        let s = r.status().as_u16();
        if matches!(s, 401 | 403 | 404 | 410) && pokus == 0 {
            url = zdroj(true).await.ok()?;
            continue;
        }
        if r.status() != StatusCode::PARTIAL_CONTENT {
            return None;
        }
        return celkom_z_rozsahu(r.headers().get(CONTENT_RANGE));
    }
    None
}

// --------------------------------------------------------------------------------------------
//  Jedno spojenie
// --------------------------------------------------------------------------------------------

pub async fn jedno_spojenie(
    k: &Client,
    zdroj: ZdrojUrl,
    ciel: &Path,
    m: &Moznosti,
) -> Vysledok<PathBuf> {
    let part = part_cesta(ciel);
    let mut url = zdroj(false).await?;
    let mut celkom = m.ocakavana_velkost;
    let (mut zlyhania, mut obnovy) = (0u32, 0u32);
    let mut posledne = Instant::now() - Duration::from_secs(1);
    let mut okno: Vec<(Instant, u64)> = vec![];

    loop {
        if m.zrusenie.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        let mut poz = tokio::fs::metadata(&part)
            .await
            .map(|x| x.len())
            .unwrap_or(0);
        if let Some(c) = celkom {
            if poz >= c {
                break;
            }
        }
        let mut req = k.get(&url).header(ACCEPT_ENCODING, "identity");
        if poz > 0 {
            req = req.header(RANGE, format!("bytes={poz}-"));
        }
        let r = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                zlyhania += 1;
                if zlyhania > MAX_POKUSOV {
                    return Err(Chyba::Siet(e.to_string()));
                }
                varovanie(
                    &m.hlasic,
                    format!("Spojenie zlyhalo, skúšam znova ({zlyhania}/{MAX_POKUSOV})…"),
                );
                spi(&m.zrusenie, caka(zlyhania)).await?;
                continue;
            }
        };
        let s = r.status().as_u16();
        if s == 416 {
            if let Some(v) = celkom_z_rozsahu(r.headers().get(CONTENT_RANGE)) {
                if poz >= v {
                    celkom = Some(v);
                    break;
                }
            }
            let _ = tokio::fs::remove_file(&part).await;
            continue;
        }
        if matches!(s, 401 | 403 | 404 | 410) && obnovy < 3 {
            obnovy += 1;
            varovanie(&m.hlasic, "Odkaz vypršal, žiadam nový…");
            url = zdroj(true).await?;
            continue;
        }
        if s >= 500 || s == 429 {
            zlyhania += 1;
            if zlyhania > MAX_POKUSOV {
                return Err(Chyba::Http(s));
            }
            spi(&m.zrusenie, caka(zlyhania)).await?;
            continue;
        }
        if s >= 400 {
            return Err(Chyba::Http(s));
        }
        if je_html(r.headers().get(CONTENT_TYPE)) {
            return Err(Chyba::Stranka);
        }
        let append = s == 206;
        if append {
            celkom = celkom_z_rozsahu(r.headers().get(CONTENT_RANGE)).or(celkom);
        } else {
            if poz > 0 {
                varovanie(
                    &m.hlasic,
                    "Server nepodporuje pokračovanie, sťahujem od začiatku.",
                );
            }
            poz = 0;
            celkom = r
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .or(celkom);
        }
        let mut f = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await?;
        let mut prud = r.bytes_stream();
        let mut dostal = 0u64;
        let mut chyba_prenosu = None;
        loop {
            tokio::select! {
                _ = m.zrusenie.cancelled() => {
                    f.flush().await?;
                    return Err(Chyba::Pauza);
                }
                kus = prud.next() => match kus {
                    None => break,
                    Some(Err(e)) => { chyba_prenosu = Some(e); break; }
                    Some(Ok(b)) => {
                        f.write_all(&b).await?;
                        poz += b.len() as u64;
                        dostal += b.len() as u64;
                        let teraz = Instant::now();
                        okno.push((teraz, poz));
                        while okno.len() > 2 && teraz.duration_since(okno[0].0) > Duration::from_secs(5) {
                            okno.remove(0);
                        }
                        if teraz.duration_since(posledne) >= Duration::from_millis(250) {
                            posledne = teraz;
                            let dt = teraz.duration_since(okno[0].0).as_secs_f64().max(1e-3);
                            (m.hlasic)(Udalost::Priebeh {
                                stiahnute: poz, celkom, rychlost: (poz - okno[0].1) as f64 / dt, cast: None,
                            });
                        }
                    }
                }
            }
        }
        f.flush().await?;
        drop(f);
        if dostal > 0 {
            zlyhania = 0;
        }
        if let Some(e) = chyba_prenosu {
            zlyhania += 1;
            if zlyhania > MAX_POKUSOV {
                return Err(Chyba::Siet(e.to_string()));
            }
            varovanie(
                &m.hlasic,
                format!("Spojenie sa prerušilo pri {} MB, pokračujem…", poz >> 20),
            );
            spi(&m.zrusenie, caka(zlyhania)).await?;
            continue;
        }
        match celkom {
            Some(c) if poz < c => continue, // server skončil predčasne → pokračuj
            _ => break,
        }
    }
    let poz = tokio::fs::metadata(&part)
        .await
        .map(|x| x.len())
        .unwrap_or(0);
    (m.hlasic)(Udalost::Priebeh {
        stiahnute: poz,
        celkom: celkom.or(Some(poz)),
        rychlost: 0.0,
        cast: None,
    });
    let finalna = volna_cesta(ciel);
    tokio::fs::rename(&part, &finalna).await?;
    Ok(finalna)
}

// --------------------------------------------------------------------------------------------
//  Viac spojení (úseky)
// --------------------------------------------------------------------------------------------

const BEZ_RANGE: &str = "__bez_range__";

#[derive(Serialize, Deserialize, Clone, Debug)]
struct StavUsekov {
    celkom: u64,
    /// [začiatok, koniec (vrátane), stiahnuté]
    useky: Vec<[u64; 3]>,
}

impl StavUsekov {
    fn nacitaj(p: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(p).ok()?).ok()
    }
}

struct Spolocne {
    useky: Vec<[u64; 3]>,
    vlastnik: Vec<bool>,
    limit: usize,
    aktivne: usize,
    url: String,
    generacia: u64,
    posledna_obnova: Option<Instant>,
    chyby: Vec<String>,
}

enum ChybaUseku {
    PrilisVela,
    BezRange,
    Obnov(u64),
    Ina(String),
    Pauza,
}

struct Useky {
    k: Client,
    zdroj: ZdrojUrl,
    ciel: PathBuf,
    part: PathBuf,
    stav: PathBuf,
    celkom: u64,
    m: Moznosti,
    zastav: CancellationToken,
    sp: Arc<Mutex<Spolocne>>,
}

impl Useky {
    fn new(k: Client, zdroj: ZdrojUrl, ciel: &Path, celkom: u64, m: &Moznosti) -> Arc<Self> {
        let part = part_cesta(ciel);
        let stav = stav_cesta(ciel);
        let ulozene = StavUsekov::nacitaj(&stav).filter(|s| {
            s.celkom == celkom
                && std::fs::metadata(&part)
                    .map(|x| x.len() == celkom)
                    .unwrap_or(false)
        });
        let useky = match ulozene {
            Some(s) => s.useky,
            None => {
                let n = (m.spojenia as u64).min((celkom / MIN_USEK).max(1));
                let krok = celkom / n;
                let mut v: Vec<[u64; 3]> =
                    (0..n).map(|i| [i * krok, (i + 1) * krok - 1, 0]).collect();
                v.last_mut().unwrap()[1] = celkom - 1;
                v
            }
        };
        let pocet = useky.len();
        Arc::new(Useky {
            k,
            zdroj,
            ciel: ciel.to_path_buf(),
            part,
            stav,
            celkom,
            zastav: m.zrusenie.child_token(),
            m: m.clone(),
            sp: Arc::new(Mutex::new(Spolocne {
                useky,
                vlastnik: vec![false; pocet],
                limit: m.spojenia.max(1),
                aktivne: 0,
                url: String::new(),
                generacia: 0,
                posledna_obnova: None,
                chyby: vec![],
            })),
        })
    }

    fn stiahnute(&self) -> u64 {
        self.sp.lock().unwrap().useky.iter().map(|u| u[2]).sum()
    }

    fn uloz_stav(&self) {
        let data = {
            let sp = self.sp.lock().unwrap();
            serde_json::to_vec(&StavUsekov {
                celkom: self.celkom,
                useky: sp.useky.clone(),
            })
            .unwrap()
        };
        let tmp = s_priponou(&self.stav, ".tmp");
        if std::fs::write(&tmp, data).is_ok() {
            let _ = std::fs::rename(&tmp, &self.stav);
        }
    }

    /// Pridelí úsek: voľný, alebo rozdelí najväčší zvyšok iného spojenia na polovicu.
    fn vezmi(&self) -> Option<usize> {
        let mut sp = self.sp.lock().unwrap();
        if sp.aktivne > sp.limit {
            return None;
        }
        for i in 0..sp.useky.len() {
            let u = sp.useky[i];
            if !sp.vlastnik[i] && u[0] + u[2] <= u[1] {
                sp.vlastnik[i] = true;
                return Some(i);
            }
        }
        let (mut naj, mut zvysok) = (None, 0u64);
        for (i, u) in sp.useky.iter().enumerate() {
            let r = (u[1] + 1).saturating_sub(u[0] + u[2]);
            if r > zvysok {
                naj = Some(i);
                zvysok = r;
            }
        }
        let i = naj?;
        if zvysok < 2 * MIN_USEK {
            return None;
        }
        let u = sp.useky[i];
        let stred = u[0] + u[2] + zvysok / 2;
        sp.useky.push([stred, u[1], 0]);
        sp.useky[i][1] = stred - 1;
        sp.vlastnik.push(true);
        Some(sp.useky.len() - 1)
    }

    fn uvolni(&self, i: usize) {
        self.sp.lock().unwrap().vlastnik[i] = false;
    }

    async fn obnov(&self, gen: u64) {
        {
            let mut sp = self.sp.lock().unwrap();
            if gen != sp.generacia {
                return;
            }
            if sp
                .posledna_obnova
                .map(|t| t.elapsed() < Duration::from_secs(5))
                .unwrap_or(false)
            {
                // odkaz bol práve obnovený a server stále odmieta → priveľa spojení
                sp.limit = sp.limit.saturating_sub(1).max(1);
                return;
            }
            sp.posledna_obnova = Some(Instant::now());
        }
        varovanie(&self.m.hlasic, "Odkaz vypršal, žiadam nový…");
        if let Ok(u) = (self.zdroj)(true).await {
            let mut sp = self.sp.lock().unwrap();
            sp.url = u;
            sp.generacia += 1;
        }
    }

    async fn pracovnik(self: Arc<Self>) -> Result<(), ChybaUseku> {
        self.sp.lock().unwrap().aktivne += 1;
        let vysledok = self.pracuj().await;
        self.sp.lock().unwrap().aktivne -= 1;
        vysledok
    }

    async fn pracuj(&self) -> Result<(), ChybaUseku> {
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&self.part)
            .await
            .map_err(|e| ChybaUseku::Ina(e.to_string()))?;
        let mut zlyhania = 0u32;
        while !self.zastav.is_cancelled() {
            let Some(i) = self.vezmi() else { return Ok(()) };
            match self.usek(i, &mut f).await {
                Ok(()) => zlyhania = 0,
                Err(ChybaUseku::BezRange) => {
                    self.uvolni(i);
                    return Err(ChybaUseku::BezRange);
                }
                Err(ChybaUseku::Pauza) => {
                    self.uvolni(i);
                    return Ok(());
                }
                Err(ChybaUseku::PrilisVela) => {
                    let novy = {
                        let mut sp = self.sp.lock().unwrap();
                        sp.limit = sp.limit.min(sp.aktivne.saturating_sub(1)).max(1);
                        sp.limit
                    };
                    varovanie(
                        &self.m.hlasic,
                        format!("Server obmedzuje počet spojení – pokračujem s {novy}."),
                    );
                    self.uvolni(i);
                    if spi(&self.zastav, Duration::from_secs(2)).await.is_err() {
                        return Ok(());
                    }
                }
                Err(ChybaUseku::Obnov(gen)) => {
                    self.uvolni(i);
                    self.obnov(gen).await;
                }
                Err(ChybaUseku::Ina(e)) => {
                    self.uvolni(i);
                    zlyhania += 1;
                    if zlyhania > MAX_POKUSOV {
                        self.sp.lock().unwrap().chyby.push(e);
                        return Ok(());
                    }
                    if spi(&self.zastav, caka(zlyhania)).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
            }
            self.uvolni(i);
        }
        Ok(())
    }

    async fn usek(&self, i: usize, f: &mut tokio::fs::File) -> Result<(), ChybaUseku> {
        let (poz, koniec, url, gen) = {
            let sp = self.sp.lock().unwrap();
            let u = sp.useky[i];
            (u[0] + u[2], u[1], sp.url.clone(), sp.generacia)
        };
        if poz > koniec {
            return Ok(());
        }
        let r = self
            .k
            .get(&url)
            .header(ACCEPT_ENCODING, "identity")
            .header(RANGE, format!("bytes={poz}-{koniec}"))
            .send()
            .await
            .map_err(|e| ChybaUseku::Ina(e.to_string()))?;
        let s = r.status().as_u16();
        if matches!(s, 401 | 403 | 404 | 410) {
            return Err(ChybaUseku::Obnov(gen));
        }
        if s == 429 || s == 503 || je_html(r.headers().get(CONTENT_TYPE)) {
            return Err(ChybaUseku::PrilisVela);
        }
        if s == 200 {
            return Err(ChybaUseku::BezRange);
        }
        if s != 206 {
            return Err(ChybaUseku::Ina(format!("chyba {s}")));
        }
        if zaciatok_z_rozsahu(r.headers().get(CONTENT_RANGE)) != Some(poz) {
            return Err(ChybaUseku::BezRange);
        }
        f.seek(SeekFrom::Start(poz))
            .await
            .map_err(|e| ChybaUseku::Ina(e.to_string()))?;
        let mut prud = r.bytes_stream();
        loop {
            let kus = tokio::select! {
                _ = self.zastav.cancelled() => { let _ = f.flush().await; return Err(ChybaUseku::Pauza); }
                kus = prud.next() => kus,
            };
            let b = match kus {
                None => break,
                Some(Err(e)) => {
                    let _ = f.flush().await;
                    return Err(ChybaUseku::Ina(e.to_string()));
                }
                Some(Ok(b)) => b,
            };
            let miesto = {
                let sp = self.sp.lock().unwrap();
                let u = sp.useky[i]; // koniec mohlo medzitým skrátiť iné spojenie
                (u[1] + 1).saturating_sub(u[0] + u[2])
            };
            if miesto == 0 {
                break;
            }
            let kus = &b[..(b.len() as u64).min(miesto) as usize];
            f.write_all(kus)
                .await
                .map_err(|e| ChybaUseku::Ina(e.to_string()))?;
            self.sp.lock().unwrap().useky[i][2] += kus.len() as u64;
            if (kus.len() as u64) == miesto {
                break;
            }
        }
        f.flush()
            .await
            .map_err(|e| ChybaUseku::Ina(e.to_string()))?;
        Ok(())
    }

    async fn spusti(self: Arc<Self>) -> Vysledok<PathBuf> {
        let mal_stav = StavUsekov::nacitaj(&self.stav).is_some()
            && std::fs::metadata(&self.part)
                .map(|x| x.len() == self.celkom)
                .unwrap_or(false);
        if !mal_stav {
            let f = std::fs::File::create(&self.part)?;
            f.set_len(self.celkom)?; // vyhradí miesto na disku
        } else {
            crate::udalosti::info(
                &self.m.hlasic,
                format!(
                    "Pokračujem v sťahovaní ({} % hotovo).",
                    self.stiahnute() * 100 / self.celkom
                ),
            );
        }
        self.sp.lock().unwrap().url = (self.zdroj)(false).await?;
        self.uloz_stav();

        let pocet = {
            let sp = self.sp.lock().unwrap();
            sp.useky
                .iter()
                .filter(|u| u[0] + u[2] <= u[1])
                .count()
                .clamp(1, sp.limit)
        };
        let mut vlakna: Vec<tokio::task::JoinHandle<Result<(), ChybaUseku>>> = (0..pocet)
            .map(|_| tokio::spawn(self.clone().pracovnik()))
            .collect();

        let mut okno = vec![(Instant::now(), self.stiahnute())];
        let mut ulozene = Instant::now();
        let mut bez_range = false;
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let mut i = 0;
            while i < vlakna.len() {
                if vlakna[i].is_finished() {
                    let h = vlakna.swap_remove(i);
                    if let Ok(Err(ChybaUseku::BezRange)) = h.await {
                        bez_range = true;
                        self.zastav.cancel();
                    }
                } else {
                    i += 1;
                }
            }
            let teraz = Instant::now();
            let hotovo = self.stiahnute();
            okno.push((teraz, hotovo));
            while okno.len() > 2 && teraz.duration_since(okno[0].0) > Duration::from_secs(5) {
                okno.remove(0);
            }
            let dt = teraz.duration_since(okno[0].0).as_secs_f64().max(1e-3);
            (self.m.hlasic)(Udalost::Priebeh {
                stiahnute: hotovo,
                celkom: Some(self.celkom),
                rychlost: (hotovo - okno[0].1) as f64 / dt,
                cast: None,
            });
            if teraz.duration_since(ulozene) > Duration::from_secs(1) {
                self.uloz_stav();
                ulozene = teraz;
            }
            if vlakna.is_empty() {
                break;
            }
            // voľný limit a veľký zvyšok → pridaj spojenie (prevezme polovicu zvyšku)
            let pridaj = {
                let sp = self.sp.lock().unwrap();
                vlakna.len() < sp.limit
                    && sp
                        .useky
                        .iter()
                        .any(|u| (u[1] + 1).saturating_sub(u[0] + u[2]) >= 2 * MIN_USEK)
            };
            if pridaj && !self.zastav.is_cancelled() {
                vlakna.push(tokio::spawn(self.clone().pracovnik()));
            }
        }
        self.uloz_stav();
        if bez_range {
            return Err(Chyba::Ine(BEZ_RANGE.into()));
        }
        if self.m.zrusenie.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        if self.stiahnute() < self.celkom {
            let posledna = self
                .sp
                .lock()
                .unwrap()
                .chyby
                .last()
                .cloned()
                .unwrap_or_else(|| "neznáma chyba".into());
            return Err(Chyba::Siet(format!(
                "sťahovanie sa nedokončilo: {posledna}"
            )));
        }
        (self.m.hlasic)(Udalost::Priebeh {
            stiahnute: self.celkom,
            celkom: Some(self.celkom),
            rychlost: 0.0,
            cast: None,
        });
        let finalna = volna_cesta(&self.ciel);
        tokio::fs::rename(&self.part, &finalna).await?;
        let _ = tokio::fs::remove_file(&self.stav).await;
        Ok(finalna)
    }
}
