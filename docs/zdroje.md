# Cudzie súčasti Komety

Kód Komety je vlastný a pôvodný. Tieto cudzie súčasti používa – každá so slobodnou licenciou:

| súčasť | licencia | kde | na čo |
|---|---|---|---|
| [Svelte](https://svelte.dev) | MIT | rozhranie (preložené do `jadro/webui`) | komponenty rozhrania |
| [Vite](https://vite.dev), [TypeScript](https://www.typescriptlang.org) | MIT, Apache-2.0 | len zostavenie rozhrania | preklad rozhrania |
| [Atkinson Hyperlegible Next](https://www.brailleinstitute.org/freefont/) | SIL OFL 1.1 | rozhranie | písmo textu |
| [Bricolage Grotesque](https://github.com/ateliertriay/bricolage) | SIL OFL 1.1 | rozhranie | písmo nadpisov |
| Rust: tokio, reqwest, rustls, roxmltree, serde, clap, thiserror | MIT / Apache-2.0 | jadro v Ruste | asynchrónny beh, HTTP, XML, CLI |
| Rust: sha2, md-5, sha1 (RustCrypto), getrandom | MIT / Apache-2.0 | jadro v Ruste | kontrola yt-dlp (SHA-256), heslo Webshare, tajný kľúč okna |
| Rust: [keyring](https://github.com/hwchen/keyring-rs) | MIT / Apache-2.0 | jadro (Windows, macOS) | heslá v Správcovi poverení / Kľúčenke |
| [Tauri](https://tauri.app) 2 + pluginy dialog, clipboard-manager, single-instance | MIT / Apache-2.0 | aplikácia | natívne okno, dialógy, schránka, jedno spustenie |
| WebView2 (Windows), WebKit (macOS), WebKitGTK (Linux) | súčasť systému | aplikácia | zobrazenie rozhrania |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) (vydania na GitHube) | Unlicense | Kometa ho sama stiahne do `~/.kometa/nastroje` (samostatný program) | YouTube a ďalšie služby; overuje sa SHA-256 z vydania |
| [Deno](https://deno.com) (vydania denoland/deno) | MIT | Kometa ho sama stiahne do `~/.kometa/nastroje` (samostatný program) | JavaScript, ktorý yt-dlp potrebuje pre YouTube; overuje sa SHA-256 z vydania |
| FFmpeg ([BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds), GPL zostavenie) | GPL-3.0 | aplikácia ho na požiadanie stiahne do `~/.kometa/nastroje` (samostatný program, nie súčasť Komety) | MP3, prevod Opus → AAC, zriedkavé formáty; overuje sa SHA-256 z vydania |
| Rust: [trash](https://github.com/Byron/trash-rs) | MIT | jadro v Ruste | presun stiahnutých súborov do Koša |
| Rust: [zip](https://github.com/zip-rs/zip2) | MIT | jadro v Ruste | rozbaľovanie ZIP (aj AES a ZipCrypto) |
| Rust: [sevenz-rust2](https://github.com/hasenbanck/sevenz-rust2) | Apache-2.0 | jadro v Ruste | rozbaľovanie 7z (aj AES) |
| Rust: [unrar](https://github.com/muja/unrar.rs) + knižnica UnRAR (RARLAB) | MIT/Apache-2.0; UnRAR: licencia UnRAR (voľné použitie na rozbaľovanie, nie na tvorbu RAR) | jadro v Ruste | rozbaľovanie RAR; skúšobný archív `jadro/testdata/zaheslovany.rar` je z balíka unrar |
| [tauri-plugin-notification](https://github.com/tauri-apps/plugins-workspace) | MIT / Apache-2.0 | aplikácia | oznámenie, keď strážca schránky zachytí odkaz |

Pravidlá pre nové súčasti: len slobodné licencie, ktoré dovoľujú použitie v projekte pod MIT;
súčasť musí byť nahraditeľná a zapísaná v tejto tabuľke. Modely a dátové sady pre AI (titulky,
preklad, hlasy) sa sem zapíšu tiež, s licenciou dát – pozri [plan-ai.md](plan-ai.md).
