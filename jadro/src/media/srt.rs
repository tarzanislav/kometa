//! Titulky: prevod z formátov YouTube (json3, WebVTT) do SRT a čítanie SRT ako stopy
//! (do MKV sa vkladajú ako S_TEXT/UTF8 – čistý text s časmi).

use super::{Citac, InfoStopy, Kodek, Vzorka};
use crate::chyba::{Chyba, Vysledok};
use serde_json::Value;
use std::path::Path;

/// Jeden titulok: začiatok a koniec v milisekundách, text (riadky oddelené `\n`).
#[derive(Clone, Debug, PartialEq)]
pub struct Titulok {
    pub od: i64,
    pub do_: i64,
    pub text: String,
}

/// Uprace zoznam: zoradí, zahodí prázdne, zlúči rovnaké za sebou a skráti prekrývanie
/// (automatické titulky YouTube sa „rolujú“ – každý riadok by inak ostal visieť dvakrát).
fn uprac(mut t: Vec<Titulok>) -> Vec<Titulok> {
    for x in &mut t {
        x.text = x
            .text
            .lines()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    t.retain(|x| !x.text.is_empty() && x.do_ > x.od);
    t.sort_by_key(|x| x.od);
    let mut out: Vec<Titulok> = vec![];
    for x in t {
        if let Some(p) = out.last_mut() {
            if p.text == x.text && x.od <= p.do_ + 50 {
                p.do_ = p.do_.max(x.do_);
                continue;
            }
            if p.do_ > x.od {
                p.do_ = x.od.max(p.od + 1);
            }
        }
        out.push(x);
    }
    out
}

/// Formát json3 z YouTube: {"events":[{"tStartMs":…,"dDurationMs":…,"segs":[{"utf8":"…"}]}]}.
pub fn z_json3(t: &str) -> Vec<Titulok> {
    let Ok(j) = serde_json::from_str::<Value>(t) else {
        return vec![];
    };
    let mut out = vec![];
    for e in j
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(segs) = e.get("segs").and_then(Value::as_array) else {
            continue;
        };
        let text: String = segs
            .iter()
            .filter_map(|s| s.get("utf8").and_then(Value::as_str))
            .collect();
        let od = e.get("tStartMs").and_then(Value::as_i64).unwrap_or(0);
        let dlzka = e.get("dDurationMs").and_then(Value::as_i64).unwrap_or(0);
        if text.trim().is_empty() || dlzka <= 0 {
            continue;
        }
        out.push(Titulok {
            od,
            do_: od + dlzka,
            text,
        });
    }
    uprac(out)
}

fn cas_vtt(t: &str) -> Option<i64> {
    // 00:01:02.345 alebo 01:02.345 (SRT má čiarku)
    let t = t.trim().replace(',', ".");
    let (hms, ms) = t.split_once('.').unwrap_or((&t, "0"));
    let casti: Vec<i64> = hms
        .split(':')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    let s = match casti.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        _ => return None,
    };
    let ms: String = ms.chars().take(3).collect();
    let ms = format!("{ms:0<3}").parse::<i64>().ok()?;
    Some(s * 1000 + ms)
}

/// Odstráni značky ako <c>, <00:00:01.000>, <i> a zmení entity na znaky.
fn bez_znaciek(r: &str) -> String {
    let mut out = String::with_capacity(r.len());
    let mut v_znacke = false;
    for c in r.chars() {
        match c {
            '<' => v_znacke = true,
            '>' if v_znacke => v_znacke = false,
            _ if !v_znacke => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// WebVTT (aj SRT – líšia sa len oddeľovačom milisekúnd a hlavičkou).
pub fn z_vtt(t: &str) -> Vec<Titulok> {
    let t = t.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = vec![];
    for blok in t.split("\n\n") {
        let riadky: Vec<&str> = blok.lines().collect();
        let Some(i) = riadky.iter().position(|r| r.contains("-->")) else {
            continue;
        };
        let (a, b) = riadky[i].split_once("-->").unwrap_or_default();
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(od), Some(do_)) = (cas_vtt(a), cas_vtt(b)) else {
            continue;
        };
        let text = riadky[i + 1..]
            .iter()
            .map(|r| bez_znaciek(r))
            .collect::<Vec<_>>()
            .join("\n");
        out.push(Titulok { od, do_, text });
    }
    uprac(out)
}

fn cas_srt(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

pub fn na_srt(t: &[Titulok]) -> String {
    let mut s = String::new();
    for (i, x) in t.iter().enumerate() {
        s.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            cas_srt(x.od),
            cas_srt(x.do_),
            x.text
        ));
    }
    s
}

/// Čítač SRT: jedna stopa titulkov, vzorky = titulky (čas v ns, text UTF-8).
pub struct CitacSrt {
    stopy: Vec<InfoStopy>,
    titulky: std::vec::IntoIter<Titulok>,
}

impl CitacSrt {
    pub fn otvor(cesta: &Path) -> Vysledok<Self> {
        let t = std::fs::read_to_string(cesta)?;
        let titulky = z_vtt(t.trim_start_matches('\u{feff}'));
        if titulky.is_empty() {
            return Err(Chyba::Format("titulky sú prázdne".into()));
        }
        Ok(CitacSrt {
            stopy: vec![InfoStopy {
                kodek: Kodek::Srt,
                konfig: vec![],
                sirka: 0,
                vyska: 0,
                frekvencia: 0,
                kanaly: 0,
                jazyk: None,
                nazov: None,
            }],
            titulky: titulky.into_iter(),
        })
    }
}

impl Citac for CitacSrt {
    fn stopy(&self) -> &[InfoStopy] {
        &self.stopy
    }
    fn dalsia(&mut self) -> Vysledok<Option<Vzorka>> {
        Ok(self.titulky.next().map(|t| Vzorka {
            stopa: 0,
            dts: t.od * 1_000_000,
            pts: t.od * 1_000_000,
            trvanie: (t.do_ - t.od) * 1_000_000,
            kluc: true,
            data: t.text.into_bytes(),
        }))
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn json3_a_rolovanie() {
        let j = r#"{"events":[
            {"tStartMs":0,"dDurationMs":5000,"id":1,"wpWinPosId":1},
            {"tStartMs":1000,"dDurationMs":3000,"segs":[{"utf8":"Ahoj "},{"utf8":"svet"}]},
            {"tStartMs":2500,"dDurationMs":3000,"segs":[{"utf8":"\n"}]},
            {"tStartMs":3000,"dDurationMs":2000,"segs":[{"utf8":"druhý riadok"}]}
        ]}"#;
        let t = z_json3(j);
        assert_eq!(
            t,
            vec![
                Titulok {
                    od: 1000,
                    do_: 3000,
                    text: "Ahoj svet".into()
                },
                Titulok {
                    od: 3000,
                    do_: 5000,
                    text: "druhý riadok".into()
                },
            ]
        );
        assert_eq!(
            na_srt(&t),
            "1\n00:00:01,000 --> 00:00:03,000\nAhoj svet\n\n2\n00:00:03,000 --> 00:00:05,000\ndruhý riadok\n\n"
        );
    }

    #[test]
    fn vtt_so_znackami() {
        let v = "WEBVTT\nKind: captions\nLanguage: cs\n\n00:00:01.000 --> 00:00:02.500 align:start position:0%\nPrvý <c>riadok</c><00:00:01.500>\n\n00:02.500 --> 00:04.000\n&lt;b&gt; &amp; druhý\n";
        let t = z_vtt(v);
        assert_eq!(t.len(), 2);
        assert_eq!(
            t[0],
            Titulok {
                od: 1000,
                do_: 2500,
                text: "Prvý riadok".into()
            }
        );
        assert_eq!(t[1].text, "<b> & druhý");
        // SRT sa číta rovnako (značky v texte sa pri čítaní odstránia)
        assert_eq!(z_vtt(&na_srt(&t[..1])), t[..1]);
    }
}
