//! Udalosti, ktoré jadro hlási počas práce (priebeh, info, hotovo…).
//! V režime `--json` ide každá udalosť na samostatný riadok – číta ich okno Komety alebo AI vrstva.

use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "typ", rename_all = "snake_case")]
pub enum Udalost {
    Info {
        sprava: String,
    },
    Varovanie {
        sprava: String,
    },
    Priebeh {
        stiahnute: u64,
        celkom: Option<u64>,
        rychlost: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cast: Option<String>,
    },
    Hotovo {
        subor: String,
    },
    Chyba {
        sprava: String,
        kod: String,
    },
}

pub type Hlasic = Arc<dyn Fn(Udalost) + Send + Sync>;

pub fn tichy() -> Hlasic {
    Arc::new(|_| {})
}

pub fn info(h: &Hlasic, s: impl Into<String>) {
    h(Udalost::Info { sprava: s.into() });
}

pub fn varovanie(h: &Hlasic, s: impl Into<String>) {
    h(Udalost::Varovanie { sprava: s.into() });
}
