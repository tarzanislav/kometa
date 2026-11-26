//! `kometa okno`: rozhranie v okne prehliadača, bez inštalácie.
//!
//! Malý server HTTP/1.1 počúva len na 127.0.0.1. Rozhranie je vložené v programe; na volania
//! odpovedá len s tajným 128-bitovým kľúčom (je v adrese za #, takže sa neposiela ďalej ani do
//! histórie servera) a len pre hlavičku Host 127.0.0.1/localhost – ochrana pred DNS rebinding.

use super::Sluzba;
use serde_json::Value;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

mod vstavane {
    include!(concat!(env!("OUT_DIR"), "/webui.rs"));
}

const MAX_HLAVICKA: usize = 32 * 1024;
const MAX_TELO: usize = 4 * 1024 * 1024;

pub fn ma_rozhranie() -> bool {
    vstavane::SUBORY.iter().any(|(n, _)| *n == "index.html")
}

fn typ(meno: &str) -> &'static str {
    match meno.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 128-bitový náhodný kľúč z generátora operačného systému.
pub fn novy_kluc() -> String {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).expect("generátor náhodných čísel systému");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Poziadavka {
    metoda: String,
    cesta: String,
    hlavicky: Vec<(String, String)>,
    telo: Vec<u8>,
}

impl Poziadavka {
    fn hlavicka(&self, k: &str) -> Option<&str> {
        self.hlavicky
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(k))
            .map(|(_, b)| b.as_str())
    }
}

async fn citaj(s: &mut TcpStream) -> Option<Poziadavka> {
    let mut buf = Vec::with_capacity(4096);
    let mut kus = [0u8; 4096];
    let koniec = loop {
        let n = tokio::time::timeout(Duration::from_secs(15), s.read(&mut kus))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&kus[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HLAVICKA {
            return None;
        }
    };
    let hlava = std::str::from_utf8(&buf[..koniec]).ok()?;
    let mut riadky = hlava.split("\r\n");
    let mut prvy = riadky.next()?.split(' ');
    let (metoda, cesta) = (prvy.next()?.to_string(), prvy.next()?.to_string());
    let hlavicky: Vec<(String, String)> = riadky
        .filter_map(|r| r.split_once(':'))
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
        .collect();
    let dlzka: usize = hlavicky
        .iter()
        .find(|(a, _)| a.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, b)| b.parse().ok())
        .unwrap_or(0);
    if dlzka > MAX_TELO {
        return None;
    }
    let mut telo = buf[koniec + 4..].to_vec();
    while telo.len() < dlzka {
        let n = tokio::time::timeout(Duration::from_secs(15), s.read(&mut kus))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            return None;
        }
        telo.extend_from_slice(&kus[..n]);
    }
    telo.truncate(dlzka);
    Some(Poziadavka {
        metoda,
        cesta,
        hlavicky,
        telo,
    })
}

async fn posli(s: &mut TcpStream, kod: u16, typ: &str, telo: &[u8], html: bool) {
    let stav = match kod {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Bad Request",
    };
    let mut h = format!(
        "HTTP/1.1 {kod} {stav}\r\nContent-Type: {typ}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nConnection: close\r\n",
        telo.len()
    );
    if html {
        h.push_str(
            "Content-Security-Policy: default-src 'self'; img-src 'self' https: data:; style-src 'self' 'unsafe-inline'; \
             font-src 'self'; connect-src 'self'; frame-ancestors 'none'\r\n",
        );
    }
    h.push_str("\r\n");
    let _ = s.write_all(h.as_bytes()).await;
    let _ = s.write_all(telo).await;
    let _ = s.shutdown().await;
}

async fn obsluz(mut s: TcpStream, sluzba: Sluzba, kluc: String, port: u16) {
    let Some(p) = citaj(&mut s).await else { return };
    let host = p.hlavicka("host").unwrap_or("");
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return posli(&mut s, 403, "text/plain", b"", false).await;
    }
    let cesta = p.cesta.split(['?', '#']).next().unwrap_or("/");
    if let Some(metoda) = cesta.strip_prefix("/api/") {
        if p.metoda != "POST" {
            return posli(&mut s, 405, "text/plain", b"", false).await;
        }
        // porovnanie v konštantnom čase
        let dany = p.hlavicka("x-kometa").unwrap_or("").as_bytes();
        let zhoda = dany.len() == kluc.len()
            && dany
                .iter()
                .zip(kluc.as_bytes())
                .fold(0u8, |a, (x, y)| a | (x ^ y))
                == 0;
        if !zhoda {
            return posli(&mut s, 403, "text/plain", b"", false).await;
        }
        let args: Vec<Value> = if p.telo.is_empty() {
            vec![]
        } else {
            match serde_json::from_slice::<Value>(&p.telo) {
                Ok(Value::Array(a)) => a,
                _ => return posli(&mut s, 400, "text/plain", b"", false).await,
            }
        };
        let odpoved = sluzba.volaj(metoda, args).await;
        let telo = serde_json::to_vec(&odpoved).unwrap_or_default();
        return posli(&mut s, 200, "application/json; charset=utf-8", &telo, false).await;
    }
    if p.metoda != "GET" {
        return posli(&mut s, 405, "text/plain", b"", false).await;
    }
    let meno = match cesta.trim_start_matches('/') {
        "" => "index.html",
        m => m,
    };
    match vstavane::SUBORY.iter().find(|(n, _)| *n == meno) {
        Some((n, data)) => posli(&mut s, 200, typ(n), data, n.ends_with(".html")).await,
        None => posli(&mut s, 404, "text/plain", b"", false).await,
    }
}

/// Spustí server; vráti adresu a kľúč. Beží, kým beží runtime.
pub async fn spusti(sluzba: Sluzba, port: u16) -> std::io::Result<(SocketAddr, String)> {
    let l = TcpListener::bind(("127.0.0.1", port)).await?;
    let adresa = l.local_addr()?;
    let kluc = novy_kluc();
    let k2 = kluc.clone();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else {
                continue;
            };
            tokio::spawn(obsluz(s, sluzba.clone(), k2.clone(), adresa.port()));
        }
    });
    Ok((adresa, kluc))
}

/// Edge, Chrome alebo Chromium – okno bez panelov prehliadača (`--app`).
pub fn prehliadac_pre_okno() -> Option<std::path::PathBuf> {
    let mut kandidati: Vec<std::path::PathBuf> = vec![];
    if cfg!(windows) {
        for env in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(env) {
                let d = std::path::PathBuf::from(d);
                kandidati.push(d.join("Microsoft/Edge/Application/msedge.exe"));
                kandidati.push(d.join("Google/Chrome/Application/chrome.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        for a in [
            "Google Chrome",
            "Microsoft Edge",
            "Chromium",
            "Brave Browser",
        ] {
            kandidati.push(format!("/Applications/{a}.app/Contents/MacOS/{a}").into());
        }
    }
    if let Some(p) = kandidati.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    [
        "microsoft-edge",
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave-browser",
    ]
    .iter()
    .find_map(|m| super::nastroje::v_ceste(m))
}
