"""Automatická aktualizácia Kometa pre Python z vydaní na GitHube.

Rovnaký postup ako v aplikácii: po spustení (a potom raz za 12 hodín) sa pozrie na posledné vydanie,
novšie `Kometa-X.zip` stiahne a overí súčtom SHA-256 zo `SHA256SUMS.txt` (aj odtlačkom, ktorý
uvádza GitHub). Nainštaluje ho, keď to potvrdíš, inak sám pri ďalšom spustení – len raz.
Inštalácia = súbory programu sa prepíšu novými (nastavenia a front sú v ~/.kometa, tie ostanú).
Program spustený z kópie repozitára (.git) sa nikdy neprepisuje.
"""

from __future__ import annotations

import hashlib
import json
import threading
import time
import zipfile
from pathlib import Path

from . import __version__
from .common import CONFIG_DIR

API = "https://api.github.com/repos/tarzanislav/kometa/releases/latest"
ROOT = Path(__file__).resolve().parent.parent          # priečinok programu (app.py)
DIR = CONFIG_DIR / "aktualizacia"

STATE: dict = {"verzia": __version__, "balik": "rucne" if (ROOT / ".git").exists() else "zip",
               "najnovsia": None, "stranka": None, "stahuje": False, "pripravena": False, "pri_starte": False,
               "instaluje": False, "nainstalovana": False, "kontrola": None, "chyba": None}
_lock = threading.Lock()


def newer(a: str, b: str) -> bool:
    """Je verzia a novšia ako b? (čísla oddelené bodkami)"""
    def nums(v: str) -> list[int]:
        out = []
        for p in v.strip().lstrip("vV").replace("-", ".").replace("+", ".").split("."):
            if not p.isdigit():
                break
            out.append(int(p))
        return out
    x, y = nums(a), nums(b)
    n = max(len(x), len(y))
    return (x + [0] * (n - len(x))) > (y + [0] * (n - len(y)))


def sum_from_list(text: str, name: str) -> str | None:
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[1].lstrip("*") == name and len(parts[0]) == 64:
            return parts[0].lower()
    return None


def _sha256(p: Path) -> str | None:
    try:
        h = hashlib.sha256()
        with open(p, "rb") as f:
            for chunk in iter(lambda: f.read(1 << 16), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def _load() -> dict | None:
    try:
        return json.loads((DIR / "caka.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def _save(c: dict) -> None:
    DIR.mkdir(parents=True, exist_ok=True)
    (DIR / "caka.json.tmp").write_text(json.dumps(c, ensure_ascii=False), encoding="utf-8")
    (DIR / "caka.json.tmp").replace(DIR / "caka.json")


def _cleanup() -> None:
    import shutil
    shutil.rmtree(DIR, ignore_errors=True)


def status() -> dict:
    with _lock:
        return dict(STATE)


def _extract(zip_path: Path) -> int:
    """Prepíše súbory programu obsahom ZIP (predpona „Kometa/“); nič mimo priečinka programu."""
    root = ROOT.resolve()
    n = 0
    with zipfile.ZipFile(zip_path) as z:
        for m in z.infolist():
            name = m.filename.split("/", 1)[1] if m.filename.startswith("Kometa/") else m.filename
            if not name or m.is_dir():
                continue
            dest = (root / name).resolve()
            if root not in dest.parents:
                continue                               # zip-slip: mimo priečinka programu nič
            dest.parent.mkdir(parents=True, exist_ok=True)
            tmp = dest.with_name(dest.name + ".nova")
            with z.open(m) as src, open(tmp, "wb") as out:
                out.write(src.read())
            tmp.replace(dest)
            n += 1
    return n


def apply_pending(log=print) -> bool:
    """Pri spustení: čakajúcu aktualizáciu nainštaluje (raz), dokončenú uprace. Vráti, či inštalovala."""
    c = _load()
    if not c:
        return False
    if not newer(c.get("verzia", ""), __version__):
        _cleanup()
        return False
    z = DIR / c.get("subor", "")
    if _sha256(z) != c.get("sha256"):
        _cleanup()
        return False
    with _lock:
        STATE.update(najnovsia=c["verzia"], pripravena=True, pri_starte=bool(c.get("pri_starte")))
    if not c.get("pri_starte") or c.get("pokusy", 0) > 0 or STATE["balik"] != "zip":
        if c.get("pokusy", 0) > 0:
            with _lock:
                STATE["chyba"] = f"Inštalácia verzie {c['verzia']} sa minule nedokončila – skús „Nainštalovať teraz“."
        return False
    return install(log) is not None


def install(log=print) -> dict | None:
    """Nainštaluje pripravenú verziu (prepíše súbory programu). Platí od ďalšieho spustenia."""
    c = _load()
    if not c or STATE["balik"] != "zip":
        return None
    z = DIR / c["subor"]
    if _sha256(z) != c["sha256"]:
        _cleanup()
        with _lock:
            STATE.update(pripravena=False, chyba="Súbor aktualizácie sa zmenil alebo poškodil – stiahne sa znova.")
        return status()
    c["pokusy"] = c.get("pokusy", 0) + 1
    _save(c)
    with _lock:
        STATE.update(instaluje=True, chyba=None)
    try:
        n = _extract(z)
    except Exception as e:  # noqa: BLE001
        with _lock:
            STATE.update(instaluje=False, chyba=f"Inštalácia sa nepodarila: {e}")
        return status()
    log(f"✔ Kometa {c['verzia']} je nainštalovaná ({n} súborov) – platí od ďalšieho spustenia.")
    with _lock:
        STATE.update(instaluje=False, nainstalovana=True)
    return status()


def later() -> dict:
    c = _load()
    if c:
        c["pri_starte"], c["pokusy"] = True, 0
        _save(c)
        with _lock:
            STATE.update(pri_starte=True, chyba=None)
    return status()


def check(auto: bool = True, log=print) -> dict:
    """Pozrie posledné vydanie; novší ZIP stiahne a overí. auto = nainštalovať pri ďalšom spustení."""
    import os

    import requests
    with _lock:
        if STATE["stahuje"] or STATE["instaluje"]:
            return dict(STATE)
        STATE.update(stahuje=True, chyba=None)
    try:
        api = os.environ.get("KOMETA_AKTUALIZACIE_ZAKLAD") or API
        h = {"Accept": "application/vnd.github+json", "User-Agent": "Kometa"}
        info = requests.get(api, headers=h, timeout=30).json()
        ver = str(info.get("tag_name") or "").lstrip("vV")
        if not ver or not newer(ver, __version__):
            with _lock:
                STATE["najnovsia"] = None
            return status()
        with _lock:
            STATE.update(najnovsia=ver, stranka=info.get("html_url"))
        if STATE["balik"] != "zip":
            return status()
        name = f"Kometa-{ver}.zip"
        c = _load()
        if c and c.get("verzia") == ver and _sha256(DIR / c["subor"]) == c["sha256"]:
            with _lock:
                STATE.update(pripravena=True, pri_starte=bool(c.get("pri_starte")))
            return status()
        assets = {a.get("name"): a for a in info.get("assets") or []}
        if name not in assets or "SHA256SUMS.txt" not in assets:
            raise RuntimeError(f"vo vydaní chýba {name} alebo SHA256SUMS.txt")
        sums = requests.get(assets["SHA256SUMS.txt"]["browser_download_url"], headers=h, timeout=30).text
        if ver not in sums.splitlines()[0]:
            raise RuntimeError("zoznam súčtov nepatrí k tejto verzii")
        expected = sum_from_list(sums, name)
        digest = str(assets[name].get("digest") or "")
        if not expected or (digest.startswith("sha256:") and digest[7:].lower() != expected):
            raise RuntimeError("súčet SHA-256 chýba alebo nesedí")
        _cleanup()
        DIR.mkdir(parents=True, exist_ok=True)
        part, hh, size = DIR / (name + ".part"), hashlib.sha256(), 0
        with requests.get(assets[name]["browser_download_url"], headers=h, timeout=60, stream=True) as r:
            r.raise_for_status()
            with open(part, "wb") as f:
                for chunk in r.iter_content(1 << 16):
                    size += len(chunk)
                    if size > 200 << 20:
                        raise RuntimeError("súbor aktualizácie je nezvyčajne veľký")
                    hh.update(chunk)
                    f.write(chunk)
        if hh.hexdigest() != expected:
            part.unlink(missing_ok=True)
            raise RuntimeError("stiahnutý súbor nesedí so súčtom SHA-256 – nepoužijem ho")
        part.replace(DIR / name)
        _save({"verzia": ver, "subor": name, "sha256": expected, "pri_starte": auto, "pokusy": 0})
        log(f"⬆ Pripravená nová verzia Komety {ver} – nainštaluješ ju hneď, alebo sa nainštaluje pri ďalšom spustení.")
        with _lock:
            STATE.update(pripravena=True, pri_starte=auto)
    except Exception as e:  # noqa: BLE001
        with _lock:
            STATE["chyba"] = f"Aktualizáciu sa nepodarilo overiť: {e}"
    finally:
        with _lock:
            STATE.update(stahuje=False, kontrola=str(int(time.time())))
    return status()


def start(enabled, log=print) -> None:
    """Kontrola na pozadí: o chvíľu po spustení a potom raz za 12 hodín (ak je zapnutá)."""
    import os

    def loop():
        time.sleep(0.3 if os.environ.get("KOMETA_AKTUALIZACIE_ZAKLAD") else 8)
        while True:
            if enabled():
                check(True, log)
            time.sleep(12 * 3600)
    threading.Thread(target=loop, daemon=True).start()
