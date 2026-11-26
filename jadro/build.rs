//! Vstavané rozhranie: súbory z `jadro/webui` (preložené UI) sa vložia priamo do programu,
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
    let webui = manifest.join("webui");
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
}
