"""Front sťahovania: súbežné sťahovanie, pauza/pokračovanie, uloženie na disk.

Stav frontu sa ukladá do ~/.kometa/fronta.json, takže po zatvorení programu
(alebo výpadku) sa nedokončené položky dajú dokončiť – pokračuje sa od miesta,
kde sa skončilo.
"""

from __future__ import annotations

import json
import re
import threading
import time
import uuid
from pathlib import Path

from .common import QUEUE_PATH, Paused, clean_error, safe_name, unique_path
from . import httpdl, media
from .hosts import WEBSHARE_RE, Accounts, HostError, detect_host

_FRIENDLY = [
    (r"HTTP Error 404|404: Not Found", "Stránka alebo súbor neexistuje (chyba 404)."),
    (r"Private video|video is private", "Video je súkromné."),
    (r"not a bot|[Ll]ogin required|[Ss]ign in|log in|logged.in|rate.limit|cookies",
     "Obsah vyžaduje prihlásenie – v karte „Účty a nastavenia“ vyber prehliadač, v ktorom si prihlásený."),
    (r"HTTP Error 403|Forbidden", "Prístup zamietnutý (403) – obsah môže vyžadovať prihlásenie."),
    (r"Video unavailable|has been removed|no longer available|does not exist", "Video už nie je dostupné."),
    (r"not available in your country|geo.?restrict", "Video nie je dostupné v tvojej krajine."),
    (r"Requested format is not available|No video formats found",
     "Nenašiel sa vhodný formát – skús iný formát alebo aktualizuj yt-dlp (Spustit.bat)."),
    (r"ffmpeg|ffprobe", "Chýba FFmpeg – spusti Instalacia.bat."),
    (r"getaddrinfo|Failed to resolve|Name or service not known|timed out|Unable to download webpage",
     "Stránka nie je dostupná – skontroluj pripojenie na internet a odkaz."),
]


def friendly_error(msg: str) -> str:
    """Technickú chybu preloží do zrozumiteľnej vety (pôvodný text ostane v podrobnostiach)."""
    for pattern, text in _FRIENDLY:
        if re.search(pattern, msg):
            return f"{text}\n\nPodrobnosti: {msg[:400]}"
    return msg


STATUS_TEXT = {"caka": "Čaká", "stahuje": "Sťahuje", "pauza": "Pozastavené", "hotovo": "Hotovo ✓",
               "chyba": "Chyba"}


class DownloadQueue:
    def __init__(self, cfg: dict, accounts: Accounts, emit=lambda item_id: None, log=lambda m: None,
                 path: Path = QUEUE_PATH):
        self.cfg = cfg
        self.accounts = accounts
        self.emit = emit
        self.log = log
        self.path = Path(path)
        self.items: dict[str, dict] = {}
        self.order: list[str] = []
        self._lock = threading.RLock()
        self._stops: dict[str, threading.Event] = {}
        self._threads: dict[str, threading.Thread] = {}
        self._last_save = 0.0
        self._save_lock = threading.Lock()
        self._closing = False
        self.load()

    # ---------- uloženie ----------
    def load(self) -> None:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except Exception:
            return
        auto = self.cfg.get("autoresume", True)
        for it in data.get("items", []):
            if it.get("status") == "stahuje":
                it["status"] = "caka" if auto else "pauza"
            it["speed"] = 0
            self.items[it["id"]] = it
            self.order.append(it["id"])

    def save(self, force: bool = True) -> None:
        now = time.time()
        if not force and now - self._last_save < 3:
            return
        self._last_save = now
        with self._lock:
            data = {"items": [dict(self.items[i], speed=0) for i in self.order if i in self.items]}
        with self._save_lock:
            try:
                self.path.parent.mkdir(parents=True, exist_ok=True)
                tmp = self.path.with_suffix(".tmp")
                tmp.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
                tmp.replace(self.path)
            except Exception:
                pass

    # ---------- čítanie ----------
    def snapshot(self, item_id: str) -> dict | None:
        with self._lock:
            it = self.items.get(item_id)
            return dict(it) if it else None

    def ids(self) -> list[str]:
        with self._lock:
            return list(self.order)

    def counts(self) -> dict:
        with self._lock:
            c = {k: 0 for k in STATUS_TEXT}
            speed = 0.0
            for it in self.items.values():
                c[it["status"]] = c.get(it["status"], 0) + 1
                if it["status"] == "stahuje":
                    speed += it.get("speed") or 0
            c["speed"] = speed
            return c

    # ---------- ovládanie ----------
    def add(self, urls: list[str], preset: str, outdir: str) -> list[str]:
        new = []
        with self._lock:
            known = {self.items[i]["url"] for i in self.order if self.items[i]["status"] != "hotovo"}
            for url in urls:
                url = url.strip()
                if not url or url in known:
                    continue
                known.add(url)
                iid = uuid.uuid4().hex[:10]
                self.items[iid] = {"id": iid, "url": url, "source": media.source_name(url), "preset": preset,
                                   "outdir": outdir, "title": "", "status": "caka", "done": 0, "total": None,
                                   "speed": 0, "file": "", "dest": "", "tmp": [], "error": "",
                                   "added": time.time()}
                self.order.append(iid)
                new.append(iid)
        self.save()
        for iid in new:
            self.emit(iid)
        self.schedule()
        return new

    def start(self, ids: list[str]) -> None:
        with self._lock:
            for iid in ids:
                it = self.items.get(iid)
                if it and it["status"] in ("pauza", "chyba"):
                    it["status"], it["error"] = "caka", ""
                    self.emit(iid)
        self.save()
        self.schedule()

    def pause(self, ids: list[str]) -> None:
        with self._lock:
            for iid in ids:
                it = self.items.get(iid)
                if not it:
                    continue
                if it["status"] == "stahuje" and iid in self._stops:
                    self._stops[iid].set()  # vlákno samo nastaví stav „pauza“
                elif it["status"] == "caka":
                    it["status"] = "pauza"
                    self.emit(iid)
        self.save()

    def remove(self, ids: list[str], delete_partial: bool = True) -> None:
        for iid in ids:
            with self._lock:
                it = self.items.get(iid)
                if not it:
                    continue
                ev = self._stops.get(iid)
                th = self._threads.get(iid)
            if ev:
                ev.set()
            if th:
                th.join(timeout=5)
            with self._lock:
                it = self.items.pop(iid, None)
                if iid in self.order:
                    self.order.remove(iid)
            if it and delete_partial and it["status"] != "hotovo":
                self._delete_partial(it)
            self.emit(iid)
        self.save()
        self.schedule()

    def clear_finished(self) -> None:
        with self._lock:
            done = [i for i in self.order if self.items[i]["status"] == "hotovo"]
        for iid in done:
            with self._lock:
                self.items.pop(iid, None)
                self.order.remove(iid)
            self.emit(iid)
        self.save()

    def pause_all(self) -> None:
        self.pause(self.ids())

    def start_all(self) -> None:
        self.start(self.ids())

    def running(self) -> int:
        with self._lock:
            return sum(1 for it in self.items.values() if it["status"] == "stahuje")

    def shutdown(self) -> None:
        """Pri zatváraní programu: zastaví bežiace sťahovania tak, aby sa po ďalšom
        spustení dalo pokračovať (rozbehnuté sa označia ako čakajúce, ak je zapnuté automatické pokračovanie)."""
        with self._lock:
            self._closing = True
            running = [i for i, it in self.items.items() if it["status"] == "stahuje"]
            evs = list(self._stops.values())
            ths = list(self._threads.values())
        for ev in evs:
            ev.set()
        for th in ths:
            th.join(timeout=8)
        with self._lock:
            for iid in running:
                if iid in self.items and self.items[iid]["status"] == "pauza":
                    self.items[iid]["status"] = "caka" if self.cfg.get("autoresume", True) else "pauza"
        self.save()

    # ---------- plánovač ----------
    def schedule(self) -> None:
        with self._lock:
            if self._closing:
                return
            limit = max(1, int(self.cfg.get("parallel") or 2))
            running = sum(1 for it in self.items.values() if it["status"] == "stahuje")
            for iid in self.order:
                if running >= limit:
                    break
                it = self.items[iid]
                if it["status"] != "caka":
                    continue
                it["status"], it["error"] = "stahuje", ""
                ev = threading.Event()
                th = threading.Thread(target=self._worker, args=(iid, ev), daemon=True)
                self._stops[iid], self._threads[iid] = ev, th
                running += 1
                self.emit(iid)
                th.start()

    def _update(self, iid: str, **kw) -> None:
        with self._lock:
            it = self.items.get(iid)
            if it:
                it.update(kw)
        self.emit(iid)

    def _worker(self, iid: str, stop: threading.Event) -> None:
        it = self.snapshot(iid)
        if not it:
            return
        try:
            path = self._process(iid, it, stop)
            with self._lock:
                cur = self.items.get(iid)
                if cur:
                    cur.update(status="hotovo", file=str(path), speed=0, tmp=[],
                               done=cur.get("total") or cur.get("done"), title=cur.get("title") or path.name)
            self.log(f"✔ Hotovo: {path.name}")
        except Paused:
            self._update(iid, status="pauza", speed=0)
        except Exception as e:  # noqa: BLE001 – chybu chceme ukázať používateľovi
            msg = friendly_error(clean_error(e) or e.__class__.__name__)
            self._update(iid, status="chyba", error=msg, speed=0)
            self.log(f"✖ {it.get('title') or it['url']}: {msg.splitlines()[0]}")
        finally:
            with self._lock:
                self._stops.pop(iid, None)
                self._threads.pop(iid, None)
            self.emit(iid)
            self.save()
            self.schedule()

    # ---------- samotné sťahovanie ----------
    def _process(self, iid: str, it: dict, stop: threading.Event) -> Path:
        url, outdir = it["url"], Path(it["outdir"])

        def progress(done, total, speed):
            self._update(iid, done=done, total=total, speed=speed or 0)
            self.save(force=False)

        host = detect_host(url)
        if host == "webshare":
            client = self.accounts.webshare()
            ident = WEBSHARE_RE.match(url).group("id")
            info = client.file_info(ident)
            dest = self._dest(iid, it, outdir, info["name"])
            self._update(iid, title=info["name"], total=info["size"])
            self.log(f"⬇ Webshare: {info['name']}")
            return httpdl.download(lambda refresh: self._ws_link(ident, refresh), dest, client.session,
                                   on_progress=progress, stop=stop, expected_size=info["size"], log=self.log)

        if host == "fastshare":
            client = self.accounts.fastshare()
            res = client.resolve(url)
            name = res["name"] or httpdl.filename_from_response({}, res["link"]) or "fastshare_subor"
            dest = self._dest(iid, it, outdir, name)
            self._update(iid, title=name, total=res["size"])
            self.log(f"⬇ Fastshare: {name}")

            def link(refresh):
                return self.accounts.fastshare().resolve(url)["link"] if refresh else res["link"]

            return httpdl.download(link, dest, client.session, on_progress=progress, stop=stop, log=self.log)

        # neznáma stránka: najprv over, či to nie je priamy odkaz na súbor (zip, pdf, mkv…)
        meta = None
        if it["source"] == "Web":
            try:
                meta = httpdl.probe(url)
            except Exception:  # noqa: BLE001 – niektoré stránky nepovolia Range; skúsi sa yt-dlp
                meta = None
            if meta and not meta["html"]:
                name = meta["name"] or "subor"
                dest = self._dest(iid, it, outdir, name)
                self._update(iid, title=name, total=meta["size"])
                self.log(f"⬇ Súbor: {name}")
                return httpdl.download(lambda refresh: url, dest, on_progress=progress, stop=stop,
                                       expected_size=meta["size"], log=self.log)

        # stránky s videom (YouTube, Facebook, Instagram, TikTok a ďalšie, ktoré pozná yt-dlp)
        tmp: set = set(it.get("tmp") or [])
        try:
            self.log(f"⬇ {it['source']}: {url}")
            return media.download_media(
                url, outdir, it.get("preset") or "video", stop=stop, log=self.log, tmp_files=tmp,
                on_progress=lambda d, t, s: (self._update(iid, tmp=sorted(tmp)), progress(d, t, s)),
                on_title=lambda title: self._update(iid, title=title))
        except media.Unsupported:
            raise HostError("Na tejto stránke sa nenašlo video ani súbor na stiahnutie.") from None

    def _ws_link(self, ident: str, refresh: bool) -> str:
        if refresh:
            self.accounts.reset("webshare")
        try:
            return self.accounts.webshare().file_link(ident)
        except HostError:
            if refresh:
                raise
            self.accounts.reset("webshare")  # token mohol vypršať – prihlásime sa znova
            return self.accounts.webshare().file_link(ident)

    def _dest(self, iid: str, it: dict, outdir: Path, name: str) -> Path:
        """Cieľový súbor – pri pokračovaní ten istý (kvôli .part), inak nový bez prepisovania."""
        if it.get("dest"):
            return Path(it["dest"])
        dest = outdir / safe_name(name, 180)
        if dest.exists() or httpdl.part_path(dest).exists():
            dest = unique_path(dest)
            while httpdl.part_path(dest).exists():
                dest = unique_path(dest.with_name(dest.stem + "_" + dest.suffix))
        self._update(iid, dest=str(dest))
        return dest

    def _delete_partial(self, it: dict) -> None:
        paths = [Path(p) for p in it.get("tmp") or []]
        if it.get("dest"):
            paths.append(httpdl.part_path(Path(it["dest"])))
        for p in paths:
            base = p.name[:-5] if p.name.endswith(".part") else p.name
            for cand in (p.with_name(base), p.with_name(base + ".part"), p.with_name(base + ".ytdl")):
                try:
                    if cand.exists() and cand.is_file():
                        cand.unlink()
                except OSError:
                    pass
