//! Spojenie stiahnutých stôp do jedného súboru (MKV alebo MP4) bez prekódovania.
//!
//! Spája vlastný kód jadra ([`crate::media::spajac`]); FFmpeg (`-c copy`) je len záloha pre formáty,
//! ktoré vlastné spájanie nepozná. Oba spôsoby zapíšu aj údaje o videu, kapitoly a obrázok.
//! Výsledok sa zapisuje do dočasného súboru a až po úspechu sa premenuje – nikdy nevznikne
//! polovičný súbor s konečným názvom.

use crate::chyba::{Chyba, Vysledok};
use crate::klient::s_priponou;
pub use crate::media::{Doplnky, Kapitola, Obal};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Vstup {
    pub cesta: PathBuf,
    /// "video", "zvuk", "video+zvuk" (stream, kde je obraz aj zvuk spolu) alebo "titulky"
    pub druh: String,
    pub jazyk: Option<String>,
    pub nazov: Option<String>,
    /// pôvodný jazyk videa (Some(true)) alebo preklad/dabing (Some(false))
    #[serde(skip_serializing_if = "Option::is_none")]
    pub povodny: Option<bool>,
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
    spoj_s(vstupy, ciel, &Doplnky::s_titulom(titul)).await
}

/// Ako [`spoj`], navyše vloží údaje o videu, kapitoly a obrázok.
pub async fn spoj_s(vstupy: &[Vstup], ciel: &Path, doplnky: &Doplnky) -> Vysledok<PathBuf> {
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
                povodny: v.povodny,
            })
            .collect();
        let (c, d) = (ciel.to_path_buf(), doplnky.clone());
        let vysledok =
            tokio::task::spawn_blocking(move || crate::media::spajac::spoj_s(&vlastne, &c, &d))
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
    spoj_ffmpeg(vstupy, ciel, doplnky).await
}

/// Text do súboru FFMETADATA: znaky so zvláštnym významom sa označia spätnou lomkou.
fn ffmeta_text(t: &str) -> String {
    let mut o = String::with_capacity(t.len());
    for c in t.chars() {
        if matches!(c, '=' | ';' | '#' | '\\' | '\n') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

/// Kapitoly vo formáte FFMETADATA (časy v ms); koniec poslednej = dĺžka videa, ak ju poznáme.
fn ffmeta_kapitoly(d: &Doplnky) -> Option<String> {
    let koniec = d.trvanie.unwrap_or(0);
    let k = d.kapitoly_do(koniec);
    if k.is_empty() {
        return None;
    }
    let mut o = String::from(";FFMETADATA1\n");
    for (i, x) in k.iter().enumerate() {
        let do_ = k.get(i + 1).map(|n| n.od).unwrap_or(if koniec > x.od {
            koniec
        } else {
            x.od + 1_000_000_000
        });
        o.push_str(&format!(
            "[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
            x.od / 1_000_000,
            do_ / 1_000_000,
            ffmeta_text(&x.nazov)
        ));
    }
    Some(o)
}

async fn spoj_ffmpeg(vstupy: &[Vstup], ciel: &Path, doplnky: &Doplnky) -> Vysledok<PathBuf> {
    let titul = doplnky.titul.as_str();
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
    // kapitoly a obrázok ako ďalšie vstupy (dočasné súbory vedľa cieľa)
    let mut docasne: Vec<PathBuf> = vec![];
    let mut dalsi_vstup = vstupy.len();
    let mut vstup_kapitol = None;
    if let Some(k) = ffmeta_kapitoly(doplnky) {
        let c = s_priponou(ciel, ".kapitoly.txt");
        tokio::fs::write(&c, k).await?;
        a.extend(["-i".into(), c.display().to_string()]);
        docasne.push(c);
        vstup_kapitol = Some(dalsi_vstup);
        dalsi_vstup += 1;
    }
    let mut vstup_obalu = None;
    let mut subor_obalu = None;
    if let Some(o) = &doplnky.obal {
        let c = s_priponou(ciel, &format!(".cover.{}", o.pripona()));
        tokio::fs::write(&c, &o.data).await?;
        if mp4 {
            a.extend(["-i".into(), c.display().to_string()]);
            vstup_obalu = Some(dalsi_vstup);
        }
        subor_obalu = Some(c.clone());
        docasne.push(c);
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
            let mut d: Vec<&str> = vec![];
            if zvuk == 0 {
                d.push("default");
            }
            match v.povodny {
                Some(true) => d.push("original"),
                Some(false) => d.push("dub"),
                None => {}
            }
            meta.extend([
                format!("-disposition:a:{zvuk}"),
                if d.is_empty() {
                    "0".into()
                } else {
                    d.join("+")
                },
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
    let video = vstupy.iter().any(|v| v.druh.contains("video"));
    if let Some(i) = vstup_obalu {
        a.extend(["-map".into(), format!("{i}:0")]);
    }
    if let Some(i) = vstup_kapitol {
        a.extend(["-map_chapters".into(), i.to_string()]);
    }
    a.extend(["-c".into(), "copy".into()]);
    if tit > 0 {
        a.extend(["-c:s".into(), if mp4 { "mov_text" } else { "srt" }.into()]);
    }
    if vstup_obalu.is_some() {
        let v = if video { 1 } else { 0 };
        a.extend([format!("-disposition:v:{v}"), "attached_pic".into()]);
    } else if let (false, Some(c)) = (mp4, &subor_obalu) {
        let o = doplnky.obal.as_ref().map(|o| (o.mime(), o.pripona()));
        let (mime, pripona) = o.unwrap_or(("image/jpeg", "jpg"));
        a.extend([
            "-attach".into(),
            c.display().to_string(),
            "-metadata:s:t:0".into(),
            format!("mimetype={mime}"),
            "-metadata:s:t:0".into(),
            format!("filename=cover.{pripona}"),
        ]);
    }
    a.extend(meta);
    a.extend(["-metadata".into(), format!("title={titul}")]);
    for (kluc, hodnota) in [
        ("artist", &doplnky.autor),
        ("date", &doplnky.datum),
        ("comment", &doplnky.zdroj),
        ("description", &doplnky.popis),
    ] {
        if let Some(h) = hodnota {
            a.extend(["-metadata".into(), format!("{kluc}={h}")]);
        }
    }
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
        .map_err(|e| Chyba::Ine(format!("FFmpeg sa nedá spustiť: {e}")));
    for c in &docasne {
        let _ = tokio::fs::remove_file(c).await;
    }
    let vystup = vystup?;
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
    use super::*;

    #[test]
    fn kapitoly_pre_ffmpeg() {
        let d = Doplnky {
            kapitoly: vec![
                Kapitola {
                    od: 0,
                    nazov: "Úvod; a=b".into(),
                },
                Kapitola {
                    od: 90_000_000_000,
                    nazov: "Koniec".into(),
                },
            ],
            trvanie: Some(120_000_000_000),
            ..Default::default()
        };
        let t = ffmeta_kapitoly(&d).unwrap();
        assert!(t.starts_with(";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=90000\n"));
        assert!(t.contains("title=Úvod\\; a\\=b\n"));
        assert!(t.ends_with("START=90000\nEND=120000\ntitle=Koniec\n"));
        assert_eq!(ffmeta_kapitoly(&Doplnky::default()), None);
    }

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
