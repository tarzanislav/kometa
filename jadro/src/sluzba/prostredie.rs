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
}

/// Nástroje operačného systému (okno v prehliadači, príkazový riadok).
pub struct Systemove;

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
