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
use std::time::Duration;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

/// Okno prehliadača kurzov – bežná stránka bez prístupu k jadru (žiadne povolenia v capabilities).
const PREHLIADAC: &str = "kurz";

/// Natívne dialógy a schránka okna aplikácie.
struct Okno {
    app: tauri::AppHandle,
}

impl Prostredie for Okno {
    fn ukonci(&self) {
        // zavrie okná; pri udalosti Exit sa uloží front (pokračuje sa po spustení)
        self.app.exit(0);
    }

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

    fn sleduje_schranku(&self) -> bool {
        true
    }

    fn zmena_schranky(&self) -> Option<u64> {
        #[cfg(windows)]
        {
            #[link(name = "user32")]
            extern "system" {
                fn GetClipboardSequenceNumber() -> u32;
            }
            // SAFETY: funkcia nemá parametre ani vedľajšie účinky, len vráti číslo zmeny schránky
            Some(unsafe { GetClipboardSequenceNumber() } as u64)
        }
        #[cfg(not(windows))]
        None
    }

    fn oznam(&self, nadpis: &str, text: &str) {
        // keď je Kometa vpredu, stačí jej vlastný zoznam – oznámenie len v pozadí
        let vpredu = self
            .app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if !vpredu {
            let _ = self
                .app
                .notification()
                .builder()
                .title(nadpis)
                .body(text)
                .show();
        }
    }

    fn ma_prehliadac(&self) -> bool {
        true
    }

    fn prehliadac_otvor(&self, url: &str) -> Result<(), String> {
        let u: tauri::Url = url.parse().map_err(|_| "Neplatná adresa.".to_string())?;
        if let Some(w) = self.app.get_webview_window(PREHLIADAC) {
            w.navigate(u).map_err(|e| e.to_string())?;
            let _ = w.unminimize();
            let _ = w.set_focus();
            return Ok(());
        }
        WebviewWindowBuilder::new(&self.app, PREHLIADAC, WebviewUrl::External(u))
            .title("Kometa – prehliadač kurzov")
            .inner_size(1200.0, 860.0)
            .min_inner_size(640.0, 480.0)
            // len bežné webové adresy (žiadne súbory ani vlastné schémy)
            .on_navigation(|u| matches!(u.scheme(), "http" | "https" | "about" | "blob" | "data"))
            .build()
            .map(|_| ())
            .map_err(|e| format!("Prehliadač sa nepodarilo otvoriť: {e}"))
    }

    fn prehliadac_stranka(&self) -> Option<(String, String)> {
        let w = self.app.get_webview_window(PREHLIADAC)?;
        Some((
            w.url().map(|u| u.to_string()).unwrap_or_default(),
            w.title().unwrap_or_default(),
        ))
    }

    fn prehliadac_skript(&self, js: &str) -> Result<String, String> {
        let w = self
            .app
            .get_webview_window(PREHLIADAC)
            .ok_or("Prehliadač kurzov nie je otvorený – otvor ho a prejdi na stránku kurzu.")?;
        let (tx, rx) = std::sync::mpsc::channel();
        w.eval_with_callback(js, move |vysledok| {
            let _ = tx.send(vysledok);
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "Stránka neodpovedá – počkaj, kým sa načíta, a skús znova.".to_string())
    }

    fn prehliadac_cookies(&self, url: &str) -> Vec<(String, String)> {
        let (Some(w), Ok(u)) = (
            self.app.get_webview_window(PREHLIADAC),
            url.parse::<tauri::Url>(),
        ) else {
            return vec![];
        };
        w.cookies_for_url(u)
            .unwrap_or_default()
            .into_iter()
            .map(|c| (url.to_string(), c.to_string()))
            .collect()
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
        .plugin(tauri_plugin_notification::init())
        .on_window_event(|w, udalost| {
            // zatvorením hlavného okna sa zatvorí aj prehliadač kurzov (a aplikácia skončí)
            if w.label() == "main" {
                if let tauri::WindowEvent::Destroyed = udalost {
                    if let Some(k) = w.app_handle().get_webview_window(PREHLIADAC) {
                        let _ = k.close();
                    }
                }
            }
        })
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
