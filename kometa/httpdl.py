"""Sťahovanie súborov cez HTTP s pokračovaním.

- rozpracovaný súbor sa ukladá ako  názov.part  a po prerušení (pauza, výpadok
  siete, zatvorenie programu) sa pokračuje od miesta, kde sa skončilo (HTTP Range),
- pri výpadku spojenia sa automaticky skúša znova,
- keď odkaz expiruje (Webshare/Fastshare dávajú dočasné odkazy), vypýta si nový.
"""

from __future__ import annotations

import json
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


# --------------------------------------------------------------------------- #
#  Sťahovanie cez viac spojení naraz (segmenty) s pokračovaním
# --------------------------------------------------------------------------- #

MIN_SEGMENT = 1024 * 1024          # menšie kúsky sa už nedelia
MIN_SEGMENTED_SIZE = 8 * 1024 * 1024


def state_path(dest: Path) -> Path:
    return dest.with_name(dest.name + ".kometa")


class _NoRange(Exception):
    """Server ignoruje Range – segmentové sťahovanie nie je možné."""


class _Segmented:
    """Súbor sa rozdelí na úseky, každý ťahá vlastné spojenie a zapisuje na svoje miesto.

    - keď spojenie dokončí svoj úsek, „ukradne“ polovicu najväčšieho zvyšného úseku,
      takže všetky spojenia pracujú až do konca (žiadne čakanie na posledného),
    - keď server ďalšie spojenia odmieta (429/503 alebo stránka namiesto dát),
      počet spojení sa automaticky zníži (adaptívne),
    - stav úsekov sa priebežne ukladá do  súbor.kometa  → po reštarte sa pokračuje.
    """

    def __init__(self, get_url, dest: Path, session, total: int, connections: int, on_progress, stop, log,
                 max_retries: int):
        self.get_url, self.dest, self.session, self.total = get_url, dest, session, total
        self.on_progress, self.stop, self.log, self.max_retries = on_progress, stop, log, max_retries
        self.part, self.state_file = part_path(dest), state_path(dest)
        self.lock = threading.Lock()
        self.url = get_url(False)
        self.url_gen = 0
        self.last_refresh = 0.0
        self.limit = max(1, connections)
        self.active = 0
        self.errors: list[str] = []
        self.segs: list[list[int]] = self._load_state() or self._initial(connections)
        self.owner: dict[int, int] = {}   # index úseku → id vlákna
        self.halt = threading.Event()     # interné zastavenie (napr. server nepodporuje Range)

    def halted(self) -> bool:
        return self.stop.is_set() or self.halt.is_set()

    def _wait(self, seconds: float) -> bool:
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if self.halted():
                return True
            time.sleep(0.1)
        return self.halted()

    # ---------- stav ----------
    def _initial(self, n: int) -> list[list[int]]:
        n = max(1, min(n, self.total // MIN_SEGMENT or 1))
        step = self.total // n
        segs = [[i * step, (i + 1) * step - 1, 0] for i in range(n)]
        segs[-1][1] = self.total - 1
        return segs

    def _load_state(self):
        try:
            data = json.loads(self.state_file.read_text(encoding="utf-8"))
            if data.get("total") == self.total and self.part.exists() and self.part.stat().st_size == self.total:
                return [list(map(int, s)) for s in data["segs"]]
        except (OSError, ValueError, KeyError, TypeError):
            pass
        return None

    def save_state(self):
        with self.lock:
            data = {"total": self.total, "segs": [list(s) for s in self.segs]}
        try:
            tmp = self.state_file.with_suffix(".tmp")
            tmp.write_text(json.dumps(data), encoding="utf-8")
            tmp.replace(self.state_file)
        except OSError:
            pass

    def done_bytes(self) -> int:
        with self.lock:
            return sum(s[2] for s in self.segs)

    # ---------- prideľovanie úsekov ----------
    def _take(self, wid: int) -> int | None:
        with self.lock:
            if self.active > self.limit:
                return None
            for i, s in enumerate(self.segs):
                if i not in self.owner and s[0] + s[2] <= s[1]:
                    self.owner[i] = wid
                    return i
            # nič voľné → rozdeľ najväčší zvyšok iného spojenia na polovicu
            best, rest = None, 0
            for i, s in enumerate(self.segs):
                r = s[1] - (s[0] + s[2]) + 1
                if r > rest:
                    best, rest = i, r
            if best is None or rest < 2 * MIN_SEGMENT:
                return None
            s = self.segs[best]
            mid = s[0] + s[2] + rest // 2
            self.segs.append([mid, s[1], 0])
            s[1] = mid - 1
            j = len(self.segs) - 1
            self.owner[j] = wid
            return j

    def _release(self, i: int):
        with self.lock:
            self.owner.pop(i, None)

    def _refresh(self, gen: int):
        with self.lock:
            if gen != self.url_gen:        # iné spojenie už odkaz obnovilo
                return
            if time.monotonic() - self.last_refresh < 5:
                # odkaz bol práve obnovený a server stále odmieta → priveľa spojení
                self.limit = max(1, self.limit - 1)
                return
            self.last_refresh = time.monotonic()
        self.log("↻ Odkaz vypršal, žiadam nový…")
        url = self.get_url(True)
        with self.lock:
            self.url, self.url_gen = url, self.url_gen + 1

    # ---------- jedno spojenie ----------
    def worker(self, wid: int):
        failures = 0
        with self.lock:
            self.active += 1
        try:
            fh = open(self.part, "r+b")
        except OSError as e:
            self.errors.append(str(e))
            return
        try:
            while not self.halted():
                i = self._take(wid)
                if i is None:
                    return
                try:
                    self._fetch_segment(i, fh)
                    failures = 0
                except _NoRange:
                    raise
                except _TooMany:
                    with self.lock:
                        self.limit = max(1, min(self.limit, self.active - 1))
                    self.log(f"⚠ Server obmedzuje počet spojení – pokračujem s {self.limit}.")
                    self._release(i)
                    if self._wait(2):
                        return
                    continue
                except (requests.RequestException, ConnectionError, TimeoutError, DownloadError) as e:
                    failures += 1
                    self._release(i)
                    if failures > self.max_retries:
                        self.errors.append(str(e))
                        return
                    if self._wait(min(2 ** failures, 30)):
                        return
                    continue
                self._release(i)
        finally:
            fh.close()
            with self.lock:
                self.active -= 1

    def _fetch_segment(self, i: int, fh):
        with self.lock:
            s = self.segs[i]
            pos, end, url, gen = s[0] + s[2], s[1], self.url, self.url_gen
        if pos > end:
            return
        headers = {"User-Agent": USER_AGENT, "Accept-Encoding": "identity", "Range": f"bytes={pos}-{end}"}
        with self.session.get(url, headers=headers, stream=True, timeout=(20, 60)) as r:
            if r.status_code in (401, 403, 404, 410):
                self._refresh(gen)
                raise DownloadError(f"chyba {r.status_code}")
            if r.status_code in (429, 503) or "text/html" in (r.headers.get("Content-Type") or "").lower():
                raise _TooMany()
            if r.status_code == 200:
                raise _NoRange()
            if r.status_code != 206:
                raise DownloadError(f"Server vrátil chybu {r.status_code}.")
            m = re.match(r"bytes\s+(\d+)-", r.headers.get("Content-Range") or "")
            if not m or int(m.group(1)) != pos:
                raise _NoRange()
            fh.seek(pos)
            for chunk in r.iter_content(CHUNK):
                if self.halted():
                    return
                if not chunk:
                    continue
                with self.lock:
                    end = s[1]                 # úsek mohlo medzitým skrátiť iné spojenie
                    room = end - (s[0] + s[2]) + 1
                if room <= 0:
                    return
                chunk = chunk[:room]
                fh.write(chunk)
                with self.lock:
                    s[2] += len(chunk)
                if len(chunk) == room:
                    return

    # ---------- riadenie ----------
    def run(self) -> Path:
        if not self._load_state():
            with open(self.part, "wb") as f:   # vyhradí miesto na disku
                f.truncate(self.total)
        else:
            self.log(f"↻ Pokračujem v sťahovaní ({self.done_bytes() * 100 // self.total} % hotovo).")
        self.save_state()
        threads: list[threading.Thread] = []
        no_range: list[bool] = []

        def run_worker(wid):
            try:
                self.worker(wid)
            except _NoRange:
                no_range.append(True)
                self.halt.set()

        n = min(self.limit, len([s for s in self.segs if s[0] + s[2] <= s[1]]) or 1)
        for wid in range(n):
            t = threading.Thread(target=run_worker, args=(wid,), daemon=True)
            t.start()
            threads.append(t)

        window = [(time.monotonic(), self.done_bytes())]
        last_save = time.monotonic()
        while any(t.is_alive() for t in threads):
            time.sleep(0.25)
            now, done = time.monotonic(), self.done_bytes()
            window.append((now, done))
            while len(window) > 2 and now - window[0][0] > 5:
                window.pop(0)
            if self.on_progress:
                self.on_progress(done, self.total, (done - window[0][1]) / max(now - window[0][0], 1e-3))
            if now - last_save > 1:
                self.save_state()
                last_save = now
            # spojenie dokončilo a iné ešte ťahá veľký zvyšok → pridaj vlákno, ak to limit dovolí
            alive = sum(t.is_alive() for t in threads)
            with self.lock:
                can_add = alive < self.limit and any(
                    (s[1] - (s[0] + s[2]) + 1) >= 2 * MIN_SEGMENT for s in self.segs)
            if can_add and not self.halted():
                t = threading.Thread(target=run_worker, args=(len(threads),), daemon=True)
                t.start()
                threads.append(t)
        self.save_state()
        if no_range:
            raise _NoRange()
        if self.stop.is_set():
            raise Paused()
        if self.done_bytes() < self.total:
            raise DownloadError("Sťahovanie sa nedokončilo: " + (self.errors[-1] if self.errors else "neznáma chyba"))
        if self.on_progress:
            self.on_progress(self.total, self.total, 0)
        final = self.dest if not self.dest.exists() else unique_path(self.dest)
        self.part.replace(final)
        self.state_file.unlink(missing_ok=True)
        return final


class _TooMany(Exception):
    pass


def download_auto(get_url, dest: Path, session: requests.Session | None = None, *, connections: int = 4,
                  expected_size: int | None = None, on_progress=None, stop: threading.Event | None = None,
                  log=lambda m: None, max_retries: int = 12) -> Path:
    """Vyberie najlepší spôsob: viac spojení naraz (veľké súbory na serveroch s Range), inak jedno spojenie."""
    session = session or requests.Session()
    stop = stop or threading.Event()
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    single = lambda: download(get_url, dest, session, on_progress=on_progress, stop=stop,  # noqa: E731
                              expected_size=expected_size, max_retries=max_retries, log=log)
    if connections <= 1:
        return single()
    if part_path(dest).exists() and not state_path(dest).exists():
        return single()   # rozpracované jedným spojením → dokonči rovnako

    total = expected_size
    if not state_path(dest).exists():
        url = get_url(False)
        try:
            for attempt in range(2):
                with session.get(url, headers={"User-Agent": USER_AGENT, "Range": "bytes=0-0",
                                               "Accept-Encoding": "identity"}, stream=True, timeout=(20, 30)) as r:
                    status = r.status_code
                    size = _total_from_range(r.headers.get("Content-Range"))
                if status in (401, 403, 404, 410) and attempt == 0:
                    url = get_url(True)       # odkaz už neplatí → vypýtaj nový a skús znova
                    continue
                break
            if status != 206:
                return single()
            total = size or total
        except requests.RequestException:
            return single()
    else:
        try:
            total = json.loads(state_path(dest).read_text(encoding="utf-8"))["total"]
        except (OSError, ValueError, KeyError):
            state_path(dest).unlink(missing_ok=True)
            return single()
    if not total or total < MIN_SEGMENTED_SIZE:
        return single()
    try:
        return _Segmented(get_url, dest, session, total, connections, on_progress, stop, log, max_retries).run()
    except _NoRange:
        log("⚠ Server nepodporuje sťahovanie po častiach – pokračujem jedným spojením.")
        part_path(dest).unlink(missing_ok=True)
        state_path(dest).unlink(missing_ok=True)
        return single()
