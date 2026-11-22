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
DEFAULT_LANGS = ["cs", "sk", "en", "en-US", "pl", "hu"]
USER_AGENT = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
              "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")

# Ďalšie voľby pre yt-dlp (napr. cookies z prehliadača); nastavuje ich apply_settings().
EXTRA_YDL_OPTS: dict = {}


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
    try:
        import keyring  # noqa: F401
        has_keyring = True
    except Exception:
        has_keyring = False
    return {
        "yt-dlp": ytdlp,
        "ffmpeg": shutil.which("ffmpeg"),
        "js": shutil.which("deno") or shutil.which("node"),
        "keyring": has_keyring,
    }


def default_outdir() -> Path:
    videos = Path.home() / "Videos"
    return (videos if videos.is_dir() else Path.home() / "Downloads") / APP_NAME


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
    "keep_separate": False,
    "max_height": "Najlepšia",
    # karta Sťahovanie
    "dl_preset": "video",
    "parallel": 2,
    "autoresume": True,
    "cookies_browser": "",
    # účty (heslá sú v Správcovi poverení Windows, nie tu)
    "webshare_user": "",
    "webshare_token": "",
    "fastshare_user": "",
}

_cfg_lock = threading.Lock()


def load_config() -> dict:
    cfg = json.loads(json.dumps(DEFAULT_CONFIG))
    try:
        cfg.update(json.loads(CONFIG_PATH.read_text(encoding="utf-8")))
    except Exception:
        pass
    if not cfg.get("outdir"):
        cfg["outdir"] = str(default_outdir())
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
    EXTRA_YDL_OPTS.pop("cookiesfrombrowser", None)
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
