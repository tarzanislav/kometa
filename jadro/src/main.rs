//! Príkazový riadok jadra Kometa.
//!
//! Návratové kódy: 0 hotovo, 1 chyba, 2 šifrovaný obsah (DRM), 4 živé vysielanie,
//! 130 pozastavené (Ctrl+C) – pri ďalšom spustení s rovnakým cieľom sa pokračuje.

use clap::{Args, Parser, Subcommand};
use kometa_jadro::chyba::{Chyba, Vysledok};
use kometa_jadro::http::{self, Moznosti};
use kometa_jadro::klient::{self, bezpecny_nazov, volna_cesta, Nastavenia};
use kometa_jadro::stream;
use kometa_jadro::udalosti::{Hlasic, Udalost};
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
    /// Zistí, čo je na adrese (pri streame kvality, jazyky a stopy) – nič nesťahuje
    Info {
        url: String,
        #[command(flatten)]
        volby: Volby,
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
            max_vyska: self.max_vyska,
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

async fn spusti(cli: Cli, zrusenie: CancellationToken) -> Vysledok<()> {
    match cli.prikaz {
        Prikaz::Info { url, volby } => {
            let n = volby.nastavenia();
            let k = klient::klient(&n)?;
            if stream::je_stream(&k, &url).await? {
                let plan = stream::naplanuj(&k, &url, &n).await?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plan).unwrap_or_default()
                );
            } else {
                println!(
                    "{}",
                    serde_json::json!({"format": "subor", "nazov": nazov_z_adresy(&url)})
                );
            }
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
            let subor = if stream::je_stream(&k, &url).await? {
                let plan = stream::naplanuj(&k, &url, &n).await?;
                let meno = nazov.clone().unwrap_or_else(|| {
                    let z = nazov_z_adresy(&url);
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
                stream::stiahni(&k, &plan, &ciel, &meno, n.spojenia, &zrusenie, &h).await?
            } else {
                let meno = nazov
                    .map(|x| bezpecny_nazov(&x))
                    .unwrap_or_else(|| bezpecny_nazov(&nazov_z_adresy(&url)));
                let m = Moznosti {
                    spojenia: n.spojenia,
                    ocakavana_velkost: None,
                    zrusenie: zrusenie.clone(),
                    hlasic: h.clone(),
                };
                http::stiahni(&k, http::pevna_url(url.clone()), &vystup.join(meno), &m).await?
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
            Chyba::Pauza => 130,
            _ => 1,
        });
    }
}
