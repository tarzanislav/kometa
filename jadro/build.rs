//! Vstavané rozhranie: súbory z `kometa/webui` (preložené UI) sa vložia priamo do programu,
//! takže `kometa okno` je jeden súbor bez inštalácie. Ak priečinok chýba, okno nebude k dispozícii.

use std::path::{Path, PathBuf};

fn zbieraj(koren: &Path, d: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let mut polozky: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    polozky.sort();
    for p in polozky {
        if p.is_dir() {
            zbieraj(koren, &p, out);
        } else if let Ok(rel) = p.strip_prefix(koren) {
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, p.clone()));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let webui = manifest.join("..").join("kometa").join("webui");
    println!("cargo:rerun-if-changed={}", webui.display());
    let mut subory = vec![];
    zbieraj(&webui, &webui, &mut subory);
    let mut kod = String::from("pub static SUBORY: &[(&str, &[u8])] = &[\n");
    for (rel, cesta) in &subory {
        println!("cargo:rerun-if-changed={}", cesta.display());
        let abs = cesta.canonicalize().unwrap_or_else(|_| cesta.clone());
        kod.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            abs.to_string_lossy()
        ));
    }
    kod.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("webui.rs");
    std::fs::write(out, kod).unwrap();
    // verzia vydania: jediný zdroj je kometa/__init__.py (ten istý číta aj postup vydania)
    let init = manifest.join("..").join("kometa").join("__init__.py");
    println!("cargo:rerun-if-changed={}", init.display());
    let verzia = std::fs::read_to_string(&init)
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|r| r.trim().strip_prefix("__version__"))
                .and_then(|r| r.split('"').nth(1).map(str::to_string))
        })
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=KOMETA_VERZIA={verzia}");
}
