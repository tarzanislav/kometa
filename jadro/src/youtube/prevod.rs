//! Odpoveď rozhrania prehrávača YouTube → údaje v tvare, ktorý pozná zvyšok Komety (ako z yt-dlp).

use super::Klient;
use crate::chyba::{Chyba, Vysledok};
use serde_json::{json, Map, Value};

fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|t| !t.is_empty())
}

/// Číslo, ktoré YouTube posiela raz ako číslo a raz ako text („contentLength“: „123“).
fn cislo(v: &Value, k: &str) -> Option<f64> {
    match v.get(k)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Text z objektu `{simpleText}` alebo `{runs:[{text}]}`.
fn beh_textu(v: &Value) -> String {
    if let Some(t) = text(v, "simpleText") {
        return t.to_string();
    }
    v.get("runs")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(|x| text(x, "text")).collect())
        .unwrap_or_default()
}

/// Prevod celej odpovede. Chyba = video sa týmto klientom nedá stiahnuť (prihlásenie, vek,
/// živé vysielanie, adresy len s podpisom) – vtedy príde na rad ďalší klient alebo yt-dlp.
pub fn na_info(o: &Value, id: &str, kl: &Klient) -> Vysledok<Value> {
    let hratelnost = &o["playabilityStatus"];
    let stav = text(hratelnost, "status").unwrap_or("bez stavu");
    if stav != "OK" {
        let dovod = text(hratelnost, "reason").unwrap_or(stav);
        return Err(Chyba::Ine(format!("{stav}: {dovod}")));
    }
    let d = &o["videoDetails"];
    if text(d, "videoId") != Some(id) {
        return Err(Chyba::Format("odpoveď patrí inému videu".into()));
    }
    if d["isLive"] == true || d["isUpcoming"] == true {
        return Err(Chyba::Zive);
    }
    let data = &o["streamingData"];
    let mut formaty: Vec<Value> = vec![];
    let mut s_podpisom = 0;
    for f in ["formats", "adaptiveFormats"]
        .iter()
        .filter_map(|k| data[k].as_array())
        .flatten()
    {
        match format(f, kl) {
            Some(x) => formaty.push(x),
            None if f.get("signatureCipher").is_some() || f.get("cipher").is_some() => {
                s_podpisom += 1
            }
            None => {}
        }
    }
    if formaty.is_empty() {
        return Err(Chyba::Format(if s_podpisom > 0 {
            "adresy sú len s podpisom (treba prehrávač s JavaScriptom)".into()
        } else {
            "odpoveď bez priamych adries".into()
        }));
    }
    let trvanie = cislo(d, "lengthSeconds");
    let popis = text(d, "shortDescription").unwrap_or("");
    let mikro = &o["microformat"]["playerMicroformatRenderer"];
    let datum = text(mikro, "publishDate")
        .or_else(|| text(mikro, "uploadDate"))
        .map(|t| {
            t.chars()
                .filter(char::is_ascii_digit)
                .take(8)
                .collect::<String>()
        })
        .filter(|t| t.len() == 8);
    let (titulky, automaticke) = titulky(o);
    let obrazky = obrazky(id, d);
    let kapitoly: Vec<Value> = {
        let k = super::kapitoly_z_popisu(popis, trvanie);
        let konce: Vec<f64> = k
            .iter()
            .skip(1)
            .map(|x| x.0)
            .chain([trvanie.unwrap_or(0.0)])
            .collect();
        k.iter()
            .zip(konce)
            .map(|((od, nazov), do_)| json!({"start_time": od, "end_time": do_, "title": nazov}))
            .collect()
    };
    let mut info = Map::new();
    let mut daj = |k: &str, v: Value| {
        if !v.is_null() {
            info.insert(k.into(), v);
        }
    };
    daj("id", json!(id));
    daj("title", json!(text(d, "title").unwrap_or(id)));
    daj("uploader", json!(text(d, "author")));
    daj("channel", json!(text(d, "author")));
    daj("channel_id", json!(text(d, "channelId")));
    daj("description", json!(popis));
    daj("duration", json!(trvanie));
    daj("view_count", json!(cislo(d, "viewCount")));
    daj("upload_date", json!(datum));
    daj(
        "thumbnail",
        obrazky
            .first()
            .map(|o| o["url"].clone())
            .unwrap_or(Value::Null),
    );
    daj("thumbnails", json!(obrazky));
    daj(
        "webpage_url",
        json!(format!("https://www.youtube.com/watch?v={id}")),
    );
    daj("extractor", json!("youtube"));
    daj("extractor_key", json!("Youtube"));
    daj("formats", json!(formaty));
    daj("subtitles", Value::Object(titulky));
    daj("automatic_captions", Value::Object(automaticke));
    daj("chapters", json!(kapitoly));
    daj("_kometa", json!({"zdroj": "vlastne", "klient": kl.meno}));
    Ok(Value::Object(info))
}

/// Jeden formát. `None` = bez priamej adresy (podpis), segmentovaný (OTF) alebo neznámy typ.
fn format(f: &Value, kl: &Klient) -> Option<Value> {
    let url = text(f, "url").filter(|u| u.starts_with("http"))?;
    if text(f, "type") == Some("FORMAT_STREAM_TYPE_OTF") {
        return None;
    }
    let itag = f.get("itag")?.as_i64()?;
    // „video/mp4; codecs="avc1.640028, mp4a.40.2"“
    let (typ, kodeky) = text(f, "mimeType")?.split_once(';')?;
    let kodeky: Vec<&str> = kodeky
        .trim()
        .strip_prefix("codecs=")?
        .trim_matches('"')
        .split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    let (druh, kontajner) = typ.trim().split_once('/')?;
    let (vcodec, acodec) = match (druh, kodeky.as_slice()) {
        ("video", [v]) => (*v, "none"),
        ("video", [v, a]) => (*v, *a),
        ("audio", [a]) => ("none", *a),
        _ => return None,
    };
    let ext = match (druh, kontajner) {
        ("audio", "mp4") => "m4a",
        (_, "mp4") => "mp4",
        (_, "webm") => "webm",
        (_, "3gpp") => "3gp",
        _ => return None,
    };
    let tbr = cislo(f, "averageBitrate")
        .or_else(|| cislo(f, "bitrate"))
        .map(|b| b / 1000.0);
    let stopa = f.get("audioTrack");
    let id_stopy = stopa.and_then(|s| text(s, "id"));
    let nazov_stopy = stopa.and_then(|s| text(s, "displayName"));
    let drc = f["isDrc"] == true;
    let mut format_id = itag.to_string();
    if let Some(s) = id_stopy {
        format_id.push('-');
        format_id.extend(s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-'));
    }
    if drc {
        format_id.push_str("-drc");
    }
    let mut poznamka = if acodec != "none" && vcodec == "none" {
        nazov_stopy
            .map(str::to_string)
            .or_else(|| {
                text(f, "audioQuality")
                    .map(|q| q.trim_start_matches("AUDIO_QUALITY_").to_ascii_lowercase())
            })
            .unwrap_or_default()
    } else {
        text(f, "qualityLabel").unwrap_or("").to_string()
    };
    if drc {
        poznamka.push_str(" DRC");
    }
    let hdr = matches!(
        text(&f["colorInfo"], "transferCharacteristics"),
        Some(
            "COLOR_TRANSFER_CHARACTERISTICS_SMPTEST2084"
                | "COLOR_TRANSFER_CHARACTERISTICS_ARIB_STD_B67"
        )
    );
    let mut m = Map::new();
    let mut daj = |k: &str, v: Value| {
        if !v.is_null() {
            m.insert(k.into(), v);
        }
    };
    daj("format_id", json!(format_id));
    daj("url", json!(url));
    daj("ext", json!(ext));
    daj(
        "protocol",
        json!(if url.starts_with("https") {
            "https"
        } else {
            "http"
        }),
    );
    daj("vcodec", json!(vcodec));
    daj("acodec", json!(acodec));
    daj("tbr", json!(tbr));
    if vcodec == "none" {
        daj("abr", json!(tbr));
    } else {
        daj("width", json!(cislo(f, "width")));
        daj("height", json!(cislo(f, "height")));
        daj("fps", json!(cislo(f, "fps")));
        daj("dynamic_range", json!(if hdr { "HDR" } else { "SDR" }));
    }
    if acodec != "none" {
        daj("asr", json!(cislo(f, "audioSampleRate")));
        daj("audio_channels", json!(cislo(f, "audioChannels")));
    }
    daj("filesize", json!(cislo(f, "contentLength")));
    daj("format_note", json!(poznamka.trim()));
    if let Some(s) = id_stopy {
        let jazyk = s.split('.').next().unwrap_or("");
        if !jazyk.is_empty() {
            daj("language", json!(jazyk));
        }
        let povodny = nazov_stopy.is_some_and(|n| n.to_lowercase().contains("original"));
        let predvoleny = stopa.is_some_and(|s| s["audioIsDefault"] == true);
        daj(
            "language_preference",
            json!(if povodny {
                10
            } else if predvoleny {
                5
            } else {
                -1
            }),
        );
    }
    daj("http_headers", json!({"User-Agent": kl.user_agent}));
    Some(Value::Object(m))
}

/// Obrázky videa: zo zoznamu odpovede plus štandardné veľkosti (najväčší nemusí existovať –
/// sťahovanie obalu skúša ďalší).
fn obrazky(id: &str, d: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = [
        ("maxresdefault", 1280, 720),
        ("sddefault", 640, 480),
        ("hqdefault", 480, 360),
    ]
    .iter()
    .map(|(meno, w, h)| {
        json!({"url": format!("https://i.ytimg.com/vi/{id}/{meno}.jpg"), "width": w, "height": h})
    })
    .collect();
    for t in d["thumbnail"]["thumbnails"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(u) = text(t, "url") {
            if !out.iter().any(|o| o["url"] == u) {
                out.push(
                    json!({"url": u, "width": cislo(t, "width"), "height": cislo(t, "height")}),
                );
            }
        }
    }
    out
}

/// Adresa titulkov s formátom (a prekladom).
fn adresa_titulkov(zaklad: &str, fmt: &str, preklad: Option<&str>) -> Option<String> {
    let mut u = url::Url::parse(zaklad).ok()?;
    let ostatne: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| k != "fmt" && k != "tlang")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    {
        let mut q = u.query_pairs_mut();
        q.clear().extend_pairs(ostatne).append_pair("fmt", fmt);
        if let Some(p) = preklad {
            q.append_pair("tlang", p);
        }
    }
    Some(u.into())
}

fn polozky_titulkov(zaklad: &str, nazov: &str, preklad: Option<&str>) -> Value {
    json!(["json3", "vtt"]
        .iter()
        .filter_map(|f| {
            Some(json!({"ext": f, "url": adresa_titulkov(zaklad, f, preklad)?, "name": nazov}))
        })
        .collect::<Vec<_>>())
}

/// (titulky od autora, automatické): automatické titulky pôvodnej reči sú pod „xx-orig“,
/// automatické preklady pod kódom jazyka – ako ich pozná zvyšok Komety.
fn titulky(o: &Value) -> (Map<String, Value>, Map<String, Value>) {
    let r = &o["captions"]["playerCaptionsTracklistRenderer"];
    let mut od_autora = Map::new();
    let mut automaticke = Map::new();
    let stopy: Vec<&Value> = r["captionTracks"]
        .as_array()
        .map(|a| a.iter().filter(|t| text(t, "baseUrl").is_some()).collect())
        .unwrap_or_default();
    for t in &stopy {
        let (Some(zaklad), Some(kod)) = (text(t, "baseUrl"), text(t, "languageCode")) else {
            continue;
        };
        let nazov = beh_textu(&t["name"]);
        if text(t, "kind") == Some("asr") {
            automaticke.insert(
                format!("{kod}-orig"),
                polozky_titulkov(zaklad, &nazov, None),
            );
        } else {
            let a = od_autora
                .entry(kod.to_string())
                .or_insert_with(|| json!([]));
            if let (Some(a), Value::Array(p)) =
                (a.as_array_mut(), polozky_titulkov(zaklad, &nazov, None))
            {
                a.extend(p);
            }
        }
    }
    // preklady: zo stopy rozpoznanej reči (pôvodný jazyk), inak z prvej preložiteľnej
    let zdroj = stopy
        .iter()
        .find(|t| text(t, "kind") == Some("asr"))
        .or_else(|| stopy.iter().find(|t| t["isTranslatable"] == true));
    if let Some(zaklad) = zdroj.and_then(|t| text(t, "baseUrl")) {
        for p in r["translationLanguages"].as_array().into_iter().flatten() {
            if let Some(kod) = text(p, "languageCode") {
                automaticke.entry(kod.to_string()).or_insert_with(|| {
                    polozky_titulkov(zaklad, &beh_textu(&p["languageName"]), Some(kod))
                });
            }
        }
    }
    (od_autora, automaticke)
}

#[cfg(test)]
mod skusky {
    use super::*;

    fn klient() -> Klient {
        Klient {
            meno: "SKUSKA".into(),
            cislo: 1,
            verzia: "1.0".into(),
            user_agent: "kometa-skuska/1.0".into(),
            klient: Map::new(),
        }
    }

    /// Vymyslená odpoveď v tvare rozhrania prehrávača (len polia, ktoré Kometa číta).
    fn odpoved() -> Value {
        json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {
                "videoId": "AbCdEfGhIjK", "title": "Kometa letí", "author": "Hvezdáreň",
                "channelId": "UC123", "lengthSeconds": "185", "viewCount": "4200",
                "shortDescription": "O kométach.\n0:00 Úvod\n0:40 Chvost\n2:00 Záver",
                "thumbnail": {"thumbnails": [{"url": "https://i.ytimg.com/vi/AbCdEfGhIjK/default.jpg", "width": 120, "height": 90}]}
            },
            "microformat": {"playerMicroformatRenderer": {"publishDate": "2026-03-14T08:00:00-07:00"}},
            "streamingData": {
                "formats": [
                    {"itag": 18, "url": "https://cdn.example/18", "mimeType": "video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"",
                     "bitrate": 500000, "width": 640, "height": 360, "qualityLabel": "360p", "fps": 25}
                ],
                "adaptiveFormats": [
                    {"itag": 137, "url": "https://cdn.example/137", "mimeType": "video/mp4; codecs=\"avc1.640028\"",
                     "bitrate": 4000000, "averageBitrate": 3500000, "width": 1920, "height": 1080, "fps": 25,
                     "qualityLabel": "1080p", "contentLength": "81000000"},
                    {"itag": 248, "signatureCipher": "s=xyz&url=https%3A%2F%2Fcdn.example%2F248", "mimeType": "video/webm; codecs=\"vp9\"",
                     "bitrate": 3000000, "width": 1920, "height": 1080},
                    {"itag": 337, "url": "https://cdn.example/337", "mimeType": "video/webm; codecs=\"vp09.02.51.10.01.09.16.09.00\"",
                     "bitrate": 9000000, "width": 3840, "height": 2160, "fps": 50, "qualityLabel": "2160p50 HDR",
                     "colorInfo": {"transferCharacteristics": "COLOR_TRANSFER_CHARACTERISTICS_SMPTEST2084"}},
                    {"itag": 299, "url": "https://cdn.example/otf", "type": "FORMAT_STREAM_TYPE_OTF",
                     "mimeType": "video/mp4; codecs=\"avc1.64002a\"", "bitrate": 6000000, "width": 1920, "height": 1080},
                    {"itag": 140, "url": "https://cdn.example/140-sk", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                     "bitrate": 130000, "audioSampleRate": "44100", "audioChannels": 2, "contentLength": "3000000",
                     "audioTrack": {"displayName": "Slovak original", "id": "sk.4", "audioIsDefault": true}},
                    {"itag": 140, "url": "https://cdn.example/140-cs", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                     "bitrate": 129000, "audioTrack": {"displayName": "Czech", "id": "cs.3", "audioIsDefault": false}},
                    {"itag": 251, "url": "https://cdn.example/251-drc", "mimeType": "audio/webm; codecs=\"opus\"",
                     "bitrate": 140000, "isDrc": true, "audioQuality": "AUDIO_QUALITY_MEDIUM"}
                ]
            },
            "captions": {"playerCaptionsTracklistRenderer": {
                "captionTracks": [
                    {"baseUrl": "https://www.youtube.com/api/timedtext?v=AbCdEfGhIjK&lang=sk&fmt=srv3", "name": {"simpleText": "Slovak"},
                     "languageCode": "sk", "isTranslatable": true},
                    {"baseUrl": "https://www.youtube.com/api/timedtext?v=AbCdEfGhIjK&lang=sk&kind=asr", "name": {"runs": [{"text": "Slovak (auto-generated)"}]},
                     "languageCode": "sk", "kind": "asr", "isTranslatable": true}
                ],
                "translationLanguages": [{"languageCode": "en", "languageName": {"simpleText": "English"}},
                                         {"languageCode": "sk", "languageName": {"simpleText": "Slovak"}}]
            }}
        })
    }

    #[test]
    fn prevod_odpovede() {
        let i = na_info(&odpoved(), "AbCdEfGhIjK", &klient()).unwrap();
        assert_eq!(i["title"], "Kometa letí");
        assert_eq!(i["uploader"], "Hvezdáreň");
        assert_eq!(i["duration"], 185.0);
        assert_eq!(i["upload_date"], "20260314");
        assert_eq!(
            i["webpage_url"],
            "https://www.youtube.com/watch?v=AbCdEfGhIjK"
        );
        let f = i["formats"].as_array().unwrap();
        let ids: Vec<&str> = f.iter().map(|x| x["format_id"].as_str().unwrap()).collect();
        // bez podpisu a bez OTF; zvukové stopy rozlíšené jazykom, DRC označené
        assert_eq!(ids, ["18", "137", "337", "140-sk4", "140-cs3", "251-drc"]);
        let muxed = &f[0];
        assert_eq!(
            (muxed["vcodec"].as_str(), muxed["acodec"].as_str()),
            (Some("avc1.42001E"), Some("mp4a.40.2"))
        );
        assert_eq!(muxed["ext"], "mp4");
        let v = &f[1];
        assert_eq!(v["acodec"], "none");
        assert_eq!(v["tbr"], 3500.0);
        assert_eq!(v["filesize"], 81000000.0);
        assert_eq!(v["dynamic_range"], "SDR");
        assert_eq!(f[2]["dynamic_range"], "HDR");
        assert_eq!(f[2]["ext"], "webm");
        let sk = &f[3];
        assert_eq!(
            (sk["ext"].as_str(), sk["vcodec"].as_str()),
            (Some("m4a"), Some("none"))
        );
        assert_eq!(sk["language"], "sk");
        assert_eq!(sk["language_preference"], 10);
        assert_eq!(sk["abr"], 130.0);
        assert_eq!(sk["http_headers"]["User-Agent"], "kometa-skuska/1.0");
        assert_eq!(f[4]["language_preference"], -1);
        assert_eq!(f[5]["format_note"], "medium DRC");
        assert!(f[5].get("language").is_none());
        // titulky: od autora json3+vtt, automatické pôvodnej reči „sk-orig“, preklad do „en“
        let t = &i["subtitles"]["sk"];
        assert_eq!(t[0]["ext"], "json3");
        assert!(t[0]["url"]
            .as_str()
            .unwrap()
            .ends_with("&lang=sk&fmt=json3"));
        let orig = i["automatic_captions"]["sk-orig"][1]["url"]
            .as_str()
            .unwrap();
        assert!(
            orig.contains("kind=asr") && orig.ends_with("fmt=vtt"),
            "{orig}"
        );
        let en = i["automatic_captions"]["en"][0]["url"].as_str().unwrap();
        assert!(
            en.contains("kind=asr") && en.ends_with("fmt=json3&tlang=en"),
            "{en}"
        );
        // kapitoly z popisu, koniec poslednej = dĺžka videa
        let k = i["chapters"].as_array().unwrap();
        assert_eq!(k.len(), 3);
        assert_eq!(
            (k[1]["start_time"].as_f64(), k[1]["end_time"].as_f64()),
            (Some(40.0), Some(120.0))
        );
        assert_eq!(k[2]["end_time"], 185.0);
        // obrázky: štandardné veľkosti + zo zoznamu
        assert!(i["thumbnail"]
            .as_str()
            .unwrap()
            .ends_with("/maxresdefault.jpg"));
        assert_eq!(i["thumbnails"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn chyby_odpovede() {
        let mut o = odpoved();
        o["playabilityStatus"] =
            json!({"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm your age"});
        let e = na_info(&o, "AbCdEfGhIjK", &klient())
            .unwrap_err()
            .to_string();
        assert!(e.contains("LOGIN_REQUIRED") && e.contains("age"), "{e}");
        assert!(na_info(&odpoved(), "InyIdentifk", &klient()).is_err());
        let mut o = odpoved();
        o["videoDetails"]["isLive"] = json!(true);
        assert!(matches!(
            na_info(&o, "AbCdEfGhIjK", &klient()),
            Err(Chyba::Zive)
        ));
        // len adresy s podpisom → treba JavaScript
        let mut o = odpoved();
        o["streamingData"] = json!({"adaptiveFormats": [{"itag": 251, "signatureCipher": "s=1", "mimeType": "audio/webm; codecs=\"opus\""}]});
        let e = na_info(&o, "AbCdEfGhIjK", &klient())
            .unwrap_err()
            .to_string();
        assert!(e.contains("podpisom"), "{e}");
    }
}
