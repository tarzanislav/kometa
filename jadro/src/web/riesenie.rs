//! Adresa (stránka, prehrávač, stream, súbor) → čo presne stiahnuť.

use super::prehravace::{self, Zdroj};
use super::skener::{self, Nalez, Prieskum, Skener};
use super::vimeo;
use super::vzory::{self, Druh};
use crate::chyba::{Chyba, Vysledok};
use crate::udalosti::{self, Hlasic};
use reqwest::Client;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "druh", rename_all = "lowercase")]
pub enum Ciel {
    Stream {
        url: String,
        referer: Option<String>,
        nazov: Option<String>,
        poskytovatel: String,
    },
    Subor {
        url: String,
        referer: Option<String>,
        nazov: Option<String>,
        pripona: Option<String>,
        poskytovatel: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Riesenie {
    pub ciel: Ciel,
    /// nález zo stránky, z ktorého je cieľ (pri priamom odkaze `None`)
    pub nalez: Option<Nalez>,
    /// ďalšie odporúčané nálezy na stránke (stiahne sa len prvý)
    pub dalsie: Vec<Nalez>,
}

fn neprazdny(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Prehrávač → stream alebo súbor.
pub async fn prehravac(k: &Client, p: &vzory::Prehravac, referer: Option<&str>) -> Vysledok<Ciel> {
    let referer_s = referer.map(str::to_string);
    if p.poskytovatel == "Vimeo" {
        if vimeo::id_a_kluc(&p.url).is_none() {
            return Err(Chyba::Nepodporovane(
                "Vimeo zbierky (showcase) a podujatia jadro zatiaľ nevie – použi okno Komety"
                    .into(),
            ));
        }
        let v = vimeo::zisti(k, &p.url, referer).await?;
        let nazov = neprazdny(v.nazov.clone());
        if let Some(h) = v.hls.first() {
            return Ok(Ciel::Stream {
                url: h.clone(),
                referer: referer_s,
                nazov,
                poskytovatel: "Vimeo".into(),
            });
        }
        if let Some((_, u)) = v.mp4.first() {
            return Ok(Ciel::Subor {
                url: u.clone(),
                referer: referer_s,
                nazov,
                pripona: Some("mp4".into()),
                poskytovatel: "Vimeo".into(),
            });
        }
        return Err(Chyba::Format("Vimeo neposkytlo video".into()));
    }
    match prehravace::zisti(k, p, referer).await? {
        Some(v) => Ok(match v.zdroj {
            Zdroj::Stream(u) => Ciel::Stream {
                url: u,
                referer: referer_s,
                nazov: neprazdny(v.nazov),
                poskytovatel: p.poskytovatel.into(),
            },
            Zdroj::Subor { url, pripona } => Ciel::Subor {
                url,
                referer: referer_s,
                nazov: neprazdny(v.nazov),
                pripona: Some(pripona),
                poskytovatel: p.poskytovatel.into(),
            },
        }),
        None => Err(Chyba::Nepodporovane(format!(
            "{}: jadro ho zatiaľ nesťahuje – použi okno Komety (yt-dlp)",
            p.poskytovatel
        ))),
    }
}

fn z_nalezu(n: &Nalez) -> Option<Ciel> {
    let referer = Some(n.referer.clone()).filter(|r| !r.is_empty() && *r != n.url);
    let nazov = neprazdny(n.nazov.clone());
    match n.druh {
        Druh::Stream => Some(Ciel::Stream {
            url: n.url.clone(),
            referer,
            nazov,
            poskytovatel: n.poskytovatel.clone(),
        }),
        Druh::Video | Druh::Zvuk | Druh::Subor => Some(Ciel::Subor {
            url: n.url.clone(),
            referer,
            nazov,
            pripona: vzory::medium(&n.url).map(|(_, p)| p.to_ascii_lowercase()),
            poskytovatel: n.poskytovatel.clone(),
        }),
        _ => None,
    }
}

/// Zistí, čo sa z adresy stiahne. Stránku prehľadá a vyberie najlepší nález.
pub async fn vyries(
    k: &Client,
    adresa: &str,
    referer: Option<&str>,
    hlasic: &Hlasic,
) -> Vysledok<Riesenie> {
    if let Some(p) = vzory::prehravac(adresa) {
        let ciel = prehravac(k, &p, referer).await?;
        return Ok(Riesenie {
            ciel,
            nalez: None,
            dalsie: vec![],
        });
    }
    let nalezy = match Skener::new(k, hlasic).preskumaj(adresa, referer).await? {
        Prieskum::Nalezy(n) => n,
        Prieskum::Priamo { adresa: a, stream } => {
            let referer = referer.map(str::to_string);
            let ciel = if stream {
                let p = if a.to_ascii_lowercase().contains(".mpd") {
                    "DASH"
                } else {
                    "HLS"
                };
                Ciel::Stream {
                    url: a,
                    referer,
                    nazov: None,
                    poskytovatel: p.into(),
                }
            } else {
                let pripona = vzory::medium(&a).map(|(_, p)| p.to_ascii_lowercase());
                Ciel::Subor {
                    url: a,
                    referer,
                    nazov: None,
                    pripona,
                    poskytovatel: "súbor".into(),
                }
            };
            return Ok(Riesenie {
                ciel,
                nalez: None,
                dalsie: vec![],
            });
        }
    };
    let vyber = skener::najlepsie(&nalezy, 10);
    let Some(&prvy) = vyber.first() else {
        return Err(Chyba::Format(
            "Na stránke sa nenašlo video ani súbor na stiahnutie. Ak je obsah len po prihlásení, použi okno Komety s prihlásením z prehliadača.".into(),
        ));
    };
    let dalsie: Vec<Nalez> = vyber.iter().skip(1).map(|&i| nalezy[i].clone()).collect();
    if !dalsie.is_empty() {
        udalosti::info(
            hlasic,
            format!(
                "Na stránke je {} ďalších videí – zoznam ukáže „kometa skenuj“.",
                dalsie.len()
            ),
        );
    }
    let n = &nalezy[prvy];
    let ciel = match n.druh {
        Druh::Prehravac => {
            let p = vzory::prehravac(&n.url)
                .ok_or_else(|| Chyba::Format("neznámy prehrávač".into()))?;
            let mut c = prehravac(k, &p, Some(&n.referer)).await?;
            // názov stránky, ak prehrávač vlastný nemá
            let (Ciel::Stream { nazov, .. } | Ciel::Subor { nazov, .. }) = &mut c;
            if nazov.is_none() {
                *nazov = neprazdny(n.nazov.clone());
            }
            c
        }
        _ => z_nalezu(n).ok_or_else(|| Chyba::Format("nález sa nedá stiahnuť".into()))?,
    };
    Ok(Riesenie {
        ciel,
        nalez: Some(n.clone()),
        dalsie,
    })
}
