//! Spojenie stiahnutých stôp do jedného súboru (MKV alebo MP4) bez prekódovania.
//!
//! Zatiaľ cez FFmpeg (`-c copy`); vlastné spájanie bez FFmpeg je ďalší krok jadra.
//! Výsledok sa zapisuje do dočasného súboru a až po úspechu sa premenuje – nikdy nevznikne
//! polovičný súbor s konečným názvom.

use crate::chyba::{Chyba, Vysledok};
use crate::klient::s_priponou;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct Vstup {
    pub cesta: PathBuf,
    /// "video", "zvuk" alebo "video+zvuk" (stream, kde je obraz aj zvuk spolu)
    pub druh: String,
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
}

/// Jazyk ISO 639-1 (alebo s regiónom, napr. „en-US“) → ISO 639-2/B, ako ho čakajú prehrávače v MKV/MP4.
pub fn iso639_2(kod: &str) -> String {
    let zaklad = kod
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let tri = match zaklad.as_str() {
        "sk" => "slo",
        "cs" => "cze",
        "en" => "eng",
        "de" => "ger",
        "hu" => "hun",
        "pl" => "pol",
        "ru" => "rus",
        "uk" => "ukr",
        "fr" => "fre",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "nl" => "dut",
        "sv" => "swe",
        "da" => "dan",
        "fi" => "fin",
        "no" | "nb" => "nor",
        "ro" => "rum",
        "bg" => "bul",
        "hr" => "hrv",
        "sr" => "srp",
        "sl" => "slv",
        "el" => "gre",
        "tr" => "tur",
        "ar" => "ara",
        "he" => "heb",
        "hi" => "hin",
        "ja" => "jpn",
        "ko" => "kor",
        "zh" => "chi",
        "vi" => "vie",
        "id" => "ind",
        "th" => "tha",
        // ISO 639-2/T → 639-2/B (MKV a väčšina prehrávačov čaká tvar B)
        "ces" => "cze",
        "slk" => "slo",
        "deu" => "ger",
        "fra" => "fre",
        "nld" => "dut",
        "ron" => "rum",
        "ell" => "gre",
        "zho" => "chi",
        "fas" => "per",
        "sqi" => "alb",
        "hye" => "arm",
        "eus" => "baq",
        "mya" => "bur",
        "kat" => "geo",
        "isl" => "ice",
        "mkd" => "mac",
        "msa" => "may",
        "cym" => "wel",
        "bod" => "tib",
        x if x.len() == 3 => return x.to_string(),
        _ => "und",
    };
    tri.to_string()
}

/// Cesta k FFmpeg: premenná KOMETA_FFMPEG alebo PATH.
pub fn ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KOMETA_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let meno = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::var_os("PATH").and_then(|cesty| {
        std::env::split_paths(&cesty)
            .map(|d| d.join(meno))
            .find(|p| p.is_file())
    })
}

/// Spojí vstupy do `ciel` (prípona .mkv alebo .mp4 určuje formát). Prvá zvuková stopa je predvolená.
///
/// Spája vlastný kód jadra ([`crate::media::spajac`]); FFmpeg sa použije len vtedy, keď vlastné
/// spájanie formát nepozná (napr. HEVC v MPEG-TS), alebo keď to vyžiada premenná
/// `KOMETA_SPAJANIE=ffmpeg`.
pub async fn spoj(vstupy: &[Vstup], ciel: &Path, titul: &str) -> Vysledok<PathBuf> {
    if vstupy.is_empty() {
        return Err(Chyba::Format("nie je čo spájať".into()));
    }
    let vynutit_ffmpeg =
        std::env::var("KOMETA_SPAJANIE").is_ok_and(|v| v.eq_ignore_ascii_case("ffmpeg"));
    if !vynutit_ffmpeg {
        let vlastne: Vec<crate::media::spajac::Vstup> = vstupy
            .iter()
            .map(|v| crate::media::spajac::Vstup {
                cesta: v.cesta.clone(),
                druh: v.druh.clone(),
                jazyk: v.jazyk.clone(),
                nazov: v.nazov.clone(),
            })
            .collect();
        let (c, t) = (ciel.to_path_buf(), titul.to_string());
        let vysledok =
            tokio::task::spawn_blocking(move || crate::media::spajac::spoj(&vlastne, &c, &t))
                .await
                .map_err(|e| Chyba::Ine(format!("spájanie sa prerušilo: {e}")))?;
        match vysledok {
            Ok(_) => return Ok(ciel.to_path_buf()),
            // šifrovanie a chyby disku sa neobchádzajú
            Err(e @ (Chyba::Drm(_) | Chyba::Io(_))) => return Err(e),
            Err(e) if ffmpeg().is_none() => return Err(e),
            Err(_) => {} // nepodporovaný alebo nezvyčajný formát → FFmpeg
        }
    }
    spoj_ffmpeg(vstupy, ciel, titul).await
}

async fn spoj_ffmpeg(vstupy: &[Vstup], ciel: &Path, titul: &str) -> Vysledok<PathBuf> {
    let Some(ff) = ffmpeg() else {
        if vstupy.len() == 1 {
            // jedna stopa sa zaobíde bez spájania – ostane v pôvodnom kontajneri
            let pripona = vstupy[0]
                .cesta
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("ts")
                .to_string();
            let vysledok = ciel.with_extension(pripona);
            tokio::fs::rename(&vstupy[0].cesta, &vysledok).await?;
            return Ok(vysledok);
        }
        return Err(Chyba::Ine(
            "Na spojenie obrazu a zvukových stôp treba FFmpeg (nainštaluj ho alebo nastav KOMETA_FFMPEG).".into(),
        ));
    };
    let mp4 = matches!(
        ciel.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mp4") | Some("m4a")
    );
    let docasny = s_priponou(ciel, ".spajanie");
    let mut a: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
    ];
    for v in vstupy {
        a.push("-i".into());
        a.push(v.cesta.display().to_string());
    }
    let mut meta = vec![];
    let mut zvuk = 0usize;
    for (i, v) in vstupy.iter().enumerate() {
        if v.druh.contains("video") {
            a.extend(["-map".into(), format!("{i}:v:0")]);
        }
        if v.druh.contains("zvuk") {
            a.extend(["-map".into(), format!("{i}:a:0?")]);
            if let Some(j) = &v.jazyk {
                meta.extend([
                    format!("-metadata:s:a:{zvuk}"),
                    format!("language={}", iso639_2(j)),
                ]);
            }
            if let Some(n) = &v.nazov {
                meta.extend([format!("-metadata:s:a:{zvuk}"), format!("title={n}")]);
                meta.extend([format!("-metadata:s:a:{zvuk}"), format!("handler_name={n}")]);
            }
            meta.extend([
                format!("-disposition:a:{zvuk}"),
                if zvuk == 0 { "default" } else { "0" }.into(),
            ]);
            zvuk += 1;
        }
    }
    // titulky až za zvukmi (v MKV ako SRT, v MP4 ako mov_text), predvolene vypnuté
    let mut tit = 0usize;
    for (i, v) in vstupy.iter().enumerate() {
        if !v.druh.contains("titulky") {
            continue;
        }
        a.extend(["-map".into(), format!("{i}:0")]);
        if let Some(j) = &v.jazyk {
            meta.extend([
                format!("-metadata:s:s:{tit}"),
                format!("language={}", iso639_2(j)),
            ]);
        }
        if let Some(n) = &v.nazov {
            meta.extend([format!("-metadata:s:s:{tit}"), format!("title={n}")]);
        }
        meta.extend([format!("-disposition:s:{tit}"), "0".into()]);
        tit += 1;
    }
    a.extend(["-c".into(), "copy".into()]);
    if tit > 0 {
        a.extend(["-c:s".into(), if mp4 { "mov_text" } else { "srt" }.into()]);
    }
    a.extend(meta);
    a.extend(["-metadata".into(), format!("title={titul}")]);
    if mp4 {
        a.extend([
            "-movflags".into(),
            "+faststart".into(),
            "-f".into(),
            "mp4".into(),
        ]);
    } else {
        a.extend(["-f".into(), "matroska".into()]);
    }
    a.push(docasny.display().to_string());
    let vystup = tokio::process::Command::new(ff)
        .args(&a)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| Chyba::Ine(format!("FFmpeg sa nedá spustiť: {e}")))?;
    let velkost = tokio::fs::metadata(&docasny)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if !vystup.status.success() || velkost == 0 {
        let _ = tokio::fs::remove_file(&docasny).await;
        let text = String::from_utf8_lossy(&vystup.stderr);
        let koniec: String = text
            .chars()
            .rev()
            .take(800)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(Chyba::Ine(format!(
            "FFmpeg nedokázal spojiť stopy: {}",
            koniec.trim()
        )));
    }
    tokio::fs::rename(&docasny, ciel).await?;
    Ok(ciel.to_path_buf())
}

#[cfg(test)]
mod testy {
    use super::iso639_2;

    #[test]
    fn jazyky() {
        assert_eq!(iso639_2("cs"), "cze");
        assert_eq!(iso639_2("en-US"), "eng");
        assert_eq!(iso639_2("sk_SK"), "slo");
        assert_eq!(iso639_2("hun"), "hun");
        assert_eq!(iso639_2("ces"), "cze");
        assert_eq!(iso639_2("slk"), "slo");
        assert_eq!(iso639_2(""), "und");
    }
}
