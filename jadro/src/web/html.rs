//! Tolerantný čítač HTML – vlastný, bez prehliadača a bez cudzích knižníc.
//!
//! Nevytvára strom dokumentu; vráti len to, čo skener potrebuje: počiatočné značky s atribútmi,
//! obsah `<title>` a skriptov (JSON-LD, konfigurácie prehrávačov). Pokazené HTML nevadí –
//! čítač pokračuje ďalej, ako to robia prehliadače.

/// Jedna počiatočná značka (`<video src=…>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Znacka {
    /// názov malými písmenami
    pub nazov: String,
    /// atribúty: názvy malými písmenami, hodnoty s rozbalenými entitami
    pub atributy: Vec<(String, String)>,
}

impl Znacka {
    pub fn atr(&self, meno: &str) -> Option<&str> {
        self.atributy
            .iter()
            .find(|(k, _)| k == meno)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Dokument {
    pub znacky: Vec<Znacka>,
    pub titul: String,
    /// obsah `<script>` (bez `src`) s typom, napr. ("application/ld+json", "{…}")
    pub skripty: Vec<(String, String)>,
}

/// Rozbalí HTML entity: pomenované najčastejšie a všetky číselné (`&#47;`, `&#x2F;`).
pub fn entity(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut zvysok = s;
    while let Some(i) = zvysok.find('&') {
        out.push_str(&zvysok[..i]);
        let za = &zvysok[i + 1..];
        let koniec = za.find(';').filter(|&k| k <= 10);
        let nahrada = koniec.and_then(|k| {
            let meno = &za[..k];
            let znak = if let Some(c) = meno.strip_prefix("#x").or_else(|| meno.strip_prefix("#X"))
            {
                u32::from_str_radix(c, 16).ok().and_then(char::from_u32)
            } else if let Some(c) = meno.strip_prefix('#') {
                c.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                match meno {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "sol" => Some('/'),
                    "colon" => Some(':'),
                    "equals" => Some('='),
                    "quest" => Some('?'),
                    "period" => Some('.'),
                    _ => None,
                }
            };
            znak.map(|c| (c, k))
        });
        match nahrada {
            Some((c, k)) => {
                out.push(c);
                zvysok = &za[k + 1..];
            }
            None => {
                out.push('&');
                zvysok = za;
            }
        }
    }
    out.push_str(zvysok);
    out
}

fn je_znak_nazvu(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')
}

/// Prečíta dokument.
pub fn citaj(text: &str) -> Dokument {
    let b = text.as_bytes();
    let mut d = Dokument::default();
    let mut i = 0;
    let n = b.len();
    while i < n {
        let Some(k) = memchr(b'<', &b[i..]) else {
            break;
        };
        i += k;
        // komentár, deklarácia, spracovanie
        if b[i..].starts_with(b"<!--") {
            i = najdi(b, i + 4, b"-->").map(|x| x + 3).unwrap_or(n);
            continue;
        }
        if b.get(i + 1).is_some_and(|c| *c == b'!' || *c == b'?') {
            i = memchr(b'>', &b[i..]).map(|x| i + x + 1).unwrap_or(n);
            continue;
        }
        if b.get(i + 1) == Some(&b'/') {
            i = memchr(b'>', &b[i..]).map(|x| i + x + 1).unwrap_or(n);
            continue;
        }
        let zac = i + 1;
        let mut j = zac;
        while j < n && je_znak_nazvu(b[j]) {
            j += 1;
        }
        if j == zac {
            i += 1; // samotné „<“ v texte
            continue;
        }
        let nazov = text[zac..j].to_ascii_lowercase();
        let (atributy, koniec) = atributy(text, j);
        i = koniec;
        let znacka = Znacka { nazov, atributy };
        match znacka.nazov.as_str() {
            "script" | "style" | "textarea" | "noscript" => {
                let uzaver = format!("</{}", znacka.nazov);
                let obsah_koniec = najdi_bez_velkosti(b, i, uzaver.as_bytes()).unwrap_or(n);
                if znacka.nazov == "script" && znacka.atr("src").is_none() {
                    let typ = znacka.atr("type").unwrap_or("").to_ascii_lowercase();
                    d.skripty.push((typ, text[i..obsah_koniec].to_string()));
                }
                i = memchr(b'>', &b[obsah_koniec.min(n)..])
                    .map(|x| obsah_koniec + x + 1)
                    .unwrap_or(n);
            }
            "title" if d.titul.is_empty() => {
                let k = najdi_bez_velkosti(b, i, b"</title").unwrap_or(n);
                d.titul = entity(text[i..k].trim());
                i = k;
            }
            _ => {}
        }
        d.znacky.push(znacka);
    }
    d
}

/// Atribúty od pozície `i` po koniec značky `>`. Vráti (atribúty, pozícia za značkou).
fn atributy(text: &str, mut i: usize) -> (Vec<(String, String)>, usize) {
    let b = text.as_bytes();
    let n = b.len();
    let mut out = vec![];
    loop {
        while i < n && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= n {
            return (out, n);
        }
        if b[i] == b'>' {
            return (out, i + 1);
        }
        let zac = i;
        while i < n && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let meno = text[zac..i].to_ascii_lowercase();
        while i < n && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut hodnota = String::new();
        if i < n && b[i] == b'=' {
            i += 1;
            while i < n && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < n && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let k = memchr(q, &b[i + 1..]).map(|x| i + 1 + x).unwrap_or(n);
                hodnota = entity(&text[i + 1..k]);
                i = (k + 1).min(n);
            } else {
                let z = i;
                while i < n && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                hodnota = entity(&text[z..i]);
            }
        }
        if !meno.is_empty() {
            out.push((meno, hodnota));
        } else {
            i += 1;
        }
    }
}

fn memchr(c: u8, b: &[u8]) -> Option<usize> {
    b.iter().position(|&x| x == c)
}

fn najdi(b: &[u8], od: usize, co: &[u8]) -> Option<usize> {
    if od >= b.len() {
        return None;
    }
    b[od..]
        .windows(co.len())
        .position(|w| w == co)
        .map(|k| od + k)
}

fn najdi_bez_velkosti(b: &[u8], od: usize, co: &[u8]) -> Option<usize> {
    if od >= b.len() {
        return None;
    }
    b[od..]
        .windows(co.len())
        .position(|w| w.eq_ignore_ascii_case(co))
        .map(|k| od + k)
}

/// Odhalí adresy zakódované v JavaScripte a JSON: `\/` → `/`, `/` → `/`, `&amp;` → `&`.
pub fn odkry(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'/' => {
                    out.push('/');
                    i += 2;
                    continue;
                }
                b'u' if i + 5 < b.len() => {
                    if let Some(c) = std::str::from_utf8(&b[i + 2..i + 6])
                        .ok()
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .and_then(char::from_u32)
                    {
                        out.push(c);
                        i += 6;
                        continue;
                    }
                }
                _ => {}
            }
        }
        // celý znak UTF-8
        let dlzka = match b[i] {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        };
        let k = (i + dlzka).min(b.len());
        out.push_str(std::str::from_utf8(&b[i..k]).unwrap_or("\u{fffd}"));
        i = k;
    }
    entity(&out)
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn znacky_a_atributy() {
        let d = citaj(
            r#"<!doctype html><html><head><title>Kurz &amp; video</title>
            <meta property="og:video" content="https://x.sk/a.mp4">
            <!-- <iframe src="komentar.html"> -->
            <script type="application/ld+json">{"@type":"VideoObject","contentUrl":"/v.m3u8"}</script>
            <script>var u = "<iframe src='nie.html'>";</script>
            </head><body><video data-src=/film.mp4 controls><source src='hls/master.m3u8' type="application/x-mpegURL"></video>
            <iframe src="https://player.vimeo.com/video/123?h=ab&amp;badge=0" allowfullscreen></iframe>
            <p>5 < 6 a <b>tučné</b></p><img src="obr.jpg"/></body></html>"#,
        );
        assert_eq!(d.titul, "Kurz & video");
        let mena: Vec<&str> = d.znacky.iter().map(|z| z.nazov.as_str()).collect();
        assert!(mena.contains(&"video") && mena.contains(&"source") && mena.contains(&"iframe"));
        assert!(
            !d.znacky
                .iter()
                .any(|z| z.atr("src") == Some("komentar.html")),
            "komentár sa číta"
        );
        assert!(
            !d.znacky.iter().any(|z| z.atr("src") == Some("nie.html")),
            "značka zo skriptu"
        );
        let video = d.znacky.iter().find(|z| z.nazov == "video").unwrap();
        assert_eq!(video.atr("data-src"), Some("/film.mp4"));
        assert_eq!(video.atr("controls"), Some(""));
        let iframe = d.znacky.iter().find(|z| z.nazov == "iframe").unwrap();
        assert_eq!(
            iframe.atr("src"),
            Some("https://player.vimeo.com/video/123?h=ab&badge=0")
        );
        assert_eq!(d.skripty.len(), 2);
        assert_eq!(d.skripty[0].0, "application/ld+json");
        assert!(d
            .znacky
            .iter()
            .any(|z| z.nazov == "img" && z.atr("src") == Some("obr.jpg")));
    }

    #[test]
    fn entity_a_odkrytie() {
        assert_eq!(
            entity("a&amp;b&#47;c&#x2F;d&nbsp;e&neznama;"),
            "a&b/c/d\u{a0}e&neznama;"
        );
        assert_eq!(
            odkry(r#"{"u":"https:\/\/cdn.sk\/v/master.m3u8?a=1&amp;b=2"}"#),
            r#"{"u":"https://cdn.sk/v/master.m3u8?a=1&b=2"}"#
        );
        assert_eq!(odkry("ľščť"), "ľščť");
    }

    #[test]
    fn pokazene_html() {
        let d = citaj("<div class=\"x\" <video src=a.mp4><iframe src=\"b.html");
        assert!(!d.znacky.is_empty());
        let d = citaj("<<<>>> <");
        assert!(d.znacky.is_empty());
    }
}
