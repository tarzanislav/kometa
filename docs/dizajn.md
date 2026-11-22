# Dizajn rozhrania Komety

Rozhranie je napísané raz – v HTML, CSS a TypeScripte (Svelte 5) v priečinku [`ui/`](../ui) – a beží
v okne aplikácie: dnes nad jadrom v Pythone (pywebview), neskôr v aplikácii Tauri nad jadrom v Ruste
(Windows, macOS, Linux, Android). S jadrom sa rozpráva len cez malé API (`ui/src/lib/api.ts` ↔
`kometa/webapp.py`), takže výmena jadra rozhranie nezmení.

## Myšlienka

Kometa je nástroj, ktorý pracuje, kým sa človek venuje inému. Rozhranie je preto tiché a prehľadné
a jediný výrazný prvok je **kométa**: priebeh sťahovania je svietiace jadro s chvostom, ktorý sa
predlžuje. Kométa je aj v logu, v prázdnom fronte a v malých náhľadoch tém. Všetko ostatné je
striedme – plochy oddelené jemným rozdielom farby, nie tieňmi; tieň majú len okná a upozornenia.

## Farby: 7 tém × deň a noc

Každá téma je päť čísel v priestore OKLCH (`ui/src/styles/tokeny.css`):

| číslo | význam |
|---|---|
| `--h`, `--c` | odtieň a sýtosť neutrálnych farieb (pozadie, text, čiary) |
| `--ha`, `--ca` | odtieň a sýtosť akcentu (tlačidlá, výber, chvost kométy) |
| `--hb` | odtieň jadra kométy |

Zvyšok (pozadia, text, čiary, akcent, stavy) sa z nich počíta. Deň aj noc majú preto v každej
téme rovnaký jas a kontrast, mení sa len tón. Nová téma = nový riadok s piatimi číslami.

| téma | nálada |
|---|---|
| Kometa (predvolená) | chladná obloha, zelená žiara kométy (kóma komét svieti zeleno) |
| Polárna žiara | fialový nádych, tyrkys a ružová |
| Súmrak | teplé tóny, malinová |
| Les | machová zelená |
| More | hlboká modrá |
| Levanduľa | pokojná fialová |
| Grafit | čisté sivé tóny a indigová |

Režim: **podľa systému** (predvolené), **deň** alebo **noc** – v Nastaveniach aj jedným klikom
v bočnom paneli.

## Písmo

- **Atkinson Hyperlegible Next** – text rozhrania. Navrhnuté pre čo najlepšiu čitateľnosť
  (odlíšené 0/O, 1/l/I), s plnou slovenskou diakritikou.
- **Bricolage Grotesque** – nadpisy a veľké čísla (rýchlosť). Variabilné písmo s optickou veľkosťou.

Obe písma sú pod licenciou SIL OFL 1.1 a sú súčasťou programu (nič sa nesťahuje z internetu).
Stupnica: 12 / 13 / 14,5 / 16,5 / 20 / 27 / 38 px. Čísla, ktoré sa menia (rýchlosť, veľkosť,
čas), majú tabuľkové číslice, aby neposkakovali.

## Zásady

- **Bez skrytého zníženia kvality.** Ak sa zvuk prekóduje (Opus → AAC), rozhranie to pri jazyku
  povie („AAC prevodom“); ak sa len prebalí, povie „AAC zo zdroja“.
- **Chyba je pokyn, nie nálada.** Hlásenia hovoria, čo sa stalo a čo urobiť („Obsah vyžaduje
  prihlásenie – na stránke Účty vyber prehliadač…“); technické podrobnosti sú o klik nižšie.
- **Prázdne miesto je pozvánka.** Prázdny front ukáže, čo sem patrí, a dá sa doň pretiahnuť odkaz.
- **Prístupnosť.** Celé rozhranie sa dá ovládať klávesnicou (viditeľný fokus, šípky v prepínačoch,
  Esc zatvára okná), čítačky obrazovky dostanú názvy ovládačov aj priebeh, animácie rešpektujú
  „obmedziť pohyb“ v systéme.
- **Úzke okno.** Pod 900 px sa bočný panel zmení na lištu ikon – základ pre mobilnú verziu.

## Skratky

| | |
|---|---|
| `Ctrl+V` kdekoľvek | vloží odkaz do sťahovania |
| `Enter` v poli odkazov | stiahnuť (`Shift+Enter` = nový riadok) |
| `Ctrl+1` … `Ctrl+4` | Sťahovanie, Jazykové stopy, Účty, Nastavenia |
| pretiahnutie odkazu z prehliadača | pridá ho do sťahovania |
