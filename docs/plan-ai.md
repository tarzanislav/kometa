# Plán: lokálna AI v Komete

Všetko beží v počítači používateľa – bez účtu, bez internetu, bez kreditov. Každý výstup AI je
v súbore označený ako strojový (názov stopy, napr. „Čeština – strojový preklad“) a pôvodné stopy
ostávajú.

## 1. Titulky a preklad (prvý krok)

1. **Reč → text:** lokálny model na prepis reči (rovnaký druh ako whisper.cpp v projekte Universum);
   rozpozná jazyk a vytvorí titulky s časmi.
2. **Text → iný jazyk:** lokálny prekladový model → preložené titulky (napr. RU → CZ/SK).
3. Titulky sa vložia do MKV/MP4 ako ďalšia stopa s jazykom.

## 2. AI dabing (špeciálna kategória)

- **Oddelenie hlasu od pozadia:** hudba a zvuky ostanú, nový hlas sa položí na ne.
- **Šablóny hlasov:** ženské a mužské, mladší, stredný a starší vek; každá s menom a ukážkou.
  Pri viacerých hovoriacich Kometa rozlíši hovoriacich a používateľ každému priradí šablónu.
- **Prirodzený prednes** – najdôležitejšia časť:
  - správne čítanie čísel, dátumov, skratiek a mien (slovenčina, čeština, maďarčina, poľština);
  - prízvuk a melódia vety podľa interpunkcie a zmyslu (otázka, oznámenie, napätie);
  - tempo a pauzy podľa originálu; keď sa veta nezmestí do času, skráti sa preklad, nezrýchli hlas;
  - hlasitosť a dôraz podľa pôvodného hovoriaceho.
- **Učenie prirodzeného prednesu** z nahrávok rozprávačov (audioknihy, staré rozprávky):
  učí sa len *spôsob rozprávania* (tempo, pauzy, melódia, dôraz) a vždy z viacerých rozprávačov
  naraz – nikdy farba hlasu konkrétneho človeka.

### Pravidlá

| | |
|---|---|
| Hlasy | syntetické; farba hlasu len z nahrávok s jasnou licenciou (CC0) alebo od dospelých dobrovoľníkov so súhlasom |
| Kontrola podobnosti | každá šablóna sa automaticky porovná so zdrojovými hlasmi (overenie hovoriaceho); ak sa niekomu priveľmi podobá, zamietne sa |
| Súkromné nahrávky | nahrávky hercov a model prednesu naučený z nich ostávajú len v počítači používateľa – nejdú na GitHub ani do vydania |
| Verejné šablóny | len z voľne licencovaných dát, zapísané v [zdroje.md](zdroje.md) |
| Označenie | každá AI stopa je označená; pri zverejnení na YouTube treba v YouTube Studio priznať syntetický obsah |
| Obsah | prekladať a dabovať vlastné videá, voľné diela alebo diela so súhlasom autora; cudzie videá nie |

### Voľne použiteľné hlasové dáta (stav november 2026)

| zdroj | jazyky | licencia | poznámka |
|---|---|---|---|
| Mozilla Common Voice | SK, CZ, HU, PL a ďalšie | CC0 | tisíce dobrovoľníkov; voliteľný vek a pohlavie – základ pre šablóny podľa veku a pohlavia; nesmie sa zisťovať totožnosť hovoriacich |
| Hlasy Piper | SK (lili), CZ, HU, PL | podľa MODEL_CARD každého hlasu | lili: nahrávky CC0; ostatné overiť jednotlivo |
| FLEURS (Google) | aj SK, CZ, HU, PL | CC BY 4.0 | treba uviesť zdroj |
| VoxPopuli | SK, CZ, HU, PL | CC BY-NC 4.0 | len nekomerčne – nie do verejných šablón Komety |

## 3. Ďalej

- vyhľadávanie vo videách podľa obsahu (čo sa kde hovorí), kapitoly a zhrnutia;
- pomocník pri chybe sťahovania;
- výpočtový motor z projektu Orchidea ako možný pohon lokálnej AI (kvantované modely, procesor aj
  grafická karta, rovnaké výsledky na všetkých zariadeniach).

## Podnety z projektu Orchidea

| zásada (bežnými slovami) | v Komete |
|---|---|
| žiadne skryté zníženie kvality | rozhranie hovorí, kedy sa zvuk prekóduje a kedy len prebalí |
| neurobiť prácu, ktorá nemení výsledok | prebalenie bez prekódovania, AAC zo zdroja, keď existuje, pokračovanie sťahovania |
| odtlačok obsahu (BLAKE3) | jadro v Ruste: overenie úsekov, duplicity vo fronte |
| beh aj na slabých zariadeniach (Raspberry Pi) | jadro na RPi doma, ovládanie z počítača či mobilu cez rozhranie; bezpečné spárovanie zariadení |
| register cudzích súčastí | [zdroje.md](zdroje.md) |
