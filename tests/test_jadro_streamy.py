"""Skúška jadra v Ruste na skutočných streamoch HLS a DASH, ktoré vyrobí FFmpeg.

Overí: výber kvality, výber a poradie jazykov, spojenie do MKV aj MP4, DASH so šablónou
(číslo aj časová os), obyčajný súbor, zastavenie pri šifrovanom HLS (kód 2) a pri živom
vysielaní (kód 4) a príkaz `info`.

Spustenie (z koreňa repozitára, po `cargo build --release` v priečinku jadro):
    python tests/test_jadro_streamy.py
"""
import functools
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

KOREN = Path(__file__).resolve().parents[1]
EXE = KOREN / "jadro" / "target" / "release" / ("kometa.exe" if os.name == "nt" else "kometa")
FF = shutil.which("ffmpeg")
FP = shutil.which("ffprobe")


def ff(*args, cwd):
    subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *args], cwd=cwd, check=True)


def priprav(d: Path) -> None:
    vod = ["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"]
    ff("-f", "lavfi", "-i", "testsrc=size=640x360:rate=25", "-t", "4", "-c:v", "libx264", "-g", "25",
       "-pix_fmt", "yuv420p", *vod, "-hls_segment_filename", "v360_%03d.ts", "v360.m3u8", cwd=d)
    ff("-f", "lavfi", "-i", "testsrc=size=320x180:rate=25", "-t", "4", "-c:v", "libx264", "-g", "25",
       "-pix_fmt", "yuv420p", *vod, "-hls_segment_filename", "v180_%03d.ts", "v180.m3u8", cwd=d)
    for jazyk, hz in (("en", 880), ("cs", 440), ("sk", 660)):
        ff("-f", "lavfi", "-i", f"sine=frequency={hz}", "-t", "4", "-c:a", "aac", *vod,
           "-hls_segment_filename", f"{jazyk}_%03d.ts", f"{jazyk}.m3u8", cwd=d)
    (d / "master.m3u8").write_text(
        "#EXTM3U\n"
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",LANGUAGE="en",NAME="English",DEFAULT=YES,URI="en.m3u8"\n'
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",LANGUAGE="cs",NAME="Čeština",URI="cs.m3u8"\n'
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",LANGUAGE="sk",NAME="Slovenčina",URI="sk.m3u8"\n'
        '#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360,AUDIO="aud"\nv360.m3u8\n'
        '#EXT-X-STREAM-INF:BANDWIDTH=300000,RESOLUTION=320x180,AUDIO="aud"\nv180.m3u8\n', encoding="utf-8")
    # šifrované AES-128
    (d / "kluc.bin").write_bytes(os.urandom(16))
    (d / "kluc.info").write_text("kluc.bin\nkluc.bin\n", encoding="utf-8")
    ff("-f", "lavfi", "-i", "testsrc=size=320x180:rate=25", "-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p",
       *vod, "-hls_key_info_file", "kluc.info", "-hls_segment_filename", "enc_%03d.ts", "enc.m3u8", cwd=d)
    # živé vysielanie (bez EXT-X-ENDLIST)
    zive = [r for r in (d / "v180.m3u8").read_text().splitlines() if r not in ("#EXT-X-ENDLIST",)
            and not r.startswith("#EXT-X-PLAYLIST-TYPE")]
    (d / "zive.m3u8").write_text("\n".join(zive) + "\n")
    # DASH: šablóna s časovou osou aj s číslom
    for meno, timeline in (("dash_tl", "1"), ("dash_num", "0")):
        (d / meno).mkdir()
        ff("-f", "lavfi", "-i", "testsrc=size=640x360:rate=25", "-f", "lavfi", "-i", "sine=frequency=500", "-t", "4",
           "-map", "0:v", "-map", "1:a", "-c:v", "libx264", "-g", "25", "-pix_fmt", "yuv420p", "-c:a", "aac",
           "-metadata:s:a:0", "language=ces", "-f", "dash", "-seg_duration", "1", "-use_template", "1",
           "-use_timeline", timeline, "manifest.mpd", cwd=d / meno)


def sonda(p: Path):
    r = subprocess.run([FP, "-v", "error", "-show_entries",
                        "stream=codec_type,height:stream_tags=language:stream_disposition=default:format=format_name",
                        "-of", "json", str(p)], capture_output=True, text=True, check=True)
    j = json.loads(r.stdout)
    return j["format"]["format_name"], [(s["codec_type"], s.get("height"), s.get("tags", {}).get("language"),
                                         s["disposition"]["default"]) for s in j["streams"]]


def kometa(*args):
    r = subprocess.run([str(EXE), *args], capture_output=True, text=True, encoding="utf-8")
    return r.returncode, r.stdout, r.stderr


def main() -> int:
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:  # noqa: BLE001
        pass
    if not (FF and FP and EXE.is_file()):
        print("Chýba FFmpeg/ffprobe alebo preložené jadro (cargo build --release).")
        return 1
    d = Path(tempfile.mkdtemp(prefix="kometa_streamy_"))
    out = d / "vysledky"
    priprav(d)
    class Tichy(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass

    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Tichy, directory=str(d)))
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{srv.server_address[1]}/"
    os.environ["NO_PROXY"] = "127.0.0.1,localhost"
    try:
        # 1) HLS master: najlepšia kvalita, jazyky cs, sk v tomto poradí, MKV
        kod, so, se = kometa("stiahni", base + "master.m3u8", "-o", str(out), "-n", "hls", "-j", "cs,sk", "--json")
        assert kod == 0, (kod, so, se)
        udalosti = [json.loads(r) for r in so.splitlines() if r.strip()]
        assert udalosti[-1]["typ"] == "hotovo", udalosti[-1]
        fmt, st = sonda(out / "hls.mkv")
        assert "matroska" in fmt and st == [("video", 360, None, 1), ("audio", None, "cze", 1), ("audio", None, "slo", 0)], st
        print("HLS master → MKV, 360p, cze+slo: OK")
        # 2) najviac 200 px, MP4
        kod, so, se = kometa("stiahni", base + "master.m3u8", "-o", str(out), "-n", "male", "--max-vyska", "200",
                             "-j", "sk", "--mp4")
        assert kod == 0, (kod, se)
        fmt, st = sonda(out / "male.mp4")
        # MP4 má jazyk podľa normy v tvare ISO 639-2/T (slk), MKV v tvare B (slo)
        assert "mp4" in fmt and st[0][1] == 180 and [s[2] for s in st[1:]] == ["slk"], st
        print("HLS --max-vyska 200 --mp4: OK")
        # 3) DASH s časovou osou aj s číslom
        for meno in ("dash_tl", "dash_num"):
            kod, so, se = kometa("stiahni", base + f"{meno}/manifest.mpd", "-o", str(out), "-n", meno)
            assert kod == 0, (meno, kod, se)
            fmt, st = sonda(out / f"{meno}.mkv")
            assert [s[0] for s in st] == ["video", "audio"] and st[1][2] == "cze", (meno, st)
        print("DASH (SegmentTimeline aj $Number$): OK")
        # 4) obyčajný súbor
        kod, so, se = kometa("stiahni", base + "v360_000.ts", "-o", str(out))
        assert kod == 0 and (out / "v360_000.ts").read_bytes() == (d / "v360_000.ts").read_bytes(), (kod, se)
        print("Obyčajný súbor: OK")
        # 5) šifrované → kód 2, nič sa nestiahne
        kod, so, se = kometa("stiahni", base + "enc.m3u8", "-o", str(out / "enc"), "--json")
        assert kod == 2 and json.loads(so.splitlines()[-1])["kod"] == "drm", (kod, so)
        assert not any((out / "enc").glob("*.ts")) if (out / "enc").exists() else True
        print("Šifrované HLS zastavené (kód 2): OK")
        # 6) živé → kód 4
        kod, so, se = kometa("stiahni", base + "zive.m3u8", "-o", str(out))
        assert kod == 4, (kod, se)
        print("Živé vysielanie (kód 4): OK")
        # 7) info
        kod, so, se = kometa("info", base + "master.m3u8", "-j", "sk,en")
        plan = json.loads(so)
        assert kod == 0 and plan["format"] == "hls" and plan["jazyky"] == ["en", "cs", "sk"], plan
        assert [s["jazyk"] for s in plan["stopy"][1:]] == ["sk", "en"] and len(plan["kvality"]) == 2, plan
        print("info: OK")
        print("VŠETKO OK")
        return 0
    finally:
        srv.shutdown()
        shutil.rmtree(d, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
