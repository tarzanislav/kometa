//! Most k yt-dlp: služby, ktoré jadro samo nepozná (YouTube, Instagram, TikTok, Facebook a stovky
//! ďalších). yt-dlp beží ako samostatný program, jeho priebeh sa číta ako JSON po riadkoch.
//!
//! Dva spôsoby:
//! * **s FFmpeg** – yt-dlp stiahne a spojí všetko sám (AAC aj Opus);
//! * **bez FFmpeg** – yt-dlp len zistí formáty, vyberie ich jadro (obraz MP4 + zvuk M4A),
//!   stiahne ich vlastným sťahovaním po úsekoch a spojí vlastným spájaním. Pozri [`vyber_formaty`].

use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Hlasic, Udalost};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub fn prikaz(exe: &Path) -> Command {
    let mut c = Command::new(exe);
    c.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    c
}

/// Spoločné voľby: prihlásenie z prehliadača, Referer, heslo videa.
#[derive(Clone, Default)]
pub struct Spolocne {
    pub prehliadac: String,
    pub referer: String,
    pub heslo: String,
}

/// Deno pre yt-dlp (JavaScript na YouTube) – nastaví údržba pomocníkov, použije každé spustenie.
static DENO: std::sync::RwLock<Option<PathBuf>> = std::sync::RwLock::new(None);

/// Nastaví cestu k Deno, ktorú dostane yt-dlp (`--js-runtimes deno:…`).
pub fn nastav_deno(p: Option<PathBuf>) {
    *DENO.write().unwrap() = p;
}

impl Spolocne {
    /// Rovnaké voľby bez prihlásenia z prehliadača.
    fn bez_cookies(&self) -> Spolocne {
        Spolocne {
            prehliadac: String::new(),
            ..self.clone()
        }
    }

    fn argumenty(&self) -> Vec<String> {
        let mut a = vec![
            "--no-colors".to_string(),
            "--encoding".into(),
            "utf-8".into(),
        ];
        if let Some(d) = DENO.read().unwrap().as_ref() {
            a.extend(["--js-runtimes".into(), format!("deno:{}", d.display())]);
        }
        if !self.prehliadac.is_empty() {
            a.extend(["--cookies-from-browser".into(), self.prehliadac.clone()]);
        }
        if !self.referer.is_empty() {
            a.extend(["--referer".into(), self.referer.clone()]);
            if let Ok(u) = url::Url::parse(&self.referer) {
                a.extend([
                    "--add-headers".into(),
                    format!("Origin:{}", u.origin().ascii_serialization()),
                ]);
            }
        }
        if !self.heslo.is_empty() {
            a.extend(["--video-password".into(), self.heslo.clone()]);
        }
        a
    }
}

/// Technickú chybu yt-dlp preloží do zrozumiteľnej vety: (veta, podrobnosti).
pub fn zrozumitelne(sprava: &str) -> (String, String) {
    let m = sprava.to_lowercase();
    let ma = |vzory: &[&str]| vzory.iter().any(|v| m.contains(v));
    let veta = if ma(&["http error 404", "404: not found"]) {
        "Stránka alebo súbor neexistuje (chyba 404)."
    } else if ma(&["private video", "video is private"]) {
        "Video je súkromné."
    } else if ma(&[
        "not a bot",
        "login required",
        "sign in",
        "log in",
        "logged in",
        "rate-limit",
        "rate limit",
        "cookies",
    ]) {
        "Obsah vyžaduje prihlásenie – v Nastaveniach vyber prehliadač, v ktorom si prihlásený."
    } else if ma(&["http error 403", "forbidden"]) {
        "Prístup zamietnutý (403) – obsah môže vyžadovať prihlásenie."
    } else if ma(&[
        "video unavailable",
        "has been removed",
        "no longer available",
        "does not exist",
    ]) {
        "Video už nie je dostupné."
    } else if ma(&[
        "not available in your country",
        "geo restrict",
        "geo-restrict",
    ]) {
        "Video nie je dostupné v tvojej krajine."
    } else if ma(&[
        "requested format is not available",
        "no video formats found",
    ]) {
        "Nenašiel sa vhodný formát – skús iný formát alebo aktualizuj yt-dlp."
    } else if ma(&["ffmpeg", "ffprobe"]) {
        "Tento formát potrebuje FFmpeg – vyber iný formát alebo nainštaluj FFmpeg."
    } else if ma(&[
        "getaddrinfo",
        "failed to resolve",
        "name or service not known",
        "timed out",
        "unable to download webpage",
    ]) {
        "Stránka nie je dostupná – skontroluj pripojenie na internet a odkaz."
    } else if ma(&["unsupported url"]) {
        "Na tejto stránke sa nenašlo video ani súbor na stiahnutie."
    } else {
        return (
            sprava
                .trim_start_matches("ERROR: ")
                .chars()
                .take(400)
                .collect(),
            String::new(),
        );
    };
    (veta.to_string(), sprava.chars().take(400).collect())
}

fn chyba_z(stderr: &str) -> Chyba {
    let posledna = stderr
        .lines()
        .rev()
        .find(|r| r.starts_with("ERROR:"))
        .or_else(|| stderr.lines().last())
        .unwrap_or("yt-dlp skončil s chybou");
    let (veta, podrobnosti) = zrozumitelne(posledna);
    Chyba::Ine(if podrobnosti.is_empty() {
        veta
    } else {
        format!("{veta}\n\nPodrobnosti: {podrobnosti}")
    })
}

/// Chyba, pri ktorej stránka žiada prihlásenie – až vtedy sa použijú cookies z prehliadača.
///
/// Cookies prehliadača sa nečítajú pri každom sťahovaní: otvorenie a dešifrovanie databázy cookies
/// je presne to, čo robia programy kradnúce prihlásenia, a antivírusy na to reagujú citlivo.
/// Väčšina videí (aj YouTube) prihlásenie nepotrebuje.
pub fn treba_prihlasenie(e: &Chyba) -> bool {
    let t = e.to_string().to_lowercase();
    [
        "sign in",
        "log in",
        "login",
        "logged-in",
        "authentication",
        "cookies",
        "private video",
        "video is private",
        "members-only",
        "members only",
        "join this channel",
        "confirm your age",
        "age-restricted",
        "inappropriate for some users",
        "registered users",
        "prihlás",
        "prihlas",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// Spustí `pokus` najprv bez cookies prehliadača; s nimi len ak stránka žiada prihlásenie.
async fn s_cookies_ak_treba<T, F, Fut>(s: &Spolocne, pokus: F) -> Vysledok<T>
where
    F: Fn(Spolocne) -> Fut,
    Fut: std::future::Future<Output = Vysledok<T>>,
{
    if s.prehliadac.is_empty() {
        return pokus(s.clone()).await;
    }
    match pokus(s.bez_cookies()).await {
        Err(e) if treba_prihlasenie(&e) => pokus(s.clone()).await,
        r => r,
    }
}

/// Spustí yt-dlp a vráti jeho JSON (napr. `-J`).
pub async fn json(exe: &Path, args: &[String], zrusenie: &CancellationToken) -> Vysledok<Value> {
    let mut c = prikaz(exe);
    c.args(args);
    let ch = c
        .spawn()
        .map_err(|e| Chyba::Ine(format!("yt-dlp sa nepodarilo spustiť: {e}")))?;
    let vystup = tokio::select! {
        _ = zrusenie.cancelled() => return Err(Chyba::Pauza),
        v = ch.wait_with_output() => v?,
    };
    if !vystup.status.success() {
        return Err(chyba_z(&String::from_utf8_lossy(&vystup.stderr)));
    }
    serde_json::from_slice(&vystup.stdout).map_err(|e| Chyba::Format(format!("yt-dlp: {e}")))
}

/// Údaje o jednom videe (bez sťahovania).
pub async fn info(
    exe: &Path,
    url: &str,
    s: &Spolocne,
    zrusenie: &CancellationToken,
) -> Vysledok<Value> {
    s_cookies_ak_treba(s, |s| async move {
        let mut a = s.argumenty();
        a.extend([
            "-J".into(),
            "--no-playlist".into(),
            "--".into(),
            url.to_string(),
        ]);
        json(exe, &a, zrusenie).await
    })
    .await
}

/// Zoznam videí playlistu alebo kanála: {title, uploader, url, entries:[{url,title,duration,…}]}.
pub async fn zoznam(exe: &Path, url: &str, s: &Spolocne) -> Vysledok<Value> {
    let v = s_cookies_ak_treba(s, |s| async move {
        let mut a = s.argumenty();
        a.extend([
            "-J".into(),
            "--flat-playlist".into(),
            "--ignore-errors".into(),
            "--".into(),
            url.to_string(),
        ]);
        json(exe, &a, &CancellationToken::new()).await
    })
    .await?;
    let mut polozky = vec![];
    for e in v
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if e.get("_type").and_then(Value::as_str) == Some("playlist") {
            continue;
        }
        let id = e.get("id").and_then(Value::as_str).unwrap_or("");
        let mut u = e
            .get("url")
            .or(e.get("webpage_url"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !u.starts_with("http") {
            let ie = e.get("ie_key").and_then(Value::as_str).unwrap_or("");
            if ie.starts_with("Youtube") && !id.is_empty() {
                u = format!("https://www.youtube.com/watch?v={id}");
            } else {
                continue;
            }
        }
        polozky.push(serde_json::json!({
            "url": u,
            "title": e.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or(if id.is_empty() { &u } else { id }),
            "duration": e.get("duration"),
            "id": id,
            "date": e.get("upload_date").and_then(Value::as_str).unwrap_or(""),
            "views": e.get("view_count"),
        }));
    }
    let mut titul = ["title", "playlist_title", "uploader"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|t| !t.is_empty()))
        .unwrap_or("Playlist")
        .to_string();
    for k in [" - Videos", " - Videá", " - Shorts", " - Live"] {
        if let Some(t) = titul.strip_suffix(k) {
            titul = t.to_string();
        }
    }
    Ok(serde_json::json!({
        "title": titul,
        "uploader": v.get("uploader").or(v.get("channel")).and_then(Value::as_str).unwrap_or(""),
        "url": url,
        "entries": polozky,
    }))
}

/// Formát pre yt-dlp podľa voľby v okne.
pub fn format_ytdlp(format: &str, zvuk: &str, tiktok: bool) -> (String, Vec<String>) {
    let (mut f, mut extra): (String, Vec<&str>) = match format {
        "mp4" => (
            "bv*+ba/b".into(),
            vec![
                "--merge-output-format",
                "mp4",
                "-S",
                "vcodec:h264,res,acodec:m4a",
            ],
        ),
        "1080" => (
            "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b".into(),
            vec!["--merge-output-format", "mp4/mkv"],
        ),
        // Full HD presne: 1080p, inak najbližšie nižšie, inak čokoľvek
        "fhd" => (
            "bv*[height=1080]+ba/b[height=1080]/bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b"
                .into(),
            vec!["--merge-output-format", "mp4/mkv"],
        ),
        "720" => (
            "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b".into(),
            vec!["--merge-output-format", "mp4/mkv"],
        ),
        "mp3" => (
            "ba/b".into(),
            vec!["-x", "--audio-format", "mp3", "--audio-quality", "192K"],
        ),
        "audio" => ("ba[ext=m4a]/ba/b".into(), vec![]),
        _ => ("bv*+ba/b".into(), vec!["--merge-output-format", "mp4/mkv"]),
    };
    if matches!(format, "video" | "fhd" | "1080" | "720") {
        let vsel = f.split('+').next().unwrap_or("bv*").to_string();
        match zvuk {
            "aac" => f = format!("{vsel}+ba[acodec^=mp4a]/{f}"),
            "opus" => {}
            _ => {
                // AAC (prehrá každý prehrávač, prvá = predvolená) + Opus (vyššia kvalita) v jednom MKV
                f = format!("{vsel}+ba[acodec^=mp4a]+ba[acodec^=opus]/{vsel}+ba[acodec^=mp4a]/{f}");
                extra = vec!["--audio-multistreams", "--merge-output-format", "mkv"];
            }
        }
    }
    if tiktok {
        // bez vodoznaku; ak by nič nevyhovelo, pôvodný výber
        let casti: Vec<&str> = f.split('/').filter(|p| !p.is_empty()).collect();
        let bez: Vec<String> = casti
            .iter()
            .map(|p| {
                p.split('+')
                    .map(|t| format!("{t}[format_note!*=?watermarked]"))
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect();
        f = bez
            .into_iter()
            .chain(casti.iter().map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join("/");
    }
    (f, extra.into_iter().map(str::to_string).collect())
}

/// Metaúdaje videa, ktoré pošle yt-dlp pred sťahovaním.
#[derive(Default, Debug, Clone)]
pub struct Meta {
    pub nazov: String,
    pub nahlad: String,
    pub dlzka: Option<f64>,
    pub autor: String,
    pub velkost: Option<u64>,
}

pub fn meta_z(v: &Value) -> Meta {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Meta {
        nazov: s("title"),
        nahlad: s("thumbnail"),
        dlzka: v.get("duration").and_then(Value::as_f64),
        autor: Some(s("uploader"))
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| s("channel")),
        velkost: v
            .get("filesize")
            .or(v.get("filesize_approx"))
            .and_then(Value::as_f64)
            .map(|f| f as u64),
    }
}

#[derive(Clone)]
pub struct Uloha<'a> {
    pub exe: &'a Path,
    pub ffmpeg: Option<&'a Path>,
    pub url: &'a str,
    pub priecinok: &'a Path,
    pub format: &'a str,
    pub zvuk: &'a str,
    pub spojenia: usize,
    /// strop rýchlosti v bajtoch za sekundu (šetrné sťahovanie)
    pub limit: Option<u64>,
    pub spolocne: Spolocne,
    /// prepínače na vloženie údajov, kapitol a obrázka (pozri [`super::doplnky::argumenty_ytdlp`])
    pub doplnky: Vec<String>,
}

/// Stiahne video samotným yt-dlp (s FFmpeg). Vráti cestu k výsledku.
pub async fn stiahni(
    u: &Uloha<'_>,
    zrusenie: &CancellationToken,
    hlasic: &Hlasic,
    pri_meta: &(dyn Fn(Meta) + Send + Sync),
    pri_docasnom: &(dyn Fn(String) + Send + Sync),
) -> Vysledok<PathBuf> {
    s_cookies_ak_treba(&u.spolocne, |s| async move {
        let mut u2 = u.clone();
        u2.spolocne = s;
        stiahni_raz(&u2, zrusenie, hlasic, pri_meta, pri_docasnom).await
    })
    .await
}

async fn stiahni_raz(
    u: &Uloha<'_>,
    zrusenie: &CancellationToken,
    hlasic: &Hlasic,
    pri_meta: &(dyn Fn(Meta) + Send + Sync),
    pri_docasnom: &(dyn Fn(String) + Send + Sync),
) -> Vysledok<PathBuf> {
    let tiktok = super::zdroje::nazov_zdroja(u.url) == "TikTok";
    let (format, extra) = format_ytdlp(u.format, u.zvuk, tiktok);
    let mut a = u.spolocne.argumenty();
    a.extend([
        "--newline".into(),
        "--progress".into(),
        "--no-playlist".into(),
        "--progress-template".into(),
        "download:KOMETA_P %(progress)j".into(),
        "--print".into(),
        "before_dl:KOMETA_M %(.{title,thumbnail,duration,uploader,channel,id,filesize,filesize_approx})j".into(),
        "--print".into(),
        "after_move:KOMETA_F %(filepath)j".into(),
        "-N".into(),
        u.spojenia.clamp(1, 16).to_string(),
        "-f".into(),
        format,
        "-P".into(),
        u.priecinok.to_string_lossy().into_owned(),
        "-o".into(),
        "%(title).120B [%(id)s].%(ext)s".into(),
    ]);
    a.extend(extra);
    if let Some(l) = u.limit {
        a.extend(["--limit-rate".into(), l.to_string()]);
    }
    if u.ffmpeg.is_some() {
        a.extend(u.doplnky.iter().cloned());
    }
    if let Some(f) = u.ffmpeg {
        a.extend(["--ffmpeg-location".into(), f.to_string_lossy().into_owned()]);
    }
    a.extend(["--".into(), u.url.to_string()]);
    tokio::fs::create_dir_all(u.priecinok).await?;
    let mut ch = prikaz(u.exe)
        .args(&a)
        .spawn()
        .map_err(|e| Chyba::Ine(format!("yt-dlp sa nepodarilo spustiť: {e}")))?;
    let mut riadky = BufReader::new(ch.stdout.take().expect("stdout")).lines();
    let mut chyby = BufReader::new(ch.stderr.take().expect("stderr")).lines();
    let (mut hotove, mut celkom, mut subor) = (0u64, None::<u64>, None::<PathBuf>);
    let mut stderr = String::new();
    let mut koniec_out = false;
    let mut koniec_err = false;
    while !(koniec_out && koniec_err) {
        tokio::select! {
            _ = zrusenie.cancelled() => {
                let _ = ch.kill().await;
                return Err(Chyba::Pauza);
            }
            r = riadky.next_line(), if !koniec_out => match r? {
                None => koniec_out = true,
                Some(r) => {
                    if let Some(j) = r.strip_prefix("KOMETA_P ") {
                        let Ok(p) = serde_json::from_str::<Value>(j) else { continue };
                        for k in ["tmpfilename", "filename"] {
                            if let Some(t) = p.get(k).and_then(Value::as_str) {
                                pri_docasnom(t.to_string());
                            }
                        }
                        let cast = p.get("downloaded_bytes").and_then(Value::as_f64).unwrap_or(0.0) as u64;
                        let tento = p.get("total_bytes").or(p.get("total_bytes_estimate")).and_then(Value::as_f64).map(|f| f as u64);
                        if p.get("status").and_then(Value::as_str) == Some("finished") {
                            hotove += tento.unwrap_or(cast);
                            continue;
                        }
                        let spolu = celkom.or(tento.map(|t| hotove + t));
                        hlasic(Udalost::Priebeh {
                            stiahnute: hotove + cast,
                            celkom: spolu.map(|s| s.max(hotove + cast)),
                            rychlost: p.get("speed").and_then(Value::as_f64).unwrap_or(0.0),
                            cast: None,
                        });
                    } else if let Some(j) = r.strip_prefix("KOMETA_M ") {
                        if let Ok(v) = serde_json::from_str::<Value>(j) {
                            let m = meta_z(&v);
                            celkom = m.velkost;
                            pri_meta(m);
                        }
                    } else if let Some(j) = r.strip_prefix("KOMETA_F ") {
                        subor = serde_json::from_str::<String>(j).ok().map(PathBuf::from);
                    }
                }
            },
            r = chyby.next_line(), if !koniec_err => match r? {
                None => koniec_err = true,
                Some(r) => {
                    if r.starts_with("WARNING:") {
                        udalosti::varovanie(hlasic, r.trim_start_matches("WARNING: ").to_string());
                    }
                    stderr.push_str(&r);
                    stderr.push('\n');
                }
            },
        }
    }
    let stav = ch.wait().await?;
    if !stav.success() {
        if zrusenie.is_cancelled() {
            return Err(Chyba::Pauza);
        }
        return Err(chyba_z(&stderr));
    }
    subor
        .filter(|s| s.exists())
        .ok_or_else(|| Chyba::Ine("yt-dlp neoznámil výsledný súbor".into()))
}

/// Formát z údajov yt-dlp, ktorý jadro stiahne samo.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatNa {
    pub id: String,
    pub url: String,
    pub pripona: String,
    pub druh: &'static str,
    pub jazyk: Option<String>,
    pub velkost: Option<u64>,
    pub hlavicky: Vec<(String, String)>,
    /// zoznam HLS (m3u8) namiesto priameho súboru
    pub hls: bool,
}

pub fn format_na(f: &Value, druh: &'static str) -> Option<FormatNa> {
    let s = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let protokol = s("protocol");
    let hls = protokol.starts_with("m3u8");
    if !(hls || protokol == "https" || protokol == "http") {
        return None;
    }
    Some(FormatNa {
        id: s("format_id"),
        url: Some(s("url")).filter(|u| u.starts_with("http"))?,
        pripona: s("ext"),
        druh,
        jazyk: Some(s("language")).filter(|j| !j.is_empty() && j != "und"),
        velkost: f
            .get("filesize")
            .or(f.get("filesize_approx"))
            .and_then(Value::as_f64)
            .map(|v| v as u64),
        hlavicky: f
            .get("http_headers")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        hls,
    })
}

/// Vyberie formáty, ktoré jadro stiahne a spojí bez FFmpeg: najlepší obraz MP4 (pri „mp4“ H.264)
/// do zvolenej výšky + najlepší zvuk M4A v pôvodnom jazyku; inak najlepší spojený MP4.
/// Pri „audio“ len zvuk. `None` = bez FFmpeg sa to nedá.
/// Čo stiahne jadro: formáty (prúdy) a prípona výsledku.
#[derive(Clone, Debug)]
pub struct VyberFormatov {
    pub formaty: Vec<FormatNa>,
    /// mp4, mkv, m4a, opus – alebo prípona jediného súboru, ktorý sa nespája
    pub pripona: String,
}

/// Výber formátov pre vlastné sťahovanie a spájanie (bez FFmpeg) – rovnaké pravidlá ako pri yt-dlp:
/// najlepší obraz (rozlíšenie, snímky, kodek AV1 > VP9 > HEVC > H.264, dátový tok) a zvuk podľa
/// nastavenia (AAC, Opus alebo oboje). MP4 len pri H.264 s AAC, inak MKV. Zvuk: AAC → .m4a,
/// len Opus → .opus. MP3 tu nie je (potrebuje prekódovanie).
pub fn vyber_formaty(info: &Value, format: &str, zvuk_rezim: &str) -> Option<VyberFormatov> {
    let formaty: &[Value] = info
        .get("formats")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let s = |f: &Value, k: &str| f.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |f: &Value, k: &str| f.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    // chýbajúci kodek = neznámy (priamy súbor zo všeobecnej stránky), „none“ = určite chýba
    let je_obraz = |f: &Value| s(f, "vcodec") != "none";
    let je_zvuk = |f: &Value| s(f, "acodec") != "none";
    let vodoznak = |f: &Value| s(f, "format_note").contains("watermarked");
    let max_vyska = match format {
        "1080" => 1080.0,
        "720" => 720.0,
        _ => f64::INFINITY,
    };
    // trieda obrazu (kratšia strana); pri Full HD presne len najbližšia trieda k 1080
    let trieda = |f: &Value| {
        crate::kvalita::trieda(
            Some(n(f, "width") as u32).filter(|w| *w > 0),
            Some(n(f, "height") as u32).filter(|h| *h > 0),
        )
    };
    let presna = (format == "fhd")
        .then(|| {
            crate::kvalita::presna_trieda(
                formaty.iter().filter(|f| je_obraz(f)).filter_map(trieda),
                1080,
            )
        })
        .flatten();
    let v_kvalite = |f: &Value| match presna {
        Some(t) => trieda(f) == Some(t),
        None => (trieda(f).unwrap_or(0) as f64) <= max_vyska,
    };
    let najlepsi_zvuk = |kodek: &str, pripony: &[&str]| {
        formaty
            .iter()
            .filter(|f| je_zvuk(f) && !je_obraz(f) && pripony.contains(&s(f, "ext").as_str()))
            .filter(|f| s(f, "acodec").starts_with(kodek))
            .max_by(|a, b| {
                let k = |f: &Value| (n(f, "language_preference"), n(f, "abr").max(n(f, "tbr")));
                k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .and_then(|f| format_na(f, "zvuk"))
    };
    let aac = najlepsi_zvuk("mp4a", &["m4a", "mp4"]);
    let opus = najlepsi_zvuk("opus", &["webm"]);
    let vysledok = |formaty: Vec<FormatNa>, pripona: &str| {
        Some(VyberFormatov {
            formaty,
            pripona: pripona.into(),
        })
    };
    match format {
        "mp3" => return None,
        "audio" => {
            return match (aac, opus) {
                (Some(a), _) => vysledok(vec![a], "m4a"),
                (None, Some(o)) => vysledok(vec![o], "opus"),
                _ => None,
            }
        }
        _ => {}
    }
    let h264 = format == "mp4";
    let poradie_kodeku = |v: &str| match v.split('.').next().unwrap_or("") {
        "av01" => 4.0,
        "vp9" | "vp09" => 3.0,
        "hev1" | "hvc1" => 2.0,
        _ => 1.0,
    };
    let obraz = formaty
        .iter()
        .filter(|f| {
            je_obraz(f)
                && !je_zvuk(f)
                && !s(f, "vcodec").is_empty()
                && matches!(s(f, "ext").as_str(), "mp4" | "webm")
                && !vodoznak(f)
        })
        .filter(|f| v_kvalite(f))
        .filter(|f| !h264 || s(f, "vcodec").starts_with("avc1"))
        .max_by(|a, b| {
            let k = |f: &Value| {
                (
                    n(f, "height"),
                    n(f, "fps"),
                    poradie_kodeku(&s(f, "vcodec")),
                    n(f, "tbr"),
                )
            };
            k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
    let zvuky: Vec<FormatNa> = if h264 {
        aac.clone().into_iter().collect()
    } else {
        match zvuk_rezim {
            "aac" => aac.clone().or(opus.clone()).into_iter().collect(),
            "opus" => opus.clone().or(aac.clone()).into_iter().collect(),
            // AAC (prehrá každý prehrávač, prvá = predvolená) + Opus (vyššia kvalita)
            _ => aac.clone().into_iter().chain(opus.clone()).collect(),
        }
    };
    if let (Some(o), false) = (obraz, zvuky.is_empty()) {
        if let Some(of) = format_na(o, "video") {
            let mp4 = s(o, "vcodec").starts_with("avc1")
                && zvuky
                    .iter()
                    .all(|z| z.pripona == "m4a" || z.pripona == "mp4");
            let mut v = vec![of];
            v.extend(zvuky);
            return vysledok(v, if mp4 { "mp4" } else { "mkv" });
        }
    }
    // jeden súbor s obrazom aj zvukom (aj priamy súbor zo stránky bez údajov o kodekoch)
    let f = formaty
        .iter()
        .filter(|f| je_obraz(f) && je_zvuk(f) && !vodoznak(f) && v_kvalite(f))
        .filter(|f| !h264 || matches!(s(f, "ext").as_str(), "mp4" | "m4v"))
        .max_by(|a, b| {
            let k = |f: &Value| (n(f, "height"), n(f, "tbr"));
            k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .and_then(|f| format_na(f, "video+zvuk"))?;
    let pripona = if f.hls {
        "mp4".to_string()
    } else {
        let p: String = f
            .pripona
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(5)
            .collect();
        if p.is_empty() {
            "mp4".into()
        } else {
            p
        }
    };
    Some(VyberFormatov {
        formaty: vec![f],
        pripona,
    })
}

#[cfg(test)]
mod testy {

    #[test]
    fn prihlasenie_len_ked_treba() {
        let e = |t: &str| crate::chyba::Chyba::Ine(t.into());
        assert!(treba_prihlasenie(&e(
            "ERROR: [youtube] x: Sign in to confirm you're not a bot"
        )));
        assert!(treba_prihlasenie(&e("Obsah vyžaduje prihlásenie")));
        assert!(treba_prihlasenie(&e(
            "ERROR: Private video. Sign in if you've been granted access"
        )));
        assert!(!treba_prihlasenie(&e(
            "Stránka nie je dostupná – skontroluj pripojenie"
        )));
        assert!(!treba_prihlasenie(&e(
            "ERROR: Requested format is not available"
        )));
    }

    use super::*;
    use serde_json::json;

    fn f(id: &str, ext: &str, v: &str, a: &str, h: f64, tbr: f64) -> Value {
        json!({"format_id": id, "ext": ext, "vcodec": v, "acodec": a, "height": h, "tbr": tbr,
               "protocol": "https", "url": format!("https://cdn/{id}"), "http_headers": {"User-Agent": "UA"}})
    }

    #[test]
    fn full_hd_presne() {
        let aac = f("140", "m4a", "none", "mp4a.40.2", 0.0, 128.0);
        let info = json!({"formats": [
            aac.clone(),
            f("136", "mp4", "avc1.4d401f", "none", 720.0, 2000.0),
            f("137", "mp4", "avc1.640028", "none", 1080.0, 4000.0),
            f("271", "webm", "vp9", "none", 1440.0, 9000.0),
            f("313", "webm", "vp9", "none", 2160.0, 18000.0),
        ]});
        let v = vyber_formaty(&info, "fhd", "aac").unwrap();
        assert_eq!(v.formaty[0].url, "https://cdn/137");
        // bez 1080p: 1440 aj 720 sú rovnako ďaleko → nižšia
        let info = json!({"formats": [aac, f("136", "mp4", "avc1.4d401f", "none", 720.0, 2000.0),
                                      f("271", "webm", "vp9", "none", 1440.0, 9000.0)]});
        assert_eq!(
            vyber_formaty(&info, "fhd", "aac").unwrap().formaty[0].url,
            "https://cdn/136"
        );
    }

    #[test]
    fn vyber_bez_ffmpeg() {
        let mut cz = f("140-cz", "m4a", "none", "mp4a.40.2", 0.0, 128.0);
        cz["language"] = json!("cs");
        cz["language_preference"] = json!(-1);
        let mut orig = f("140-en", "m4a", "none", "mp4a.40.2", 0.0, 128.0);
        orig["language"] = json!("en");
        orig["language_preference"] = json!(10);
        let info = json!({"formats": [
            f("18", "mp4", "avc1.42001E", "mp4a.40.2", 360.0, 500.0),
            f("137", "mp4", "avc1.640028", "none", 1080.0, 4000.0),
            f("399", "mp4", "av01.0.08M.08", "none", 1080.0, 2500.0),
            f("401", "mp4", "av01.0.12M.08", "none", 2160.0, 12000.0),
            f("248", "webm", "vp9", "none", 1080.0, 3000.0),
            f("251", "webm", "none", "opus", 0.0, 160.0),
            cz, orig,
        ]});
        let vf = |i: &Value, f: &str, z: &str| vyber_formaty(i, f, z).unwrap();
        let ids = |v: &VyberFormatov| v.formaty.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
        // najlepší obraz (AV1 4K) + AAC aj Opus → MKV
        let v = vf(&info, "video", "oboje");
        assert_eq!(
            (ids(&v), v.pripona.as_str()),
            (
                vec!["401".to_string(), "140-en".into(), "251".into()],
                "mkv"
            )
        );
        assert_eq!(v.formaty[1].jazyk.as_deref(), Some("en"));
        assert_eq!(
            v.formaty[0].hlavicky,
            vec![("User-Agent".to_string(), "UA".to_string())]
        );
        // 1080: AV1 a VP9 pred H.264 pri rovnakom rozlíšení; len AAC
        let v = vf(&info, "1080", "aac");
        assert_eq!(
            (ids(&v), v.pripona.as_str()),
            (vec!["399".to_string(), "140-en".into()], "mkv")
        );
        // MP4 pre TV: H.264 + AAC
        let v = vf(&info, "mp4", "oboje");
        assert_eq!(
            (ids(&v), v.pripona.as_str()),
            (vec!["137".to_string(), "140-en".into()], "mp4")
        );
        assert_eq!(vf(&info, "audio", "oboje").formaty[0].id, "140-en");
        assert!(vyber_formaty(&info, "mp3", "oboje").is_none());
        // len Opus: zvuk → .opus, video VP9 + Opus → MKV (už nie je potrebný FFmpeg)
        let info3 = json!({"formats": [f("248", "webm", "vp9", "none", 1080.0, 3000.0), f("251", "webm", "none", "opus", 0.0, 160.0)]});
        let v = vf(&info3, "video", "oboje");
        assert_eq!(
            (ids(&v), v.pripona.as_str()),
            (vec!["248".to_string(), "251".into()], "mkv")
        );
        assert_eq!(vf(&info3, "audio", "oboje").pripona, "opus");
        // len spojený formát
        let info2 = json!({"formats": [f("18", "mp4", "avc1", "mp4a.40.2", 360.0, 500.0)]});
        let v2 = vf(&info2, "video", "oboje");
        assert_eq!(
            (v2.formaty.len(), v2.formaty[0].druh, v2.pripona.as_str()),
            (1, "video+zvuk", "mp4")
        );
        // priamy súbor bez údajov o kodekoch (všeobecná stránka) ostane vo svojom formáte
        let info4 = json!({"formats": [{"format_id": "0", "ext": "mov", "protocol": "http", "url": "http://a/v.mov"}]});
        let v4 = vf(&info4, "video", "oboje");
        assert_eq!(
            (v4.formaty[0].druh, v4.pripona.as_str()),
            ("video+zvuk", "mov")
        );
    }

    #[test]
    fn formaty_ako_v_pythone() {
        assert_eq!(
            format_ytdlp("video", "oboje", false).0,
            "bv*+ba[acodec^=mp4a]+ba[acodec^=opus]/bv*+ba[acodec^=mp4a]/bv*+ba/b"
        );
        assert_eq!(
            format_ytdlp("720", "aac", false).0,
            "bv*[height<=720]+ba[acodec^=mp4a]/bv*[height<=720]+ba/b[height<=720]/bv*+ba/b"
        );
        let (t, _) = format_ytdlp("audio", "oboje", true);
        assert_eq!(t, "ba[ext=m4a][format_note!*=?watermarked]/ba[format_note!*=?watermarked]/b[format_note!*=?watermarked]/ba[ext=m4a]/ba/b");
        assert!(format_ytdlp("mp3", "oboje", false)
            .1
            .contains(&"-x".to_string()));
    }

    #[test]
    fn zrozumitelne_chyby() {
        let (v, p) = zrozumitelne(
            "ERROR: [youtube] abc: Private video. Sign in if you've been granted access",
        );
        assert_eq!(v, "Video je súkromné.");
        assert!(p.contains("Private video"));
        assert_eq!(zrozumitelne("ERROR: niečo iné").0, "niečo iné");
    }
}
