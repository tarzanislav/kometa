//! Čo služba potrebuje od operačného systému: výber priečinka a súboru, schránka, otvorenie
//! priečinka. Aplikácia Tauri dá vlastné (natívne dialógy okna); bez nej sa použijú nástroje
//! systému – PowerShell vo Windows, AppleScript v macOS, zenity/kdialog v Linuxe – bez ďalších knižníc.

use std::path::Path;
use std::process::{Command, Stdio};

pub trait Prostredie: Send + Sync {
    /// Výber priečinka; prázdny reťazec = zrušené.
    fn vyber_priecinok(&self, zaciatok: &str) -> String;
    /// Výber videa na doplnenie zvukových stôp.
    fn vyber_video(&self, zaciatok: &str) -> String;
    fn schranka(&self) -> String;
    /// Otvorí priečinok, prípadne v ňom označí súbor.
    fn otvor(&self, priecinok: &Path, oznac: Option<&Path>);

    /// Dá sa schránka sledovať na pozadí? (Len aplikácia – lacné čítanie schránky.)
    fn sleduje_schranku(&self) -> bool {
        false
    }
    /// Poradové číslo poslednej zmeny schránky, ak ho systém dáva (Windows). Strážca potom
    /// číta obsah schránky len po zmene.
    fn zmena_schranky(&self) -> Option<u64> {
        None
    }
    /// Krátke systémové oznámenie (aj keď je okno v pozadí).
    fn oznam(&self, _nadpis: &str, _text: &str) {}

    /// Zavrie Kometu (pred inštaláciou aktualizácie). Aplikácia zavrie okno a uloží front.
    fn ukonci(&self) {
        std::process::exit(0);
    }

    /// Má prostredie vlastné okno prehliadača (na kurzy po prihlásení)?
    fn ma_prehliadac(&self) -> bool {
        false
    }
    /// Otvorí (alebo prenesie na adresu) okno prehliadača kurzov.
    fn prehliadac_otvor(&self, _url: &str) -> Result<(), String> {
        Err("Prehliadač kurzov je len v aplikácii Kometa.".into())
    }
    /// Adresa a titulok stránky v okne prehliadača (None = okno je zatvorené).
    fn prehliadac_stranka(&self) -> Option<(String, String)> {
        None
    }
    /// Spustí skript na stránke v prehliadači a vráti jeho výsledok ako JSON (najviac ~15 s).
    fn prehliadac_skript(&self, _js: &str) -> Result<String, String> {
        Err("Prehliadač kurzov nie je otvorený.".into())
    }
    /// Cookies prehliadača pre adresu: (adresa, „meno=hodnota; Domain=…; Path=…“).
    fn prehliadac_cookies(&self, _url: &str) -> Vec<(String, String)> {
        vec![]
    }
}

/// Nástroje operačného systému (okno v prehliadači, príkazový riadok).
pub struct Systemove;

/// Pre skúšky: schránka zo súboru (`KOMETA_SKUSKA_SCHRANKA`) a „prehliadač“ z priečinka
/// (`KOMETA_SKUSKA_PREHLIADAC`: adresa.txt, zber.json, cookies.json).
fn skuska(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn skuska_prehliadaca(subor: &str) -> Option<std::path::PathBuf> {
    skuska("KOMETA_SKUSKA_PREHLIADAC").map(|d| Path::new(&d).join(subor))
}

fn spusti(prikaz: &str, argumenty: &[&str]) -> Option<String> {
    let mut c = Command::new(prikaz);
    c.args(argumenty).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // bez okna konzoly
    }
    let o = c.output().ok()?;
    o.status.success().then(|| {
        String::from_utf8_lossy(&o.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string()
    })
}

#[cfg(windows)]
fn powershell(skript: &str) -> Option<String> {
    let s = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8;{skript}");
    spusti(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-STA", "-Command", &s],
    )
}

#[cfg(windows)]
fn ps_retazec(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(target_os = "macos")]
fn as_retazec(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Prostredie for Systemove {
    fn sleduje_schranku(&self) -> bool {
        skuska("KOMETA_SKUSKA_SCHRANKA").is_some()
    }

    fn ma_prehliadac(&self) -> bool {
        skuska("KOMETA_SKUSKA_PREHLIADAC").is_some()
    }

    fn prehliadac_otvor(&self, url: &str) -> Result<(), String> {
        let f = skuska_prehliadaca("adresa.txt")
            .ok_or("Prehliadač kurzov je len v aplikácii Kometa.")?;
        std::fs::write(f, url).map_err(|e| e.to_string())
    }

    fn prehliadac_stranka(&self) -> Option<(String, String)> {
        let a = std::fs::read_to_string(skuska_prehliadaca("adresa.txt")?).ok()?;
        Some((a, "Skúška".into()))
    }

    fn prehliadac_skript(&self, _js: &str) -> Result<String, String> {
        let f = skuska_prehliadaca("zber.json").ok_or("Prehliadač kurzov nie je otvorený.")?;
        std::fs::read_to_string(f).map_err(|e| e.to_string())
    }

    fn prehliadac_cookies(&self, url: &str) -> Vec<(String, String)> {
        let Some(t) =
            skuska_prehliadaca("cookies.json").and_then(|f| std::fs::read_to_string(f).ok())
        else {
            return vec![];
        };
        let vsetky: Vec<(String, String)> = serde_json::from_str(&t).unwrap_or_default();
        let host = |u: &str| {
            url::Url::parse(u)
                .ok()
                .and_then(|x| x.host_str().map(str::to_string))
        };
        vsetky
            .into_iter()
            .filter(|(u, _)| host(u) == host(url))
            .collect()
    }

    fn vyber_priecinok(&self, zaciatok: &str) -> String {
        #[cfg(windows)]
        {
            let s = format!(
                "Add-Type -AssemblyName System.Windows.Forms;$d=New-Object System.Windows.Forms.FolderBrowserDialog;\
                 $d.Description='Kam ukladať stiahnuté súbory';$d.UseDescriptionForTitle=$true;$d.SelectedPath={};\
                 if($d.ShowDialog() -eq 'OK'){{$d.SelectedPath}}",
                ps_retazec(zaciatok)
            );
            powershell(&s).unwrap_or_default()
        }
        #[cfg(target_os = "macos")]
        {
            let mut s = String::from(
                "POSIX path of (choose folder with prompt \"Kam ukladať stiahnuté súbory\"",
            );
            if Path::new(zaciatok).is_dir() {
                s.push_str(&format!(
                    " default location (POSIX file {})",
                    as_retazec(zaciatok)
                ));
            }
            s.push(')');
            spusti("osascript", &["-e", &s])
                .map(|p| p.trim_end_matches('/').to_string())
                .unwrap_or_default()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let z = format!("{}/", zaciatok.trim_end_matches('/'));
            spusti(
                "zenity",
                &[
                    "--file-selection",
                    "--directory",
                    "--title=Kam ukladať stiahnuté súbory",
                    &format!("--filename={z}"),
                ],
            )
            .or_else(|| spusti("kdialog", &["--getexistingdirectory", zaciatok]))
            .unwrap_or_default()
        }
    }

    fn vyber_video(&self, zaciatok: &str) -> String {
        #[cfg(windows)]
        {
            let s = format!(
                "Add-Type -AssemblyName System.Windows.Forms;$d=New-Object System.Windows.Forms.OpenFileDialog;\
                 $d.Filter='Video (*.mkv;*.mp4;*.webm;*.mov;*.m4v)|*.mkv;*.mp4;*.webm;*.mov;*.m4v|Všetky súbory (*.*)|*.*';\
                 $d.InitialDirectory={};if($d.ShowDialog() -eq 'OK'){{$d.FileName}}",
                ps_retazec(zaciatok)
            );
            powershell(&s).unwrap_or_default()
        }
        #[cfg(target_os = "macos")]
        {
            let _ = zaciatok;
            spusti(
                "osascript",
                &[
                    "-e",
                    "POSIX path of (choose file with prompt \"Vyber stiahnuté video\")",
                ],
            )
            .unwrap_or_default()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let z = format!("{}/", zaciatok.trim_end_matches('/'));
            spusti(
                "zenity",
                &[
                    "--file-selection",
                    "--title=Vyber stiahnuté video",
                    &format!("--filename={z}"),
                    "--file-filter=Video | *.mkv *.mp4 *.webm *.mov *.m4v",
                    "--file-filter=Všetky súbory | *",
                ],
            )
            .or_else(|| {
                spusti(
                    "kdialog",
                    &[
                        "--getopenfilename",
                        zaciatok,
                        "*.mkv *.mp4 *.webm *.mov *.m4v",
                    ],
                )
            })
            .unwrap_or_default()
        }
    }

    fn schranka(&self) -> String {
        if let Some(f) = skuska("KOMETA_SKUSKA_SCHRANKA") {
            return std::fs::read_to_string(f).unwrap_or_default();
        }
        #[cfg(windows)]
        {
            powershell("Get-Clipboard -Raw").unwrap_or_default()
        }
        #[cfg(target_os = "macos")]
        {
            spusti("pbpaste", &[]).unwrap_or_default()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            spusti("wl-paste", &["--no-newline"])
                .or_else(|| spusti("xclip", &["-selection", "clipboard", "-o"]))
                .or_else(|| spusti("xsel", &["--clipboard", "--output"]))
                .unwrap_or_default()
        }
    }

    fn otvor(&self, priecinok: &Path, oznac: Option<&Path>) {
        let _ = std::fs::create_dir_all(priecinok);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Prieskumník chce `/select,"cesta"` doslova. Úvodzovky okolo celého argumentu, ktoré
            // pridá Command::arg pri medzerách v názve, nepochopí a otvorí predvolený priečinok.
            let mut c = Command::new("explorer.exe");
            match oznac {
                Some(s) => c.raw_arg(argument_oznac(s)),
                None => c.raw_arg(format!("\"{}\"", priecinok.display())),
            };
            c.creation_flags(0x0800_0000);
            let _ = c
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            match oznac {
                Some(s) => spusti("open", &["-R", &s.to_string_lossy()]),
                None => spusti("open", &[&priecinok.to_string_lossy()]),
            };
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = oznac;
            let p = priecinok.to_string_lossy().into_owned();
            let _ = Command::new("xdg-open")
                .arg(p)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
}

/// Argument Prieskumníka, ktorý otvorí priečinok a označí v ňom súbor.
pub fn argument_oznac(subor: &Path) -> String {
    format!("/select,\"{}\"", subor.display())
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn prieskumnik_oznac() {
        let p = Path::new(r"D:\DOKUMENTY\Videos\Kometa\Výhľad z Lomnického štítu.mkv");
        assert_eq!(
            argument_oznac(p),
            r#"/select,"D:\DOKUMENTY\Videos\Kometa\Výhľad z Lomnického štítu.mkv""#
        );
    }
}
