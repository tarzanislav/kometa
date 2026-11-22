#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Kometa – sťahovanie videí a súborov (spúšťač).

Bez parametrov otvorí okno (nové rozhranie). Ďalšie možnosti:

    py app.py --klasicke                 # pôvodné jednoduché okno (tkinter)
    py app.py --prehliadac [--port N]    # rozhranie v okne prehliadača Edge/Chrome (bez pywebview)

Príkazový riadok:

    py app.py ODKAZ [ODKAZ …] [-f video|mp4|1080|720|mp3|audio] [-o PRIEČINOK]
    py app.py jazyky ODKAZ [-j cs sk en en-US pl hu] [--len-zvuk] [--k-videu SÚBOR] …
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))


def cli_download(argv: list[str]) -> int:
    from kometa import media
    from kometa.common import CONFIG_DIR, apply_settings, human_size, human_speed, load_config
    from kometa.fronta import STATUS_TEXT, DownloadQueue
    from kometa.hosts import Accounts

    cfg = load_config()
    ap = argparse.ArgumentParser(prog="app.py", description="Stiahne videá a súbory s pokračovaním.")
    ap.add_argument("urls", nargs="+", metavar="ODKAZ")
    ap.add_argument("-f", "--format", default=cfg.get("dl_preset", "video"), choices=list(media.PRESETS),
                    help="video | mp4 | 1080 | 720 | mp3 | audio")
    ap.add_argument("-o", "--vystup", default=cfg["outdir"], metavar="PRIEČINOK")
    a = ap.parse_args(argv)
    apply_settings(cfg)

    q = DownloadQueue(cfg, Accounts(cfg), log=print, path=CONFIG_DIR / "fronta_cli.json")
    ids = q.add(a.urls, a.format, a.vystup) or [i for i in q.ids()
                                                if q.snapshot(i)["url"] in a.urls and q.snapshot(i)["status"] != "hotovo"]
    q.start(ids)
    try:
        while any(q.snapshot(i)["status"] in ("caka", "stahuje") for i in ids):
            line = []
            for i in ids:
                it = q.snapshot(i)
                if it["status"] == "stahuje":
                    pct = f"{it['done'] / it['total'] * 100:.0f} %" if it.get("total") else human_size(it["done"])
                    line.append(f"{(it['title'] or it['url'])[:30]}: {pct} {human_speed(it['speed'])}")
            print("\r" + "  |  ".join(line)[:150].ljust(150), end="", flush=True)
            time.sleep(0.5)
    except KeyboardInterrupt:
        print("\nPozastavujem… (pri ďalšom spustení s rovnakým odkazom sa pokračuje)")
        q.shutdown()
        return 1
    print()
    bad = 0
    for i in ids:
        it = q.snapshot(i)
        print(f"{STATUS_TEXT[it['status']]:12} {it.get('file') or it.get('error') or it['url']}")
        bad += it["status"] != "hotovo"
    q.clear_finished()
    return 1 if bad else 0


def main() -> int:
    args = sys.argv[1:]
    if not args or args[0] in ("--prehliadac", "--bez-okna", "--port"):
        ap = argparse.ArgumentParser(prog="app.py")
        ap.add_argument("--prehliadac", action="store_true")
        ap.add_argument("--bez-okna", action="store_true", help="len lokálny server (na testy)")
        ap.add_argument("--port", type=int, default=0)
        a = ap.parse_args(args)
        try:
            from kometa.webapp import run
            run(force_browser=a.prehliadac or a.bez_okna, port=a.port, open_window=not a.bez_okna)
        except SystemExit:
            raise
        except Exception:  # noqa: BLE001 – nové rozhranie sa nedá spustiť → pôvodné okno
            if a.bez_okna:
                raise
            from kometa.gui import main as gui
            gui()
        return 0
    if args[0] == "--klasicke":
        from kometa.gui import main as gui
        gui()
        return 0
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
        except Exception:
            pass
    if args[0] == "jazyky":
        from kometa.jazyky import cli
        return cli(args[1:])
    if args[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    return cli_download(args[1:] if args[0] == "stiahni" else args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # noqa: BLE001
        from kometa.common import clean_error
        print("\n✖ " + clean_error(exc))
        sys.exit(1)
