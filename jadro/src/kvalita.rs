//! Kvalita obrazu: „najviac“ (strop) alebo „presne“ (napr. Full HD – práve 1080p, a keď ho video
//! nemá, najbližšia kvalita). Trieda obrazu je jeho kratšia strana: 1920×1080 aj zvislé
//! 1080×1920 sú „1080p“; keď šírka chýba, rozhoduje výška.

/// Trieda obrazu (napr. 1080) z rozmerov.
pub fn trieda(sirka: Option<u32>, vyska: Option<u32>) -> Option<u32> {
    match (sirka.filter(|w| *w > 0), vyska.filter(|h| *h > 0)) {
        (Some(w), Some(h)) => Some(w.min(h)),
        (_, Some(h)) => Some(h),
        _ => None,
    }
}

/// Pri „presne“: trieda, ktorú treba vybrať – zadaná, ak ju video má, inak najbližšia (pri
/// rovnakej vzdialenosti nižšia, aby súbor nebol zbytočne veľký).
pub fn presna_trieda(triedy: impl IntoIterator<Item = u32>, ciel: u32) -> Option<u32> {
    triedy.into_iter().min_by_key(|t| (t.abs_diff(ciel), *t))
}

/// Vyhovuje trieda stropu `max` (neznáma trieda vyhovuje vždy)?
pub fn do_stropu(t: Option<u32>, max: Option<u32>) -> bool {
    match (t, max) {
        (Some(t), Some(m)) => t <= m,
        _ => true,
    }
}

#[cfg(test)]
mod skusky {
    use super::*;

    #[test]
    fn triedy() {
        assert_eq!(trieda(Some(1920), Some(1080)), Some(1080));
        assert_eq!(trieda(Some(1080), Some(1920)), Some(1080)); // zvislé video
        assert_eq!(trieda(Some(1920), Some(800)), Some(800)); // širokouhlý film
        assert_eq!(trieda(None, Some(720)), Some(720));
        assert_eq!(trieda(Some(0), None), None);
    }

    #[test]
    fn presne_full_hd() {
        assert_eq!(presna_trieda([360, 720, 1080, 2160], 1080), Some(1080));
        assert_eq!(presna_trieda([360, 720, 1440], 1080), Some(720)); // rovnako ďaleko → nižšia
        assert_eq!(presna_trieda([1440, 2160], 1080), Some(1440));
        assert_eq!(presna_trieda([480, 900], 1080), Some(900));
        assert_eq!(presna_trieda([], 1080), None);
    }
}
