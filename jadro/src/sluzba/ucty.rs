//! Úložiská súborov: Webshare.cz (oficiálne API), Fastshare.cz (prihlásenie + stránka súboru)
//! a Datoid.cz (rozhranie api.datoid.cz).
//!
//! Sťahuje sa vždy cez vlastný účet používateľa. Kometa neobchádza obmedzenia bezplatných účtov
//! (čakanie, captcha, rýchlosť) – na rýchle sťahovanie treba VIP (Webshare) alebo kredit
//! (Fastshare, Datoid).
//! Heslá sú v trezore systému (Windows: Správca poverení, macOS: Kľúčenka, Linux: Secret Service
//! cez `secret-tool`), inak v súbore `ucty.dat`. Miesta sú tie isté, aké používala Kometa pre
//! Python, takže účty z nej ostanú.

use super::fronta::Polozka;
use super::stahovanie::Koniec;
use super::Vnutro;
use crate::chyba::{Chyba, Vysledok};
use crate::http::{self, Moznosti, ZdrojUrl};
use crate::udalosti::Hlasic;
use md5::{Digest as _, Md5};
use reqwest::Client;
use serde_json::{json, Value};
use sha1::Sha1;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const SLUZBY: [&str; 3] = ["webshare", "fastshare", "datoid"];

/// Názov služby pre ľudí („webshare“ → „Webshare“) – zároveň `zdroj` položiek vo fronte.
pub fn nazov_sluzby(s: &str) -> &'static str {
    match s {
        "webshare" => "Webshare",
        "fastshare" => "Fastshare",
        "datoid" => "Datoid",
        _ => "Web",
    }
}

fn chyba(t: impl Into<String>) -> Chyba {
    Chyba::Ine(t.into())
}

// ---------------------------------------------------------------------------------------------
// MD5-crypt (rovnaký ako `openssl passwd -1`) – Webshare ním solí heslo
// ---------------------------------------------------------------------------------------------

const ITOA64: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn md5_crypt(heslo: &str, sol: &str) -> String {
    let pw = heslo.as_bytes();
    let sol: String = sol.chars().take(8).collect();
    let sb = sol.as_bytes();
    let alt = Md5::new()
        .chain_update(pw)
        .chain_update(sb)
        .chain_update(pw)
        .finalize();
    let mut ctx = Md5::new()
        .chain_update(pw)
        .chain_update(b"$1$")
        .chain_update(sb);
    let mut n = pw.len() as isize;
    while n > 0 {
        ctx.update(&alt[..(n.min(16) as usize)]);
        n -= 16;
    }
    let mut n = pw.len();
    while n > 0 {
        if n & 1 == 1 {
            ctx.update([0u8]);
        } else {
            ctx.update(&pw[..pw.len().min(1)]);
        }
        n >>= 1;
    }
    let mut f = ctx.finalize();
    for i in 0..1000 {
        let mut c = Md5::new();
        if i & 1 == 1 {
            c.update(pw)
        } else {
            c.update(f)
        }
        if i % 3 != 0 {
            c.update(sb);
        }
        if i % 7 != 0 {
            c.update(pw);
        }
        if i & 1 == 1 {
            c.update(f)
        } else {
            c.update(pw)
        }
        f = c.finalize();
    }
    let to64 = |mut v: u32, pocet: usize, out: &mut String| {
        for _ in 0..pocet {
            out.push(ITOA64[(v & 0x3f) as usize] as char);
            v >>= 6;
        }
    };
    let b = |i: usize| f[i] as u32;
    let mut out = format!("$1${sol}$");
    for (a, c, d) in [(0, 6, 12), (1, 7, 13), (2, 8, 14), (3, 9, 15), (4, 10, 5)] {
        to64((b(a) << 16) | (b(c) << 8) | b(d), 4, &mut out);
    }
    to64(b(11), 2, &mut out);
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------------------------------------
// Webshare
// ---------------------------------------------------------------------------------------------

fn zaklad_webshare() -> String {
    std::env::var("KOMETA_WEBSHARE_ZAKLAD")
        .unwrap_or_else(|_| "https://webshare.cz/api".into())
        .trim_end_matches('/')
        .to_string()
}

#[derive(Clone)]
pub struct Webshare {
    k: Client,
    pub token: String,
}

/// ID súboru z odkazu `webshare.cz/#/file/ID/…`.
pub fn id_webshare(url: &str) -> Option<String> {
    let u = url.trim();
    let i = u.find("webshare.cz/")? + "webshare.cz/".len();
    let zvysok = u[i..].trim_start_matches("#/");
    let id: String = zvysok
        .strip_prefix("file/")?
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!id.is_empty()).then_some(id)
}

impl Webshare {
    pub fn new(token: &str) -> Vysledok<Self> {
        let k = Client::builder()
            .user_agent(UA)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Webshare {
            k,
            token: token.to_string(),
        })
    }

    async fn volaj(
        &self,
        metoda: &str,
        data: &[(&str, &str)],
    ) -> Vysledok<HashMap<String, String>> {
        let r = self
            .k
            .post(format!("{}/{metoda}/", zaklad_webshare()))
            .header("Accept", "text/xml; charset=UTF-8")
            .form(data)
            .send()
            .await
            .map_err(|e| chyba(format!("Webshare nie je dostupný: {e}")))?;
        let r = r
            .error_for_status()
            .map_err(|e| chyba(format!("Webshare nie je dostupný: {e}")))?;
        let t = r.text().await?;
        let d = roxmltree::Document::parse(&t)
            .map_err(|_| chyba("Webshare vrátil neočakávanú odpoveď."))?;
        let m: HashMap<String, String> = d
            .root_element()
            .children()
            .filter(|n| n.is_element())
            .map(|n| {
                (
                    n.tag_name().name().to_string(),
                    n.text().unwrap_or("").trim().to_string(),
                )
            })
            .collect();
        if m.get("status").map(|s| s.to_ascii_uppercase()) != Some("OK".into()) {
            let sprava = m
                .get("message")
                .or(m.get("code"))
                .cloned()
                .unwrap_or_else(|| "neznáma chyba".into());
            return Err(chyba(format!("Webshare: {sprava}")));
        }
        Ok(m)
    }

    pub async fn prihlas(&mut self, meno: &str, heslo: &str) -> Vysledok<String> {
        let sol = self
            .volaj("salt", &[("username_or_email", meno)])
            .await?
            .remove("salt")
            .unwrap_or_default();
        let hashed = hex(&Sha1::digest(md5_crypt(heslo, &sol).as_bytes()));
        let digest = hex(&Md5::digest(format!("{meno}:Webshare:{heslo}").as_bytes()));
        let m = self
            .volaj(
                "login",
                &[
                    ("username_or_email", meno),
                    ("password", &hashed),
                    ("digest", &digest),
                    ("keep_logged_in", "1"),
                ],
            )
            .await
            .map_err(|e| {
                chyba(format!(
                    "Prihlásenie na Webshare zlyhalo – skontroluj meno a heslo. ({e})"
                ))
            })?;
        self.token = m
            .get("token")
            .cloned()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| chyba("Webshare neposlal prihlasovací token."))?;
        Ok(self.token.clone())
    }

    pub async fn info(&self) -> Vysledok<(bool, String)> {
        let m = self.volaj("user_data", &[("wst", &self.token)]).await?;
        let vip = m.get("vip").map(|v| v == "1").unwrap_or(false);
        let dni = m.get("vip_days").cloned().unwrap_or_default();
        let do_: String = m
            .get("vip_until")
            .cloned()
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect();
        Ok(if vip {
            (
                true,
                format!(
                    "VIP do {do_}{}",
                    if dni.is_empty() {
                        String::new()
                    } else {
                        format!(" ({dni} dní)")
                    }
                ),
            )
        } else {
            (false, "bez VIP – sťahovanie bude pomalé".into())
        })
    }

    /// (názov, veľkosť, chránený heslom)
    pub async fn subor(&self, id: &str) -> Vysledok<(String, Option<u64>, bool)> {
        let m = self
            .volaj("file_info", &[("ident", id), ("wst", &self.token)])
            .await?;
        Ok((
            m.get("name")
                .cloned()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| id.to_string()),
            m.get("size").and_then(|s| s.parse().ok()),
            m.get("password").is_some_and(|p| p == "1"),
        ))
    }

    /// Heslo k súboru pre file_link: sha1(md5_crypt(heslo, soľ súboru)) – ako pri prihlásení.
    async fn heslo_suboru(&self, id: &str, heslo: &str) -> Vysledok<String> {
        let sol = self
            .volaj("file_password_salt", &[("ident", id), ("wst", &self.token)])
            .await?
            .remove("salt")
            .unwrap_or_default();
        Ok(hex(&Sha1::digest(md5_crypt(heslo, &sol).as_bytes())))
    }

    pub async fn odkaz(&self, id: &str, heslo: &str) -> Vysledok<String> {
        let zakodovane = if heslo.is_empty() {
            String::new()
        } else {
            self.heslo_suboru(id, heslo).await?
        };
        let mut data = vec![
            ("ident", id),
            ("wst", &self.token),
            ("download_type", "file_download"),
            ("force_https", "1"),
        ];
        if !zakodovane.is_empty() {
            data.push(("password", &zakodovane));
        }
        let m = self.volaj("file_link", &data).await.map_err(|e| {
            if heslo.is_empty() {
                e
            } else {
                Chyba::Heslo(format!(
                    "Heslo k súboru na Webshare nesedí – zadaj ho znova. ({e})"
                ))
            }
        })?;
        m.get("link")
            .cloned()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| chyba("Webshare nevrátil odkaz na stiahnutie."))
    }
}

// ---------------------------------------------------------------------------------------------
// Datoid – rozhranie api.datoid.cz/v1 (JSON): login, get-user-details, get-file-details,
// get-download-link
// ---------------------------------------------------------------------------------------------

fn zaklad_datoid() -> String {
    std::env::var("KOMETA_DATOID_ZAKLAD")
        .unwrap_or_else(|_| "https://api.datoid.cz/v1".into())
        .trim_end_matches('/')
        .to_string()
}

#[derive(Clone)]
pub struct Datoid {
    k: Client,
    pub token: String,
}

/// Údaje o súbore na Datoide.
#[derive(Debug, PartialEq)]
pub struct SuborDatoid {
    pub nazov: Option<String>,
    pub velkost: Option<u64>,
}

/// Chyba z odpovede Datoidu ako zrozumiteľná veta (None = bez chyby).
pub fn chyba_datoid(j: &Value) -> Option<Chyba> {
    if j.get("success").and_then(Value::as_bool) == Some(false) && j.get("error").is_none() {
        return Some(chyba(
            "Prihlásenie na Datoid zlyhalo – skontroluj e-mail a heslo.",
        ));
    }
    let e = j.get("error").and_then(Value::as_str)?;
    let m = e.to_ascii_lowercase();
    Some(if m.contains("password") {
        Chyba::Heslo("Súbor na Datoide je chránený heslom – zadaj ho.".into())
    } else if m.contains("not found") || m.contains("deleted") || m.contains("blocked") {
        chyba("Súbor na Datoide už neexistuje.")
    } else if m.contains("credit") {
        chyba("Na Datoide nemáš dosť kreditu na tento súbor.")
    } else if m.contains("token") || m.contains("login") {
        chyba("Na Datoide nie si prihlásený – zadaj účet v karte Účty.")
    } else {
        chyba(format!("Datoid: {e}"))
    })
}

/// Číslo z JSON (Datoid niekedy posiela čísla ako text).
fn cislo(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f as u64))
        .or_else(|| v.as_str()?.trim().parse().ok())
}

impl Datoid {
    pub fn new() -> Vysledok<Self> {
        let k = Client::builder()
            .user_agent(UA)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Datoid {
            k,
            token: String::new(),
        })
    }

    async fn volaj(&self, metoda: &str, parametre: &[(&str, &str)]) -> Vysledok<Value> {
        let r = self
            .k
            .get(format!("{}/{metoda}", zaklad_datoid()))
            .query(parametre)
            .send()
            .await
            .map_err(|e| chyba(format!("Datoid nie je dostupný: {e}")))?
            .error_for_status()
            .map_err(|e| chyba(format!("Datoid nie je dostupný: {e}")))?;
        let j: Value = serde_json::from_str(&r.text().await?)
            .map_err(|_| chyba("Datoid vrátil neočakávanú odpoveď."))?;
        match chyba_datoid(&j) {
            Some(e) => Err(e),
            None => Ok(j),
        }
    }

    pub async fn prihlas(&mut self, email: &str, heslo: &str) -> Vysledok<String> {
        let j = self
            .volaj("login", &[("email", email), ("password", heslo)])
            .await?;
        self.token = j
            .get("token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| chyba("Prihlásenie na Datoid zlyhalo – skontroluj e-mail a heslo."))?
            .to_string();
        Ok(self.token.clone())
    }

    pub async fn info(&self) -> Vysledok<(bool, String)> {
        let j = self
            .volaj("get-user-details", &[("token", &self.token)])
            .await?;
        let kredit = j.get("credits").map(|c| match c {
            Value::String(s) => s.clone(),
            c => c.to_string(),
        });
        Ok(match kredit {
            Some(k) if k.trim_start_matches(['0', '.', ',']).is_empty() => {
                (false, "bez kreditu – sťahovanie nebude možné".into())
            }
            Some(k) => (true, format!("kredit {k}")),
            None => (true, "prihlásený".into()),
        })
    }

    pub async fn subor(&self, url: &str, heslo: &str) -> Vysledok<SuborDatoid> {
        let mut q = vec![("url", url)];
        if !heslo.is_empty() {
            q.push(("password", heslo));
        }
        let j = self.volaj("get-file-details", &q).await?;
        Ok(SuborDatoid {
            nazov: j
                .get("filename")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|n| !n.trim().is_empty()),
            velkost: cislo(j.get("filesize_bytes")).or_else(|| cislo(j.get("filesize"))),
        })
    }

    pub async fn odkaz(&self, url: &str, heslo: &str) -> Vysledok<String> {
        let mut q = vec![("token", self.token.as_str()), ("url", url)];
        if !heslo.is_empty() {
            q.push(("password", heslo));
        }
        let j = self.volaj("get-download-link", &q).await?;
        j.get("download_link")
            .and_then(Value::as_str)
            .map(|l| l.replace('\\', ""))
            .filter(|l| l.starts_with("http"))
            .ok_or_else(|| chyba("Datoid nevrátil odkaz na stiahnutie."))
    }
}

// ---------------------------------------------------------------------------------------------
// Fastshare
// ---------------------------------------------------------------------------------------------

fn zaklad_fastshare() -> String {
    std::env::var("KOMETA_FASTSHARE_ZAKLAD")
        .unwrap_or_else(|_| "https://fastshare.cz".into())
        .trim_end_matches('/')
        .to_string()
}

const PRIHLASENY: &str = "href=\"/logout.php\"";

#[derive(Clone)]
pub struct Fastshare {
    pub k: Client,
}

/// Údaje zo stránky súboru Fastshare.
#[derive(Debug, PartialEq)]
pub struct SuborFastshare {
    pub nazov: Option<String>,
    pub velkost: Option<u64>,
    pub odkaz: String,
}

fn medzi<'a>(t: &'a str, zac: &str, kon: &str) -> Option<&'a str> {
    let i = t.find(zac)? + zac.len();
    let j = t[i..].find(kon)?;
    Some(&t[i..i + j])
}

/// Vyzerá ako názov súboru s príponou („film.2024.1080p.mkv“)?
pub fn je_nazov_suboru(n: &str) -> bool {
    let n = n.trim();
    match n.rsplit_once('.') {
        Some((a, b)) => {
            !a.trim().is_empty()
                && (1..=5).contains(&b.len())
                && b.chars().all(|c| c.is_ascii_alphanumeric())
                && b.chars().any(|c| c.is_ascii_alphabetic())
                && !n.contains(['/', '\\', '<', '>', '"'])
                // nie stránka ani doména („FastShare.cloud“, „download.php“)
                && !["php", "html", "htm", "asp", "cz", "sk", "com", "cloud", "net", "org", "eu"]
                    .iter()
                    .any(|x| b.eq_ignore_ascii_case(x))
        }
        None => false,
    }
}

/// Názov z hlavičky Content-Disposition (filename*=UTF-8''… má prednosť pred filename=…).
pub fn nazov_z_hlavicky(h: &str) -> Option<String> {
    let mut obycajny = None;
    for cast in h.split(';') {
        let Some((k, hod)) = cast.trim().split_once('=') else {
            continue;
        };
        let k = k.trim().to_ascii_lowercase();
        let hod = hod.trim().trim_matches('"');
        if k == "filename*" {
            let kod = hod.rsplit_once("''").map_or(hod, |(_, x)| x);
            let n = super::stahovanie::nazov_z_adresy(&format!("x/{}", kod.replace('+', "%2B")));
            if !n.is_empty() && n != "kometa" {
                return Some(n);
            }
        } else if k == "filename" && !hod.is_empty() {
            // servery často posielajú UTF-8 ako „latin-1“ bajty
            let b: Option<Vec<u8>> = hod.chars().map(|c| u8::try_from(c as u32).ok()).collect();
            obycajny = Some(
                b.and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| hod.to_string()),
            );
        }
    }
    obycajny.map(|n| n.rsplit(['/', '\\']).next().unwrap_or(&n).to_string())
}

/// Názov z odkazu Fastshare – adresa má tvar …/12345/nazov.suboru.mkv.
pub fn nazov_z_odkazu_fastshare(url: &str) -> Option<String> {
    let n = super::stahovanie::nazov_z_adresy(url);
    je_nazov_suboru(&n).then_some(n)
}

/// Rozoberie stránku súboru (bez regulárnych výrazov – presne ako ju Fastshare píše).
pub fn rozober_fastshare(html: &str) -> Vysledok<SuborFastshare> {
    for x in [
        "The file has been deleted",
        "Requested page not found",
        "This file is no longer available",
    ] {
        if html.contains(&format!(">{x}")) {
            return Err(chyba("Súbor na Fastshare už neexistuje."));
        }
    }
    let nazov = medzi(html, "<h2 title=\"", "\" class=\"section_title")
        .map(crate::web::html::entity)
        .or_else(|| {
            // náhradné miesta, ak sa vzhľad stránky zmení – len ak to naozaj vyzerá ako názov súboru
            [
                ("property=\"og:title\" content=\"", "\""),
                ("<title>", "</title>"),
            ]
            .iter()
            .filter_map(|(z, k)| medzi(html, z, k))
            .map(|t| crate::web::html::entity(t.trim()))
            .map(|t| {
                t.split(" | ")
                    .next()
                    .unwrap_or(&t)
                    .split(" - FastShare")
                    .next()
                    .unwrap_or(&t)
                    .trim()
                    .to_string()
            })
            .find(|t| je_nazov_suboru(t))
        });
    let velkost = html.find("<i class=\"fa fa-bars\"></i>").and_then(|i| {
        let t = html[i + 26..].trim_start().replace("&nbsp;", " ");
        let cislo: String = t
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
            .collect();
        let jednotka: String = t[cislo.len()..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let nasob = match jednotka.to_ascii_uppercase().as_str() {
            "B" => 1u64,
            "KB" => 1 << 10,
            "MB" => 1 << 20,
            "GB" => 1 << 30,
            "TB" => 1 << 40,
            _ => return None,
        };
        Some((cislo.replace(',', ".").parse::<f64>().ok()? * nasob as f64) as u64)
    });
    let mut od = 0;
    while let Some(i) = html[od..].find("/download.php?id=") {
        let i = od + i;
        let zac = html[..i].rfind("http").unwrap_or(i);
        let host = &html[zac..i];
        let cisla: String = html[i + 17..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let za = html[i + 17 + cisla.len()..].chars().next();
        let host_ok = (host.starts_with("https://") || host.starts_with("http://"))
            && (host.ends_with(".fastshare.cz") || host.ends_with(".fastshare.cloud"))
            && !host.contains(['"', '\'', ' ', '<']);
        if host_ok && !cisla.is_empty() && za == Some('&') {
            return Ok(SuborFastshare {
                nazov,
                velkost,
                odkaz: format!("{host}/download.php?id={cisla}"),
            });
        }
        od = i + 17;
    }
    if html.contains(" credit for ") {
        return Err(chyba("Na Fastshare nemáš dosť kreditu na tento súbor."));
    }
    if !html.contains(PRIHLASENY) {
        return Err(chyba(
            "Na Fastshare nie si prihlásený – zadaj účet v karte Účty.",
        ));
    }
    Err(chyba(
        "Na stránke Fastshare sa nenašiel odkaz na stiahnutie (stránka sa mohla zmeniť).",
    ))
}

/// Stav účtu zo stránky /user.
pub fn info_fastshare(html: &str) -> Vysledok<(bool, String)> {
    if !html.contains(PRIHLASENY) {
        return Err(chyba("Na Fastshare nie si prihlásený."));
    }
    if let Some(d) = medzi(html, ">Active until ", "<") {
        return Ok((true, format!("neobmedzené sťahovanie do {}", d.trim())));
    }
    if html.contains(">Unlimited downloading<") {
        return Ok((true, "neobmedzené sťahovanie".into()));
    }
    let mut od = 0;
    while let Some(i) = html[od..].find("<td>") {
        let zac = od + i + 4;
        if let Some(j) = html[zac..].find("</td>") {
            let bunka = html[zac..zac + j].trim();
            if let Some((c, u)) = bunka.split_once(' ') {
                if c.chars()
                    .all(|x| x.is_ascii_digit() || x == '.' || x == ',')
                    && !c.is_empty()
                    && matches!(u, "KB" | "MB" | "GB" | "TB")
                {
                    return Ok((true, format!("kredit {c} {u}")));
                }
            }
        }
        od = zac;
    }
    Ok((false, "bez kreditu – sťahovanie nebude možné".into()))
}

impl Fastshare {
    pub fn new() -> Vysledok<Self> {
        let jar = Arc::new(reqwest::cookie::Jar::default());
        for d in [
            "https://fastshare.cz",
            "https://fastshare.cloud",
            &zaklad_fastshare(),
        ] {
            if let Ok(u) = d.parse::<url::Url>() {
                jar.add_cookie_str("lang=en; Path=/", &u);
            }
        }
        let k = Client::builder()
            .user_agent(UA)
            .cookie_provider(jar)
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Fastshare { k })
    }

    async fn text(&self, url: &str) -> Vysledok<String> {
        let r = self
            .k
            .get(url)
            .send()
            .await
            .map_err(|e| chyba(format!("Fastshare nie je dostupný: {e}")))?;
        Ok(r.error_for_status()
            .map_err(|e| chyba(format!("Fastshare nie je dostupný: {e}")))?
            .text()
            .await?)
    }

    pub async fn prihlas(&self, meno: &str, heslo: &str) -> Vysledok<()> {
        let z = zaklad_fastshare();
        if self.text(&format!("{z}/user")).await?.contains(PRIHLASENY) {
            return Ok(());
        }
        let r = self
            .k
            .post(format!("{z}/sql.php"))
            .form(&[("login", meno), ("heslo", heslo)])
            .send()
            .await
            .map_err(|e| chyba(format!("Fastshare nie je dostupný: {e}")))?;
        let t = r.text().await.unwrap_or_default();
        if !t.contains(PRIHLASENY) && !self.text(&format!("{z}/user")).await?.contains(PRIHLASENY) {
            return Err(chyba(
                "Prihlásenie na Fastshare zlyhalo – skontroluj meno a heslo.",
            ));
        }
        Ok(())
    }

    pub async fn info(&self) -> Vysledok<(bool, String)> {
        info_fastshare(&self.text(&format!("{}/user", zaklad_fastshare())).await?)
    }

    pub async fn subor(&self, url: &str) -> Vysledok<SuborFastshare> {
        let url = url.split('#').next().unwrap_or(url);
        // pre skúšky: stránka súboru z napodobeného servera (cesta ostane rovnaká)
        let url = match std::env::var("KOMETA_FASTSHARE_ZAKLAD") {
            Ok(z) if !z.is_empty() => match url::Url::parse(url) {
                Ok(u) => format!("{}{}", z.trim_end_matches('/'), u.path()),
                Err(_) => url.to_string(),
            },
            _ => url.to_string(),
        };
        let r = self
            .k
            .get(&url)
            .send()
            .await
            .map_err(|e| chyba(format!("Fastshare nie je dostupný: {e}")))?;
        let r = r
            .error_for_status()
            .map_err(|e| chyba(format!("Fastshare nie je dostupný: {e}")))?;
        let typ = r
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !typ.contains("text/html") {
            // s kreditom niekedy presmeruje rovno na súbor
            let odkaz = r.url().to_string();
            let velkost = r.content_length();
            let nazov = r
                .headers()
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(|h| nazov_z_hlavicky(&String::from_utf8_lossy(h.as_bytes())))
                .or_else(|| {
                    let n = super::stahovanie::nazov_z_adresy(&odkaz);
                    je_nazov_suboru(&n).then_some(n)
                });
            return Ok(SuborFastshare {
                nazov,
                velkost,
                odkaz,
            });
        }
        rozober_fastshare(&r.text().await?)
    }

    /// Názov, ktorý pošle server pri sťahovaní (Content-Disposition) – stiahne sa len 1 bajt.
    pub async fn nazov_zo_servera(&self, odkaz: &str) -> Option<String> {
        let r = self
            .k
            .get(odkaz)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
            .ok()?;
        let h = r.headers().get(reqwest::header::CONTENT_DISPOSITION)?;
        nazov_z_hlavicky(&String::from_utf8_lossy(h.as_bytes())).filter(|n| !n.trim().is_empty())
    }
}

// ---------------------------------------------------------------------------------------------
// Heslá
// ---------------------------------------------------------------------------------------------

fn subor_hesiel() -> PathBuf {
    super::nastavenia::priecinok_nastaveni().join("ucty.dat")
}

fn base64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            out.push(if i <= ch.len() {
                A[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn z_base64(t: &str) -> Option<Vec<u8>> {
    let mut out = vec![];
    let mut buf = 0u32;
    let mut bity = 0;
    for c in t.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        buf = buf << 6 | v;
        bity += 6;
        if bity >= 8 {
            bity -= 8;
            out.push((buf >> bity & 0xff) as u8);
        }
    }
    Some(out)
}

fn subor_nacitaj() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(subor_hesiel())
        .ok()
        .and_then(|t| z_base64(&t))
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn subor_uloz(m: &serde_json::Map<String, Value>) {
    let p = subor_hesiel();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(
        p,
        base64(serde_json::to_string(m).unwrap_or_default().as_bytes()),
    );
}

fn sluzba_trezoru(s: &str) -> String {
    format!("kometa:{s}")
}

#[cfg(any(windows, target_os = "macos"))]
mod trezor {
    pub fn uloz(s: &str, u: &str, h: &str) -> bool {
        keyring::Entry::new(s, u)
            .and_then(|e| e.set_password(h))
            .is_ok()
    }
    pub fn nacitaj(s: &str, u: &str) -> Option<String> {
        keyring::Entry::new(s, u).ok()?.get_password().ok()
    }
    pub fn zmaz(s: &str, u: &str) {
        if let Ok(e) = keyring::Entry::new(s, u) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod trezor {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // Secret Service (GNOME Keyring, KWallet) cez secret-tool – rovnaké miesto, aké mala Kometa pre Python
    pub fn uloz(s: &str, u: &str, h: &str) -> bool {
        let Ok(mut c) = Command::new("secret-tool")
            .args([
                "store",
                "--label",
                &format!("Kometa {s}"),
                "service",
                s,
                "username",
                u,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let ok = c
            .stdin
            .take()
            .map(|mut i| i.write_all(h.as_bytes()).is_ok())
            .unwrap_or(false);
        ok && c.wait().map(|s| s.success()).unwrap_or(false)
    }
    pub fn nacitaj(s: &str, u: &str) -> Option<String> {
        let o = Command::new("secret-tool")
            .args(["lookup", "service", s, "username", u])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        (o.status.success() && !o.stdout.is_empty())
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }
    pub fn zmaz(s: &str, u: &str) {
        let _ = Command::new("secret-tool")
            .args(["clear", "service", s, "username", u])
            .stderr(Stdio::null())
            .status();
    }
}

pub fn uloz_heslo(s: &str, u: &str, h: &str) {
    if std::env::var("KOMETA_BEZ_TREZORU").is_err() && trezor::uloz(&sluzba_trezoru(s), u, h) {
        return;
    }
    let mut m = subor_nacitaj();
    m.insert(format!("{s}:{u}"), json!(h));
    subor_uloz(&m);
}

pub fn heslo(s: &str, u: &str) -> Option<String> {
    if u.is_empty() {
        return None;
    }
    if std::env::var("KOMETA_BEZ_TREZORU").is_err() {
        if let Some(h) = trezor::nacitaj(&sluzba_trezoru(s), u) {
            return Some(h);
        }
    }
    subor_nacitaj()
        .get(&format!("{s}:{u}"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub fn zmaz_heslo(s: &str, u: &str) {
    if std::env::var("KOMETA_BEZ_TREZORU").is_err() {
        trezor::zmaz(&sluzba_trezoru(s), u);
    }
    let mut m = subor_nacitaj();
    if m.remove(&format!("{s}:{u}")).is_some() {
        subor_uloz(&m);
    }
}

// ---------------------------------------------------------------------------------------------
// Stav účtov pre okno a sťahovanie
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Ucty {
    stav: Mutex<HashMap<&'static str, Value>>,
    webshare: tokio::sync::Mutex<Option<Webshare>>,
    fastshare: tokio::sync::Mutex<Option<Fastshare>>,
    datoid: tokio::sync::Mutex<Option<Datoid>>,
}

impl Ucty {
    pub fn new() -> Self {
        let u = Ucty::default();
        for s in SLUZBY {
            u.nastav(s, "neprihlaseny", "", "");
        }
        u
    }

    fn nastav(&self, s: &str, stav: &str, text: &str, user: &str) {
        let Some(k) = SLUZBY.iter().copied().find(|x| *x == s) else {
            return;
        };
        self.stav
            .lock()
            .unwrap()
            .insert(k, json!({"stav": stav, "text": text, "user": user}));
    }

    pub fn stav(&self) -> Value {
        json!(self.stav.lock().unwrap().clone())
    }

    async fn reset(&self, s: &str) {
        match s {
            "webshare" => *self.webshare.lock().await = None,
            "fastshare" => *self.fastshare.lock().await = None,
            _ => *self.datoid.lock().await = None,
        }
    }
}

fn meno_v_nastaveniach(v: &Vnutro, s: &str) -> String {
    v.nastavenia.lock().unwrap().text(&format!("{s}_user"))
}

/// Klient Webshare s platným tokenom (uložený token, inak nové prihlásenie).
async fn webshare(v: &Vnutro, znova: bool) -> Vysledok<Webshare> {
    let mut g = v.ucty.webshare.lock().await;
    if let (Some(w), false) = (g.as_ref(), znova) {
        return Ok(w.clone());
    }
    let meno = meno_v_nastaveniach(v, "webshare");
    let token = if znova {
        String::new()
    } else {
        v.nastavenia.lock().unwrap().text("webshare_token")
    };
    let mut w = Webshare::new(&token)?;
    if token.is_empty() || w.info().await.is_err() {
        let h = heslo("webshare", &meno)
            .filter(|_| !meno.is_empty())
            .ok_or_else(|| chyba("Na Webshare nie si prihlásený – zadaj účet v karte Účty."))?;
        let t = w.prihlas(&meno, &h).await?;
        let mut n = v.nastavenia.lock().unwrap();
        n.nastav("webshare_token", json!(t));
        n.uloz();
    }
    *g = Some(w.clone());
    Ok(w)
}

async fn fastshare(v: &Vnutro, znova: bool) -> Vysledok<Fastshare> {
    let mut g = v.ucty.fastshare.lock().await;
    if let (Some(f), false) = (g.as_ref(), znova) {
        return Ok(f.clone());
    }
    let meno = meno_v_nastaveniach(v, "fastshare");
    let f = Fastshare::new()?;
    if let Some(h) = heslo("fastshare", &meno) {
        f.prihlas(&meno, &h).await?;
    }
    *g = Some(f.clone());
    Ok(f)
}

async fn datoid(v: &Vnutro, znova: bool) -> Vysledok<Datoid> {
    let mut g = v.ucty.datoid.lock().await;
    if let (Some(d), false) = (g.as_ref(), znova) {
        return Ok(d.clone());
    }
    let meno = meno_v_nastaveniach(v, "datoid");
    let h = heslo("datoid", &meno)
        .filter(|_| !meno.is_empty())
        .ok_or_else(|| chyba("Na Datoide nie si prihlásený – zadaj účet v karte Účty."))?;
    let mut d = Datoid::new()?;
    d.prihlas(&meno, &h).await?;
    *g = Some(d.clone());
    Ok(d)
}

/// Stav účtu (VIP/kredit, text) – prihlási sa, ak treba.
async fn info_uctu(v: &Vnutro, s: &str) -> Vysledok<(bool, String)> {
    match s {
        "webshare" => webshare(v, false).await?.info().await,
        "fastshare" => fastshare(v, false).await?.info().await,
        _ => datoid(v, false).await?.info().await,
    }
}

/// Po spustení overí uložené účty (na pozadí).
pub fn over_pri_starte(v: &Arc<Vnutro>) {
    for s in SLUZBY {
        let meno = meno_v_nastaveniach(v, s);
        if meno.is_empty() || heslo(s, &meno).is_none() {
            continue;
        }
        v.ucty.nastav(s, "overujem", "Overujem účet…", &meno);
        let v2 = v.clone();
        v.rt_spawn(async move {
            let v = v2;
            let r = info_uctu(&v, s).await;
            match r {
                Ok((vip, text)) => {
                    v.ucty
                        .nastav(s, if vip { "ok" } else { "bez_vip" }, &text, &meno)
                }
                Err(e) => v.ucty.nastav(s, "chyba", &e.to_string(), &meno),
            }
        });
    }
}

pub async fn prihlas(
    v: &Arc<Vnutro>,
    sluzba: &str,
    meno: &str,
    heslo_okna: &str,
) -> Vysledok<Value> {
    if !SLUZBY.contains(&sluzba) {
        return Err(chyba("Neznáma služba."));
    }
    let meno = meno.trim();
    if meno.is_empty() {
        return Err(chyba("Zadaj prihlasovacie meno."));
    }
    let h = if heslo_okna.is_empty() {
        heslo(sluzba, meno).unwrap_or_default()
    } else {
        heslo_okna.to_string()
    };
    if h.is_empty() {
        return Err(chyba("Zadaj heslo."));
    }
    v.ucty.nastav(sluzba, "overujem", "Prihlasujem…", meno);
    let r: Vysledok<(bool, String)> = async {
        if sluzba == "webshare" {
            let mut w = Webshare::new("")?;
            let t = w.prihlas(meno, &h).await?;
            let i = w.info().await?;
            v.nastavenia
                .lock()
                .unwrap()
                .nastav("webshare_token", json!(t));
            *v.ucty.webshare.lock().await = Some(w);
            Ok(i)
        } else if sluzba == "fastshare" {
            let f = Fastshare::new()?;
            f.prihlas(meno, &h).await?;
            let i = f.info().await?;
            *v.ucty.fastshare.lock().await = Some(f);
            Ok(i)
        } else {
            let mut d = Datoid::new()?;
            d.prihlas(meno, &h).await?;
            let i = d.info().await?;
            *v.ucty.datoid.lock().await = Some(d);
            Ok(i)
        }
    }
    .await;
    let (vip, text) = match r {
        Ok(x) => x,
        Err(e) => {
            v.ucty.nastav(sluzba, "chyba", &e.to_string(), meno);
            return Err(e);
        }
    };
    uloz_heslo(sluzba, meno, &h);
    {
        let mut n = v.nastavenia.lock().unwrap();
        n.nastav(&format!("{sluzba}_user"), json!(meno));
        n.uloz();
    }
    v.ucty
        .nastav(sluzba, if vip { "ok" } else { "bez_vip" }, &text, meno);
    // zlyhané položky tejto služby skúsiť znova
    let zdroj = nazov_sluzby(sluzba);
    let ids: Vec<String> = {
        let f = v.fronta.lock().unwrap();
        f.polozky
            .iter()
            .filter(|p| p.stav == super::fronta::Stav::Chyba && p.zdroj == zdroj)
            .map(|p| p.id.clone())
            .collect()
    };
    v.fronta.lock().unwrap().spusti(&ids);
    v.naplanuj();
    Ok(v.ucty.stav()[sluzba].clone())
}

pub fn odhlas(v: &Arc<Vnutro>, sluzba: &str) {
    if !SLUZBY.contains(&sluzba) {
        return;
    }
    let meno = meno_v_nastaveniach(v, sluzba);
    if !meno.is_empty() {
        zmaz_heslo(sluzba, &meno);
    }
    {
        let mut n = v.nastavenia.lock().unwrap();
        n.nastav(&format!("{sluzba}_user"), json!(""));
        if sluzba == "webshare" {
            n.nastav("webshare_token", json!(""));
        }
        n.uloz();
    }
    v.ucty.nastav(sluzba, "neprihlaseny", "", "");
    let v2 = v.clone();
    let s = sluzba.to_string();
    v.rt_spawn(async move { v2.ucty.reset(&s).await });
}

/// Stiahne súbor z Webshare, Fastshare alebo Datoidu cez účet používateľa.
pub async fn stiahni(
    v: &Arc<Vnutro>,
    p: &Polozka,
    z: &CancellationToken,
    h: &Hlasic,
) -> Vysledok<Koniec> {
    let spojenia = v
        .nastavenia
        .lock()
        .unwrap()
        .cislo("connections", 4)
        .clamp(1, 16) as usize;
    if p.zdroj == "Webshare" {
        let id = id_webshare(&p.url).ok_or_else(|| chyba("Neplatný odkaz na Webshare."))?;
        let w = webshare(v, false).await?;
        let (nazov, velkost, zamknuty) = w.subor(&id).await?;
        if zamknuty && p.heslo.is_empty() {
            v.fronta.lock().unwrap().uprav(&p.id, |x| {
                x.nazov = nazov.clone();
                x.celkom = velkost.or(x.celkom);
            });
            return Err(Chyba::Heslo(
                "Súbor na Webshare je chránený heslom – zadaj ho.".into(),
            ));
        }
        v.zapis(format!("⬇ Webshare: {nazov}"));
        v.fronta.lock().unwrap().uprav(&p.id, |x| {
            x.nazov = nazov.clone();
            x.celkom = velkost.or(x.celkom);
        });
        let ciel = super::stahovanie::ciel_suboru(v, p, &nazov);
        let v2 = v.clone();
        let heslo = if zamknuty {
            p.heslo.clone()
        } else {
            String::new()
        };
        // heslo over hneď – zlé heslo nech sa ukáže pred sťahovaním, nie uprostred
        if zamknuty {
            w.odkaz(&id, &heslo).await?;
        }
        let zdroj: ZdrojUrl = Arc::new(move |novy| {
            let (v2, id, heslo) = (v2.clone(), id.clone(), heslo.clone());
            Box::pin(async move {
                let w = webshare(&v2, novy).await?;
                match w.odkaz(&id, &heslo).await {
                    Ok(o) => Ok(o),
                    Err(Chyba::Heslo(t)) => Err(Chyba::Heslo(t)),
                    // token mohol vypršať
                    Err(_) if !novy => webshare(&v2, true).await?.odkaz(&id, &heslo).await,
                    Err(e) => Err(e),
                }
            })
        });
        let m = Moznosti {
            spojenia,
            ocakavana_velkost: velkost,
            zrusenie: z.clone(),
            hlasic: h.clone(),
        };
        return Ok(Koniec::Subor(http::stiahni(&w.k, zdroj, &ciel, &m).await?));
    }
    if p.zdroj == "Datoid" {
        let d = datoid(v, false).await?;
        let url = p.url.trim().replace("datoid.sk/", "datoid.cz/");
        let s = d.subor(&url, &p.heslo).await?;
        let mut odkaz = match d.odkaz(&url, &p.heslo).await {
            Err(Chyba::Heslo(t)) => return Err(Chyba::Heslo(t)),
            Err(_) => datoid(v, true).await?.odkaz(&url, &p.heslo).await?, // token mohol vypršať
            Ok(o) => o,
        };
        let nazov = match s.nazov.clone() {
            Some(n) => n,
            None => {
                let n = super::stahovanie::nazov_z_adresy(&odkaz);
                if je_nazov_suboru(&n) {
                    n
                } else {
                    super::stahovanie::nazov_z_adresy(&url)
                }
            }
        };
        v.zapis(format!("⬇ Datoid: {nazov}"));
        v.fronta.lock().unwrap().uprav(&p.id, |x| {
            x.nazov = nazov.clone();
            x.celkom = s.velkost.or(x.celkom);
        });
        let ciel = super::stahovanie::ciel_suboru(v, p, &nazov);
        let (v2, heslo) = (v.clone(), p.heslo.clone());
        let prvy = std::mem::take(&mut odkaz);
        let zdroj: ZdrojUrl = Arc::new(move |novy| {
            let (v2, url, heslo, prvy) = (v2.clone(), url.clone(), heslo.clone(), prvy.clone());
            Box::pin(async move {
                if !novy {
                    return Ok(prvy);
                }
                datoid(&v2, true).await?.odkaz(&url, &heslo).await
            })
        });
        let m = Moznosti {
            spojenia,
            ocakavana_velkost: s.velkost,
            zrusenie: z.clone(),
            hlasic: h.clone(),
        };
        return Ok(Koniec::Subor(http::stiahni(&d.k, zdroj, &ciel, &m).await?));
    }
    let f = fastshare(v, false).await?;
    let s = f.subor(&p.url).await?;
    // názov: zo stránky → od servera pri sťahovaní → z konca odkazu → náhradný
    let nazov = match s.nazov.clone().filter(|n| !n.trim().is_empty()) {
        Some(n) => n,
        None => match f.nazov_zo_servera(&s.odkaz).await {
            Some(n) => n,
            None => nazov_z_odkazu_fastshare(&p.url).unwrap_or_else(|| "fastshare_subor".into()),
        },
    };
    v.zapis(format!("⬇ Fastshare: {nazov}"));
    v.fronta.lock().unwrap().uprav(&p.id, |x| {
        x.nazov = nazov.clone();
        x.celkom = s.velkost.or(x.celkom);
    });
    let ciel = super::stahovanie::ciel_suboru(v, p, &nazov);
    let (v2, url, prvy) = (v.clone(), p.url.clone(), s.odkaz.clone());
    let zdroj: ZdrojUrl = Arc::new(move |novy| {
        let (v2, url, prvy) = (v2.clone(), url.clone(), prvy.clone());
        Box::pin(async move {
            if !novy {
                return Ok(prvy);
            }
            Ok(fastshare(&v2, false).await?.subor(&url).await?.odkaz)
        })
    });
    let m = Moznosti {
        spojenia,
        ocakavana_velkost: None,
        zrusenie: z.clone(),
        hlasic: h.clone(),
    };
    Ok(Koniec::Subor(http::stiahni(&f.k, zdroj, &ciel, &m).await?))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn md5_crypt_ako_openssl() {
        // openssl passwd -1 -salt saltsalt heslo
        assert_eq!(
            md5_crypt("password", "saltsalt"),
            "$1$saltsalt$qjXMvbEw8oaL.CzflDtaK/"
        );
        assert_eq!(md5_crypt("", "abc"), "$1$abc$Or2rbeUYTvt12aiVzMuS/.");
        // rovnaký výsledok ako `openssl passwd -1`
        assert_eq!(
            md5_crypt("dlhé heslo s diakritikou 123", "Ws0Lt12345"),
            "$1$Ws0Lt123$sb4oeW6sW1EzBBwvGWXMr/"
        );
    }

    #[test]
    fn nazvy_suborov_fastshare() {
        assert_eq!(
            nazov_z_odkazu_fastshare(
                "https://fastshare.cloud/35565309/a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv"
            )
            .as_deref(),
            Some("a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv")
        );
        assert_eq!(
            nazov_z_odkazu_fastshare("https://fastshare.cz/123/Letn%C3%AD%20film.avi#x").as_deref(),
            Some("Letní film.avi")
        );
        assert_eq!(nazov_z_odkazu_fastshare("https://fastshare.cz/123/"), None);
        assert_eq!(
            nazov_z_odkazu_fastshare("https://x.fastshare.cz/download.php?id=5"),
            None
        );
        assert_eq!(
            nazov_z_hlavicky("attachment; filename=\"A.Loose.End.mkv\"").as_deref(),
            Some("A.Loose.End.mkv")
        );
        assert_eq!(
            nazov_z_hlavicky(
                "attachment; filename=\"x.mkv\"; filename*=UTF-8''Letn%C3%AD%20film.mkv"
            )
            .as_deref(),
            Some("Letní film.mkv")
        );
        assert_eq!(nazov_z_hlavicky("inline"), None);
        // stránka bez nadpisu h2 – názov z <title>, ak vyzerá ako súbor
        let html = format!(
            "<title>Film.2024.1080p.mkv | FastShare</title>{PRIHLASENY}<a href=\"https://d1.fastshare.cloud/download.php?id=77&h=1\">"
        );
        let s = rozober_fastshare(&html).unwrap();
        assert_eq!(s.nazov.as_deref(), Some("Film.2024.1080p.mkv"));
        let html = format!("<title>FastShare.cloud</title>{PRIHLASENY}<a href=\"https://d1.fastshare.cloud/download.php?id=77&h=1\">");
        assert_eq!(rozober_fastshare(&html).unwrap().nazov, None);
    }

    #[test]
    fn base64_tam_a_spat() {
        for t in ["", "a", "ab", "abc", "{\"webshare:ja\":\"heslo\"}"] {
            assert_eq!(z_base64(&base64(t.as_bytes())).unwrap(), t.as_bytes());
        }
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
    }

    #[test]
    fn odkazy_webshare() {
        assert_eq!(
            id_webshare("https://webshare.cz/#/file/4n4Lz3kP2/film-mkv").as_deref(),
            Some("4n4Lz3kP2")
        );
        assert_eq!(
            id_webshare("https://webshare.cz/file/abc123/x").as_deref(),
            Some("abc123")
        );
        assert_eq!(id_webshare("https://webshare.cz/#/search/film"), None);
    }

    #[test]
    fn stranka_fastshare() {
        let html = r#"<a href="/logout.php">Odhlásiť</a><h2 title="Film &amp; seriál.mkv" class="section_title">Film</h2>
            <i class="fa fa-bars"></i> 1,5&nbsp;GB <a href="https://data12.fastshare.cz/download.php?id=123456&h=abc">Stiahnuť</a>"#;
        let s = rozober_fastshare(html).unwrap();
        assert_eq!(s.nazov.as_deref(), Some("Film & seriál.mkv"));
        assert_eq!(s.velkost, Some((1.5 * (1u64 << 30) as f64) as u64));
        assert_eq!(
            s.odkaz,
            "https://data12.fastshare.cz/download.php?id=123456"
        );
        assert!(rozober_fastshare("<p>>The file has been deleted</p>")
            .unwrap_err()
            .to_string()
            .contains("neexistuje"));
        assert!(rozober_fastshare("You need more credit for this file")
            .unwrap_err()
            .to_string()
            .contains("kreditu"));
        assert_eq!(
            info_fastshare(r#"<a href="/logout.php"></a><td>12,3 GB</td>"#).unwrap(),
            (true, "kredit 12,3 GB".into())
        );
        assert_eq!(
            info_fastshare(r#"<a href="/logout.php"></a><span>Active until 01.02.2027</span>"#)
                .unwrap()
                .1,
            "neobmedzené sťahovanie do 01.02.2027"
        );
    }
}
