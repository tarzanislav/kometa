//! Rozpoznanie vložených prehrávačov a mediálnych súborov podľa rozobranej adresy
//! (hostiteľ, časti cesty, parametre) – presnejšie ako hľadanie vzorov v texte.

use serde::Serialize;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Druh {
    Prehravac,
    Stream,
    Video,
    Zvuk,
    Subor,
    Titulky,
}

impl Druh {
    pub fn text(self) -> &'static str {
        match self {
            Druh::Prehravac => "Prehrávač",
            Druh::Stream => "Stream",
            Druh::Video => "Video",
            Druh::Zvuk => "Zvuk",
            Druh::Subor => "Súbor",
            Druh::Titulky => "Titulky",
        }
    }

    pub fn da_sa_stiahnut(self) -> bool {
        self != Druh::Titulky
    }
}

/// Vložený prehrávač: poskytovateľ, kanonická adresa, kľúč na odstránenie duplicít.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prehravac {
    pub poskytovatel: &'static str,
    pub url: String,
    pub kluc: String,
}

fn segmenty(u: &Url) -> Vec<&str> {
    u.path_segments()
        .map(|s| s.filter(|x| !x.is_empty()).collect())
        .unwrap_or_default()
}

fn parameter(u: &Url, meno: &str) -> Option<String> {
    u.query_pairs()
        .find(|(k, _)| k == meno)
        .map(|(_, v)| v.into_owned())
}

fn host(u: &Url) -> String {
    u.host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .trim_start_matches("m.")
        .to_ascii_lowercase()
}

fn len_cislice(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn hex(s: &str, min: usize) -> bool {
    s.len() >= min && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn yt_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn vimeo(id: &str, h: Option<String>) -> Prehravac {
    let url = match &h {
        Some(h) => format!("https://player.vimeo.com/video/{id}?h={h}"),
        None => format!("https://player.vimeo.com/video/{id}"),
    };
    Prehravac {
        poskytovatel: "Vimeo",
        url,
        kluc: format!("vimeo:{id}"),
    }
}

fn p(poskytovatel: &'static str, url: String, kluc: String) -> Option<Prehravac> {
    Some(Prehravac {
        poskytovatel,
        url,
        kluc,
    })
}

/// Ak adresa patrí známemu prehrávaču, vráti ho.
pub fn prehravac(adresa: &str) -> Option<Prehravac> {
    let u = Url::parse(adresa).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let h = host(&u);
    let s = segmenty(&u);
    let seg = |i: usize| s.get(i).copied().unwrap_or("");
    match h.as_str() {
        "player.vimeo.com" if seg(0) == "video" && len_cislice(seg(1)) => {
            Some(vimeo(seg(1), parameter(&u, "h")))
        }
        "vimeo.com" => {
            if matches!(seg(0), "event" | "showcase") && len_cislice(seg(1)) {
                return p(
                    "Vimeo",
                    format!("https://vimeo.com/{}/{}", seg(0), seg(1)),
                    format!("vimeo:{}:{}", seg(0), seg(1)),
                );
            }
            // vimeo.com/123, /123/hash, /video/123, /channels/x/123, /groups/x/videos/123, /album/1/video/123
            let (k, id) = s
                .iter()
                .enumerate()
                .find(|(_, x)| len_cislice(x) && x.len() >= 5)?;
            let pred: Vec<&str> = s[..k].to_vec();
            let dovolene = matches!(
                pred.as_slice(),
                [] | ["video"] | ["channels", _] | ["groups", _, "videos"] | ["album", _, "video"]
            );
            if !dovolene {
                return None;
            }
            let hash = s
                .get(k + 1)
                .filter(|x| hex(x, 6))
                .map(|x| x.to_string())
                .or_else(|| parameter(&u, "h"));
            Some(vimeo(id, hash))
        }
        "embed.vhx.tv" if seg(0) == "videos" && len_cislice(seg(1)) => p(
            "Vimeo OTT",
            format!("https://embed.vhx.tv/videos/{}", seg(1)),
            format!("vhx:{}", seg(1)),
        ),
        "youtube.com" | "youtube-nocookie.com" | "music.youtube.com" => {
            let id = match seg(0) {
                "embed" | "v" | "shorts" | "live" => Some(seg(1).to_string()),
                "watch" => parameter(&u, "v"),
                _ => None,
            }?;
            yt_id(&id).then(|| Prehravac {
                poskytovatel: "YouTube",
                url: format!("https://www.youtube.com/watch?v={id}"),
                kluc: format!("yt:{id}"),
            })
        }
        "youtu.be" if yt_id(seg(0)) => p(
            "YouTube",
            format!("https://www.youtube.com/watch?v={}", seg(0)),
            format!("yt:{}", seg(0)),
        ),
        "fast.wistia.net" | "fast.wistia.com" | "wistia.com" | "home.wistia.com" => {
            let id = match (seg(0), seg(1)) {
                ("embed", "iframe" | "medias") => seg(2),
                ("medias", _) => seg(1),
                _ => "",
            };
            let id = id.split('.').next().unwrap_or("");
            (id.len() >= 8 && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| Prehravac {
                poskytovatel: "Wistia",
                url: format!("https://fast.wistia.net/embed/iframe/{id}"),
                kluc: format!("wistia:{id}"),
            })
        }
        "iframe.mediadelivery.net" | "player.mediadelivery.net"
            if matches!(seg(0), "embed" | "play") && len_cislice(seg(1)) && seg(2).len() == 36 =>
        {
            p(
                "Bunny Stream",
                format!(
                    "https://iframe.mediadelivery.net/embed/{}/{}",
                    seg(1),
                    seg(2)
                ),
                format!("bunny:{}", seg(2)),
            )
        }
        "cdn.jwplayer.com" | "content.jwplatform.com"
            if matches!(
                seg(0),
                "players" | "previews" | "videos" | "manifests" | "v2"
            ) =>
        {
            let surovy = if seg(0) == "v2" { seg(2) } else { seg(1) };
            let id: String = surovy.chars().take(8).collect();
            (id.len() == 8 && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| Prehravac {
                poskytovatel: "JW Player",
                url: format!("https://cdn.jwplayer.com/v2/media/{id}"),
                kluc: format!("jw:{id}"),
            })
        }
        "players.brightcove.net" if seg(2) == "index.html" => {
            let id = parameter(&u, "videoId")?;
            p("Brightcove", adresa.to_string(), format!("bc:{id}"))
        }
        x if (x.ends_with(".cloudflarestream.com")
            || x == "iframe.videodelivery.net"
            || x == "videodelivery.net")
            && hex(seg(0), 32) =>
        {
            p(
                "Cloudflare Stream",
                format!("https://iframe.videodelivery.net/{}", seg(0)),
                format!("cf:{}", seg(0)),
            )
        }
        "dailymotion.com" | "dai.ly" | "geo.dailymotion.com" => {
            let id = match (h.as_str(), seg(0), seg(1)) {
                ("dai.ly", x, _) => x.to_string(),
                (_, "video", x) => x.to_string(),
                (_, "embed", "video") => seg(2).to_string(),
                _ => parameter(&u, "video").unwrap_or_default(),
            };
            let id = id.split('_').next().unwrap_or("").to_string();
            (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| Prehravac {
                poskytovatel: "Dailymotion",
                url: format!("https://www.dailymotion.com/video/{id}"),
                kluc: format!("dm:{id}"),
            })
        }
        "loom.com" if matches!(seg(0), "embed" | "share") && hex(seg(1), 32) => p(
            "Loom",
            format!("https://www.loom.com/share/{}", seg(1)),
            format!("loom:{}", seg(1)),
        ),
        "streamable.com" => {
            let id = if matches!(seg(0), "e" | "o" | "s") {
                seg(1)
            } else {
                seg(0)
            };
            if !(5..=8).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return None;
            }
            p(
                "Streamable",
                format!("https://streamable.com/{id}"),
                format!("stb:{id}"),
            )
        }
        "rumble.com" if seg(0) == "embed" && seg(1).starts_with('v') => p(
            "Rumble",
            format!("https://rumble.com/embed/{}/", seg(1)),
            format!("rumble:{}", seg(1)),
        ),
        "player.twitch.tv" => {
            let v = parameter(&u, "video")?;
            let v = v.trim_start_matches('v').to_string();
            if !len_cislice(&v) {
                return None;
            }
            p(
                "Twitch",
                format!("https://www.twitch.tv/videos/{v}"),
                format!("tw:{v}"),
            )
        }
        "clips.twitch.tv" => {
            let c = parameter(&u, "clip").unwrap_or_else(|| seg(0).to_string());
            if c.is_empty() || c == "embed" {
                return None;
            }
            p(
                "Twitch",
                format!("https://clips.twitch.tv/{c}"),
                format!("twc:{c}"),
            )
        }
        "instagram.com"
            if matches!(seg(0), "p" | "reel" | "reels" | "tv") && !seg(1).is_empty() =>
        {
            let t = if seg(0) == "reels" { "reel" } else { seg(0) };
            p(
                "Instagram",
                format!("https://www.instagram.com/{t}/{}/", seg(1)),
                format!("ig:{}", seg(1)),
            )
        }
        "tiktok.com" => {
            let id = match (seg(0), seg(1)) {
                ("embed", "v2") | ("player", "v1") => seg(2),
                (u, "video") if u.starts_with('@') => seg(2),
                ("embed", x) => x,
                _ => "",
            };
            if id.len() < 15 || !len_cislice(id) {
                return None;
            }
            p(
                "TikTok",
                format!("https://www.tiktok.com/embed/v2/{id}"),
                format!("tt:{id}"),
            )
        }
        "facebook.com" if seg(0) == "plugins" && seg(1) == "video.php" => {
            let href = parameter(&u, "href")?;
            p("Facebook", href.clone(), format!("fb:{href}"))
        }
        "w.soundcloud.com" if seg(0) == "player" => {
            let url = parameter(&u, "url")?;
            p("SoundCloud", url.clone(), format!("sc:{url}"))
        }
        _ => None,
    }
}

/// Druh a poskytovateľ mediálneho súboru podľa prípony v ceste.
pub fn medium(adresa: &str) -> Option<(Druh, &'static str)> {
    let u = Url::parse(adresa).ok()?;
    let cesta = u.path().to_ascii_lowercase();
    let pripona = cesta
        .rsplit_once('.')
        .map(|(_, p)| p)
        .filter(|p| !p.contains('/'))?;
    Some(match pripona {
        "m3u8" => (Druh::Stream, "HLS"),
        "mpd" => (Druh::Stream, "DASH"),
        "ism" => (Druh::Stream, "Smooth"),
        "mp4" | "m4v" => (Druh::Video, "MP4"),
        "webm" => (Druh::Video, "WebM"),
        "mov" => (Druh::Video, "MOV"),
        "mkv" => (Druh::Video, "MKV"),
        "avi" => (Druh::Video, "AVI"),
        "flv" => (Druh::Video, "FLV"),
        "ts" => (Druh::Video, "TS"),
        "mp3" => (Druh::Zvuk, "MP3"),
        "m4a" => (Druh::Zvuk, "M4A"),
        "aac" => (Druh::Zvuk, "AAC"),
        "ogg" | "oga" => (Druh::Zvuk, "OGG"),
        "opus" => (Druh::Zvuk, "Opus"),
        "flac" => (Druh::Zvuk, "FLAC"),
        "wav" => (Druh::Zvuk, "WAV"),
        "zip" => (Druh::Subor, "ZIP"),
        "rar" => (Druh::Subor, "RAR"),
        "7z" => (Druh::Subor, "7z"),
        "pdf" => (Druh::Subor, "PDF"),
        "iso" => (Druh::Subor, "ISO"),
        "srt" => (Druh::Titulky, "SRT"),
        "vtt" => (Druh::Titulky, "VTT"),
        _ => return None,
    })
}

/// Reklamy, meranie návštevnosti a predvideá.
pub fn je_reklama(adresa: &str) -> bool {
    let a = adresa.to_ascii_lowercase();
    const SLOVA: [&str; 16] = [
        "doubleclick",
        "googlesyndication",
        "googleads",
        "adservice",
        "adnxs",
        "imasdk",
        "taboola",
        "outbrain",
        "teads",
        "spotx",
        "springserve",
        "preroll",
        "vpaid",
        "/pixel",
        "analytics",
        "/vast",
    ];
    if SLOVA.iter().any(|s| a.contains(s)) {
        return true;
    }
    // časti cesty „ad“, „ads“, „adv“ (nie „download“, „upload“, „head“)
    if let Ok(u) = Url::parse(&a) {
        if segmenty(&u).iter().any(|s| {
            matches!(
                *s,
                "ad" | "ads" | "adv" | "advert" | "adverts" | "advertising"
            )
        }) {
            return true;
        }
        if u.host_str()
            .is_some_and(|h| h.starts_with("ads.") || h.starts_with("ad."))
        {
            return true;
        }
    }
    false
}

const KONIEC_ADRESY: &[u8] = b" \t\r\n\"'<>\\(){}[]`";

/// Všetky adresy http(s) a `//…` v texte (HTML, JavaScript, JSON).
pub fn adresy_v_texte(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i + 2 < b.len() {
        // celé adresy aj adresy bez protokolu („//cdn…“) hneď za úvodzovkou, „=“ alebo zátvorkou
        let bez_protokolu = b[i] == b'/'
            && b[i + 1] == b'/'
            && i > 0
            && matches!(b[i - 1], b'"' | b'\'' | b'=' | b'(' | b' ');
        let zac =
            (b[i..].starts_with(b"https://") || b[i..].starts_with(b"http://") || bez_protokolu)
                .then_some(i);
        let Some(z) = zac else {
            i += 1;
            continue;
        };
        let mut k = z;
        while k < b.len() && !KONIEC_ADRESY.contains(&b[k]) {
            k += 1;
        }
        let mut a = &text[z..k];
        // interpunkcia za adresou nepatrí k nej
        a = a.trim_end_matches([',', ';', '.', ':', '!', '?']);
        if a.len() > 10 {
            out.push(if a.starts_with("//") {
                format!("https:{a}")
            } else {
                a.to_string()
            });
        }
        i = k.max(i + 1);
    }
    out
}

#[cfg(test)]
mod testy {
    use super::*;

    fn pr(a: &str) -> Option<(String, String)> {
        prehravac(a).map(|p| (p.poskytovatel.to_string(), p.url))
    }

    #[test]
    fn vimeo_vsetky_tvary() {
        let o = |u: &str| Some(("Vimeo".to_string(), u.to_string()));
        assert_eq!(
            pr("https://player.vimeo.com/video/76979871?h=8272103f6e&badge=0"),
            o("https://player.vimeo.com/video/76979871?h=8272103f6e")
        );
        assert_eq!(
            pr("https://vimeo.com/76979871"),
            o("https://player.vimeo.com/video/76979871")
        );
        assert_eq!(
            pr("https://vimeo.com/76979871/8272103f6e"),
            o("https://player.vimeo.com/video/76979871?h=8272103f6e")
        );
        assert_eq!(
            pr("https://vimeo.com/channels/staffpicks/76979871"),
            o("https://player.vimeo.com/video/76979871")
        );
        assert_eq!(
            pr("https://vimeo.com/groups/kurz/videos/76979871"),
            o("https://player.vimeo.com/video/76979871")
        );
        assert_eq!(
            pr("https://vimeo.com/album/123/video/76979871"),
            o("https://player.vimeo.com/video/76979871")
        );
        assert_eq!(
            pr("https://vimeo.com/showcase/9870"),
            Some(("Vimeo".into(), "https://vimeo.com/showcase/9870".into()))
        );
        assert_eq!(pr("https://vimeo.com/user12345678"), None);
        assert_eq!(pr("https://vimeo.com/blog/post/123456"), None);
    }

    #[test]
    fn ini_prehravaci() {
        let y = Some((
            "YouTube".to_string(),
            "https://www.youtube.com/watch?v=uL9-e9OlLfw".to_string(),
        ));
        for a in [
            "https://www.youtube.com/embed/uL9-e9OlLfw?autoplay=1",
            "https://www.youtube-nocookie.com/embed/uL9-e9OlLfw",
            "https://youtu.be/uL9-e9OlLfw?t=10",
            "https://m.youtube.com/watch?feature=share&v=uL9-e9OlLfw",
            "https://www.youtube.com/shorts/uL9-e9OlLfw",
        ] {
            assert_eq!(pr(a), y, "{a}");
        }
        assert_eq!(
            pr("https://www.youtube.com/watch?v=kratke").map(|x| x.0),
            None
        );
        assert_eq!(
            pr("https://fast.wistia.net/embed/iframe/abcd1234ef")
                .unwrap()
                .1,
            "https://fast.wistia.net/embed/iframe/abcd1234ef"
        );
        assert_eq!(
            pr("https://fast.wistia.com/embed/medias/abcd1234ef.jsonp")
                .unwrap()
                .0,
            "Wistia"
        );
        assert_eq!(
            pr("https://iframe.mediadelivery.net/embed/12345/0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0?autoplay=false").unwrap().0,
            "Bunny Stream"
        );
        assert_eq!(
            pr("https://cdn.jwplayer.com/players/AbCd1234-XyZ98765.html")
                .unwrap()
                .1,
            "https://cdn.jwplayer.com/v2/media/AbCd1234"
        );
        assert_eq!(
            pr("https://cdn.jwplayer.com/manifests/AbCd1234.m3u8")
                .unwrap()
                .0,
            "JW Player"
        );
        assert_eq!(
            pr("https://customer-abc.cloudflarestream.com/0123456789abcdef0123456789abcdef/iframe")
                .unwrap()
                .1,
            "https://iframe.videodelivery.net/0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            pr("https://www.dailymotion.com/embed/video/x8abc12")
                .unwrap()
                .1,
            "https://www.dailymotion.com/video/x8abc12"
        );
        assert_eq!(
            pr("https://geo.dailymotion.com/player/xyz.html?video=x8abc12")
                .unwrap()
                .1,
            "https://www.dailymotion.com/video/x8abc12"
        );
        assert_eq!(
            pr("https://www.instagram.com/reel/C1abcDEF/").unwrap().1,
            "https://www.instagram.com/reel/C1abcDEF/"
        );
        assert_eq!(
            pr("https://www.tiktok.com/@clovek/video/7212345678901234567")
                .unwrap()
                .1,
            "https://www.tiktok.com/embed/v2/7212345678901234567"
        );
        assert_eq!(
            pr("https://www.facebook.com/plugins/video.php?href=https%3A%2F%2Fwww.facebook.com%2Fx%2Fvideos%2F123%2F&show_text=0").unwrap().1,
            "https://www.facebook.com/x/videos/123/"
        );
        assert_eq!(
            pr("https://streamable.com/e/abc12").unwrap().1,
            "https://streamable.com/abc12"
        );
        assert_eq!(
            pr("https://player.twitch.tv/?video=v123456&parent=x.sk")
                .unwrap()
                .1,
            "https://www.twitch.tv/videos/123456"
        );
        assert_eq!(pr("https://example.com/video/123"), None);
    }

    #[test]
    fn media_a_reklamy() {
        assert_eq!(
            medium("https://cdn.sk/hls/master.m3u8?token=1"),
            Some((Druh::Stream, "HLS"))
        );
        assert_eq!(
            medium("https://cdn.sk/a/film.MP4"),
            Some((Druh::Video, "MP4"))
        );
        assert_eq!(medium("https://cdn.sk/dir.mp4/index"), None);
        assert_eq!(medium("https://cdn.sk/stranka.html"), None);
        assert!(je_reklama("https://pubads.g.doubleclick.net/x.mp4"));
        assert!(je_reklama("https://cdn.sk/ads/preroll.mp4"));
        assert!(je_reklama("https://ads.example.com/v.m3u8"));
        assert!(!je_reklama("https://cdn.sk/download/headphones.mp4"));
        assert!(!je_reklama("https://cdn.sk/uploads/2026/video.mp4"));
    }

    #[test]
    fn adresy_z_textu() {
        let t = r#"var cfg = {"hls":"https://cdn.sk/v/master.m3u8?t=1","mp4":'//static.sk/a.mp4'}; see http://x.sk/stranka, ok"#;
        assert_eq!(
            adresy_v_texte(t),
            vec![
                "https://cdn.sk/v/master.m3u8?t=1",
                "https://static.sk/a.mp4",
                "http://x.sk/stranka"
            ]
        );
    }
}
