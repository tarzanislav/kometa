//! Kometa – aplikácia: rozhranie z `kometa/webui` v natívnom okne (Tauri), jadro v Ruste.
//!
//! Okno sa s jadrom rozpráva jediným príkazom `volaj(metoda, args)` – rovnaké metódy a tvary dát
//! ako okno v Pythone, takže rozhranie je pre obe verzie to isté.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kometa_jadro::sluzba::prostredie::{Prostredie, Systemove};
use kometa_jadro::sluzba::Sluzba;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tauri::{Manager, RunEvent};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

/// Natívne dialógy a schránka okna aplikácie.
struct Okno {
    app: tauri::AppHandle,
}

impl Prostredie for Okno {
    fn vyber_priecinok(&self, zaciatok: &str) -> String {
        let mut d = self
            .app
            .dialog()
            .file()
            .set_title("Kam ukladať stiahnuté súbory");
        if Path::new(zaciatok).is_dir() {
            d = d.set_directory(zaciatok);
        }
        if let Some(w) = self.app.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        d.blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn vyber_video(&self, zaciatok: &str) -> String {
        let mut d = self
            .app
            .dialog()
            .file()
            .set_title("Vyber stiahnuté video")
            .add_filter("Video", &["mkv", "mp4", "webm", "mov", "m4v"]);
        if Path::new(zaciatok).is_dir() {
            d = d.set_directory(zaciatok);
        }
        if let Some(w) = self.app.get_webview_window("main") {
            d = d.set_parent(&w);
        }
        d.blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn schranka(&self) -> String {
        self.app.clipboard().read_text().unwrap_or_default()
    }

    fn otvor(&self, priecinok: &Path, oznac: Option<&Path>) {
        Systemove.otvor(priecinok, oznac);
    }
}

/// Jediný príkaz okna: zavolá metódu služby a vráti `{"ok": …}`.
#[tauri::command]
async fn volaj(
    sluzba: tauri::State<'_, Sluzba>,
    metoda: String,
    args: Vec<Value>,
) -> Result<Value, ()> {
    let s = sluzba.inner().clone();
    Ok(s.volaj(&metoda, args).await)
}

fn main() {
    // sťahovania bežia na vlastnom runtime – nezávisle od okna
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("kometa")
            .build()
            .expect("runtime"),
    );
    let rt2 = rt.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // druhé spustenie len ukáže už otvorené okno (jeden front, jeden zápis na disk)
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let okno = Okno {
                app: app.handle().clone(),
            };
            app.manage(Sluzba::new(
                Box::new(okno),
                rt2.handle().clone(),
                "aplikacia",
                env!("CARGO_PKG_VERSION"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![volaj])
        .build(tauri::generate_context!())
        .expect("Kometa sa nepodarilo spustiť");
    app.run(move |app, udalost| {
        if let RunEvent::Exit = udalost {
            // rozbehnuté sťahovania sa zastavia tak, aby pri ďalšom spustení pokračovali
            if let Some(s) = app.try_state::<Sluzba>() {
                let s = s.inner().clone();
                rt.block_on(s.zatvor());
            }
        }
    });
}
