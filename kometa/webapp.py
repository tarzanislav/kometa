"""Nové rozhranie Kometa – webové UI (kometa/webui) v okne aplikácie.

Okno:
  1. pywebview – Windows: WebView2 (Edge), macOS: WebKit, Linux: GTK/Qt WebKit;
  2. ak pywebview chýba, okno Edge/Chrome/Chromium v režime aplikácie (--app) nad lokálnym serverom,
     ktorý počúva len na 127.0.0.1 a odpovedá len so 128-bitovým tajným kľúčom;
  3. inak predvolený prehliadač.

UI sa s Pythonom rozpráva len cez metódy triedy Jadro (zoznam API_METHODS). Rovnaké UI neskôr
použije aj aplikácia Tauri s jadrom v Ruste – stačí, aby jadro ponúklo tie isté metódy.
"""

from __future__ import annotations

import json
import mimetypes
import os
import re
import secrets
import shutil
import subprocess
import sys
import threading
import time
import webbrowser
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from . import __version__, common, hosts, media, skener
from .common import (Cancelled, apply_settings, check_dependencies, clean_error, delete_password, get_password,
                     load_config, open_folder, safe_name, save_config, set_password)
from .fronta import DownloadQueue
from .hosts import Accounts, DatoidClient, FastshareClient, WebshareClient
from .jazyky import AUDIO_MODES, fetch_info, lang_label, list_audio_tracks, pick_tracks, run_job

WEBUI = Path(__file__).with_name("webui")
YT_ID = re.compile(r"(?:youtube\.com/(?:watch\?(?:[^#]*&)?v=|shorts/|live/|embed/)|youtu\.be/)([\w-]{11})")
URL_RE = re.compile(r"https?://[^\s<>\"']+")
BROWSERS = ["firefox", "chrome", "edge", "brave", "opera", "vivaldi", "chromium", "safari"]
THEMES = {"kometa", "polarna", "sumrak", "les", "more", "levandula", "grafit"}

# kľúče nastavení, ktoré smie UI meniť, a ich kontrola
_SETTINGS = {
    "outdir": lambda v: str(v).strip() or str(common.default_outdir()),
    "dl_preset": lambda v: v if v in media.PRESETS or v == "jazyky" else "video",
    "parallel": lambda v: max(1, min(6, int(v))),
    "connections": lambda v: max(1, min(16, int(v))),
    "autoresume": bool,
    "cookies_browser": lambda v: v if v in BROWSERS else "",
    "audio_mode": lambda v: v if v in AUDIO_MODES else "oboje",
    "tema": lambda v: v if v in THEMES else "kometa",
    "rezim": lambda v: v if v in ("auto", "den", "noc") else "auto",
    "include_original": bool,
    "keep_separate": bool,
    "mode": lambda v: v if v in ("video", "existing", "audio") else "video",
    "kontajner": lambda v: v if v in ("mkv", "mp4") else "mkv",
    "max_height": lambda v: v if v in ("Najlepšia", "2160p", "1440p", "1080p", "720p", "480p") else "Najlepšia",
    "rozbalovat": bool,
    "po_rozbaleni": lambda v: v if v in ("nechat", "kos") else "nechat",
    "titulky": lambda v: v if v in ("vypnute", "jazyky") else "vypnute",
    "titulky_auto": bool,
    "vlozit_udaje": bool,
    "vlozit_kapitoly": bool,
    "vlozit_obal": bool,
    "aktualizacie": bool,
}


def popis_jazykov(o: dict) -> str:
    """Krátky popis volieb jazykov pre položku frontu (napr. „CZ, SK, EN · AAC + Opus · MKV“)."""
    kody = ", ".join(c.upper() for c in o.get("poradie") or [])
    mp4 = o.get("kontajner") == "mp4"
    zvuk = {"aac": "AAC", "opus": "Opus"}.get(o.get("zvuk"), "AAC + Opus")
    t = f"{kody}{' + originál' if o.get('original') else ''} · {'AAC' if mp4 else zvuk} · " \
        f"{'MP4' if mp4 else 'MKA' if o.get('rezim') == 'audio' else 'MKV'}"
    t += " · rôzny zvuk pri jazykoch" if o.get("zvuky") and not mp4 else ""
    if o.get("titulky") == "jazyky":
        t += " · titulky (aj automatické)" if o.get("titulky_auto") else " · titulky"
    return t


def yt_thumb(url: str) -> str:
    m = YT_ID.search(url or "")
    return f"https://i.ytimg.com/vi/{m.group(1)}/mqdefault.jpg" if m else ""


def read_clipboard() -> str:
    """Text zo schránky (cez Tk – je súčasťou Pythonu vo Windows aj macOS)."""
    try:
        import tkinter as tk
        r = tk.Tk()
        r.withdraw()
        try:
            return r.clipboard_get()
        finally:
            r.destroy()
    except Exception:  # noqa: BLE001
        return ""


class Jadro:
    """Všetko, čo UI potrebuje: front, jazykové stopy, playlisty, skener stránok, účty, nastavenia."""

    def __init__(self):
        self.cfg = load_config()
        apply_settings(self.cfg)
        self.accounts = Accounts(self.cfg)
        self._log: deque[str] = deque(maxlen=300)
        self.dq = DownloadQueue(self.cfg, self.accounts, log=self._log.append, path=common.QUEUE_PATH)
        self.window = None                       # okno pywebview (dialógy na výber súboru/priečinka)
        self._speed = deque([0.0] * 90, maxlen=90)
        self._speed_t = 0.0
        self._lang_info: dict | None = None
        self._lang_tracks: list[dict] = []
        self._lang = {"busy": False, "frac": 0.0, "text": "", "log": [], "file": "", "error": "", "rev": 0}
        self._lang_cancel = threading.Event()
        self._accounts = {s: {"stav": "neprihlaseny", "text": "", "user": self.cfg.get(f"{s}_user") or ""}
                          for s in ("webshare", "fastshare", "datoid")}
        self.last_call = time.time()
        threading.Thread(target=self._check_accounts, daemon=True).start()
        from . import aktualizacia
        aktualizacia.start(lambda: bool(self.cfg.get("aktualizacie", True)), self._log.append)

    # ------------------------------------------------------------------ úvod
    def init(self) -> dict:
        deps = check_dependencies()
        c = self.cfg
        return {
            "verzia": __version__,
            "platforma": sys.platform,
            "nastavenia": {k: c.get(k) for k in _SETTINGS},
            "jazyky_poradie": c.get("langs") or common.DEFAULT_LANGS,
            "formaty": [{"id": k, "nazov": v[0]} for k, v in media.PRESETS.items()][:1]
            + [{"id": "jazyky", "nazov": "Video s jazykmi (YouTube, podľa karty Jazyky)"}]
            + [{"id": k, "nazov": v[0]} for k, v in media.PRESETS.items()][1:],
            "prehliadace": BROWSERS,
            "zvuk": AUDIO_MODES,
            "chyba_ffmpeg": not deps.get("ffmpeg"),
            "chyba_ytdlp": not deps.get("yt-dlp"),
        }

    # ------------------------------------------------------------------ stav (UI sa pýta ~2× za sekundu)
    def stav(self) -> dict:
        self.last_call = time.time()
        items = []
        for iid in self.dq.ids():
            it = self.dq.snapshot(iid)
            if it:
                items.append(self._item(it))
        counts = self.dq.counts()
        now = time.time()
        if now - self._speed_t >= 1.0:           # graf rýchlosti: jeden bod za sekundu
            self._speed.append(float(counts.get("speed") or 0))
            self._speed_t = now
        from . import aktualizacia
        return {"polozky": items, "rychlost": counts.get("speed") or 0, "graf": list(self._speed),
                "jazyky": dict(self._lang, log=self._lang["log"][-80:]), "ucty": self._accounts,
                "aktualizacia": aktualizacia.status()}

    # ------------------------------------------------------------------ aktualizácia
    def aktualizacia_skontroluj(self) -> dict:
        from . import aktualizacia
        return aktualizacia.check(bool(self.cfg.get("aktualizacie", True)), self._log.append)

    def aktualizacia_instaluj(self) -> dict:
        from . import aktualizacia
        return aktualizacia.install(self._log.append) or aktualizacia.status()

    def aktualizacia_pri_starte(self) -> dict:
        from . import aktualizacia
        return aktualizacia.later()

    def _item(self, it: dict) -> dict:
        total, done, speed = it.get("total"), it.get("done") or 0, it.get("speed") or 0
        eta = (total - done) / speed if total and speed and total > done else None
        err = it.get("error") or ""
        short, _, detail = err.partition("\n\nPodrobnosti: ")
        path = it.get("file") or ""
        return {"id": it["id"], "url": it["url"], "zdroj": it.get("source") or "Web", "format": it.get("preset"),
                "nazov": it.get("title") or "", "stav": it["status"], "hotovo": done, "celkom": total,
                "rychlost": speed, "zostava": eta, "subor": Path(path).name if path else "", "chyba": short,
                "podrobnosti": detail, "nahlad": it.get("thumb") or yt_thumb(it["url"]),
                "dlzka": it.get("duration"), "autor": it.get("uploader") or "", "pridane": it.get("added"),
                "podiel": it.get("frac"), "krok": it.get("step") or "",
                "jazyky": popis_jazykov(it["jazyky"]) if it.get("jazyky") else None,
                "treba_heslo": it.get("need_password") or "",
                "rozbalenie": dict(it["extract"], ciel=Path(it["extract"].get("ciel") or "").name)
                if it.get("extract") else None}

    # ------------------------------------------------------------------ pridávanie
    def _outdir(self) -> str:
        out = self.cfg.get("outdir") or str(common.default_outdir())
        Path(out).mkdir(parents=True, exist_ok=True)
        return out

    def _preset(self, preset: str) -> str:
        return preset if preset in media.PRESETS or preset == "jazyky" else self.cfg.get("dl_preset") or "video"

    def _jazyky_z_nastaveni(self) -> dict:
        c = self.cfg
        return {"poradie": list(c.get("langs") or common.DEFAULT_LANGS), "original": bool(c.get("include_original", True)),
                "zvuk": c.get("audio_mode") or "oboje", "zvuky": {}, "kontajner": c.get("kontajner") or "mkv",
                "kvalita": c.get("max_height") or "Najlepšia", "rezim": "video", "bez_jazykov": "povodny",
                "titulky": c.get("titulky") or "vypnute", "titulky_auto": bool(c.get("titulky_auto"))}

    def _pridaj(self, urls: list[str], preset: str, out: str, titles: list[str] | None = None,
                jazyky: dict | None = None, password: str = "") -> list[str]:
        """„Video s jazykmi“ len pri YouTube – ostatné stránky sa stiahnu ako video."""
        password = (password or "").strip()
        if preset != "jazyky":
            return self.dq.add(urls, preset, out, titles=titles, password=password)
        titles = titles or [""] * len(urls)
        yt = [i for i, u in enumerate(urls) if media.source_name(u) == "YouTube"]
        ine = [i for i in range(len(urls)) if i not in yt]
        new = self.dq.add([urls[i] for i in yt], "jazyky", out, titles=[titles[i] for i in yt],
                          jazyky=jazyky or self._jazyky_z_nastaveni()) if yt else []
        return new + (self.dq.add([urls[i] for i in ine], "video", out, titles=[titles[i] for i in ine],
                                  password=password) if ine else [])

    def zadat_heslo(self, iid: str, heslo: str) -> None:
        self.dq.set_password(iid, heslo or "")

    def rozbalit(self, iid: str) -> None:
        self.dq.try_extract(iid, manual=True)

    def pridat(self, text: str, preset: str = "", heslo: str = "") -> dict:
        urls = list(dict.fromkeys(u.rstrip(").,;]") for u in URL_RE.findall(text or "")))
        if not urls:
            raise ValueError("Vlož aspoň jeden odkaz – začína na http:// alebo https://.")
        preset = self._preset(preset)
        self._save(dl_preset=preset)
        collections = [u for u in urls if media.is_collection(u)]
        single = [u for u in urls if u not in collections]
        new = self._pridaj(single, preset, self._outdir(), password=heslo) if single else []
        return {"pridane": len(new), "uz_vo_fronte": len(single) - len(new), "kolekcie": collections}

    def rozpoznat(self, text: str) -> list[dict]:
        """Zdroje odkazov počas písania (štítky pod poľom na odkazy)."""
        out = []
        for u in list(dict.fromkeys(URL_RE.findall(text or "")))[:50]:
            out.append({"url": u, "zdroj": media.source_name(u), "kolekcia": media.is_collection(u),
                        "ulozisko": bool(hosts.detect_host(u))})
        return out

    def kolekcia(self, url: str, karta: str = "videos") -> dict:
        if not re.match(r"^https?://", (url or "").strip()):
            raise ValueError("Zadaj odkaz na playlist alebo kanál.")
        data = media.list_collection(media.collection_url(url, karta), log=lambda m: None)
        for e in data["entries"]:
            e["nahlad"] = yt_thumb(e["url"])
        data["kanal"] = bool(media._YT_CHANNEL_RE.match(url.strip())) if hasattr(media, "_YT_CHANNEL_RE") else False
        return data

    def pridat_vybrane(self, polozky: list[dict], preset: str = "", priecinok: str = "",
                       jazyky: dict | None = None) -> dict:
        if jazyky is not None:
            if not (jazyky.get("poradie") or jazyky.get("original")):
                raise ValueError("Vyber aspoň jeden jazyk.")
            preset = "jazyky"
        else:
            preset = self._preset(preset)
        out = self._outdir()
        if priecinok:
            out = str(Path(out) / safe_name(priecinok, 100))
        new = self._pridaj([p["url"] for p in polozky], preset, out, titles=[p.get("title") or "" for p in polozky],
                           jazyky=jazyky)
        return {"pridane": len(new), "uz_vo_fronte": len(polozky) - len(new)}

    def skenovat(self, url: str) -> list[dict]:
        url = (url or "").strip()
        if not re.match(r"^https?://", url):
            raise ValueError("Zadaj adresu stránky – začína na http:// alebo https://.")
        found = [n for n in skener.Skener(log=lambda m: None, cookies_browser=self.cfg.get("cookies_browser") or None)
                 .scan(url) if n.kind != "stranka"]
        best = {id(n) for n in skener.best_candidates(found)}
        return [{"url": n.url, "druh": n.kind, "druh_text": skener.KIND_TEXT.get(n.kind, n.kind),
                 "poskytovatel": n.provider, "nazov": n.title, "poznamka": n.note, "skore": n.score,
                 "referer": n.referer, "odporucane": id(n) in best, "da_sa": n.downloadable} for n in found]

    def pridat_zo_skenu(self, polozky: list[dict], preset: str = "", heslo: str = "") -> dict:
        preset = preset if preset in media.PRESETS else self.cfg.get("dl_preset") or "video"
        out, n = self._outdir(), 0
        for p in polozky:
            n += len(self.dq.add([p["url"]], preset, out, referer=p.get("referer") or None, password=heslo or None,
                                 titles=[p.get("nazov") or ""], providers=[p.get("poskytovatel") or ""],
                                 from_scan=True))
        return {"pridane": n}

    # ------------------------------------------------------------------ front
    def pozastavit(self, ids: list[str]) -> None:
        self.dq.pause(ids)

    def spustit(self, ids: list[str]) -> None:
        self.dq.start(ids)

    def odstranit(self, ids: list[str]) -> None:
        self.dq.remove(ids)

    def vycistit_hotove(self) -> None:
        self.dq.clear_finished()

    def vycistit_front(self) -> None:
        """Všetko z frontu preč; stiahnuté súbory ostanú, rozpracované sa upracú."""
        self.dq.remove(list(self.dq.order))

    def _stiahnute(self, ids: list[str] | None) -> list[Path]:
        out: list[Path] = []
        with self.dq._lock:
            items = [self.dq.items[i] for i in self.dq.order if i in self.dq.items]
        for it in items:
            if (ids is not None and it["id"] not in ids) or it["status"] != "hotovo" or not it.get("file"):
                continue
            f = Path(it["file"])
            if f.is_file() and f not in out:
                out.append(f)
        return out

    def subory_na_zmazanie(self, ids: list[str] | None = None) -> dict:
        files = self._stiahnute(ids)
        return {"pocet": len(files), "velkost": sum(f.stat().st_size for f in files)}

    def zmazat_subory(self, ids: list[str] | None = None) -> dict:
        """Stiahnuté súbory do Koša a položky z frontu (pri ids=None celý front). Iné súbory ostanú."""
        try:
            from send2trash import send2trash
        except ImportError:
            raise ValueError("Na presun do Koša chýba balík send2trash – spusti Instalacia.bat.") from None
        root = Path(self._outdir()).resolve()
        zmazane, chyby = [], []
        for f in self._stiahnute(ids):
            try:
                send2trash(str(f))
                zmazane.append(f)
            except Exception as e:  # noqa: BLE001
                chyby.append(f"{f.name}: {e}")
        for f in zmazane:   # prázdne priečinky kanálov a playlistov – len vnútri priečinka sťahovania
            d = f.parent.resolve()
            while d != root and root in d.parents:
                try:
                    d.rmdir()
                except OSError:
                    break
                d = d.parent
        with self.dq._lock:
            items = [self.dq.items[i] for i in self.dq.order if i in self.dq.items]
        nechat = {str(f) for f in self._stiahnute(ids)}   # čo sa nepodarilo, ostane vo fronte
        self.dq.remove([it["id"] for it in items if (ids is None or it["id"] in ids)
                        and not (it["status"] == "hotovo" and it.get("file") in nechat)])
        if zmazane:
            self.dq.log(f"🗑 Do Koša: {len(zmazane)} súborov")
        for c in chyby:
            self.dq.log(f"Nepodarilo sa presunúť do Koša – {c}")
        return {"zmazane": len(zmazane), "chyby": chyby}

    def pozastavit_vsetko(self) -> None:
        self.dq.pause_all()

    def spustit_vsetko(self) -> None:
        self.dq.start_all()

    def otvorit(self, iid: str = "") -> None:
        it = self.dq.snapshot(iid) if iid else None
        if it and it.get("file") and Path(it["file"]).exists():
            open_folder(Path(it["file"]).parent, select=Path(it["file"]))
        elif it:
            open_folder(Path(it["outdir"]))
        else:
            open_folder(Path(self._outdir()))

    def otvorit_subor(self, path: str) -> None:
        p = Path(path)
        if p.exists():
            open_folder(p.parent, select=p)

    def schranka(self) -> str:
        return read_clipboard()

    # ------------------------------------------------------------------ jazykové stopy
    def jazyky_nacitat(self, url: str) -> dict:
        url = (url or "").strip()
        if not re.match(r"^(https?://)?([\w-]+\.)*(youtube\.com|youtu\.be)/", url):
            raise ValueError("Vlož odkaz na video z YouTube, napríklad https://www.youtube.com/watch?v=…")
        info = fetch_info(url, log=lambda m: None)
        tracks = list_audio_tracks(info)
        self._lang_info, self._lang_tracks = info, tracks
        chosen, _ = pick_tracks(tracks, self.cfg.get("langs") or common.DEFAULT_LANGS)
        orig = next((t for t in tracks if t["original"]), None)
        return {"nazov": info.get("title") or info.get("id"), "autor": info.get("uploader") or info.get("channel") or "",
                "dlzka": info.get("duration"), "nahlad": yt_thumb(url) or info.get("thumbnail") or "",
                "original": orig["label"] if orig else "",
                "stopy": [{"kod": t["code"], "nazov": t["label"], "original": t["original"],
                           "aac": bool(t.get("aac")), "kbps": round((t["format"].get("abr") or 0))}
                          for t in tracks],
                "vybrane": [t["code"] for t in chosen if not t["original"]],
                "titulky": sorted({k.lower().split("-")[0] for k in (info.get("subtitles") or {}) if k != "live_chat"}),
                "auto_titulky": bool(info.get("automatic_captions"))}

    def jazyky_zistit(self, urls: list[str]) -> dict:
        """Jazyky zvuku vo viacerých videách (najviac 4 naraz, najviac 60 videí)."""
        from concurrent.futures import ThreadPoolExecutor
        yt = re.compile(r"^(https?://)?([\w-]+\.)*(youtube\.com|youtu\.be)/")
        urls = [u.strip() for u in urls or [] if yt.match((u or "").strip())][:60]

        def jeden(url: str) -> dict:
            try:
                info = fetch_info(url, log=lambda m: None)
            except Exception as e:  # noqa: BLE001
                return {"url": url, "chyba": common.clean_error(e) or str(e)}
            if not info.get("formats"):
                return {"url": url, "chyba": "Nie je to jedno video."}
            kody: dict[str, bool] = {}
            for t in list_audio_tracks(info):
                k = (t["code"] or "").split("-")[0].lower()
                if k:
                    kody[k] = kody.get(k, False) or bool(t["original"])
            return {"url": url, "nazov": info.get("title") or "", "dlzka": info.get("duration"),
                    "nahlad": yt_thumb(url) or "", "jazyky": list(kody),
                    "original": next((k for k, o in kody.items() if o), None)}

        with ThreadPoolExecutor(2) as ex:
            videa = list(ex.map(jeden, urls))
        pocty: dict[str, list] = {}
        for v in videa:
            for k in v.get("jazyky", []):
                x = pocty.setdefault(k, [0, 0])
                x[0] += 1
                x[1] += int(v.get("original") == k)
        poradie = list(common.DEFAULT_LANGS)
        jazyky = sorted(pocty.items(), key=lambda kv: (-kv[1][0], poradie.index(kv[0]) if kv[0] in poradie else 99))
        return {"preskumanych": sum(1 for v in videa if "chyba" not in v), "videa": videa,
                "jazyky": [{"kod": k, "nazov": lang_label(k), "pocet": c, "original": o} for k, (c, o) in jazyky]}

    def jazyky_stiahnut(self, volby: dict) -> None:
        if self._lang["busy"]:
            raise RuntimeError("Sťahovanie jazykov už beží.")
        if self._lang_info is None:
            raise ValueError("Najprv načítaj jazyky videa.")
        if not shutil.which("ffmpeg"):
            raise RuntimeError("Chýba FFmpeg – spusti Instalacia.bat a potom Kometu znova.")
        by_code = {t["code"]: t for t in self._lang_tracks}
        tracks = [by_code[c] for c in volby.get("poradie") or [] if c in by_code]
        mode = volby.get("rezim") if volby.get("rezim") in ("video", "existing", "audio") else "video"
        if not tracks and not volby.get("original"):
            raise ValueError("Vyber aspoň jeden jazyk.")
        existing = (volby.get("subor") or "").strip()
        if mode == "existing" and not Path(existing).is_file():
            raise ValueError("Vyber svoje stiahnuté video.")
        q = volby.get("kvalita") or "Najlepšia"
        audio_mode = volby.get("zvuk") if volby.get("zvuk") in AUDIO_MODES else "oboje"
        kontajner = "mp4" if volby.get("kontajner") == "mp4" else "mkv"
        self._save(langs=[t["code"] for t in tracks if t["code"] and not t["original"]] or self.cfg.get("langs"),
                   mode=mode, include_original=bool(volby.get("original")),
                   keep_separate=bool(volby.get("samostatne")), max_height=q, audio_mode=audio_mode,
                   kontajner=kontajner, titulky="jazyky" if volby.get("titulky") == "jazyky" else "vypnute",
                   titulky_auto=bool(volby.get("titulky_auto")))
        job = {"info": self._lang_info, "tracks": tracks, "outdir": self._outdir(), "mode": mode,
               "existing_path": existing, "include_original": bool(volby.get("original")),
               "keep_separate": bool(volby.get("samostatne")), "audio_mode": audio_mode, "kontajner": kontajner,
               "zvuky": {str(k): v for k, v in (volby.get("zvuky") or {}).items() if v in AUDIO_MODES},
               "titulky": volby.get("titulky"), "titulky_auto": bool(volby.get("titulky_auto")),
               "max_height": int(q[:-1]) if q.endswith("p") and q[:-1].isdigit() else None}
        self._lang_cancel = threading.Event()
        self._lang.update(busy=True, frac=0.0, text="Pripravujem…", log=[], file="", error="")
        self._lang["rev"] += 1

        def log(m):
            self._lang["log"].append(m)

        def work():
            try:
                out = run_job(job, log=log, progress=lambda f, t: self._lang.update(frac=f, text=t),
                              cancel=self._lang_cancel)
                self._lang.update(busy=False, frac=1.0, text="Hotovo", file=str(out))
            except Cancelled:
                self._lang.update(busy=False, text="Zrušené", error="")
            except Exception as e:  # noqa: BLE001
                if self._lang_cancel.is_set():
                    self._lang.update(busy=False, text="Zrušené")
                else:
                    self._lang.update(busy=False, text="Chyba", error=clean_error(e))
            self._lang["rev"] += 1

        threading.Thread(target=work, daemon=True).start()

    def jazyky_zrusit(self) -> None:
        self._lang_cancel.set()
        self._lang["text"] = "Ruším…"

    # ------------------------------------------------------------------ výber súborov
    def vybrat_priecinok(self) -> str:
        path = self._dialog(folder=True)
        if path:
            self._save(outdir=path)
        return path

    def vybrat_video(self) -> str:
        return self._dialog(folder=False)

    def _dialog(self, folder: bool) -> str:
        if self.window is not None:
            import webview
            fd = getattr(webview, "FileDialog", None)
            kind = (fd.FOLDER if folder else fd.OPEN) if fd else (webview.FOLDER_DIALOG if folder else webview.OPEN_DIALOG)
            kw = {} if folder else {"file_types": ("Video (*.mkv;*.mp4;*.webm;*.mov;*.m4v)", "Všetky súbory (*.*)")}
            res = self.window.create_file_dialog(kind, directory=self.cfg.get("outdir") or "", **kw)
            return (res[0] if isinstance(res, (list, tuple)) else res or "") if res else ""
        try:                                    # okno prehliadača: dialóg cez Tk
            import tkinter as tk
            from tkinter import filedialog
            r = tk.Tk()
            r.withdraw()
            r.attributes("-topmost", True)
            try:
                if folder:
                    return filedialog.askdirectory(initialdir=self.cfg.get("outdir") or None) or ""
                return filedialog.askopenfilename(filetypes=[("Video", "*.mkv *.mp4 *.webm *.mov *.m4v"),
                                                             ("Všetky súbory", "*.*")]) or ""
            finally:
                r.destroy()
        except Exception:  # noqa: BLE001
            return ""

    # ------------------------------------------------------------------ účty
    def _account_text(self, info: dict) -> str:
        return info.get("text") or ""

    def _check_accounts(self) -> None:
        for s in ("webshare", "fastshare", "datoid"):
            user = self.cfg.get(f"{s}_user")
            if not user or not get_password(s, user):
                continue
            self._accounts[s].update(stav="overujem", text="Overujem účet…", user=user)
            try:
                client = getattr(self.accounts, s)()
                info = client.user_info()
                self._accounts[s].update(stav="ok" if info.get("vip") else "bez_vip", text=self._account_text(info))
            except Exception as e:  # noqa: BLE001
                self._accounts[s].update(stav="chyba", text=clean_error(e))

    def prihlasit(self, sluzba: str, meno: str, heslo: str) -> dict:
        if sluzba not in self._accounts:
            raise ValueError("Neznáma služba.")
        meno = (meno or "").strip()
        if not meno:
            raise ValueError("Zadaj prihlasovacie meno.")
        heslo = heslo or get_password(sluzba, meno) or ""
        if not heslo:
            raise ValueError("Zadaj heslo.")
        self._accounts[sluzba].update(stav="overujem", text="Prihlasujem…", user=meno)
        try:
            if sluzba == "webshare":
                client = WebshareClient()
                token = client.login(meno, heslo)
                info = client.user_info()
                self.cfg["webshare_token"] = token
            elif sluzba == "fastshare":
                client = FastshareClient()
                client.login(meno, heslo)
                info = client.user_info()
            else:
                client = DatoidClient()
                client.login(meno, heslo)
                info = client.user_info()
        except Exception as e:  # noqa: BLE001
            self._accounts[sluzba].update(stav="chyba", text=clean_error(e))
            raise
        set_password(sluzba, meno, heslo)
        self._save(**{f"{sluzba}_user": meno})
        self.accounts.reset(sluzba)
        self._accounts[sluzba].update(stav="ok" if info.get("vip") else "bez_vip", text=self._account_text(info))
        failed = [i for i in self.dq.ids() if (self.dq.snapshot(i) or {}).get("status") == "chyba"
                  and (self.dq.snapshot(i) or {}).get("source", "").lower() == sluzba]
        self.dq.start(failed)
        return self._accounts[sluzba]

    def odhlasit(self, sluzba: str) -> None:
        user = self.cfg.get(f"{sluzba}_user") or ""
        if user:
            delete_password(sluzba, user)
        changes = {f"{sluzba}_user": ""}
        if sluzba == "webshare":
            changes["webshare_token"] = ""
        self._save(**changes)
        self.accounts.reset(sluzba)
        self._accounts[sluzba].update(stav="neprihlaseny", text="", user="")

    # ------------------------------------------------------------------ nastavenia
    def ulozit_nastavenia(self, zmeny: dict) -> dict:
        clean = {}
        for k, v in (zmeny or {}).items():
            if k in _SETTINGS:
                try:
                    clean[k] = _SETTINGS[k](v)
                except (TypeError, ValueError):
                    continue
        self._save(**clean)
        self.dq.schedule()
        return {k: self.cfg.get(k) for k in _SETTINGS}

    def _save(self, **changes) -> None:
        self.cfg.update(changes)
        save_config(self.cfg)
        apply_settings(self.cfg)

    def zavriet(self) -> None:
        self.dq.shutdown()


API_METHODS = ["init", "stav", "pridat", "rozpoznat", "kolekcia", "pridat_vybrane", "skenovat", "pridat_zo_skenu",
               "pozastavit", "spustit", "odstranit", "vycistit_hotove", "pozastavit_vsetko", "spustit_vsetko",
               "vycistit_front", "subory_na_zmazanie", "zmazat_subory", "zadat_heslo", "rozbalit", "otvorit",
               "otvorit_subor", "schranka",
               "jazyky_nacitat", "jazyky_zistit", "jazyky_stiahnut", "jazyky_zrusit",
               "vybrat_priecinok", "vybrat_video", "prihlasit", "odhlasit", "ulozit_nastavenia",
               "aktualizacia_skontroluj", "aktualizacia_instaluj", "aktualizacia_pri_starte"]


def call(jadro: Jadro, name: str, args: list) -> dict:
    """Jednotná odpoveď pre UI: {"ok": true, "data": …} alebo {"ok": false, "chyba": "…"}."""
    if name not in API_METHODS:
        return {"ok": False, "chyba": "Neznáma akcia."}
    try:
        return {"ok": True, "data": getattr(jadro, name)(*(args or []))}
    except Exception as e:  # noqa: BLE001 – chybu ukáže UI
        msg = clean_error(e) or e.__class__.__name__
        return {"ok": False, "chyba": msg.splitlines()[0][:300] if "Traceback" not in msg else "Neočakávaná chyba.",
                "podrobnosti": msg[:2000]}


class Api:
    """Objekt pre pywebview: len metódy z API_METHODS, žiadne vnútorné objekty (tie by pywebview sprístupnilo)."""

    def __init__(self, jadro: Jadro):
        self._jadro = jadro


for _name in API_METHODS:
    def _make(name):
        def method(self, *args):
            return call(self._jadro, name, list(args))
        method.__name__ = name
        return method
    setattr(Api, _name, _make(_name))


# --------------------------------------------------------------------------- #
#  Záložný režim: lokálny server + okno prehliadača
# --------------------------------------------------------------------------- #

class _Server(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, addr, jadro: Jadro, token: str):
        super().__init__(addr, _Handler)
        self.jadro, self.token = jadro, token


class _Handler(BaseHTTPRequestHandler):
    server: _Server

    def log_message(self, *a):  # ticho
        pass

    def _host_ok(self) -> bool:     # ochrana pred DNS rebinding
        host = (self.headers.get("Host") or "").split(":")[0]
        return host in ("127.0.0.1", "localhost")

    def _send(self, code: int, body: bytes, ctype: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):  # noqa: N802
        if not self._host_ok():
            return self._send(403, b"", "text/plain")
        rel = self.path.split("?", 1)[0].lstrip("/") or "index.html"
        root = WEBUI.resolve()
        target = (root / rel).resolve()
        if root not in target.parents and target != root or not target.is_file():
            return self._send(404, b"", "text/plain")
        ctype = mimetypes.guess_type(target.name)[0] or "application/octet-stream"
        if target.suffix == ".js":
            ctype = "text/javascript"
        self._send(200, target.read_bytes(), ctype + ("; charset=utf-8" if ctype.startswith("text/") else ""))

    def do_POST(self):  # noqa: N802
        if not self._host_ok() or not secrets.compare_digest(self.headers.get("X-Kometa") or "", self.server.token):
            return self._send(403, b"", "text/plain")
        name = self.path.rsplit("/", 1)[-1]
        try:
            n = min(int(self.headers.get("Content-Length") or 0), 4_000_000)
            args = json.loads(self.rfile.read(n) or b"[]")
            if not isinstance(args, list):
                raise ValueError
        except Exception:  # noqa: BLE001
            return self._send(400, b"", "text/plain")
        body = json.dumps(call(self.server.jadro, name, args), ensure_ascii=False).encode("utf-8")
        self._send(200, body, "application/json; charset=utf-8")


def serve(jadro: Jadro, port: int = 0) -> tuple[_Server, str]:
    token = secrets.token_urlsafe(16)
    srv = _Server(("127.0.0.1", port), jadro, token)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, f"http://127.0.0.1:{srv.server_address[1]}/#t={token}"


def _app_browser() -> str | None:
    cands = []
    if os.name == "nt":
        for base in (os.environ.get("ProgramFiles(x86)"), os.environ.get("ProgramFiles"), os.environ.get("LOCALAPPDATA")):
            if base:
                cands += [Path(base) / "Microsoft/Edge/Application/msedge.exe",
                          Path(base) / "Google/Chrome/Application/chrome.exe"]
    elif sys.platform == "darwin":
        cands += [Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                  Path("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                  Path("/Applications/Chromium.app/Contents/MacOS/Chromium")]
    for c in cands:
        if c.is_file():
            return str(c)
    for name in ("microsoft-edge", "google-chrome", "chromium", "chromium-browser", "msedge", "chrome"):
        if shutil.which(name):
            return shutil.which(name)
    return None


def run_browser(jadro: Jadro, port: int = 0, open_window: bool = True) -> None:
    srv, url = serve(jadro, port)
    print(f"Kometa beží na {url.split('#')[0]}", flush=True)
    proc = None
    if open_window:
        exe = _app_browser()
        if exe:
            profile = common.CONFIG_DIR / "okno"
            proc = subprocess.Popen([exe, f"--app={url}", f"--user-data-dir={profile}", "--no-first-run",
                                     "--no-default-browser-check", "--window-size=1240,820"])
        else:
            webbrowser.open(url)
    try:
        if proc is not None:
            proc.wait()                      # okno zatvorené → koniec
        else:
            while time.time() - jadro.last_call < 45:   # karta prehliadača zatvorená → koniec
                time.sleep(2)
    except KeyboardInterrupt:
        pass
    finally:
        jadro.zavriet()
        srv.shutdown()


def run(force_browser: bool = False, port: int = 0, open_window: bool = True) -> None:
    if not (WEBUI / "index.html").is_file():
        raise SystemExit("Chýba rozhranie kometa/webui – spusti Kometu s voľbou --klasicke.")
    jadro = Jadro()
    if not force_browser:
        try:
            import webview
        except Exception:  # noqa: BLE001
            webview = None
        if webview is not None:
            bg = "#10172A" if jadro.cfg.get("rezim") == "noc" else "#F2F5FA"
            win = webview.create_window("Kometa", str(WEBUI / "index.html"), js_api=Api(jadro), width=1240,
                                        height=820, min_size=(900, 600), background_color=bg, text_select=False)
            jadro.window = win
            win.events.closing += lambda: jadro.zavriet()
            try:
                webview.start(http_server=True, private_mode=True)
                return
            except Exception:  # noqa: BLE001 – napr. chýba WebView2 alebo GTK → okno prehliadača
                jadro.window = None
    run_browser(jadro, port, open_window)
