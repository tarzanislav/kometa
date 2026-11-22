"""Videá zo stránok (YouTube, Facebook, Instagram, TikTok a ďalších 1000+) cez yt-dlp.

yt-dlp si rozpracované časti ukladá ako .part a pri ďalšom spustení v nich pokračuje.
TikTok sa sťahuje vo verzii bez vodoznaku (verzie s vodoznakom sú vylúčené).
"""

from __future__ import annotations

import re
import threading
from pathlib import Path
from urllib.parse import urlparse

from . import common
from .common import Paused, ydl_opts

# kľúč: (popis, voľby pre yt-dlp)
PRESETS: dict[str, tuple[str, dict]] = {
    "video": ("Video – najlepšia kvalita", {"format": "bv*+ba/b", "merge_output_format": "mp4/mkv"}),
    "mp4": ("Video – MP4 pre TV a mobil (H.264)", {"format": "bv*+ba/b", "merge_output_format": "mp4",
                                                   "format_sort": ["vcodec:h264", "res", "acodec:m4a"]}),
    "1080": ("Video – najviac 1080p", {"format": "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b",
                                       "merge_output_format": "mp4/mkv"}),
    "720": ("Video – najviac 720p", {"format": "bv*[height<=720]+ba/b[height<=720]/bv*+ba/b",
                                     "merge_output_format": "mp4/mkv"}),
    "mp3": ("Zvuk – MP3", {"format": "ba/b", "postprocessors": [
        {"key": "FFmpegExtractAudio", "preferredcodec": "mp3", "preferredquality": "192"}]}),
    "audio": ("Zvuk – pôvodná kvalita (M4A/Opus)", {"format": "ba[ext=m4a]/ba/b"}),
}

SOURCES = [
    ("YouTube", r"(youtube\.com|youtu\.be)/"),
    ("TikTok", r"tiktok\.com/"),
    ("Instagram", r"instagram\.com/"),
    ("Facebook", r"(facebook\.com|fb\.watch|fb\.com)/"),
    ("Webshare", r"webshare\.cz/"),
    ("Fastshare", r"fastshare\.(cz|cloud)/"),
    ("X/Twitter", r"(twitter\.com|x\.com)/"),
    ("Vimeo", r"vimeo\.com/"),
]


def source_name(url: str) -> str:
    for name, pat in SOURCES:
        if re.search(pat, url, re.I):
            return name
    return "Web"


def _no_watermark(fmt: str) -> str:
    """K formátu pridá podmienku „bez vodoznaku“ (TikTok); ak by nič nevyhovelo, použije sa pôvodný."""
    parts = [p for p in fmt.split("/") if p]
    filtered = ["+".join(t + "[format_note!*=?watermarked]" for t in p.split("+")) for p in parts]
    return "/".join(filtered + parts)


class Unsupported(Exception):
    """yt-dlp stránku nepozná – skúsi sa priame stiahnutie súboru."""


def download_media(url: str, outdir: Path, preset: str = "video", *, on_progress=None, on_title=None, on_meta=None,
                   stop: threading.Event | None = None, log=print, tmp_files: set | None = None,
                   referer: str | None = None, password: str | None = None) -> Path:
    """Stiahne video/zvuk a vráti cestu k výslednému súboru."""
    from yt_dlp import YoutubeDL
    from yt_dlp.utils import DownloadError, UnsupportedError

    stop = stop or threading.Event()
    outdir = Path(outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    _, preset_opts = PRESETS.get(preset, PRESETS["video"])
    preset_opts = dict(preset_opts)
    if preset in ("video", "1080", "720"):
        vsel = preset_opts["format"].split("+")[0]
        if common.AUDIO_MODE == "oboje":
            # AAC (prehrá každý prehrávač, prvá = predvolená) + Opus (vyššia kvalita) v jednom MKV
            preset_opts["format"] = (f"{vsel}+ba[acodec^=mp4a]+ba[acodec^=opus]/{vsel}+ba[acodec^=mp4a]/"
                                     + preset_opts["format"])
            preset_opts["allow_multiple_audio_streams"] = True
            preset_opts["merge_output_format"] = "mkv"
        elif common.AUDIO_MODE == "aac":
            preset_opts["format"] = f"{vsel}+ba[acodec^=mp4a]/" + preset_opts["format"]
    if source_name(url) == "TikTok":
        preset_opts["format"] = _no_watermark(preset_opts["format"])

    state = {"done": 0, "total": None, "title_sent": False}

    def hook(d):
        if stop.is_set():
            raise Paused()
        info = d.get("info_dict") or {}
        if not state["title_sent"] and info.get("title"):
            state["title_sent"] = True
            if on_title:
                on_title(info.get("title"))
            if on_meta:
                on_meta({k: v for k, v in (("thumb", info.get("thumbnail")), ("duration", info.get("duration")),
                                           ("uploader", info.get("uploader") or info.get("channel")))
                         if v})
        if state["total"] is None:
            req = info.get("requested_formats") or [info]
            sizes = [f.get("filesize") or f.get("filesize_approx") for f in req]
            if sizes and all(sizes):
                state["total"] = int(sum(sizes))
        if d.get("tmpfilename") and tmp_files is not None:
            tmp_files.add(d["tmpfilename"])
        if d.get("status") == "downloading":
            cur = d.get("downloaded_bytes") or 0
            this_total = d.get("total_bytes") or d.get("total_bytes_estimate")
            total = state["total"] or ((state["done"] + this_total) if this_total else None)
            if on_progress:
                on_progress(state["done"] + cur, total, d.get("speed"))
        elif d.get("status") == "finished":
            state["done"] += d.get("total_bytes") or d.get("downloaded_bytes") or 0

    extra = dict(preset_opts)
    if referer:   # vložené prehrávače (Vimeo s obmedzením na doménu) chcú adresu stránky, kde sú vložené
        extra["http_headers"] = {"Referer": referer, "Origin": "{0.scheme}://{0.netloc}".format(urlparse(referer))}
    if password:
        extra["videopassword"] = password
    opts = ydl_opts(log, outtmpl={"default": str(outdir / "%(title).120B [%(id)s].%(ext)s")},
                    progress_hooks=[hook], **extra)
    try:
        with YoutubeDL(opts) as ydl:
            info = ydl.extract_info(url, download=True)
            if info.get("_type") == "playlist":
                entries = [e for e in (info.get("entries") or []) if e]
                info = entries[0] if entries else info
            downloads = info.get("requested_downloads") or []
            path = downloads[0].get("filepath") if downloads else None
            path = path or ydl.prepare_filename(info)
    except Paused:
        raise
    except UnsupportedError as e:
        raise Unsupported(str(e)) from e
    except DownloadError as e:
        if stop.is_set():
            raise Paused() from e
        if "Unsupported URL" in str(e) or "No video formats found" in str(e):
            raise Unsupported(str(e)) from e
        raise
    return Path(path)


# --------------------------------------------------------------------------- #
#  Playlisty a kanály
# --------------------------------------------------------------------------- #

_YT_CHANNEL_RE = re.compile(r"^(https?://(?:www\.|m\.)?youtube\.com/(?:@[\w.%-]+|channel/[\w-]+|c/[\w.%-]+|user/[\w.%-]+))"
                            r"(/(?:videos|shorts|streams|playlists|featured|live)?)?/?(?:[?#].*)?$", re.I)
_COLLECTION_RES = [
    re.compile(r"youtube\.com/playlist\?", re.I),
    _YT_CHANNEL_RE,
    re.compile(r"vimeo\.com/(?:showcase|album)/\d+", re.I),
    re.compile(r"vimeo\.com/channels/[\w-]+/?(?:[?#].*)?$", re.I),
    re.compile(r"soundcloud\.com/[\w-]+/sets/", re.I),
    re.compile(r"dailymotion\.com/playlist/", re.I),
]


def is_collection(url: str) -> bool:
    """Playlist alebo kanál (nie jedno video). Video z playlistu (watch?v=…&list=…) je jedno video."""
    return any(rx.search(url.strip()) for rx in _COLLECTION_RES)


def collection_url(url: str, tab: str = "videos") -> str:
    """Kanál YouTube bez karty → karta Videá (prípadne Shorts / Naživo)."""
    m = _YT_CHANNEL_RE.match(url.strip())
    if m and not (m.group(2) or "").strip("/"):
        return f"{m.group(1)}/{tab}"
    return url.strip()


def list_collection(url: str, log=print, limit: int | None = None) -> dict:
    """Načíta zoznam videí playlistu/kanála bez sťahovania: {title, uploader, url, entries:[{url,title,duration,…}]}."""
    from yt_dlp import YoutubeDL

    url = collection_url(url)
    opts = ydl_opts(log, extract_flat="in_playlist", noplaylist=False, skip_download=True)
    if limit:
        opts["playlistend"] = limit
    with YoutubeDL(opts) as ydl:
        info = ydl.extract_info(url, download=False)
    entries = []
    for e in info.get("entries") or []:
        if not e or e.get("_type") == "playlist":
            continue
        u = e.get("url") or e.get("webpage_url") or ""
        if not u.startswith("http"):
            if (e.get("ie_key") or "").startswith("Youtube") and e.get("id"):
                u = f"https://www.youtube.com/watch?v={e['id']}"
            else:
                continue
        entries.append({"url": u, "title": e.get("title") or e.get("id") or u, "duration": e.get("duration"),
                        "id": e.get("id"), "date": e.get("upload_date") or "", "views": e.get("view_count")})
    title = info.get("title") or info.get("playlist_title") or info.get("uploader") or "Playlist"
    title = re.sub(r"\s+-\s+(Videos|Videá|Shorts|Live)$", "", title)
    return {"title": title, "uploader": info.get("uploader") or info.get("channel") or "", "url": url,
            "entries": entries}
