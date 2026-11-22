//! Príkazový riadok jadra Kometa (dočasná kostra – rozširuje sa o HLS/DASH).

use kometa_jadro::http::{self, Moznosti};
use kometa_jadro::klient::{self, Nastavenia};
use kometa_jadro::udalosti::{Hlasic, Udalost};
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Použitie: kometa URL CIEĽ [spojenia]");
        std::process::exit(1);
    }
    let n = Nastavenia { spojenia: args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4), ..Default::default() };
    let k = klient::klient(&n).expect("klient");
    let z = CancellationToken::new();
    let z2 = z.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        z2.cancel();
    });
    let hlasic: Hlasic = Arc::new(|u| println!("{}", serde_json::to_string(&u).unwrap()));
    let m = Moznosti { spojenia: n.spojenia, ocakavana_velkost: None, zrusenie: z, hlasic: hlasic.clone() };
    match http::stiahni(&k, http::pevna_url(args[1].clone()), &PathBuf::from(&args[2]), &m).await {
        Ok(p) => hlasic(Udalost::Hotovo { subor: p.display().to_string() }),
        Err(e) => {
            hlasic(Udalost::Chyba { sprava: e.to_string(), kod: e.kod().into() });
            std::process::exit(if matches!(e, kometa_jadro::chyba::Chyba::Pauza) { 130 } else { 1 });
        }
    }
}
