//! Súbory stiahnuté v prehliadači kurzov: idú do MATERIALY otvoreného kurzu, rovnaký názov
//! dostane číslo, hotový súbor pribudne do zoznamu a do prehľadu kurzu.

use kometa_jadro::sluzba::prostredie::Systemove;
use kometa_jadro::sluzba::Sluzba;
use serde_json::{json, Value};
use std::path::Path;

#[test]
fn stiahnute_v_prehliadaci_idu_do_materialov_kurzu() {
    let d = std::env::temp_dir().join(format!("kometa-prehliadac-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (cfg, out, pr) = (d.join("cfg"), d.join("out"), d.join("pr"));
    for x in [&cfg, &out, &pr] {
        std::fs::create_dir_all(x).unwrap();
    }
    // SAFETY: test je v tomto súbore jediný, premenné sa nastavia pred spustením služby
    unsafe {
        std::env::set_var("KOMETA_PRIECINOK", &cfg);
        std::env::set_var("KOMETA_BEZ_TREZORU", "1");
        std::env::set_var("KOMETA_POMOCNICI", "nie");
        std::env::set_var("KOMETA_SKUSKA_PREHLIADAC", &pr);
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let s = Sluzba::new(
        Box::new(Systemove),
        rt.handle().clone(),
        "aplikacia",
        "skuska",
    );
    let volaj = |m: &str, a: Value| {
        let r = rt.block_on(s.volaj(m, a.as_array().unwrap().clone()));
        assert_eq!(r["ok"], true, "{m}: {r}");
        r["data"].clone()
    };
    volaj("ulozit_nastavenia", json!([{"outdir": out}]));

    // bez otvoreného kurzu: spoločný priečinok
    let bez = s.priecinok_z_prehliadaca();
    assert!(
        bez.ends_with("KURZY/Z prehliadača kurzov/MATERIALY"),
        "{bez:?}"
    );

    // kurz otvorený z Vybraných kurzov (podľa názvu)
    volaj(
        "kurz_otvorit",
        json!(["https://kurzy.example.com/kurz/1", {"nazov": "Môj kurz | Example"}]),
    );
    let materialy = out.join("KURZY").join("Môj kurz").join("MATERIALY");
    assert_eq!(s.priecinok_z_prehliadaca(), materialy);
    let stav = volaj("stav", json!([]));
    assert_eq!(stav["prehliadac_materialy"], json!(materialy));

    // dve sťahovania s rovnakým názvom naraz a tretie po nich: nič sa neprepíše
    let navrh = Path::new("/home/ja/Downloads/! Materiály kurzu.zip");
    let a = s.ciel_z_prehliadaca(navrh);
    let b = s.ciel_z_prehliadaca(navrh);
    assert_eq!(a, materialy.join("! Materiály kurzu.zip"));
    assert_eq!(b, materialy.join("! Materiály kurzu (2).zip"));
    std::fs::write(&a, b"zip 1").unwrap();
    s.stiahnute_z_prehliadaca("https://zdielane.example.com/subor/1", &a, true);
    s.stiahnute_z_prehliadaca("https://zdielane.example.com/subor/2", &b, false);
    let c = s.ciel_z_prehliadaca(navrh);
    assert_eq!(c, materialy.join("! Materiály kurzu (2).zip")); // nepodarené uvoľnilo názov
    std::fs::write(&c, b"zip 2").unwrap();
    s.stiahnute_z_prehliadaca("https://zdielane.example.com/subor/3", &c, true);
    assert_eq!(
        s.ciel_z_prehliadaca(navrh),
        materialy.join("! Materiály kurzu (3).zip")
    );

    // hotové súbory sú v zozname
    let stav = volaj("stav", json!([]));
    let hotove: Vec<(String, String)> = stav["polozky"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["nazov"].as_str().unwrap().to_string(),
                p["stav"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        hotove,
        [
            ("! Materiály kurzu.zip".to_string(), "hotovo".to_string()),
            (
                "! Materiály kurzu (2).zip".to_string(),
                "hotovo".to_string()
            )
        ]
    );

    // stiahnutý kurz s odkazom v .txt: odkaz sa nájde a otvorí, súbory idú do jeho MATERIALY
    let kurz = out.join("KURZY").join("Iný kurz");
    std::fs::create_dir_all(kurz.join("MATERIALY")).unwrap();
    std::fs::write(
        kurz.join("MATERIALY/02 Materiály (Odkaz vo vnútri súboru).txt"),
        "Materiály kurzu (Odkaz vo vnútri súboru)\r\nhttps://zdielane.example.com/:f:/g/priecinok?e=abc\r\n",
    )
    .unwrap();
    let o = volaj("kurz_odkazy_materialov", json!([kurz]));
    assert_eq!(
        o,
        json!([{"subor": "02 Materiály (Odkaz vo vnútri súboru).txt", "host": "zdielane.example.com",
                "odkaz": "https://zdielane.example.com/:f:/g/priecinok?e=abc"}])
    );
    volaj(
        "kurz_otvorit",
        json!(["https://zdielane.example.com/:f:/g/priecinok?e=abc", {"priecinok": kurz}]),
    );
    assert_eq!(s.priecinok_z_prehliadaca(), kurz.join("MATERIALY"));
    // priečinok mimo KURZY sa neprijme
    let r = rt.block_on(s.volaj("kurz_odkazy_materialov", vec![json!(d)]));
    assert_eq!(r["ok"], false);
    let _ = std::fs::remove_dir_all(&d);
}
