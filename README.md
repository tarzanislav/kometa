<p align="center"><img src="assets/kometa_256.png" width="96" alt="Kometa"></p>

<h1 align="center">Kometa</h1>

<p align="center">Sťahovanie videí a súborov s pokračovaním prerušeného sťahovania<br>
YouTube · Facebook · Instagram · TikTok (bez vodoznaku) · Webshare · Fastshare</p>

![Sťahovanie](docs/stahovanie.png)

## Čo Kometa vie

- **Sťahuje z YouTube, Facebooku, Instagramu, TikToku** a z ďalších viac ako 1000 stránok, ktoré podporuje [yt-dlp](https://github.com/yt-dlp/yt-dlp), aj z priamych odkazov na súbory.
- **TikTok bez vodoznaku.** Verzie s vodoznakom sa vynechávajú.
- **Webshare.cz a Fastshare.cz** cez tvoj vlastný účet (Webshare cez oficiálne API).
- **Pokračovanie prerušeného sťahovania.** Pri pauze, výpadku siete aj po zatvorení programu sa pokračuje od miesta, kde sa skončilo. Keď odkaz z úložiska vyprší, Kometa si vypýta nový.
- **Front sťahovania**: viac súborov naraz, pauza a pokračovanie, front ostane uložený aj po reštarte.
- **Jazykové stopy z YouTube**: z videí s preloženým zvukom stiahne vybrané jazyky a spojí ich s videom do jedného `.mkv`. Predvolené poradie je CZ, SK, EN, EN-US, PL, HU a pôvodná stopa ide na koniec.
- Formáty: najlepšia kvalita, MP4 pre TV a mobil, najviac 1080p alebo 720p, MP3, zvuk v pôvodnej kvalite.

| Jazykové stopy | Účty a nastavenia |
|---|---|
| ![Jazyky](docs/jazyky.png) | ![Účty](docs/ucty.png) |

## Inštalácia (Windows 10/11)

1. Stiahni Kometu: zelené tlačidlo **Code → Download ZIP** a rozbaľ ju (napr. do `Dokumenty\Kometa`).
2. Dvakrát klikni na **`Instalacia.bat`**. Cez `winget` nainštaluje Python, FFmpeg, Deno a knižnice (yt-dlp, requests, keyring) a na plochu dá ikonu **Kometa**.
   - Ak na konci napíše, že ho treba spustiť ešte raz (pri prvej inštalácii Pythonu), zavri okno a spusti ho znova.
   - Ak sa objaví „Windows ochránil váš počítač“, klikni na **Ďalšie informácie → Spustiť aj tak**.
3. Kometu spúšťaj ikonou na ploche alebo cez **`Spustit.bat`**. Pri každom spustení sa aktualizuje yt-dlp, pretože stránky sa často menia.

<details>
<summary>Ručná inštalácia / iný systém</summary>

Treba Python 3.10+, [FFmpeg](https://ffmpeg.org) a [Deno](https://deno.com) (YouTube ho dnes vyžaduje).

```
pip install -U -r requirements.txt
python app.py
```
</details>

## Používanie

### Karta Sťahovanie
1. Vlož jeden alebo viac odkazov (každý na nový riadok) alebo klikni na **Vložiť zo schránky**.
2. Vyber formát a priečinok a klikni na **Stiahnuť**.
3. Vo fronte vidíš priebeh, rýchlosť a zostávajúci čas.
   - **Dvojklik:** hotové otvorí priečinok so súborom, rozbehnuté pozastaví, pozastavené spustí, pri chybe ukáže podrobnosti.
   - **Pravé tlačidlo:** ďalšie možnosti (kopírovať odkaz, odstrániť…).

### Karta Jazykové stopy (YouTube)
Pre videá, ktoré majú na YouTube viac zvukových stôp (preklad či dabing). Vlož odkaz, klikni na **Načítať jazyky** a zaškrtni jazyky. Pod zoznamom vidíš výsledné poradie stôp. Vznikne jedno `.mkv` video; jazyk prepneš v prehrávači (VLC: *Zvuk → Zvuková stopa*). Stopy sa dajú pridať aj k videu, ktoré už máš stiahnuté.

### Karta Účty a nastavenia
- **Webshare / Fastshare:** zadaj meno a heslo a klikni na **Prihlásiť**. Heslo sa uloží do *Správcu poverení Windows*, nikam inam sa neposiela. Na rýchle sťahovanie treba **VIP (Webshare)** alebo **kredit (Fastshare)**. Kometa neobchádza obmedzenia bezplatných účtov.
- **Instagram / Facebook:** súkromné príspevky a príspevky len pre prihlásených potrebujú prihlásenie. Prihlás sa v prehliadači a tu ho vyber. Najspoľahlivejší je **Firefox**; Chrome a Edge treba pri sťahovaní zavrieť.
- Počet súčasných sťahovaní a automatické pokračovanie po spustení.

## Ako funguje pokračovanie sťahovania

Rozpracovaný súbor sa ukladá ako `názov.part`. Pri ďalšom pokuse sa server požiada len o zvyšok (HTTP Range), takže sa nesťahuje od začiatku. Pri výpadku siete Kometa skúša znova (až 12-krát, s narastajúcim čakaním). Keď dočasný odkaz z Webshare alebo Fastshare vyprší, vypýta si nový a pokračuje. Front sa ukladá do `%USERPROFILE%\.kometa\fronta.json`, takže po zatvorení programu sa nedokončené súbory po ďalšom spustení samy dokončia.

## Príkazový riadok

```
py app.py ODKAZ [ODKAZ …] [-f video|mp4|1080|720|mp3|audio] [-o PRIEČINOK]
py app.py jazyky ODKAZ                         # video + CZ, SK, EN, EN-US, PL, HU → .mkv
py app.py jazyky ODKAZ --zoznam                # vypíše dostupné jazyky
py app.py jazyky ODKAZ -j cs sk en -o "D:\Videa"
py app.py jazyky ODKAZ --k-videu "moje.mp4"    # pridá stopy k stiahnutému videu
```

## Keď niečo nejde

- **YouTube / TikTok zrazu nefunguje:** spusti Kometu znova cez `Spustit.bat` (aktualizuje yt-dlp).
- **„Obsah vyžaduje prihlásenie“:** v karte *Účty a nastavenia* vyber prehliadač, v ktorom si prihlásený.
- **Webshare / Fastshare hlási chybu účtu:** v karte *Účty a nastavenia* sa prihlás znova a skontroluj VIP alebo kredit.
- **„Chýba FFmpeg / Deno“:** spusti `Instalacia.bat`.

## Štruktúra projektu

```
app.py               spúšťač (okno aj príkazový riadok)
kometa/
  gui.py             okno s kartami
  fronta.py          front sťahovania, pauza/pokračovanie, uloženie
  httpdl.py          HTTP sťahovanie s pokračovaním (.part + Range)
  hosts.py           Webshare (API) a Fastshare (prihlásenie, stránka súboru)
  media.py           YouTube/Facebook/Instagram/TikTok cez yt-dlp
  jazyky.py          jazykové stopy YouTube a spájanie cez FFmpeg
  common.py          nastavenia, heslá, pomôcky
```

## Upozornenie

Kometa je určená na súkromné použitie. Sťahuj len obsah, na ktorý máš právo, a rešpektuj podmienky používania jednotlivých služieb.

## Licencia

[MIT](LICENSE). Kometa stojí na projektoch [yt-dlp](https://github.com/yt-dlp/yt-dlp) a [FFmpeg](https://ffmpeg.org).
