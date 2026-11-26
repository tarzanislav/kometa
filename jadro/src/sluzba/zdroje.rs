//! Rozpoznanie zdroja odkazu (štítky v okne), playlistov a kanálov a odkazov v texte.

use url::Url;

fn hostitel(adresa: &str) -> Option<(String, Url)> {
    let u = Url::parse(adresa.trim()).ok()?;
    let h = u.host_str()?.to_ascii_lowercase();
    let h = h
        .strip_prefix("www.")
        .or(h.strip_prefix("m."))
        .unwrap_or(&h)
        .to_string();
    Some((h, u))
}

fn je_domena(h: &str, d: &str) -> bool {
    h == d || h.ends_with(&format!(".{d}"))
}

/// Názov služby pre štítok položky.
pub fn nazov_zdroja(adresa: &str) -> &'static str {
    let Some((h, _)) = hostitel(adresa) else {
        return "Web";
    };
    const ZNAME: &[(&str, &[&str])] = &[
        (
            "YouTube",
            &["youtube.com", "youtu.be", "youtube-nocookie.com"],
        ),
        ("TikTok", &["tiktok.com"]),
        ("Instagram", &["instagram.com"]),
        ("Facebook", &["facebook.com", "fb.watch", "fb.com"]),
        ("Webshare", &["webshare.cz"]),
        ("Fastshare", &["fastshare.cz", "fastshare.cloud"]),
        ("Datoid", &["datoid.cz", "datoid.sk"]),
        ("X/Twitter", &["twitter.com", "x.com"]),
        ("Vimeo", &["vimeo.com"]),
    ];
    ZNAME
        .iter()
        .find(|(_, d)| d.iter().any(|d| je_domena(&h, d)))
        .map(|(n, _)| *n)
        .unwrap_or("Web")
}

/// Úložiská súborov – odkaz vedie na jeden hotový súbor, formát ani kvalita sa nevyberajú.
pub const ULOZISKA: [&str; 3] = ["Webshare", "Fastshare", "Datoid"];

pub fn je_ulozisko(adresa: &str) -> bool {
    ULOZISKA.contains(&nazov_zdroja(adresa))
}

/// Kanál YouTube: (základ kanála, karta) – napr. ("https://www.youtube.com/@ucitel", Some("videos")).
fn kanal_youtube(adresa: &str) -> Option<(String, Option<String>)> {
    let (h, u) = hostitel(adresa)?;
    if h != "youtube.com" {
        return None;
    }
    let s: Vec<&str> = u.path_segments()?.filter(|s| !s.is_empty()).collect();
    let (zaklad, zvysok) = match s.as_slice() {
        [a, z @ ..] if a.starts_with('@') => (format!("/{a}"), z),
        ["channel" | "c" | "user", b, z @ ..] => (format!("/{}/{b}", s[0]), z),
        _ => return None,
    };
    const KARTY: [&str; 6] = [
        "videos",
        "shorts",
        "streams",
        "playlists",
        "featured",
        "live",
    ];
    match zvysok {
        [] => Some((format!("https://www.youtube.com{zaklad}"), None)),
        [k] if KARTY.contains(k) => Some((
            format!("https://www.youtube.com{zaklad}"),
            Some(k.to_string()),
        )),
        _ => None,
    }
}

/// Playlist alebo kanál (nie jedno video). Video z playlistu (watch?v=…&list=…) je jedno video.
pub fn je_kolekcia(adresa: &str) -> bool {
    let Some((h, u)) = hostitel(adresa) else {
        return false;
    };
    let s: Vec<&str> = u
        .path_segments()
        .map(|s| s.filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    match h.as_str() {
        "youtube.com" => u.path() == "/playlist" || kanal_youtube(adresa).is_some(),
        "vimeo.com" => {
            matches!(s.as_slice(), ["showcase" | "album", id, ..] if id.bytes().all(|b| b.is_ascii_digit()))
                || matches!(s.as_slice(), ["channels", _])
        }
        "soundcloud.com" => s.len() >= 3 && s[1] == "sets",
        "dailymotion.com" => s.first() == Some(&"playlist"),
        _ => false,
    }
}

/// Kanál bez karty → karta Videá (prípadne Shorts / Naživo).
pub fn adresa_kolekcie(adresa: &str, karta: &str) -> String {
    match kanal_youtube(adresa) {
        Some((zaklad, None)) => format!("{zaklad}/{karta}"),
        _ => adresa.trim().to_string(),
    }
}

pub fn je_kanal(adresa: &str) -> bool {
    kanal_youtube(adresa).is_some()
}

/// ID videa YouTube (pre náhľad).
pub fn id_youtube(adresa: &str) -> Option<String> {
    let (h, u) = hostitel(adresa)?;
    let id = match h.as_str() {
        "youtu.be" => u.path_segments()?.next()?.to_string(),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
            if u.path() == "/watch" {
                u.query_pairs().find(|(k, _)| k == "v")?.1.into_owned()
            } else {
                let mut s = u.path_segments()?;
                match s.next()? {
                    "shorts" | "live" | "embed" => s.next()?.to_string(),
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then_some(id)
}

pub fn nahlad_youtube(adresa: &str) -> String {
    id_youtube(adresa)
        .map(|id| format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
        .unwrap_or_default()
}

/// Všetky odkazy http(s) v texte, bez opakovaní a bez interpunkcie na konci.
pub fn odkazy(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut zvysok = text;
    while let Some(i) = zvysok.find("http") {
        let kus = &zvysok[i..];
        if !(kus.starts_with("http://") || kus.starts_with("https://")) {
            zvysok = &kus[4..];
            continue;
        }
        let koniec = kus
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\''))
            .unwrap_or(kus.len());
        let u = kus[..koniec].trim_end_matches([')', '.', ',', ';', ']']);
        if u.len() > 8 && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
        }
        zvysok = &kus[koniec..];
    }
    out
}

/// Heslo uvedené v skopírovanom texte („Heslo: …“, „Password = …“, „pw - …“), aké býva pri
/// odkazoch na fórach a v správach. Riadok musí začínať slovom heslo/password (aj „Heslo k
/// archívu:“) a hneď za ním oddeľovač. Keď je v texte viac rôznych hesiel, nevie sa, ku ktorému
/// odkazu patria – vtedy `None` a heslo zadá človek.
pub fn heslo_z_textu(text: &str) -> Option<String> {
    let mut najdene: Vec<String> = vec![];
    for riadok in text.lines() {
        let r = riadok
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim();
        let Some(i) = r.find([':', '=', '-', '–', '—']) else {
            continue;
        };
        let (stitok, hodnota) = (r[..i].trim().to_lowercase(), &r[i..]);
        let mut slova = stitok.split_whitespace();
        let prve = slova.next().unwrap_or("");
        // „Heslo k archívu“, „heslo na rozbalenie“, „password for archive“ – nie „password reset“
        const DOPLNKY: [&str; 22] = [
            "k",
            "ku",
            "na",
            "pre",
            "do",
            "je",
            "archívu",
            "archivu",
            "archívom",
            "archivom",
            "rozbalenie",
            "súboru",
            "suboru",
            "súborom",
            "suborom",
            "for",
            "is",
            "archive",
            "archives",
            "file",
            "files",
            "rar",
        ];
        let je_stitok = matches!(
            prve,
            "heslo" | "hesla" | "heslá" | "password" | "passwort" | "pass" | "pw" | "pwd"
        ) && slova.all(|w| DOPLNKY.contains(&w));
        if !je_stitok {
            continue;
        }
        let h = hodnota
            .trim_start_matches([':', '=', '-', '–', '—'])
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '„' | '“' | '”' | '`'));
        // heslo je jedno slovo, nanajvýš pár slov – nie celá veta
        if h.is_empty()
            || h.chars().count() > 128
            || h.contains("http")
            || h.split_whitespace().count() > 3
        {
            continue;
        }
        if !najdene.iter().any(|x| x == h) {
            najdene.push(h.to_string());
        }
    }
    (najdene.len() == 1).then(|| najdene.remove(0))
}

/// Bezpečný názov súboru pre Windows, macOS aj Linux .
pub fn bezpecny_nazov(text: &str, limit: usize) -> String {
    let mut s = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            ':' => s.push_str(" -"),
            '"' => s.push('\''),
            '/' | '\\' | '|' => s.push('-'),
            '?' | '*' => {}
            '<' => s.push('('),
            '>' => s.push(')'),
            c if (c as u32) < 0x20 => {}
            c => s.push(c),
        }
    }
    let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    if s.chars().count() > limit {
        let (kmen, pripona) = match s.rsplit_once('.') {
            Some((k, p)) if (1..=5).contains(&p.len()) => (k.to_string(), format!(".{p}")),
            _ => (s.clone(), String::new()),
        };
        let mut k: String = kmen
            .chars()
            .take(limit.saturating_sub(pripona.chars().count()))
            .collect();
        while k.ends_with(['.', ' ']) {
            k.pop();
        }
        s = k + &pripona;
    }
    if s.is_empty() {
        "kometa".into()
    } else {
        s
    }
}

#[cfg(test)]
mod testy {
    #[test]
    fn heslo_zo_skopirovaneho_textu() {
        let t = "Film 2024\nhttps://webshare.cz/#/file/abc/film.part1.rar\nHeslo: KometaLeti\n";
        assert_eq!(heslo_z_textu(t).as_deref(), Some("KometaLeti"));
        assert_eq!(
            heslo_z_textu("• Heslo k archívu: „tajne 1“").as_deref(),
            Some("tajne 1")
        );
        assert_eq!(
            heslo_z_textu("Password = abc123").as_deref(),
            Some("abc123")
        );
        assert_eq!(heslo_z_textu("pw - x\nPW: x").as_deref(), Some("x"));
        // bez hesla, viac rôznych hesiel, heslo je odkaz, „heslo“ uprostred vety
        assert_eq!(heslo_z_textu("https://webshare.cz/#/file/abc"), None);
        assert_eq!(heslo_z_textu("Heslo: a\nHeslo: b"), None);
        assert_eq!(heslo_z_textu("Heslo: https://example.com/x"), None);
        assert_eq!(heslo_z_textu("Zabudol som heslo: neviem"), None);
        assert_eq!(
            heslo_z_textu("Password reset - klikni sem a zmeň si ho"),
            None
        );
    }

    use super::*;

    #[test]
    fn zdroje_a_kolekcie() {
        assert_eq!(nazov_zdroja("https://youtu.be/dQw4w9WgXcQ"), "YouTube");
        assert_eq!(
            nazov_zdroja("https://m.facebook.com/watch/?v=1"),
            "Facebook"
        );
        assert_eq!(nazov_zdroja("https://x.com/a/status/1"), "X/Twitter");
        assert_eq!(nazov_zdroja("https://webshare.cz/#/file/abc"), "Webshare");
        assert_eq!(nazov_zdroja("https://box.example.sk/v.mp4"), "Web");
        assert!(je_kolekcia("https://www.youtube.com/playlist?list=PL123"));
        assert!(je_kolekcia("https://www.youtube.com/@ucitel"));
        assert!(je_kolekcia("https://www.youtube.com/@ucitel/shorts"));
        assert!(!je_kolekcia(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123"
        ));
        assert!(je_kolekcia("https://vimeo.com/showcase/123456"));
        assert!(!je_kolekcia("https://vimeo.com/123456"));
        assert_eq!(
            adresa_kolekcie("https://www.youtube.com/@ucitel", "videos"),
            "https://www.youtube.com/@ucitel/videos"
        );
        assert_eq!(
            adresa_kolekcie("https://www.youtube.com/@ucitel/shorts", "videos"),
            "https://www.youtube.com/@ucitel/shorts"
        );
    }

    #[test]
    fn youtube_id_a_odkazy() {
        assert_eq!(
            id_youtube("https://www.youtube.com/watch?feature=x&v=dQw4w9WgXcQ").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(
            id_youtube("https://youtube.com/shorts/dQw4w9WgXcQ?si=1").as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert_eq!(id_youtube("https://vimeo.com/1"), None);
        let t = "pozri (https://a.sk/x.mp4), a tiež https://b.cz/y; a znova https://a.sk/x.mp4.";
        assert_eq!(odkazy(t), vec!["https://a.sk/x.mp4", "https://b.cz/y"]);
    }

    #[test]
    fn nazvy_suborov() {
        assert_eq!(
            bezpecny_nazov("Film: časť 1/2 \"nový\"?", 140),
            "Film - časť 1-2 'nový'"
        );
        assert_eq!(bezpecny_nazov("  ...  ", 140), "kometa");
        let dlhy = format!("{}.mkv", "á".repeat(300));
        let k = bezpecny_nazov(&dlhy, 100);
        assert!(k.ends_with(".mkv") && k.chars().count() == 100);
    }
}
