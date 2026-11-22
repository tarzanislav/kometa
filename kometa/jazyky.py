"""Preložené (dabované) zvukové stopy YouTube videa.

Stiahne vybrané jazykové stopy a spojí ich s videom do jedného .mkv
(alebo len zvuk do .mka, prípadne pridá stopy k už stiahnutému videu).
"""

from __future__ import annotations

import argparse
import copy
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import unicodedata
from pathlib import Path

from .common import (DEFAULT_LANGS, NO_WINDOW, Cancelled, check_dependencies, format_duration,
                     load_config, safe_name, unique_path, ydl_opts)

# --------------------------------------------------------------------------- #
#  Jazyky
# --------------------------------------------------------------------------- #

LANG_NAMES = {
    "en": "Angličtina", "en-us": "Angličtina (USA)", "en-gb": "Angličtina (Británia)",
    "en-in": "Angličtina (India)", "en-au": "Angličtina (Austrália)", "en-ca": "Angličtina (Kanada)",
    "cs": "Čeština", "sk": "Slovenčina", "pl": "Poľština", "hu": "Maďarčina",
    "ru": "Ruština", "uk": "Ukrajinčina", "be": "Bieloruština",
    "de": "Nemčina", "fr": "Francúzština", "fr-ca": "Francúzština (Kanada)",
    "es": "Španielčina", "es-419": "Španielčina (Latinská Amerika)", "es-us": "Španielčina (USA)",
    "es-es": "Španielčina (Španielsko)", "es-mx": "Španielčina (Mexiko)",
    "it": "Taliančina", "pt": "Portugalčina", "pt-br": "Portugalčina (Brazília)",
    "pt-pt": "Portugalčina (Portugalsko)", "nl": "Holandčina", "ro": "Rumunčina",
    "bg": "Bulharčina", "hr": "Chorvátčina", "sr": "Srbčina", "sl": "Slovinčina",
    "el": "Gréčtina", "tr": "Turečtina", "ar": "Arabčina", "he": "Hebrejčina",
    "fa": "Perzština", "hi": "Hindčina", "bn": "Bengálčina", "ur": "Urdčina",
    "ta": "Tamilčina", "te": "Telugčina", "ml": "Malajálamčina", "mr": "Maráthčina",
    "id": "Indonézština", "ms": "Malajčina", "fil": "Filipínčina", "th": "Thajčina",
    "vi": "Vietnamčina", "ja": "Japončina", "ko": "Kórejčina", "zh": "Čínština",
    "zh-hans": "Čínština (zjednodušená)", "zh-hant": "Čínština (tradičná)",
    "zh-cn": "Čínština (Čína)", "zh-tw": "Čínština (Taiwan)",
    "sv": "Švédčina", "da": "Dánčina", "no": "Nórčina", "nb": "Nórčina", "fi": "Fínčina",
    "lt": "Litovčina", "lv": "Lotyština", "et": "Estónčina", "kk": "Kazaština",
}

# ISO 639-2 (bibliografické kódy, ktoré používa Matroska)
ISO639_2 = {
    "en": "eng", "cs": "cze", "sk": "slo", "pl": "pol", "hu": "hun", "ru": "rus",
    "uk": "ukr", "be": "bel", "de": "ger", "fr": "fre", "es": "spa", "it": "ita",
    "pt": "por", "nl": "dut", "ro": "rum", "bg": "bul", "hr": "hrv", "sr": "srp",
    "sl": "slv", "el": "gre", "tr": "tur", "ar": "ara", "he": "heb", "fa": "per",
    "hi": "hin", "bn": "ben", "ur": "urd", "ta": "tam", "te": "tel", "ml": "mal",
    "mr": "mar", "id": "ind", "ms": "may", "fil": "fil", "th": "tha", "vi": "vie",
    "ja": "jpn", "ko": "kor", "zh": "chi", "sv": "swe", "da": "dan", "no": "nor",
    "nb": "nob", "fi": "fin", "lt": "lit", "lv": "lav", "et": "est", "kk": "kaz",
}


def lang_label(code: str, display: str = "") -> str:
    """Pekný slovenský názov jazyka podľa kódu (napr. 'en-US' -> 'Angličtina (USA)')."""
    if not code:
        return display or "Pôvodná stopa"
    c = code.lower()
    suffix = ""
    if c.endswith("-desc"):
        c, suffix = c[:-5], " – zvukový popis"
    name = LANG_NAMES.get(c)
    if not name:
        base, _, region = c.partition("-")
        if base in LANG_NAMES and region:
            name = f"{LANG_NAMES[base]} ({region.upper()})"
        else:
            name = display or code
    return name + suffix


def iso3(code: str) -> str:
    return ISO639_2.get((code or "").lower().split("-")[0], "und")



# --------------------------------------------------------------------------- #
#  Jadro: načítanie stôp, výber, sťahovanie, spojenie
# --------------------------------------------------------------------------- #

def fetch_info(url: str, log=print) -> dict:
    """Načíta informácie o videe vrátane všetkých formátov (bez sťahovania)."""
    from yt_dlp import YoutubeDL

    with YoutubeDL(ydl_opts(log)) as ydl:
        info = ydl.extract_info(url, download=False)
        if info.get("_type") == "playlist" or "formats" not in info:
            raise RuntimeError("Zadaj odkaz na jedno konkrétne video (nie na playlist alebo kanál).")
        return ydl.sanitize_info(info, remove_private_keys=True)


def _is_audio_only(f: dict) -> bool:
    return f.get("vcodec") == "none" and f.get("acodec") not in (None, "none")


def _format_rank(f: dict):
    note = f.get("format_note") or ""
    is_drc = "DRC" in note or "drc" in str(f.get("format_id", ""))
    return (
        not is_drc,                                         # bez kompresie dynamiky
        f.get("protocol", "https") in ("https", "http"),    # priamy súbor, nie HLS
        f.get("abr") or f.get("tbr") or 0,                  # vyšší dátový tok
        str(f.get("acodec", "")).startswith("opus"),
    )


def _is_aac(f: dict) -> bool:
    return str(f.get("acodec", "")).startswith("mp4a") or (f.get("ext") == "m4a")


AUDIO_MODES = {"oboje": "AAC + Opus v jednom súbore (odporúčané)", "aac": "Len AAC – prehrá každý prehrávač",
               "opus": "Len Opus – najvyššia kvalita"}


def _clean_display(note: str) -> str:
    first = (note or "").split(",")[0]
    first = re.sub(r"\((default|original)\)|\boriginal\b", "", first, flags=re.I)
    return re.sub(r"\s+", " ", first).strip()


def list_audio_tracks(info: dict) -> list[dict]:
    """Vráti zoznam jazykových stôp: [{code, label, format, original}], originál prvý."""
    groups: dict[str, list] = {}
    for f in info.get("formats") or []:
        if _is_audio_only(f):
            groups.setdefault(f.get("language") or "", []).append(f)

    tracks = []
    for code, fmts in groups.items():
        best = max(fmts, key=_format_rank)
        aac = [x for x in fmts if _is_aac(x)]
        opus = [x for x in fmts if str(x.get("acodec", "")).startswith("opus")]
        original = any(
            x.get("language_preference") == 10 or "original" in (x.get("format_note") or "").lower()
            for x in fmts
        )
        tracks.append({
            "code": code,
            "label": lang_label(code, _clean_display(best.get("format_note"))),
            "format": best,
            "aac": max(aac, key=_format_rank) if aac else None,     # prehrá každý prehrávač
            "opus": max(opus, key=_format_rank) if opus else None,  # vyššia kvalita
            "original": original,
            "_pref": max((x.get("language_preference") or -1) for x in fmts),
        })

    if tracks and not any(t["original"] for t in tracks):
        top = max(tracks, key=lambda t: t["_pref"])
        if len(tracks) == 1 or top["_pref"] > 0:
            top["original"] = True

    def sort_key(t):  # abecedne bez diakritiky, aby "Čeština" nebola až na konci
        plain = unicodedata.normalize("NFKD", t["label"]).encode("ascii", "ignore").decode().lower()
        return (not t["original"], plain)

    tracks.sort(key=sort_key)
    return tracks


def pick_tracks(tracks: list[dict], wanted: list[str]) -> tuple[list[dict], list[str]]:
    """Vyberie stopy podľa kódov v poradí, v akom boli zadané. Vráti (vybrané, nenájdené)."""
    by_code = {t["code"].lower(): t for t in tracks}
    wanted_l = [w.strip().lower() for w in wanted if w.strip()]
    chosen, missing, seen = [], [], set()
    for w in wanted_l:
        t = by_code.get(w)
        if t is None:  # napr. "cs" nájde "cs-CZ", ak presná zhoda neexistuje
            base = w.split("-")[0]
            cands = [x for x in tracks
                     if x["code"].lower().split("-")[0] == base
                     and x["code"].lower() not in wanted_l
                     and not x["code"].lower().endswith("-desc")]
            # pri angličtine bez „en“ prednostne americká (dabingy bývajú v nej), potom britská
            pref = {"en-us": 0, "en-gb": 1}
            cands.sort(key=lambda x: pref.get(x["code"].lower(), 2))
            t = cands[0] if cands else None
        if t is None:
            missing.append(w)
        elif t["code"] not in seen:
            chosen.append(t)
            seen.add(t["code"])
    return chosen, missing


def download_format(info: dict, fmt: str, out_stem: Path, log, on_progress, cancel: threading.Event) -> Path:
    """Stiahne jeden formát (napr. jednu jazykovú stopu) z už načítaných informácií."""
    from yt_dlp import YoutubeDL

    def hook(d):
        if cancel.is_set():
            raise Cancelled()
        if d.get("status") == "downloading":
            total = d.get("total_bytes") or d.get("total_bytes_estimate") or 0
            done = d.get("downloaded_bytes") or 0
            on_progress(min(done / total, 1.0) if total else None)
        elif d.get("status") == "finished":
            on_progress(1.0)

    opts = ydl_opts(log, format=fmt, outtmpl={"default": str(out_stem) + ".%(ext)s"},
                     progress_hooks=[hook], overwrites=True)
    with YoutubeDL(opts) as ydl:
        try:
            ydl.process_ie_result(copy.deepcopy(info), download=True)
        except Cancelled:
            raise
        except Exception:
            if cancel.is_set():
                raise Cancelled()
            if not info.get("webpage_url"):
                raise
            # odkazy na súbory od YouTube po čase expirujú – načítame ich nanovo
            log("↻ Skúšam znova s čerstvým načítaním odkazu…")
            ydl.download([info["webpage_url"]])

    files = [p for p in out_stem.parent.glob(out_stem.name + ".*")
             if not p.name.endswith((".part", ".ytdl", ".temp")) and p.is_file()]
    if not files:
        raise RuntimeError(f"Súbor sa nestiahol ({fmt}).")
    return max(files, key=lambda p: p.stat().st_size)


def _ffmpeg() -> str:
    exe = shutil.which("ffmpeg")
    if not exe:
        raise RuntimeError("Nenašiel som FFmpeg. Spusti Instalacia.bat (alebo nainštaluj FFmpeg).")
    return exe


def count_audio_streams(path: Path) -> int:
    r = subprocess.run([_ffmpeg(), "-hide_banner", "-i", str(path)],
                       capture_output=True, text=True, encoding="utf-8", errors="replace", **NO_WINDOW)
    return len(re.findall(r"Stream #0:\d+.*?: Audio:", r.stderr))


def mux(out_path: Path, inputs: list[dict], title: str) -> None:
    """Spojí vstupy do jedného súboru bez prekódovania – Matroska (.mkv/.mka) alebo MP4 (.mp4/.m4a) podľa prípony.

    inputs: {"path", "kind": "existing"|"video"|"audio", "lang3", "title", "audio_count"}
    """
    cmd = [_ffmpeg(), "-hide_banner", "-loglevel", "error", "-y"]
    for inp in inputs:
        cmd += ["-i", str(inp["path"])]
    maps, meta, tail = [], [], []
    a = 0
    for i, inp in enumerate(inputs):
        if inp["kind"] == "existing":
            # obraz z vlastného videa hneď, jeho pôvodný zvuk až za nové jazykové stopy
            maps += ["-map", f"{i}:V?"]
            tail += ["-map", f"{i}:a?"]
        elif inp["kind"] == "video":
            maps += ["-map", f"{i}:v:0"]
        else:
            maps += ["-map", f"{i}:a:0"]
            meta += [f"-metadata:s:a:{a}", f"language={inp['lang3']}",
                     f"-metadata:s:a:{a}", f"title={inp['title']}",
                     f"-metadata:s:a:{a}", f"handler_name={inp['title']}"]   # názov stopy v MP4
            a += 1
    for inp in inputs:
        if inp["kind"] == "existing":
            for k in range(inp.get("audio_count", 0)):
                if inp.get("lang3"):
                    meta += [f"-metadata:s:a:{a + k}", f"language={inp['lang3']}"]
                meta += [f"-metadata:s:a:{a + k}", f"title={inp.get('title') or 'Pôvodný zvuk'}"]
            a += inp.get("audio_count", 0)
    maps += tail
    disp = []
    for k in range(a):
        disp += [f"-disposition:a:{k}", "default" if k == 0 else "0"]
    mp4 = out_path.suffix.lower() in (".mp4", ".m4a")
    cmd += maps + ["-c", "copy"] + meta + disp + ["-metadata", f"title={title}"]
    if mp4:
        cmd += ["-movflags", "+faststart"]       # prehrávanie môže začať skôr, než sa súbor celý načíta
    cmd.append(str(out_path))
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", **NO_WINDOW)
    if r.returncode != 0 or not out_path.exists():
        hint = ("MP4 tieto stopy neudrží – vyber formát MKV.\n" if mp4 and re.search(
            r"codec not currently supported|Could not find tag|not supported in container", r.stderr or "") else "")
        raise RuntimeError(hint + "FFmpeg nedokázal spojiť stopy:\n" + (r.stderr or "").strip()[-1500:])


def to_aac(src: Path, dest: Path, bitrate: str = "192k") -> Path:
    """Prevedie zvuk do AAC (prehrá ho každý prehrávač) – len keď ho zdroj neponúka priamo."""
    r = subprocess.run([_ffmpeg(), "-hide_banner", "-loglevel", "error", "-y", "-i", str(src), "-vn", "-map", "0:a:0",
                        "-c:a", "aac", "-b:a", bitrate, str(dest)],
                       capture_output=True, text=True, encoding="utf-8", errors="replace", **NO_WINDOW)
    if r.returncode != 0 or not dest.exists():
        raise RuntimeError("Prevod zvuku do AAC zlyhal:\n" + (r.stderr or "").strip()[-800:])
    return dest


def _export_single(src: Path, dest_stem: Path) -> Path:
    """Samostatná stopa: opus z webm prebalí do .opus (bez prekódovania), ostatné nechá."""
    if src.suffix.lower() == ".webm":
        dest = unique_path(dest_stem.with_suffix(".opus"))
        r = subprocess.run([_ffmpeg(), "-hide_banner", "-loglevel", "error", "-y", "-i", str(src),
                            "-map", "0:a:0", "-c", "copy", str(dest)], capture_output=True, **NO_WINDOW)
        if r.returncode == 0 and dest.exists():
            return dest
    dest = unique_path(dest_stem.with_suffix(src.suffix))
    shutil.copy2(src, dest)
    return dest


def run_job(job: dict, log=print, progress=lambda frac, text: None, cancel: threading.Event | None = None) -> Path:
    """Celý postup: stiahne zvolené stopy (a príp. video) a spojí ich do jedného súboru.

    job: info, tracks, outdir, mode ("audio" | "video" | "existing"), existing_path,
         include_original, keep_separate, max_height, audio_mode ("oboje" | "aac" | "opus") pre všetky jazyky,
         zvuky ({kód: "oboje" | "aac" | "opus"} – voľba pri jednotlivom jazyku),
         kontajner ("mkv" | "mp4" – MP4 má vždy len stopy AAC)

    Poradie stôp: CZ (AAC), CZ (Opus), SK (AAC), SK (Opus)… – prvá je predvolená.
    """
    cancel = cancel or threading.Event()
    info = job["info"]
    mode = job.get("mode", "audio")
    outdir = Path(job["outdir"]).expanduser()
    outdir.mkdir(parents=True, exist_ok=True)
    title = info.get("title") or info.get("id") or "video"

    tracks = list(job["tracks"])
    original = next((t for t in list_audio_tracks(info) if t["original"]), None)
    # pôvodná stopa (napr. ruština) ide až na koniec – prvé sú jazyky v poradí, ako si ich vybral
    if job.get("include_original") and original and all(t["code"] != original["code"] for t in tracks):
        tracks.append(original)
    if not tracks:
        raise RuntimeError("Nie je vybraný žiadny jazyk.")

    existing = Path(job["existing_path"]) if mode == "existing" else None
    if existing is not None and not existing.is_file():
        raise RuntimeError("Vybrané video na disku neexistuje.")

    audio_mode = job.get("audio_mode") or "oboje"
    if audio_mode not in AUDIO_MODES:
        audio_mode = "oboje"
    mp4 = job.get("kontajner") == "mp4"
    per_track = {k: v for k, v in (job.get("zvuky") or {}).items() if v in AUDIO_MODES}

    def track_mode(t):
        # MP4 = najširšia kompatibilita: obraz H.264 a zvuk AAC (Opus by mnohé zariadenia neprehrali)
        return "aac" if mp4 else per_track.get(t["code"], audio_mode)

    total_steps = sum(2 if track_mode(t) == "oboje" else 1 for t in tracks) + (1 if mode == "video" else 0) + 1
    state = {"step": 0, "text": ""}

    def sub(frac):
        base = state["step"] + (frac if frac is not None else 0)
        progress(min(base / total_steps, 1.0), state["text"])

    tmp = Path(tempfile.mkdtemp(prefix=".ytj_tmp_", dir=outdir))
    try:
        video_path = None
        if mode == "video":
            h = job.get("max_height")
            fmt = f"bv[height<={h}]/bv/bv*" if h else "bv/bv*"
            if mp4:   # H.264 (YouTube ho dáva najviac v 1080p); ak chýba, MP4 udrží aj VP9/AV1
                fmt = (f"bv[vcodec^=avc1][height<={h}]/bv[vcodec^=avc1]/" if h else "bv[vcodec^=avc1]/") + fmt
            state["text"] = "Sťahujem video…"
            log("⬇ Video" + (f" (max {h}p)" if h else " (najlepšia kvalita)") + (" – H.264 pre MP4" if mp4 else ""))
            sub(0)
            video_path = download_format(info, fmt, tmp / "video", log, sub, cancel)
            state["step"] += 1

        def fetch(t, fmt, n, kind):
            stem = tmp / ("a%02d_%s_%s" % (n, re.sub(r"[^\w-]", "_", t["code"] or "orig"), kind))
            return download_format(info, fmt["format_id"], stem, log, sub, cancel)

        per_lang = []     # [(stopa, [(cesta, "aac" | "opus"), …])] – súbory jazyka v poradí AAC, Opus
        for n, t in enumerate(tracks, 1):
            if cancel.is_set():
                raise Cancelled()
            tm = track_mode(t)
            want_aac, want_opus = tm != "opus", tm != "aac"
            state["text"] = f"Sťahujem: {t['label']} ({n}/{len(tracks)})…"
            log(f"⬇ {t['label']}  [{t['code'] or '—'}] – {dict(aac='AAC', opus='Opus').get(tm, 'AAC + Opus')}")
            sub(0)
            got, opus_path = [], None
            if want_aac:
                if t.get("aac"):
                    got.append((fetch(t, t["aac"], n, "aac"), "aac"))
                else:  # zdroj AAC neponúka → prevedieme z Opus (Opus ostane, len ak ho chceš)
                    opus_path = fetch(t, t.get("opus") or t["format"], n, "opus")
                    log(f"↻ Prevádzam {t['label']} do AAC (zdroj ho neponúka)…")
                    got.append((to_aac(opus_path, tmp / ("a%02d_aac.m4a" % n)), "aac"))
                state["step"] += 1
            if want_opus:
                sub(0)
                if t.get("opus"):
                    got.append((opus_path or fetch(t, t["opus"], n, "opus"), "opus"))
                elif not want_aac:
                    log(f"ℹ {t['label']}: zdroj nemá Opus – použijem AAC")
                    got.append((fetch(t, t.get("aac") or t["format"], n, "aac"), "aac"))
                state["step"] += 1
            per_lang.append((t, got))

        if cancel.is_set():
            raise Cancelled()
        state["text"] = "Spájam stopy do jedného súboru…"
        log("⇒ Spájam stopy…")
        sub(0)

        codes = [t["code"] or "orig" for t, _ in per_lang]
        tag = "+".join(codes) if len(codes) <= 8 else f"{len(codes)} jazykov"
        inputs: list[dict] = []
        if mode == "existing":
            inputs.append({"path": existing, "kind": "existing", "audio_count": count_audio_streams(existing),
                           "lang3": iso3(original["code"]) if original and original["code"] else None,
                           "title": f"{original['label']} – originál" if original else "Pôvodný zvuk"})
            out = unique_path(outdir / f"{safe_name(existing.stem)} [{tag}].{'mp4' if mp4 else 'mkv'}")
        elif mode == "video":
            inputs.append({"path": video_path, "kind": "video"})
            out = unique_path(outdir / f"{safe_name(title)} [{tag}].{'mp4' if mp4 else 'mkv'}")
        else:
            out = unique_path(outdir / f"{safe_name(title)} [{tag}].{'m4a' if mp4 else 'mka'}")
        # CZ (AAC), CZ (Opus), SK (AAC), SK (Opus)… – prvá je predvolená
        for t, got in per_lang:
            for p, codec in got:
                suffix = (" – AAC" if codec == "aac" else " – Opus") if len(got) > 1 else ""
                inputs.append({"path": p, "kind": "audio", "lang3": iso3(t["code"]),
                               "title": t["label"] + (" – originál" if t["original"] else "") + suffix})
        mux(out, inputs, title)

        if job.get("keep_separate"):
            for t, got in per_lang:
                if not got:
                    continue
                p = got[0][0]
                single = _export_single(p, outdir / f"{safe_name(title)} [{t['code'] or 'orig'}]")
                log(f"   ↳ {single.name}")

        progress(1.0, "Hotovo")
        log(f"✔ Uložené: {out}")
        return out
    finally:
        shutil.rmtree(tmp, ignore_errors=True)



# --------------------------------------------------------------------------- #
#  Príkazový riadok
# --------------------------------------------------------------------------- #

def cli(argv: list[str]) -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
        except Exception:
            pass

    cfg = load_config()
    ap = argparse.ArgumentParser(prog="app.py jazyky",
                                 description="Stiahne preložené zvukové stopy YouTube videa a spojí ich do jedného súboru.")
    ap.add_argument("url", help="odkaz na YouTube video")
    ap.add_argument("-j", "--jazyky", nargs="+", metavar="KÓD",
                    help=f"kódy jazykov v požadovanom poradí (predvolené: {' '.join(DEFAULT_LANGS)})")
    ap.add_argument("-o", "--vystup", metavar="PRIEČINOK", help=f"kam uložiť (predvolené: {cfg['outdir']})")
    ap.add_argument("--zoznam", action="store_true", help="len vypíše dostupné jazyky")
    ap.add_argument("--len-zvuk", action="store_true", help="bez videa – len zvukové stopy v jednom .mka")
    ap.add_argument("--k-videu", metavar="SÚBOR", help="pridá jazyky k už stiahnutému videu (výsledok .mkv)")
    ap.add_argument("--bez-originalu", action="store_true", help="nepridá pôvodnú stopu (inak je posledná)")
    ap.add_argument("--samostatne", action="store_true", help="uloží aj každý jazyk ako samostatný súbor")
    ap.add_argument("--max-vyska", type=int, metavar="PX", help="max. výška videa, napr. 1080")
    ap.add_argument("--mp4", action="store_true", help="výsledok ako MP4 (obraz H.264, zvuk AAC) namiesto MKV")
    ap.add_argument("--zvuk", choices=list(AUDIO_MODES), default=None,
                    help="oboje = AAC (predvolená, prehrá každý prehrávač) + Opus; aac = len AAC; opus = len Opus "
                         "(predvolené podľa nastavenia, inak oboje)")
    a = ap.parse_args(argv)

    deps = check_dependencies()
    if not deps["yt-dlp"]:
        print('Chýba yt-dlp. Nainštaluj ho:  py -m pip install -U "yt-dlp[default]"')
        return 2
    if not deps["js"]:
        print("⚠ Nenašiel som Deno – YouTube ho dnes vyžaduje. Nainštaluj ho (winget install DenoLand.Deno).")

    print("Načítavam informácie o videu…")
    info = fetch_info(a.url)
    tracks = list_audio_tracks(info)
    print(f"\n{info.get('title')}  ({format_duration(info.get('duration'))})")
    print(f"Dostupné jazyky ({len(tracks)}):")
    for t in tracks:
        f = t["format"]
        print(f"  {t['code'] or '—':10} {t['label']:34} {('★ originál' if t['original'] else ''):11}"
              f" {f.get('ext')}/{f.get('acodec')} ~{round(f.get('abr') or f.get('tbr') or 0)} kbps")
    if a.zoznam:
        return 0

    chosen, missing = pick_tracks(tracks, a.jazyky or cfg.get("langs") or DEFAULT_LANGS)
    if missing:
        print("⚠ Nedostupné jazyky: " + ", ".join(missing))
    mode = "existing" if a.k_videu else ("audio" if a.len_zvuk else "video")
    job = {"info": info, "tracks": chosen, "outdir": a.vystup or cfg["outdir"], "mode": mode,
           "existing_path": a.k_videu, "include_original": not a.bez_originalu,
           "keep_separate": a.samostatne, "max_height": a.max_vyska,
           "audio_mode": a.zvuk or cfg.get("audio_mode", "oboje"),
           "kontajner": "mp4" if a.mp4 else cfg.get("kontajner", "mkv")}

    def progress(frac, text):
        bar = "█" * int(frac * 30)
        print(f"\r[{bar:<30}] {frac * 100:5.1f} %  {text[:50]:<50}", end="", flush=True)

    def log(msg):
        print("\r" + " " * 100 + "\r" + msg)

    try:
        out = run_job(job, log=log, progress=progress)
    except KeyboardInterrupt:
        print("\nZrušené.")
        return 1
    print()
    return 0 if out else 1

