//! Pomocné programy: yt-dlp (YouTube a ďalšie služby), Deno (JavaScript, ktorý yt-dlp potrebuje na
//! YouTube) a FFmpeg (len MP3 a prevod Opus → AAC). Kometa si yt-dlp a Deno doinštaluje sama a
//! udržiava ich aktuálne – nezávisí od toho, čo je v systéme (napr. z Kometa pre Python). Všetko
//! z oficiálnych vydaní (yt-dlp, denoland/deno, BtbN/FFmpeg-Builds) a vždy sa pred použitím overí
//! kontrolný súčet SHA-256.

use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Hlasic};
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Nastroje {
    priecinok: PathBuf,
}

/// Program v ceste PATH (vo Windows aj s príponami z PATHEXT).
pub fn v_ceste(meno: &str) -> Option<PathBuf> {
    let cesta = std::env::var_os("PATH")?;
    let pripony: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|p| p.to_ascii_lowercase())
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&cesta).find_map(|d| {
        pripony
            .iter()
            .map(|p| d.join(format!("{meno}{p}")))
            .find(|k| k.is_file())
    })
}

/// Názov súboru yt-dlp vo vydaniach pre tento systém.
fn subor_vydania() -> Option<&'static str> {
    if cfg!(windows) {
        Some("yt-dlp.exe")
    } else if cfg!(target_os = "macos") {
        Some("yt-dlp_macos")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("yt-dlp_linux")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("yt-dlp_linux_aarch64")
    } else {
        None
    }
}

/// Archív Deno vo vydaniach pre tento systém.
fn subor_deno() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("deno-x86_64-pc-windows-msvc.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("deno-aarch64-apple-darwin.zip")
    } else if cfg!(target_os = "macos") {
        Some("deno-x86_64-apple-darwin.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("deno-x86_64-unknown-linux-gnu.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("deno-aarch64-unknown-linux-gnu.zip")
    } else {
        None
    }
}

/// Súčet SHA-256 zo súboru „.sha256sum“ (riadok „súčet  názov“ aj výpis PowerShellu „Hash : …“).
pub fn sucet_zo_suboru(t: &str) -> Option<String> {
    t.split(|c: char| !c.is_ascii_hexdigit())
        .find(|w| w.len() == 64)
        .map(str::to_ascii_lowercase)
}

/// Verzia z odpovede „releases/latest“ (tag_name bez „v“).
pub fn verzia_vydania(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let t = v
        .get("tag_name")?
        .as_str()?
        .trim_start_matches(['v', 'V'])
        .trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Zostavenie FFmpeg pre tento systém: (časť názvu, prípona archívu).
fn zostavenie_ffmpeg() -> Option<(&'static str, &'static str)> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(("win64", ".zip"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(("linux64", ".tar.xz"))
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some(("linuxarm64", ".tar.xz"))
    } else {
        None
    }
}

/// Dá sa FFmpeg doinštalovať jedným klikom? (V macOS nie – tam cez Homebrew.)
pub fn ffmpeg_doinstalovatelny() -> bool {
    zostavenie_ffmpeg().is_some()
}

/// Vyberie archív z vydania: najprv „ffmpeg-master-latest-<sys>-gpl“, inak iné GPL zostavenie (nie „shared“).
fn vyber_archiv(nazvy: &[String], sys: &str, pripona: &str) -> Option<String> {
    let presny = format!("ffmpeg-master-latest-{sys}-gpl{pripona}");
    nazvy.iter().find(|n| **n == presny).cloned().or_else(|| {
        nazvy
            .iter()
            .find(|n| {
                n.starts_with("ffmpeg-")
                    && n.contains(&format!("-{sys}-gpl"))
                    && !n.contains("shared")
                    && n.ends_with(pripona)
            })
            .cloned()
    })
}

/// Riadok „<súčet>  <názov>“ zo súboru so súčtami.
fn sucet_v_zozname(sucty: &str, meno: &str) -> Option<String> {
    sucty.lines().find_map(|r| {
        let (h, n) = r.split_once(char::is_whitespace)?;
        (n.trim().trim_start_matches('*') == meno).then(|| h.trim().to_ascii_lowercase())
    })
}

fn najdi_subor(dir: &Path, meno: &str, hlbka: u8) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut priecinky = vec![];
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            priecinky.push(p);
        } else if e.file_name().to_string_lossy().eq_ignore_ascii_case(meno) {
            return Some(p);
        }
    }
    if hlbka == 0 {
        return None;
    }
    priecinky
        .iter()
        .find_map(|d| najdi_subor(d, meno, hlbka - 1))
}

impl Nastroje {
    pub fn new(priecinok_nastaveni: &Path) -> Self {
        Nastroje {
            priecinok: priecinok_nastaveni.join("nastroje"),
        }
    }

    fn lokalny(&self, meno: &str) -> PathBuf {
        self.priecinok.join(if cfg!(windows) {
            format!("{meno}.exe")
        } else {
            meno.to_string()
        })
    }

    /// `KOMETA_YTDLP` = cesta k programu, alebo `nie` (ako keby chýbal – pre skúšky).
    pub fn ytdlp(&self) -> Option<PathBuf> {
        match std::env::var("KOMETA_YTDLP") {
            Ok(v) if v == "nie" => return None,
            Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
            _ => {}
        }
        let l = self.lokalny("yt-dlp");
        if l.is_file() {
            return Some(l);
        }
        v_ceste("yt-dlp")
    }

    /// Vlastná kópia yt-dlp (doinštalovaná Kometou)?
    pub fn ma_vlastny_ytdlp(&self) -> bool {
        self.lokalny("yt-dlp").is_file()
    }

    /// Deno pre yt-dlp: `KOMETA_DENO` = cesta alebo `nie`; inak vlastná kópia, inak zo systému.
    pub fn deno(&self) -> Option<PathBuf> {
        match std::env::var("KOMETA_DENO") {
            Ok(v) if v == "nie" => return None,
            Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
            _ => {}
        }
        let l = self.lokalny("deno");
        if l.is_file() {
            return Some(l);
        }
        v_ceste("deno")
    }

    pub fn ma_vlastne_deno(&self) -> bool {
        self.lokalny("deno").is_file()
    }

    /// Verzia vlastnej kópie (zapísaná pri inštalácii).
    pub fn verzia(&self, meno: &str) -> Option<String> {
        std::fs::read_to_string(self.priecinok.join(format!("{meno}.verzia")))
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    }

    fn zapis_verziu(&self, meno: &str, verzia: &str) {
        let _ = std::fs::write(self.priecinok.join(format!("{meno}.verzia")), verzia);
    }

    /// Posledná verzia na GitHube (`repo` = „yt-dlp/yt-dlp“, „denoland/deno“).
    pub async fn najnovsia(&self, k: &Client, repo: &str) -> Vysledok<String> {
        let premenna = format!(
            "KOMETA_{}_API",
            if repo.contains("deno") {
                "DENO"
            } else {
                "YTDLP"
            }
        );
        let adresa = std::env::var(premenna)
            .unwrap_or_else(|_| format!("https://api.github.com/repos/{repo}/releases/latest"));
        let t = k
            .get(adresa)
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        verzia_vydania(&t).ok_or_else(|| Chyba::Format(format!("{repo}: chýba verzia vydania")))
    }

    /// Stiahne Deno z oficiálnych vydaní, overí SHA-256 a rozbalí ho vlastným kódom.
    pub async fn doinstaluj_deno(&self, k: &Client, hlasic: &Hlasic) -> Vysledok<PathBuf> {
        let meno = subor_deno().ok_or_else(|| {
            Chyba::Ine("Pre tento systém nie je hotové Deno – nainštaluj ho z deno.com.".into())
        })?;
        let zaklad = std::env::var("KOMETA_DENO_ZAKLAD")
            .unwrap_or_else(|_| "https://github.com/denoland/deno/releases/latest/download".into());
        let zaklad = zaklad.trim_end_matches('/');
        udalosti::info(hlasic, "Sťahujem Deno z oficiálnych vydaní…");
        let sucet_t = k
            .get(format!("{zaklad}/{meno}.sha256sum"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let ocakavany = sucet_zo_suboru(&sucet_t)
            .ok_or_else(|| Chyba::Format(format!("chýba súčet SHA-256 pre {meno}")))?;
        let data = self
            .stiahni(k, &format!("{zaklad}/{meno}"), "Deno", hlasic, 300 << 20)
            .await?;
        if hex(&Sha256::digest(&data)) != ocakavany {
            return Err(Chyba::Format(
                "stiahnuté Deno nesedí s kontrolným súčtom – neuložil som ho".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.priecinok).await?;
        let archiv = self.priecinok.join("deno.zip.stahuje");
        let rozbal = self.priecinok.join("deno.rozbal");
        let _ = tokio::fs::remove_dir_all(&rozbal).await;
        tokio::fs::write(&archiv, &data).await?;
        drop(data);
        let (a, r) = (archiv.clone(), rozbal.clone());
        let vysledok = tokio::task::spawn_blocking(move || super::archivy::rozbal_zip_do(&a, &r))
            .await
            .map_err(|e| Chyba::Ine(e.to_string()))?;
        let _ = tokio::fs::remove_file(&archiv).await;
        vysledok.map_err(|e| Chyba::Ine(format!("archív Deno sa nedá rozbaliť: {e}")))?;
        let exe = if cfg!(windows) { "deno.exe" } else { "deno" };
        let novy = najdi_subor(&rozbal, exe, 2)
            .ok_or_else(|| Chyba::Format("v archíve Deno chýba program deno".into()))?;
        let ciel = self.lokalny("deno");
        nastav_spustitelny(&novy).await?;
        tokio::fs::rename(&novy, &ciel).await?;
        let _ = tokio::fs::remove_dir_all(&rozbal).await;
        if let Ok(v) = self.najnovsia(k, "denoland/deno").await {
            self.zapis_verziu("deno", &v);
        }
        udalosti::info(hlasic, "Deno je pripravené.");
        Ok(ciel)
    }

    /// Stiahne súbor do pamäte s hlásením priebehu (najviac `max` bajtov).
    async fn stiahni(
        &self,
        k: &Client,
        url: &str,
        cast: &str,
        hlasic: &Hlasic,
        max: u64,
    ) -> Vysledok<Vec<u8>> {
        let r = k.get(url).send().await?.error_for_status()?;
        let celkom = r.content_length();
        let mut data: Vec<u8> = Vec::with_capacity(celkom.unwrap_or(0).min(max) as usize);
        let mut s = r.bytes_stream();
        let zaciatok = std::time::Instant::now();
        while let Some(c) = s.next().await {
            data.extend_from_slice(&c?);
            if data.len() as u64 > max {
                return Err(Chyba::Format(format!("{cast}: súbor je nezvyčajne veľký")));
            }
            hlasic(udalosti::Udalost::Priebeh {
                stiahnute: data.len() as u64,
                celkom,
                rychlost: data.len() as f64 / zaciatok.elapsed().as_secs_f64().max(0.001),
                cast: Some(cast.into()),
            });
        }
        Ok(data)
    }

    pub fn ffmpeg(&self) -> Option<PathBuf> {
        match std::env::var("KOMETA_FFMPEG") {
            Ok(v) if v == "nie" => return None,
            Ok(v) if !v.is_empty() => return Some(PathBuf::from(v)),
            _ => {}
        }
        let l = self.lokalny("ffmpeg");
        if l.is_file() {
            return Some(l);
        }
        v_ceste("ffmpeg")
    }

    /// Stiahne FFmpeg (aj ffprobe) zo zostavení BtbN/FFmpeg-Builds do `~/.kometa/nastroje`,
    /// overí SHA-256 a rozbalí ho systémovým `tar` (vo Windows 10/11 je súčasťou systému).
    pub async fn doinstaluj_ffmpeg(&self, k: &Client, hlasic: &Hlasic) -> Vysledok<PathBuf> {
        let (sys, pripona) = zostavenie_ffmpeg().ok_or_else(|| {
            Chyba::Ine(if cfg!(target_os = "macos") {
                "V macOS nainštaluj FFmpeg cez Homebrew: brew install ffmpeg".into()
            } else {
                "Pre tento systém nie je hotový FFmpeg – nainštaluj ho cez správcu balíkov.".into()
            })
        })?;
        let vydanie = std::env::var("KOMETA_FFMPEG_ZAKLAD").unwrap_or_else(|_| {
            "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/latest".into()
        });
        udalosti::info(hlasic, "Hľadám FFmpeg…");
        let info: serde_json::Value = k
            .get(&vydanie)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
            .map(|t| serde_json::from_str(&t))?
            .map_err(|_| Chyba::Format("zoznam vydaní FFmpeg sa nedá prečítať".into()))?;
        let subory: Vec<(String, String)> = info["assets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some((
                            x["name"].as_str()?.to_string(),
                            x["browser_download_url"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let nazvy: Vec<String> = subory.iter().map(|(n, _)| n.clone()).collect();
        let adresa = |meno: &str| {
            subory
                .iter()
                .find(|(n, _)| n == meno)
                .map(|(_, u)| u.clone())
        };
        let archiv = vyber_archiv(&nazvy, sys, pripona)
            .ok_or_else(|| Chyba::Format(format!("vo vydaní FFmpeg chýba zostavenie {sys}")))?;
        let sucty_url = adresa("checksums.sha256")
            .ok_or_else(|| Chyba::Format("vo vydaní FFmpeg chýbajú kontrolné súčty".into()))?;
        let sucty = k
            .get(sucty_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let ocakavany = sucet_v_zozname(&sucty, &archiv)
            .ok_or_else(|| Chyba::Format(format!("v kontrolných súčtoch chýba {archiv}")))?;

        tokio::fs::create_dir_all(&self.priecinok).await?;
        let docasny = self.priecinok.join(format!("ffmpeg-stahuje{pripona}"));
        let rozbal = self.priecinok.join("ffmpeg-rozbaluje");
        let vysledok = async {
            let r = k
                .get(adresa(&archiv).unwrap_or_default())
                .send()
                .await?
                .error_for_status()?;
            let celkom = r.content_length();
            let mut subor = tokio::fs::File::create(&docasny).await?;
            let mut hash = Sha256::new();
            let mut stiahnute = 0u64;
            let mut s = r.bytes_stream();
            let zaciatok = std::time::Instant::now();
            while let Some(c) = s.next().await {
                let c = c?;
                hash.update(&c);
                tokio::io::AsyncWriteExt::write_all(&mut subor, &c).await?;
                stiahnute += c.len() as u64;
                hlasic(udalosti::Udalost::Priebeh {
                    stiahnute,
                    celkom,
                    rychlost: stiahnute as f64 / zaciatok.elapsed().as_secs_f64().max(0.001),
                    cast: Some("FFmpeg".into()),
                });
            }
            tokio::io::AsyncWriteExt::flush(&mut subor).await?;
            drop(subor);
            let sucet: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if sucet != ocakavany {
                return Err(Chyba::Format(
                    "stiahnutý FFmpeg nesedí s kontrolným súčtom – neuložil som ho".into(),
                ));
            }
            udalosti::info(hlasic, "Rozbaľujem FFmpeg…");
            let _ = tokio::fs::remove_dir_all(&rozbal).await;
            tokio::fs::create_dir_all(&rozbal).await?;
            if docasny
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
            {
                // ZIP (Windows) rozbalí Kometa sama – bez spúšťania systémových nástrojov
                let (a, c) = (docasny.clone(), rozbal.clone());
                tokio::task::spawn_blocking(move || super::archivy::rozbal_zip_do(&a, &c))
                    .await
                    .map_err(|e| Chyba::Ine(e.to_string()))?
                    .map_err(|e| Chyba::Ine(format!("archív FFmpeg sa nedá rozbaliť: {e}")))?;
            } else {
                // .tar.xz (Linux) rozbalí systémový tar
                let mut prikaz = tokio::process::Command::new("tar");
                prikaz.arg("-xf").arg(&docasny).arg("-C").arg(&rozbal);
                let o = prikaz.output().await.map_err(|e| {
                    Chyba::Ine(format!("archív FFmpeg sa nedá rozbaliť (tar): {e}"))
                })?;
                if !o.status.success() {
                    return Err(Chyba::Ine(format!(
                        "archív FFmpeg sa nedá rozbaliť: {}",
                        String::from_utf8_lossy(&o.stderr).trim()
                    )));
                }
            }
            let mut ciel = None;
            for meno in ["ffmpeg", "ffprobe"] {
                let subor = if cfg!(windows) {
                    format!("{meno}.exe")
                } else {
                    meno.into()
                };
                let Some(z) = najdi_subor(&rozbal, &subor, 3) else {
                    if meno == "ffmpeg" {
                        return Err(Chyba::Format(
                            "v archíve FFmpeg chýba program ffmpeg".into(),
                        ));
                    }
                    continue;
                };
                let c = self.lokalny(meno);
                let tmp = c.with_extension("novy");
                tokio::fs::copy(&z, &tmp).await?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
                        .await?;
                }
                tokio::fs::rename(&tmp, &c).await?;
                if meno == "ffmpeg" {
                    ciel = Some(c);
                }
            }
            ciel.ok_or_else(|| Chyba::Format("v archíve FFmpeg chýba program ffmpeg".into()))
        }
        .await;
        let _ = tokio::fs::remove_file(&docasny).await;
        let _ = tokio::fs::remove_dir_all(&rozbal).await;
        let ciel = vysledok?;
        udalosti::info(hlasic, "FFmpeg je pripravený.");
        Ok(ciel)
    }

    /// Stiahne yt-dlp z oficiálnych vydaní do `~/.kometa/nastroje` a overí SHA-256.
    pub async fn doinstaluj_ytdlp(&self, k: &Client, hlasic: &Hlasic) -> Vysledok<PathBuf> {
        // verzia sa zapíše vedľa programu – podľa nej sa potom zisťuje, či je novšia
        let verzia = self.najnovsia(k, "yt-dlp/yt-dlp").await.ok();
        let meno = subor_vydania().ok_or_else(|| {
            Chyba::Ine(
                "Pre tento systém nie je hotový yt-dlp – nainštaluj ho cez pip install yt-dlp."
                    .into(),
            )
        })?;
        let zaklad = std::env::var("KOMETA_YTDLP_ZAKLAD")
            .unwrap_or_else(|_| "https://github.com/yt-dlp/yt-dlp/releases/latest/download".into());
        let zaklad = zaklad.trim_end_matches('/');
        udalosti::info(hlasic, "Sťahujem yt-dlp z oficiálnych vydaní…");
        let sucty = k
            .get(format!("{zaklad}/SHA2-256SUMS"))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let ocakavany = sucty
            .lines()
            .find_map(|r| {
                let (h, n) = r.split_once(char::is_whitespace)?;
                (n.trim().trim_start_matches('*') == meno).then(|| h.trim().to_ascii_lowercase())
            })
            .ok_or_else(|| Chyba::Format(format!("v SHA2-256SUMS chýba {meno}")))?;
        let r = k
            .get(format!("{zaklad}/{meno}"))
            .send()
            .await?
            .error_for_status()?;
        let celkom = r.content_length();
        let mut data: Vec<u8> = Vec::with_capacity(celkom.unwrap_or(0) as usize);
        let mut s = r.bytes_stream();
        let zaciatok = std::time::Instant::now();
        while let Some(c) = s.next().await {
            data.extend_from_slice(&c?);
            hlasic(udalosti::Udalost::Priebeh {
                stiahnute: data.len() as u64,
                celkom,
                rychlost: data.len() as f64 / zaciatok.elapsed().as_secs_f64().max(0.001),
                cast: Some("yt-dlp".into()),
            });
        }
        let sucet: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if sucet != ocakavany {
            return Err(Chyba::Format(
                "stiahnutý yt-dlp nesedí s kontrolným súčtom – neuložil som ho".into(),
            ));
        }
        tokio::fs::create_dir_all(&self.priecinok).await?;
        let ciel = self.lokalny("yt-dlp");
        let tmp = ciel.with_extension("stahuje");
        tokio::fs::write(&tmp, &data).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).await?;
        }
        tokio::fs::rename(&tmp, &ciel).await?;
        if let Some(v) = verzia {
            self.zapis_verziu("yt-dlp", &v);
        }
        udalosti::info(hlasic, "yt-dlp je pripravený.");
        Ok(ciel)
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

async fn nastav_spustitelny(p: &Path) -> Vysledok<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).await?;
    }
    let _ = p;
    Ok(())
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn sucty_a_verzie() {
        let ps = "\r\nAlgorithm : SHA256\r\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\r\nPath      : C:\\a\\deno.zip\r\n";
        assert_eq!(
            sucet_zo_suboru(ps).as_deref(),
            Some("cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf")
        );
        let lin = "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609  deno-x86_64-unknown-linux-gnu.zip\n";
        assert_eq!(sucet_zo_suboru(lin).unwrap().len(), 64);
        assert_eq!(sucet_zo_suboru("bez súčtu"), None);
        assert_eq!(
            verzia_vydania(r#"{"tag_name":"v2.5.6"}"#).as_deref(),
            Some("2.5.6")
        );
        assert_eq!(
            verzia_vydania(r#"{"tag_name":"2026.10.26"}"#).as_deref(),
            Some("2026.10.26")
        );
    }

    #[test]
    fn vyber_archivu_ffmpeg() {
        let n: Vec<String> = [
            "checksums.sha256",
            "ffmpeg-master-latest-win64-gpl-shared.zip",
            "ffmpeg-master-latest-win64-gpl.zip",
            "ffmpeg-master-latest-linux64-gpl.tar.xz",
            "ffmpeg-n7.1-latest-linuxarm64-gpl-7.1.tar.xz",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            vyber_archiv(&n, "win64", ".zip").as_deref(),
            Some("ffmpeg-master-latest-win64-gpl.zip")
        );
        assert_eq!(
            vyber_archiv(&n, "linuxarm64", ".tar.xz").as_deref(),
            Some("ffmpeg-n7.1-latest-linuxarm64-gpl-7.1.tar.xz")
        );
        assert_eq!(vyber_archiv(&n, "win32", ".zip"), None);
        let sucty = "abc  ffmpeg-master-latest-win64-gpl.zip\nDEF *ine.zip\n";
        assert_eq!(
            sucet_v_zozname(sucty, "ffmpeg-master-latest-win64-gpl.zip").as_deref(),
            Some("abc")
        );
        assert_eq!(sucet_v_zozname(sucty, "ine.zip").as_deref(), Some("def"));
    }
}
