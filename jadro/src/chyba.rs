//! Chyby jadra – každá má zrozumiteľnú slovenskú vetu.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Chyba {
    /// Obsah je šifrovaný (DRM alebo kľúč v streame). Kometa šifrovaný obsah zámerne nesťahuje.
    #[error("Obsah je chránený šifrovaním ({0}) – Kometa šifrovaný obsah nesťahuje.")]
    Drm(String),
    #[error("Živé vysielanie zatiaľ nie je podporované.")]
    Zive,
    #[error("Server vrátil chybu {0}.")]
    Http(u16),
    #[error("Spojenie zlyhalo: {0}")]
    Siet(String),
    #[error("Neplatný formát: {0}")]
    Format(String),
    #[error("Sťahovanie bolo pozastavené – pri ďalšom spustení sa pokračuje.")]
    Pauza,
    #[error("Server poslal webovú stránku namiesto súboru – skontroluj prihlásenie, kredit alebo platnosť odkazu.")]
    Stranka,
    #[error("Chyba disku: {0}")]
    Io(#[from] std::io::Error),
    /// Vlastné spájanie tento kodek alebo formát (zatiaľ) nepozná – použije sa FFmpeg, ak je k dispozícii.
    #[error("Nepodporovaný formát: {0}")]
    Nepodporovane(String),
    #[error("{0}")]
    Ine(String),
}

impl From<reqwest::Error> for Chyba {
    fn from(e: reqwest::Error) -> Self {
        match e.status() {
            Some(s) => Chyba::Http(s.as_u16()),
            None => Chyba::Siet(e.to_string()),
        }
    }
}

impl Chyba {
    /// Krátky strojový kód (pre okno a skripty).
    pub fn kod(&self) -> &'static str {
        match self {
            Chyba::Drm(_) => "drm",
            Chyba::Zive => "zive",
            Chyba::Http(_) => "http",
            Chyba::Siet(_) => "siet",
            Chyba::Format(_) => "format",
            Chyba::Pauza => "pauza",
            Chyba::Stranka => "stranka",
            Chyba::Io(_) => "disk",
            Chyba::Nepodporovane(_) => "nepodporovane",
            Chyba::Ine(_) => "ine",
        }
    }
}

pub type Vysledok<T> = Result<T, Chyba>;
