"""Spoločné pomôcky: nastavenia, cesty, uloženie hesiel, formátovanie."""

from __future__ import annotations

import base64
import json
import os
import re
import shutil
import subprocess
import sys
import threading
from pathlib import Path

APP_NAME = "Kometa"
APP_ID = "kometa"
CONFIG_DIR = Path.home() / ".kometa"
CONFIG_PATH = CONFIG_DIR / "nastavenia.json"
QUEUE_PATH = CONFIG_DIR / "fronta.json"
DEFAULT_LANGS = ["cs", "sk", "en", "pl", "hu", "ru"]   # CZ, SK, EN, PL, HU, RU – ostatné vybrané za nimi
_OLD_DEFAULT_LANGS = ["cs", "sk", "en", "en-US", "pl", "hu"]
USER_AGENT = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
              "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")

# Ďalšie voľby pre yt-dlp (napr. cookies z prehliadača); nastavuje ich apply_settings().
EXTRA_YDL_OPTS: dict = {}
AUDIO_MODE = "oboje"   # zvuk vo videách: „oboje“ (AAC + Opus), „aac“, „opus“ – nastavuje apply_settings()
# čo vložiť do stiahnutých súborov: údaje o videu, kapitoly, obrázok – nastavuje apply_settings()
VLOZIT = {"udaje": True, "kapitoly": True, "obal": True}


class Cancelled(Exception):
    """Používateľ úlohu zrušil."""


class Paused(Exception):
    """Sťahovanie bolo pozastavené – rozpracovaný súbor ostáva a dá sa v ňom pokračovať."""


# --------------------------------------------------------------------------- #
#  Programy (FFmpeg, Deno) a cesty
# --------------------------------------------------------------------------- #

def _extend_path() -> None:
    """Pridá priečinky, kam winget/deno inštalujú programy (ak PATH ešte nie je obnovený)."""
    here = Path(__file__).resolve().parent.parent
    extra = [here, here / "bin"]
    if os.name == "nt":
        local = os.environ.get("LOCALAPPDATA", "")
        if local:
            extra.append(Path(local) / "Microsoft" / "WinGet" / "Links")
        extra.append(Path.home() / ".deno" / "bin")
    current = os.environ.get("PATH", "")
    add = [str(p) for p in extra if p.is_dir() and str(p) not in current]
    if add:
        os.environ["PATH"] = os.pathsep.join(add + [current])


_extend_path()

NO_WINDOW = {"creationflags": 0x08000000} if os.name == "nt" else {}  # CREATE_NO_WINDOW
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def clean_error(e) -> str:
    return ANSI.sub("", str(e)).replace("ERROR: ", "").strip()


def check_dependencies() -> dict:
    try:
        from yt_dlp.version import __version__ as ytdlp
    except Exception:
        ytdlp = None
    import importlib.util
    has_keyring = importlib.util.find_spec("keyring") is not None
    return {
        "yt-dlp": ytdlp,
        "ffmpeg": shutil.which("ffmpeg"),
        "js": shutil.which("deno") or shutil.which("node"),
        "keyring": has_keyring,
    }


_FOLDERID = {"videos": "18989B1D-99B5-455B-841C-AB7C74E4DDFC", "downloads": "374DE290-123F-4565-9164-39C4925E467B"}


def known_folder(kind: str) -> Path | None:
    """Systémový priečinok Videá/Stiahnuté – vo Windows aj presmerovaný (napr. na D:),
    v Linuxe podľa XDG (aj preložený „Videá“), v macOS „Movies“."""
    if os.name == "nt":
        try:
            import ctypes
            import uuid
            from ctypes import wintypes

            class GUID(ctypes.Structure):
                _fields_ = [("d1", wintypes.DWORD), ("d2", wintypes.WORD), ("d3", wintypes.WORD), ("d4", ctypes.c_ubyte * 8)]

            u = uuid.UUID(_FOLDERID[kind])
            g = GUID(u.time_low, u.time_mid, u.time_hi_version, (ctypes.c_ubyte * 8).from_buffer_copy(u.bytes[8:]))
            p = ctypes.c_wchar_p()
            hr = ctypes.windll.shell32.SHGetKnownFolderPath(ctypes.byref(g), 0, None, ctypes.byref(p))
            try:
                return Path(p.value) if hr == 0 and p.value else None
            finally:
                ctypes.windll.ole32.CoTaskMemFree(p)
        except Exception:
            return None
    if sys.platform == "darwin":
        return Path.home() / ("Movies" if kind == "videos" else "Downloads")
    key = "XDG_VIDEOS_DIR" if kind == "videos" else "XDG_DOWNLOAD_DIR"
    conf = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "user-dirs.dirs"
    try:
        for line in conf.read_text(encoding="utf-8").splitlines():
            m = re.match(rf'\s*{key}\s*=\s*"?([^"]*)"?', line)
            if m:
                path = Path(m.group(1).replace("$HOME", str(Path.home())))
                return None if path == Path.home() else path
    except OSError:
        pass
    return Path.home() / ("Videos" if kind == "videos" else "Downloads")


def default_outdir() -> Path:
    videos = known_folder("videos")
    if videos and videos.is_dir():
        return videos / APP_NAME
    downloads = known_folder("downloads")
    return (downloads if downloads and downloads.is_dir() else Path.home() / "Downloads") / APP_NAME


def _same_path(a: str, b: Path) -> bool:
    norm = lambda x: str(x).replace("\\", "/").rstrip("/")  # noqa: E731
    return norm(a).lower() == norm(b).lower() if os.name == "nt" else norm(a) == norm(b)


# --------------------------------------------------------------------------- #
#  Súbory a formátovanie
# --------------------------------------------------------------------------- #

def safe_name(text: str, limit: int = 140) -> str:
    """Názov súboru bezpečný pre Windows (bez : " / \\ | ? * < >)."""
    text = (text or "").translate(str.maketrans({":": " -", '"': "'", "/": "-", "\\": "-", "|": "-",
                                                 "?": "", "*": "", "<": "(", ">": ")"}))
    text = re.sub(r"[\x00-\x1f]", "", text)
    text = re.sub(r"\s+", " ", text).strip().rstrip(". ")
    if len(text) > limit:
        stem, dot, ext = text.rpartition(".")
        if dot and 1 <= len(ext) <= 5:
            text = stem[:limit - len(ext) - 1].rstrip(". ") + "." + ext
        else:
            text = text[:limit].rstrip(". ")
    return text or "subor"


def reserve_path(path: Path) -> Path:
    """Voľný názov súboru, ktorý si hneď „zaberie“ (prázdny súbor) – dve súbežné sťahovania
    videí s rovnakým názvom si tak nikdy nevyberú ten istý."""
    path.parent.mkdir(parents=True, exist_ok=True)
    n = 1
    while True:
        cand = path if n == 1 else path.with_name(f"{path.stem} ({n}){path.suffix}")
        try:
            with open(cand, "x"):
                return cand
        except FileExistsError:
            n += 1


def unique_path(path: Path) -> Path:
    if not path.exists():
        return path
    n = 2
    while True:
        cand = path.with_name(f"{path.stem} ({n}){path.suffix}")
        if not cand.exists():
            return cand
        n += 1


def open_folder(path: Path, select: Path | None = None) -> None:
    path = Path(path)
    try:
        if os.name == "nt":
            if select and Path(select).exists():
                subprocess.Popen(["explorer", "/select,", str(select)])
            else:
                os.startfile(str(path))  # type: ignore[attr-defined]
        elif sys.platform == "darwin":
            subprocess.Popen(["open", str(path)])
        else:
            subprocess.Popen(["xdg-open", str(path)])
    except Exception:
        pass


def format_duration(seconds) -> str:
    try:
        s = int(seconds)
    except (TypeError, ValueError):
        return ""
    h, s = divmod(s, 3600)
    m, s = divmod(s, 60)
    return f"{h}:{m:02d}:{s:02d}" if h else f"{m}:{s:02d}"


def human_size(n) -> str:
    if not n:
        return ""
    n = float(n)
    for unit in ("B", "kB", "MB", "GB", "TB"):
        if n < 1024 or unit == "TB":
            return f"{n:.0f} {unit}" if unit in ("B", "kB") else f"{n:.1f} {unit}".replace(".", ",")
        n /= 1024
    return ""


def human_speed(bps) -> str:
    return f"{human_size(bps)}/s" if bps else ""


def human_eta(seconds) -> str:
    if not seconds or seconds < 0 or seconds > 7 * 86400:
        return ""
    return format_duration(seconds)


# --------------------------------------------------------------------------- #
#  yt-dlp
# --------------------------------------------------------------------------- #

class YDLLogger:
    """Presmeruje hlásenia yt-dlp do nášho logu (bez technického šumu)."""

    def __init__(self, log):
        self.log = log

    def debug(self, msg):
        pass

    def info(self, msg):
        pass

    def warning(self, msg):
        msg = ANSI.sub("", str(msg)).replace("WARNING: ", "").strip()
        if "JavaScript runtime" in msg:
            msg = "Chýba Deno (JavaScript runtime) – niektoré formáty nemusia byť dostupné. Spusti Instalacia.bat."
        self.log("⚠ " + msg)

    def error(self, msg):
        self.log("✖ " + clean_error(msg))


def ydl_opts(log=print, **extra) -> dict:
    opts = {
        "quiet": True,
        "no_warnings": False,
        "noprogress": True,
        "noplaylist": True,
        "logger": YDLLogger(log),
        "retries": 10,
        "fragment_retries": 10,
        "continuedl": True,
        "windowsfilenames": True,
        "js_runtimes": {"deno": {}, "node": {}},
    }
    opts.update(EXTRA_YDL_OPTS)
    opts.update(extra)
    return opts


# --------------------------------------------------------------------------- #
#  Nastavenia
# --------------------------------------------------------------------------- #

DEFAULT_CONFIG = {
    "outdir": "",
    # karta Jazyky
    "langs": DEFAULT_LANGS,
    "mode": "video",
    "include_original": True,
    "audio_mode": "oboje",        # oboje = AAC (predvolená) + Opus, aac, opus
    "keep_separate": False,
    "kontajner": "mkv",           # mkv | mp4 (MP4: obraz H.264 a zvuk AAC)
    "max_height": "Najlepšia",
    # karta Sťahovanie
    "dl_preset": "video",
    "parallel": 2,
    "connections": 4,
    "autoresume": True,
    "cookies_browser": "",
    # vzhľad (nové rozhranie)
    "tema": "kometa",
    "rezim": "auto",              # auto = podľa systému, den, noc
    # účty (heslá sú v Správcovi poverení Windows, nie tu)
    "webshare_user": "",
    "webshare_token": "",
    "fastshare_user": "",
    "datoid_user": "",
    # archívy po stiahnutí
    "rozbalovat": True,
    "po_rozbaleni": "nechat",     # nechat | kos
    # titulky k jazykovým stopám
    "titulky": "vypnute",          # vypnute | jazyky
    "titulky_auto": False,
    # do stiahnutých súborov vložiť údaje o videu, kapitoly a obrázok
    "vlozit_udaje": True,
    "vlozit_kapitoly": True,
    "vlozit_obal": True,
    # kontrolovať nové verzie a inštalovať ich
    "aktualizacie": True,
}

_cfg_lock = threading.Lock()


def load_config() -> dict:
    cfg = json.loads(json.dumps(DEFAULT_CONFIG))
    try:
        cfg.update(json.loads(CONFIG_PATH.read_text(encoding="utf-8")))
    except Exception:
        pass
    if cfg.get("langs") == _OLD_DEFAULT_LANGS:
        cfg["langs"] = list(DEFAULT_LANGS)
    new = default_outdir()
    if not cfg.get("outdir"):
        cfg["outdir"] = str(new)
    elif not _same_path(cfg["outdir"], new) and any(
            _same_path(cfg["outdir"], Path.home() / d / APP_NAME) for d in ("Videos", "Downloads")):
        # predvolený priečinok starších verzií (pevne ~/Videos) – systém ho medzitým presmeroval
        cfg["outdir"] = str(new)
        save_config(cfg)
    return cfg


def save_config(cfg: dict) -> None:
    with _cfg_lock:
        try:
            CONFIG_DIR.mkdir(parents=True, exist_ok=True)
            tmp = CONFIG_PATH.with_suffix(".tmp")
            tmp.write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")
            tmp.replace(CONFIG_PATH)
        except Exception:
            pass


def apply_settings(cfg: dict) -> None:
    """Premietne nastavenia do yt-dlp (cookies z prehliadača pre Instagram/Facebook)."""
    global AUDIO_MODE
    AUDIO_MODE = cfg.get("audio_mode") if cfg.get("audio_mode") in ("oboje", "aac", "opus") else "oboje"
    for k in VLOZIT:
        VLOZIT[k] = bool(cfg.get(f"vlozit_{k}", True))
    EXTRA_YDL_OPTS.pop("cookiesfrombrowser", None)
    # HLS/DASH: viac kúskov naraz (rovnaké nastavenie ako „spojenia na súbor“)
    EXTRA_YDL_OPTS["concurrent_fragment_downloads"] = max(1, min(16, int(cfg.get("connections") or 4)))
    browser = (cfg.get("cookies_browser") or "").strip().lower()
    if browser:
        EXTRA_YDL_OPTS["cookiesfrombrowser"] = (browser,)


# --------------------------------------------------------------------------- #
#  Heslá – Správca poverení Windows (keyring), inak zakódované v nastaveniach
# --------------------------------------------------------------------------- #

def _keyring():
    try:
        import keyring
        keyring.get_keyring()
        return keyring
    except Exception:
        return None


SECRETS_PATH = CONFIG_DIR / "ucty.dat"


def _secrets_file() -> dict:
    try:
        return json.loads(base64.b64decode(SECRETS_PATH.read_bytes()).decode("utf-8"))
    except Exception:
        return {}


def _secrets_save(data: dict) -> None:
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    SECRETS_PATH.write_bytes(base64.b64encode(json.dumps(data).encode("utf-8")))


def set_password(service: str, user: str, password: str) -> None:
    kr = _keyring()
    if kr:
        try:
            kr.set_password(f"{APP_ID}:{service}", user, password)
            return
        except Exception:
            pass
    data = _secrets_file()
    data[f"{service}:{user}"] = password
    _secrets_save(data)


def get_password(service: str, user: str) -> str | None:
    if not user:
        return None
    kr = _keyring()
    if kr:
        try:
            pw = kr.get_password(f"{APP_ID}:{service}", user)
            if pw is not None:
                return pw
        except Exception:
            pass
    return _secrets_file().get(f"{service}:{user}")


def delete_password(service: str, user: str) -> None:
    kr = _keyring()
    if kr:
        try:
            kr.delete_password(f"{APP_ID}:{service}", user)
        except Exception:
            pass
    data = _secrets_file()
    if data.pop(f"{service}:{user}", None) is not None:
        _secrets_save(data)
