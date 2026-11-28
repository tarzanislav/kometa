//! Príkazový riadok jadra Kometa.
//!
//! Návratové kódy: 0 hotovo, 1 chyba, 2 šifrovaný obsah (DRM), 4 živé vysielanie,
//! 130 pozastavené (Ctrl+C) – pri ďalšom spustení s rovnakým cieľom sa pokračuje.

use clap::{Args, Parser, Subcommand};
use kometa_jadro::chyba::{Chyba, Vysledok};
use kometa_jadro::http::{self, Moznosti};
use kometa_jadro::klient::{self, bezpecny_nazov, volna_cesta, Nastavenia};
use kometa_jadro::media;
use kometa_jadro::stream;
use kometa_jadro::udalosti;
use kometa_jadro::udalosti::{Hlasic, Udalost};
use kometa_jadro::web::{self, riesenie::Ciel};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "kometa",
    version,
    about = "Kometa – vlastné jadro na sťahovanie: súbory s pokračovaním, HLS a DASH"
)]
struct Cli {
    #[command(subcommand)]
    prikaz: Prikaz,
}

#[derive(Subcommand)]
enum Prikaz {
    /// Otvorí okno Komety (rozhranie v okne prehliadača Edge/Chrome, bez inštalácie)
    Okno {
        /// Port lokálneho servera (0 = voľný)
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Neotvárať okno – len vypísať adresu (pre skúšky a vzdialený prístup cez tunel)
        #[arg(long)]
        bez_okna: bool,
    },
    /// Prehľadá stránku (aj vnorené rámčeky) a vypíše nájdené videá, prehrávače a súbory
    Skenuj {
        url: String,
        /// Výsledok ako JSON
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        volby: Volby,
    },
    /// Zistí, čo je na adrese (pri streame kvality, jazyky a stopy) – nič nesťahuje
    Info {
        url: String,
        #[command(flatten)]
        volby: Volby,
    },
    /// Spojí súbory (MPEG-TS, MP4) do jedného MKV alebo MP4 bez prekódovania – vlastným kódom, bez FFmpeg
    ///
    /// VSTUP je cesta, voliteľne s údajmi oddelenými znakom #: cesta#druh#jazyk#názov#rola,
    /// napr. zvuk.ts#zvuk#cs#Čeština#dabing (druh: video, zvuk, video+zvuk alebo titulky;
    /// rola: povodny alebo dabing).
    Spoj {
        /// Výsledný súbor (.mkv, .mka, .mp4 alebo .m4a)
        #[arg(short = 'o', long = "vystup")]
        vystup: PathBuf,
        /// Názov (titul) výsledku
        #[arg(short = 't', long)]
        titul: Option<String>,
        /// Autor (kanál)
        #[arg(long)]
        autor: Option<String>,
        /// Dátum zverejnenia (RRRR-MM-DD)
        #[arg(long)]
        datum: Option<String>,
        /// Adresa, odkiaľ video pochádza
        #[arg(long)]
        zdroj: Option<String>,
        /// Popis videa
        #[arg(long)]
        popis: Option<String>,
        /// Kapitola v tvare SEKUNDY=Názov (dá sa opakovať)
        #[arg(long = "kapitola")]
        kapitoly: Vec<String>,
        /// Obrázok videa (JPEG alebo PNG)
        #[arg(long)]
        obal: Option<PathBuf>,
        #[arg(required = true)]
        vstupy: Vec<String>,
    },
    /// Stiahne súbor alebo stream (HLS/DASH sa spojí do jedného MKV alebo MP4)
    Stiahni {
        url: String,
        /// Priečinok, kam uložiť
        #[arg(short = 'o', long = "vystup", default_value = ".")]
        vystup: PathBuf,
        /// Názov súboru bez prípony
        #[arg(short = 'n', long)]
        nazov: Option<String>,
        /// Stream ulož ako MP4 namiesto MKV
        #[arg(long)]
        mp4: bool,
        /// Udalosti ako JSON, jedna na riadok (pre okno Komety a skripty)
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        volby: Volby,
    },
}

#[derive(Args)]
struct Volby {
    /// Počet spojení na súbor / súbežných kúskov streamu
    #[arg(short = 's', long, default_value_t = 4)]
    spojenia: usize,
    /// Najvyššia výška obrazu, napr. 1080
    #[arg(long)]
    max_vyska: Option<u32>,
    /// Presne táto kvalita (napr. --presne 1080 = Full HD), inak najbližšia
    #[arg(long, conflicts_with = "max_vyska")]
    presne: Option<u32>,
    /// Jazyky zvuku v poradí, napr. cs,sk,en
    #[arg(short = 'j', long, value_delimiter = ',')]
    jazyky: Vec<String>,
    /// Adresa stránky, na ktorej je prehrávač vložený
    #[arg(long)]
    referer: Option<String>,
}

impl Volby {
    fn nastavenia(&self) -> Nastavenia {
        Nastavenia {
            referer: self.referer.clone(),
            spojenia: self.spojenia.clamp(1, 32),
            max_vyska: self.presne.or(self.max_vyska),
            presna_vyska: self.presne.is_some(),
            jazyky: self.jazyky.clone(),
            ..Default::default()
        }
    }
}

fn hlasic(json: bool) -> Hlasic {
    if json {
        return Arc::new(|u| {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{}", serde_json::to_string(&u).unwrap_or_default());
            let _ = out.flush();
        });
    }
    Arc::new(|u| match u {
        Udalost::Priebeh {
            stiahnute,
            celkom,
            rychlost,
            cast,
        } => {
            let pct = celkom
                .filter(|c| *c > 0)
                .map(|c| format!("{:5.1} %", stiahnute as f64 * 100.0 / c as f64));
            eprint!(
                "\r{:<10} {}  {:>8.2} MB/s   ",
                cast.unwrap_or_default(),
                pct.unwrap_or_else(|| format!("{:.1} MB", stiahnute as f64 / 1_048_576.0)),
                rychlost / 1_048_576.0
            );
        }
        Udalost::Info { sprava } => eprintln!("\r{sprava}"),
        Udalost::Varovanie { sprava } => eprintln!("\r⚠ {sprava}"),
        Udalost::Hotovo { subor } => eprintln!("\r✔ Uložené: {subor}"),
        Udalost::Chyba { sprava, .. } => eprintln!("\r✖ {sprava}"),
    })
}

fn nazov_z_adresy(url: &str) -> String {
    let cesta = url.split(['?', '#']).next().unwrap_or("");
    let posledny = cesta.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let posledny = urlencoding_dekod(posledny);
    if posledny.is_empty() || posledny.contains(':') {
        "kometa".into()
    } else {
        posledny
    }
}

fn urlencoding_dekod(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn s_refererom(n: &Nastavenia, referer: &Option<String>) -> Nastavenia {
    let mut n2 = n.clone();
    if referer.is_some() {
        n2.referer = referer.clone();
    }
    n2
}

/// Čo sa z adresy stiahne: prehrávač, stream, súbor alebo najlepší nález zo stránky.
async fn ciel_adresy(
    k: &reqwest::Client,
    url: &str,
    n: &Nastavenia,
    h: &Hlasic,
) -> Vysledok<(Ciel, Vec<web::skener::Nalez>)> {
    let r = web::riesenie::vyries(k, url, n.referer.as_deref(), h).await?;
    Ok((r.ciel, r.dalsie))
}

/// Okno Komety nad vlastnou službou – rovnaké rozhranie ako aplikácia, v okne prehliadača.
async fn okno(port: u16, bez_okna: bool, zrusenie: CancellationToken) -> Vysledok<()> {
    use kometa_jadro::sluzba::{nastavenia, prostredie::Systemove, server, Sluzba};
    if !server::ma_rozhranie() {
        return Err(Chyba::Ine(
            "Program bol zostavený bez rozhrania (chýba jadro/webui).".into(),
        ));
    }
    let sluzba = Sluzba::new(
        Box::new(Systemove),
        tokio::runtime::Handle::current(),
        "prehliadac",
        kometa_jadro::VERZIA,
    );
    let (adresa, kluc) = server::spusti(sluzba.clone(), port).await?;
    let url = format!("http://127.0.0.1:{}/#t={kluc}", adresa.port());
    println!("Kometa beží na http://127.0.0.1:{}/", adresa.port());
    let mut proces = None;
    if bez_okna {
        println!("KOMETA_ADRESA {url}");
    } else {
        if let Some(exe) = server::prehliadac_pre_okno() {
            let profil = nastavenia::priecinok_nastaveni().join("okno-jadro");
            proces = std::process::Command::new(exe)
                .arg(format!("--app={url}"))
                .arg(format!("--user-data-dir={}", profil.display()))
                .args([
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--window-size=1240,820",
                ])
                .spawn()
                .ok()
                .map(|p| (p, std::time::Instant::now()));
        }
        if proces.is_none() {
            otvor_v_prehliadaci(&url);
        }
    }
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    loop {
        tokio::select! {
            _ = zrusenie.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
        }
        match proces.as_mut() {
            Some((p, zaciatok)) => {
                if let Ok(Some(_)) = p.try_wait() {
                    // okno odovzdané už bežiacemu prehliadaču → ďalej podľa toho, či sa rozhranie ozýva
                    if zaciatok.elapsed().as_secs() < 5 {
                        proces = None;
                    } else {
                        break;
                    }
                }
            }
            None if !bez_okna && sluzba.ticho_sekund() > 45.0 => break,
            None => {}
        }
    }
    sluzba.zatvor().await;
    Ok(())
}

fn otvor_v_prehliadaci(url: &str) {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = c.spawn();
}

async fn spusti(cli: Cli, zrusenie: CancellationToken) -> Vysledok<()> {
    match cli.prikaz {
        Prikaz::Spoj {
            vystup,
            titul,
            autor,
            datum,
            zdroj,
            popis,
            kapitoly,
            obal,
            vstupy,
        } => {
            let vstupy: Vec<media::spajac::Vstup> = vstupy
                .iter()
                .map(|v| {
                    let mut casti = v.split('#');
                    let cesta = PathBuf::from(casti.next().unwrap_or_default());
                    let druh = casti
                        .next()
                        .filter(|x| !x.is_empty())
                        .unwrap_or("video+zvuk")
                        .to_string();
                    let jazyk = casti.next().filter(|x| !x.is_empty()).map(str::to_string);
                    let nazov = casti.next().filter(|x| !x.is_empty()).map(str::to_string);
                    let povodny = match casti.next() {
                        Some("povodny") => Some(true),
                        Some("dabing") => Some(false),
                        _ => None,
                    };
                    media::spajac::Vstup {
                        cesta,
                        druh,
                        jazyk,
                        nazov,
                        povodny,
                    }
                })
                .collect();
            let titul = titul.unwrap_or_else(|| {
                vystup
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string()
            });
            let mut kap = vec![];
            for k in &kapitoly {
                let (cas, meno) = k.split_once('=').unwrap_or((k, ""));
                let sekundy: f64 = cas
                    .trim()
                    .parse()
                    .map_err(|_| Chyba::Ine(format!("kapitola „{k}“: čakám SEKUNDY=Názov")))?;
                kap.push(media::Kapitola {
                    od: (sekundy * 1e9) as i64,
                    nazov: meno.to_string(),
                });
            }
            let obal = match obal {
                Some(c) => Some(media::Obal::z_dat(std::fs::read(&c)?).ok_or_else(|| {
                    Chyba::Ine("obrázok musí byť JPEG alebo PNG (do 20 MB)".into())
                })?),
                None => None,
            };
            let doplnky = media::Doplnky {
                titul,
                autor,
                datum,
                zdroj,
                popis,
                kapitoly: kap,
                obal,
                trvanie: None,
            };
            let ciel = vystup.clone();
            let pocty = tokio::task::spawn_blocking(move || {
                media::spajac::spoj_s(&vstupy, &ciel, &doplnky)
            })
            .await
            .map_err(|e| Chyba::Ine(e.to_string()))??;
            println!(
                "{}",
                serde_json::json!({"subor": vystup.display().to_string(), "vzorky": pocty})
            );
            Ok(())
        }
        Prikaz::Okno { port, bez_okna } => okno(port, bez_okna, zrusenie).await,
        Prikaz::Skenuj { url, json, volby } => {
            let n = volby.nastavenia();
            let k = klient::klient(&n)?;
            let h = if json {
                udalosti::tichy()
            } else {
                hlasic(false)
            };
            let nalezy = web::skener::Skener::new(&k, &h)
                .prehladaj(&url, n.referer.as_deref())
                .await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&nalezy).unwrap_or_default()
                );
            } else if nalezy.is_empty() {
                println!("Na stránke sa nenašlo nič na stiahnutie.");
            } else {
                for x in &nalezy {
                    println!(
                        "{} {:>3}  {:<10} {:<14} {}{}",
                        if x.odporucane { "★" } else { " " },
                        x.skore,
                        x.druh_text,
                        x.poskytovatel,
                        x.url,
                        if x.poznamka.is_empty() {
                            String::new()
                        } else {
                            format!("  ({})", x.poznamka)
                        }
                    );
                }
            }
            Ok(())
        }
        Prikaz::Info { url, volby } => {
            let n = volby.nastavenia();
            let k = klient::klient(&n)?;
            let h = udalosti::tichy();
            let (ciel, dalsie) = ciel_adresy(&k, &url, &n, &h).await?;
            let plan = match &ciel {
                Ciel::Stream {
                    url: s, referer, ..
                } => {
                    let n2 = s_refererom(&n, referer);
                    Some(stream::naplanuj(&klient::klient(&n2)?, s, &n2).await?)
                }
                Ciel::Subor { .. } => None,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"ciel": ciel, "plan": plan, "dalsie": dalsie})
                )
                .unwrap_or_default()
            );
            Ok(())
        }
        Prikaz::Stiahni {
            url,
            vystup,
            nazov,
            mp4,
            json,
            volby,
        } => {
            let n = volby.nastavenia();
            let k = klient::klient(&n)?;
            let h = hlasic(json);
            tokio::fs::create_dir_all(&vystup).await?;
            let (ciel, _) = ciel_adresy(&k, &url, &n, &h).await?;
            let subor = match ciel {
                Ciel::Stream {
                    url: s_url,
                    referer,
                    nazov: zisteny,
                    poskytovatel,
                } => {
                    let n2 = s_refererom(&n, &referer);
                    let k2 = klient::klient(&n2)?;
                    if let Some(z) = &zisteny {
                        udalosti::info(&h, format!("{poskytovatel}: {z}"));
                    }
                    let plan = stream::naplanuj(&k2, &s_url, &n2).await?;
                    let meno = nazov.clone().or(zisteny).unwrap_or_else(|| {
                        let z = nazov_z_adresy(&s_url);
                        Path::new(&z)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("kometa")
                            .to_string()
                    });
                    let pripona = if mp4 { "mp4" } else { "mkv" };
                    // rovnaký cieľ pri opakovanom spustení = pokračovanie (kúsky ostali na disku)
                    let ciel = vystup.join(format!("{}.{pripona}", bezpecny_nazov(&meno)));
                    let ciel = if tokio::fs::try_exists(&ciel).await.unwrap_or(false) {
                        volna_cesta(&ciel)
                    } else {
                        ciel
                    };
                    stream::stiahni(&k2, &plan, &ciel, &meno, n2.spojenia, &zrusenie, &h).await?
                }
                Ciel::Subor {
                    url: f_url,
                    referer,
                    nazov: zisteny,
                    pripona,
                    ..
                } => {
                    let n2 = s_refererom(&n, &referer);
                    let k2 = klient::klient(&n2)?;
                    let s_priponou = |meno: String| match &pripona {
                        Some(p) if Path::new(&meno).extension().is_none() => format!("{meno}.{p}"),
                        _ => meno,
                    };
                    let meno = match (nazov, zisteny) {
                        (Some(x), _) => s_priponou(x),
                        (None, Some(z)) => s_priponou(z),
                        (None, None) => nazov_z_adresy(&f_url),
                    };
                    let m = Moznosti {
                        spojenia: n2.spojenia,
                        ocakavana_velkost: None,
                        zrusenie: zrusenie.clone(),
                        hlasic: h.clone(),
                    };
                    http::stiahni(
                        &k2,
                        http::pevna_url(f_url.clone()),
                        &vystup.join(bezpecny_nazov(&meno)),
                        &m,
                    )
                    .await?
                }
            };
            h(Udalost::Hotovo {
                subor: subor.display().to_string(),
            });
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json = matches!(&cli.prikaz, Prikaz::Stiahni { json: true, .. });
    let zrusenie = CancellationToken::new();
    let z = zrusenie.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        z.cancel();
    });
    if let Err(e) = spusti(cli, zrusenie).await {
        hlasic(json)(Udalost::Chyba {
            sprava: e.to_string(),
            kod: e.kod().into(),
        });
        std::process::exit(match e {
            Chyba::Drm(_) => 2,
            Chyba::Zive => 4,
            Chyba::Nepodporovane(_) => 5,
            Chyba::Pauza => 130,
            _ => 1,
        });
    }
}
