"""Videá zo stránok (YouTube, Facebook, Instagram, TikTok a ďalších 1000+) cez yt-dlp.

yt-dlp si rozpracované časti ukladá ako .part a pri ďalšom spustení v nich pokračuje.
TikTok sa sťahuje vo verzii bez vodoznaku (verzie s vodoznakom sú vylúčené).
"""

from __future__ import annotations

import re
import threading
from pathlib import Path
from urllib.parse import urlparse

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


def download_media(url: str, outdir: Path, preset: str = "video", *, on_progress=None, on_title=None,
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
