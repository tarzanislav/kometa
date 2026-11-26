//! Údaje o videu z informácií yt-dlp – názov, autor, dátum, zdroj, popis, kapitoly a obrázok –
//! pripravené na vloženie do výsledného súboru ([`crate::media::Doplnky`]).
//!
//! Čo sa vloží, určujú nastavenia „vlozit_udaje“, „vlozit_kapitoly“ a „vlozit_obal“.

use crate::media::{Doplnky, Kapitola, Obal, NS};
use serde_json::Value;

/// Čo vkladať do súborov (podľa nastavení).
#[derive(Clone, Copy, Debug)]
pub struct Volby {
    pub udaje: bool,
    pub kapitoly: bool,
    pub obal: bool,
}

impl Volby {
    pub fn z_nastaveni(n: &super::nastavenia::Nastavenia) -> Self {
        Volby {
            udaje: n.ano("vlozit_udaje", true),
            kapitoly: n.ano("vlozit_kapitoly", true),
            obal: n.ano("vlozit_obal", true),
        }
    }

    pub fn nic(&self) -> bool {
        !(self.udaje || self.kapitoly || self.obal)
    }
}

fn text(info: &Value, k: &str) -> Option<String> {
    info.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Dátum zverejnenia (RRRRMMDD z yt-dlp) → RRRR-MM-DD.
pub fn datum(info: &Value) -> Option<String> {
    let d = text(info, "release_date").or_else(|| text(info, "upload_date"))?;
    let b = d.as_bytes();
    if b.len() == 8 && b.iter().all(u8::is_ascii_digit) {
        Some(format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]))
    } else if b.len() == 4 && b.iter().all(u8::is_ascii_digit) {
        Some(d)
    } else {
        None
    }
}

/// Kapitoly z yt-dlp (YouTube: od autora, z popisu aj automatické).
pub fn kapitoly(info: &Value) -> Vec<Kapitola> {
    info.get("chapters")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|k| {
                    let od = k.get("start_time")?.as_f64()?;
                    Some(Kapitola {
                        od: (od.max(0.0) * NS as f64).round() as i64,
                        nazov: text(k, "title").unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Adresy obrázkov videa v poradí od najlepšieho; len JPEG a PNG (WebP prehrávače v súboroch
/// nezobrazia a prevod by potreboval dekodér obrázkov).
pub fn adresy_obrazkov(info: &Value) -> Vec<String> {
    let je_jpg_png = |u: &str| {
        let cesta = u
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        cesta.ends_with(".jpg") || cesta.ends_with(".jpeg") || cesta.ends_with(".png")
    };
    let mut zoznam: Vec<(i64, u64, String)> = info
        .get("thumbnails")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let u = t.get("url")?.as_str()?;
                    je_jpg_png(u).then(|| {
                        let pref = t.get("preference").and_then(Value::as_i64).unwrap_or(0);
                        let plocha = t.get("width").and_then(Value::as_u64).unwrap_or(0)
                            * t.get("height").and_then(Value::as_u64).unwrap_or(0);
                        (pref, plocha, u.to_string())
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    zoznam.sort_by_key(|x| std::cmp::Reverse((x.0, x.1)));
    let mut out: Vec<String> = zoznam.into_iter().map(|x| x.2).collect();
    if let Some(t) = text(info, "thumbnail").filter(|t| je_jpg_png(t)) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out.truncate(4);
    out
}

/// Stiahne prvý dostupný obrázok (JPEG/PNG, najviac 10 MB).
pub async fn stiahni_obal(adresy: &[String]) -> Option<Obal> {
    let k = crate::klient::klient(&crate::klient::Nastavenia::default()).ok()?;
    for u in adresy {
        let Ok(Ok(odp)) =
            tokio::time::timeout(std::time::Duration::from_secs(20), k.get(u).send()).await
        else {
            continue;
        };
        if !odp.status().is_success() || odp.content_length().unwrap_or(0) > 10 << 20 {
            continue;
        }
        if let Ok(b) = odp.bytes().await {
            if let Some(o) = Obal::z_dat(b.to_vec()) {
                return Some(o);
            }
        }
    }
    None
}

/// Doplnky pre súbor z informácií yt-dlp podľa volieb (obrázok sa stiahne).
pub async fn z_info(info: &Value, volby: Volby) -> Doplnky {
    let mut d = Doplnky {
        titul: text(info, "title").unwrap_or_default(),
        trvanie: info
            .get("duration")
            .and_then(Value::as_f64)
            .map(|s| (s * NS as f64) as i64),
        ..Default::default()
    };
    if volby.udaje {
        d.autor = text(info, "uploader").or_else(|| text(info, "channel"));
        d.datum = datum(info);
        d.zdroj = text(info, "webpage_url").or_else(|| text(info, "original_url"));
        d.popis = text(info, "description");
    }
    if volby.kapitoly {
        d.kapitoly = kapitoly(info);
    }
    if volby.obal {
        d.obal = stiahni_obal(&adresy_obrazkov(info)).await;
    }
    d
}

/// Prepínače yt-dlp (keď spája on sám s FFmpeg) podľa volieb a formátu.
pub fn argumenty_ytdlp(volby: Volby, format: &str) -> Vec<String> {
    let mut a = vec![];
    if volby.udaje {
        a.push("--embed-metadata");
    }
    if volby.kapitoly {
        a.push("--embed-chapters");
    }
    // obrázok len tam, kde výsledok určite vie obrázok niesť (MP4/MKV/MP3), nie WebM/Opus
    if volby.obal && matches!(format, "video" | "mp4" | "1080" | "720" | "mp3") {
        a.extend(["--embed-thumbnail", "--convert-thumbnails", "jpg"]);
    }
    a.into_iter().map(str::to_string).collect()
}

#[cfg(test)]
mod testy {
    use super::*;
    use serde_json::json;

    #[test]
    fn z_informacii() {
        let info = json!({
            "title": "Video", "uploader": "Kanál", "upload_date": "20240517",
            "webpage_url": "https://www.youtube.com/watch?v=abc", "description": " Popis ",
            "chapters": [{"start_time": 0.0, "end_time": 61.5, "title": "Úvod"},
                         {"start_time": 61.5, "end_time": 120.0, "title": "Druhá"}],
            "thumbnails": [
                {"url": "https://i.ytimg.com/vi/abc/hqdefault.jpg?sqp=1", "preference": -7, "width": 480, "height": 360},
                {"url": "https://i.ytimg.com/vi_webp/abc/maxresdefault.webp", "preference": 0},
                {"url": "https://i.ytimg.com/vi/abc/maxresdefault.jpg", "preference": -1, "width": 1280, "height": 720}
            ],
            "thumbnail": "https://i.ytimg.com/vi_webp/abc/maxresdefault.webp"
        });
        assert_eq!(datum(&info).as_deref(), Some("2024-05-17"));
        let k = kapitoly(&info);
        assert_eq!(
            k[1],
            Kapitola {
                od: 61_500_000_000,
                nazov: "Druhá".into()
            }
        );
        assert_eq!(
            adresy_obrazkov(&info),
            [
                "https://i.ytimg.com/vi/abc/maxresdefault.jpg",
                "https://i.ytimg.com/vi/abc/hqdefault.jpg?sqp=1"
            ]
        );
        let v = Volby {
            udaje: true,
            kapitoly: false,
            obal: true,
        };
        assert_eq!(argumenty_ytdlp(v, "audio"), ["--embed-metadata"]);
        assert_eq!(argumenty_ytdlp(v, "video").len(), 4);
    }
}
