"""Sťahovanie súborov cez HTTP s pokračovaním.

- rozpracovaný súbor sa ukladá ako  názov.part  a po prerušení (pauza, výpadok
  siete, zatvorenie programu) sa pokračuje od miesta, kde sa skončilo (HTTP Range),
- pri výpadku spojenia sa automaticky skúša znova,
- keď odkaz expiruje (Webshare/Fastshare dávajú dočasné odkazy), vypýta si nový.
"""

from __future__ import annotations

import re
import threading
import time
from pathlib import Path
from urllib.parse import unquote, urlparse

import requests

from .common import USER_AGENT, Paused, unique_path

CHUNK = 256 * 1024


class DownloadError(Exception):
    pass


def part_path(dest: Path) -> Path:
    return dest.with_name(dest.name + ".part")


def filename_from_response(headers, url: str) -> str | None:
    cd = headers.get("Content-Disposition", "") or ""
    m = re.search(r"filename\*\s*=\s*[\w-]*''([^;]+)", cd, re.I)
    if m:
        return unquote(m.group(1).strip().strip('"'))
    m = re.search(r'filename\s*=\s*"([^"]+)"', cd, re.I) or re.search(r"filename\s*=\s*([^;]+)", cd, re.I)
    if m:
        name = m.group(1).strip()
        try:  # requests dekóduje hlavičky ako latin-1, servery často posielajú UTF-8
            name = name.encode("latin-1").decode("utf-8")
        except (UnicodeEncodeError, UnicodeDecodeError):
            pass
        return name
    name = unquote(Path(urlparse(url).path).name)
    return name or None


def _total_from_range(value: str | None) -> int | None:
    m = re.search(r"/(\d+)\s*$", value or "")
    return int(m.group(1)) if m else None


def _sleep(stop: threading.Event, seconds: float) -> None:
    if stop.wait(seconds):
        raise Paused()


def download(get_url, dest: Path, session: requests.Session | None = None, *, on_progress=None,
             stop: threading.Event | None = None, expected_size: int | None = None,
             max_retries: int = 12, log=lambda m: None) -> Path:
    """Stiahne súbor do `dest` s pokračovaním.

    get_url(refresh: bool) -> str   vráti odkaz; refresh=True znamená „starý odkaz prestal fungovať“.
    on_progress(stiahnuté_bajty, celkom_alebo_None, rýchlosť_B/s)
    """
    session = session or requests.Session()
    stop = stop or threading.Event()
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = part_path(dest)
    url = get_url(False)
    total = expected_size
    failures = refreshes = 0
    last_emit = 0.0

    while True:
        if stop.is_set():
            raise Paused()
        pos = part.stat().st_size if part.exists() else 0
        if total and pos >= total:
            break
        headers = {"User-Agent": USER_AGENT, "Accept-Encoding": "identity"}
        if pos:
            headers["Range"] = f"bytes={pos}-"
        try:
            r = session.get(url, headers=headers, stream=True, timeout=(20, 60), allow_redirects=True)
        except requests.RequestException as e:
            failures += 1
            if failures > max_retries:
                raise DownloadError(f"Spojenie zlyhalo: {e}") from e
            log(f"↻ Spojenie zlyhalo, skúšam znova ({failures}/{max_retries})…")
            _sleep(stop, min(2 ** failures, 30))
            continue

        with r:
            if r.status_code == 416:  # požadovaný rozsah je mimo súboru
                size = _total_from_range(r.headers.get("Content-Range"))
                if size is not None and pos >= size:
                    total = size
                    break
                part.unlink(missing_ok=True)
                continue
            if r.status_code in (401, 403, 404, 410) and refreshes < 3:
                refreshes += 1
                log("↻ Odkaz vypršal, žiadam nový…")
                url = get_url(True)
                continue
            if r.status_code >= 500 or r.status_code == 429:
                failures += 1
                if failures > max_retries:
                    raise DownloadError(f"Server je nedostupný (chyba {r.status_code}).")
                log(f"↻ Server odpovedal {r.status_code}, skúšam znova…")
                _sleep(stop, min(2 ** failures, 60))
                continue
            if r.status_code >= 400:
                raise DownloadError(f"Server vrátil chybu {r.status_code}.")
            if "text/html" in (r.headers.get("Content-Type") or "").lower():
                raise DownloadError("Server poslal webovú stránku namiesto súboru – "
                                    "skontroluj prihlásenie, kredit alebo platnosť odkazu.")

            if r.status_code == 206:
                size = _total_from_range(r.headers.get("Content-Range"))
                total = size or total
                mode = "ab"
            else:  # 200 – server nepodporuje pokračovanie alebo začíname od nuly
                if pos:
                    log("⚠ Server nepodporuje pokračovanie, sťahujem od začiatku.")
                pos = 0
                mode = "wb"
                length = r.headers.get("Content-Length")
                total = int(length) if length and length.isdigit() else total

            got = 0
            window = [(time.monotonic(), pos)]
            try:
                with open(part, mode) as f:
                    for chunk in r.iter_content(CHUNK):
                        if stop.is_set():
                            raise Paused()
                        if not chunk:
                            continue
                        f.write(chunk)
                        pos += len(chunk)
                        got += len(chunk)
                        now = time.monotonic()
                        window.append((now, pos))
                        while len(window) > 2 and now - window[0][0] > 5:
                            window.pop(0)
                        if on_progress and now - last_emit >= 0.25:
                            dt = max(now - window[0][0], 1e-3)
                            on_progress(pos, total, (pos - window[0][1]) / dt)
                            last_emit = now
            except (requests.RequestException, ConnectionError, TimeoutError) as e:
                failures += 1
                if failures > max_retries:
                    raise DownloadError(f"Sťahovanie sa opakovane prerušilo: {e}") from e
                log(f"↻ Spojenie sa prerušilo pri {pos // (1024 * 1024)} MB, pokračujem…")
                _sleep(stop, min(2 ** failures, 30))
                continue
            if got:
                failures = 0

        if total is None or pos >= total:
            break
        # server ukončil prenos predčasne → ďalšie kolo pokračuje od aktuálnej pozície

    if on_progress:
        on_progress(pos, total or pos, 0)
    final = dest if not dest.exists() else unique_path(dest)
    part.replace(final)
    return final


def probe(url: str, session: requests.Session | None = None) -> dict:
    """Zistí názov a veľkosť súboru na priamom odkaze (bez sťahovania)."""
    session = session or requests.Session()
    with session.get(url, headers={"User-Agent": USER_AGENT, "Range": "bytes=0-0"},
                     stream=True, timeout=(20, 30), allow_redirects=True) as r:
        r.raise_for_status()
        size = _total_from_range(r.headers.get("Content-Range"))
        if size is None and r.headers.get("Content-Length", "").isdigit():
            size = int(r.headers["Content-Length"])
        return {"name": filename_from_response(r.headers, r.url), "size": size,
                "html": "text/html" in (r.headers.get("Content-Type") or "").lower()}
