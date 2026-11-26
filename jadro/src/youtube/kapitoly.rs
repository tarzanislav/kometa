//! Kapitoly z časových značiek v popise videa.
//!
//! Pravidlá sú tie, ktoré YouTube ukazuje autorom: aspoň tri značky, prvá na 0:00, idú za sebou
//! a každá kapitola trvá aspoň 10 sekúnd. Inak popis kapitoly nemá (len zmienky o čase).

const NAJMENEJ_ZNACIEK: usize = 3;
const NAJKRATSIA: f64 = 10.0;

/// Kapitoly (začiatok v sekundách, názov) alebo prázdny zoznam.
pub fn z_popisu(popis: &str, trvanie: Option<f64>) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = vec![];
    for riadok in popis.lines() {
        let Some((cas, nazov)) = riadok_kapitoly(riadok) else {
            continue;
        };
        if out.is_empty() && cas != 0.0 {
            continue; // značky pred zoznamom kapitol (napr. „pozri 3:20“) nerátame
        }
        match out.last() {
            Some((posledny, _)) if cas <= *posledny => return vec![],
            _ => out.push((cas, nazov)),
        }
    }
    let koniec = trvanie.unwrap_or(f64::INFINITY);
    let dlzky_ok = out
        .iter()
        .zip(out.iter().skip(1).map(|x| x.0).chain([koniec]))
        .all(|((od, _), do_)| do_ - od >= NAJKRATSIA);
    if out.len() < NAJMENEJ_ZNACIEK || !dlzky_ok {
        return vec![];
    }
    out
}

/// Riadok so značkou: prvá značka času v riadku a zvyšok riadka ako názov.
fn riadok_kapitoly(riadok: &str) -> Option<(f64, String)> {
    let slova: Vec<&str> = riadok.split_whitespace().collect();
    let (poloha, cas) = slova
        .iter()
        .enumerate()
        .find_map(|(i, s)| cas_slova(s).map(|c| (i, c)))?;
    // značka má byť na začiatku alebo na konci riadka, nie uprostred vety
    if poloha != 0 && poloha != slova.len() - 1 {
        return None;
    }
    let nazov: Vec<&str> = slova
        .iter()
        .filter(|s| cas_slova(s).is_none())
        .copied()
        .collect();
    let nazov = nazov
        .join(" ")
        .trim_matches(|c: char| ODDELOVACE.contains(c) || c.is_whitespace())
        .to_string();
    (!nazov.is_empty()).then_some((cas, nazov))
}

const ODDELOVACE: &str = "-–—:|•·▶►>»()[]";

/// Čas zo slova „1:02:03“, „12:05“ alebo „0:00“ (aj v zátvorkách či s oddeľovačom).
fn cas_slova(slovo: &str) -> Option<f64> {
    let s = slovo.trim_matches(|c: char| ODDELOVACE.contains(c) && c != ':');
    let s = s.trim_end_matches([':', ',', '.']);
    let casti: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&casti.len()) || casti.iter().any(|c| c.is_empty() || c.len() > 2) {
        return None;
    }
    let cisla: Vec<u32> = casti
        .iter()
        .map(|c| {
            c.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| c.parse().ok())?
        })
        .collect::<Option<_>>()?;
    // minúty a sekundy za dvojbodkou sú dvojmiestne a menšie ako 60
    if casti[1..].iter().any(|c| c.len() != 2) || cisla[1..].iter().any(|&c| c >= 60) {
        return None;
    }
    Some(cisla.iter().fold(0.0, |a, &c| a * 60.0 + c as f64))
}

#[cfg(test)]
mod skusky {
    use super::*;

    #[test]
    fn kapitoly_z_popisu() {
        let p = "Video o Komete.\n\n0:00 Úvod\n1:15 - Inštalácia\n(12:40) Jazykové stopy\nZáver 1:02:03\n\nViac na webe.";
        let k = z_popisu(p, Some(4000.0));
        assert_eq!(
            k,
            vec![
                (0.0, "Úvod".to_string()),
                (75.0, "Inštalácia".to_string()),
                (760.0, "Jazykové stopy".to_string()),
                (3723.0, "Záver".to_string()),
            ]
        );
    }

    #[test]
    fn bez_kapitol() {
        // menej ako tri značky, prvá nie je 0:00, nejdú za sebou, príliš krátka kapitola
        assert!(z_popisu("0:00 A\n1:00 B", None).is_empty());
        assert!(z_popisu("0:05 A\n1:00 B\n2:00 C", None).is_empty());
        assert!(z_popisu("0:00 A\n2:00 B\n1:00 C", None).is_empty());
        assert!(z_popisu("0:00 A\n0:05 B\n1:00 C", None).is_empty());
        assert!(z_popisu("0:00 A\n1:00 B\n2:00 C", Some(125.0)).is_empty());
        // čas uprostred vety nie je kapitola; zlé časy sa neberú
        assert!(z_popisu("pozri 0:00 tu\npozri 1:00 tu\npozri 2:00 tu", None).is_empty());
        assert_eq!(cas_slova("1:60"), None);
        assert_eq!(cas_slova("1:5"), None);
        assert_eq!(cas_slova("123:00"), None);
        assert_eq!(cas_slova("[02:30]"), Some(150.0));
    }

    #[test]
    fn znacky_pred_zoznamom() {
        let p = "Najlepšie je 3:20!\n00:00 Začiatok\n00:30 Stred\n01:00 Koniec";
        assert_eq!(z_popisu(p, Some(90.0)).len(), 3);
    }
}
