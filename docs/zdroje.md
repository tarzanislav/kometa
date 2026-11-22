# Cudzie súčasti Komety

Kód Komety je vlastný a pôvodný. Tieto cudzie súčasti používa – každá so slobodnou licenciou:

| súčasť | licencia | kde | na čo |
|---|---|---|---|
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Unlicense | program (Python) | sťahovanie z YouTube a ďalších stránok – dočasne, kým ho nenahradí jadro v Ruste |
| [FFmpeg](https://ffmpeg.org) | LGPL/GPL (samostatný program) | nainštalovaný zvlášť | spájanie stôp, prevod zvuku do AAC |
| [Deno](https://deno.com) | MIT | nainštalovaný zvlášť | yt-dlp ho potrebuje pre YouTube |
| [requests](https://github.com/psf/requests) | Apache-2.0 | program (Python) | HTTP |
| [keyring](https://github.com/jaraco/keyring) | MIT | program (Python) | heslá v systémovej kľúčenke |
| [pywebview](https://github.com/r0x0r/pywebview) | BSD-3-Clause | program (Python) | okno aplikácie s novým rozhraním |
| [Svelte](https://svelte.dev) | MIT | rozhranie (preložené do `kometa/webui`) | komponenty rozhrania |
| [Vite](https://vite.dev), [TypeScript](https://www.typescriptlang.org) | MIT, Apache-2.0 | len zostavenie rozhrania | preklad rozhrania |
| [Atkinson Hyperlegible Next](https://www.brailleinstitute.org/freefont/) | SIL OFL 1.1 | rozhranie | písmo textu |
| [Bricolage Grotesque](https://github.com/ateliertriay/bricolage) | SIL OFL 1.1 | rozhranie | písmo nadpisov |
| Rust: tokio, reqwest, rustls, roxmltree, serde, clap, thiserror | MIT / Apache-2.0 | jadro v Ruste | asynchrónny beh, HTTP, XML, CLI |

Pravidlá pre nové súčasti: len slobodné licencie, ktoré dovoľujú použitie v projekte pod MIT;
súčasť musí byť nahraditeľná a zapísaná v tejto tabuľke. Modely a dátové sady pre AI (titulky,
preklad, hlasy) sa sem zapíšu tiež, s licenciou dát – pozri [plan-ai.md](plan-ai.md).
