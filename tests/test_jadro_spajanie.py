"""Skúška vlastného spájania jadra (MKV a MP4 bez FFmpeg) na súboroch, ktoré vyrobí FFmpeg.

FFmpeg tu slúži len na prípravu vstupov a na nezávislú kontrolu výsledku (ffprobe, plné
dekódovanie). Spája výhradne jadro Komety: `kometa spoj`.

Overí pre MKV aj MP4:
  * MPEG-TS s H.264 (B-snímky) + AAC v jednom súbore,
  * HLS: obraz a dva jazyky zvuku ako samostatné poskladané kúsky TS (poradie, jazyky, názvy, predvolená),
  * DASH/fMP4: H.264, HEVC, AV1, VP9 + AAC a Opus,
  * bežné MP4 (moov na konci aj na začiatku, edit list, ctts),
  * počty vzoriek, trvanie, vzájomný posun obrazu a zvuku, dekódovanie bez jedinej chyby,
  * nepodporovaný kodek (HEVC v TS) skončí jasnou chybou, nie potichu.

Spustenie (z koreňa repozitára, po `cargo build --release` v priečinku jadro):
    python tests/test_jadro_spajanie.py
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

KOREN = Path(__file__).resolve().parents[1]
EXE = KOREN / "target" / "release" / ("kometa.exe" if os.name == "nt" else "kometa")
FF = shutil.which("ffmpeg")
FP = shutil.which("ffprobe")
D = Path(tempfile.mkdtemp(prefix="kometa_spajanie_"))
os.environ.setdefault("SVT_LOG", "1")  # kodér AV1 len s chybami
CHYBY = []


def ff(*args, cwd=D):
    subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *args], cwd=cwd, check=True)


def sonda(p: Path) -> dict:
    r = subprocess.run([FP, "-v", "error", "-count_packets", "-show_entries",
                        "stream=index,codec_type,codec_name,width,height,nb_read_packets,start_time:"
                        "stream_tags=language,title,handler_name:stream_disposition=default:format=duration,format_name",
                        "-of", "json", str(p)], capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


def dekoduj(p: Path) -> str:
    r = subprocess.run([FF, "-hide_banner", "-v", "error", "-i", str(p), "-f", "null", "-"],
                       capture_output=True, text=True)
    return (r.stderr or "").strip() if r.returncode == 0 else f"návratový kód {r.returncode}: {r.stderr[-400:]}"


def spoj(vystup: Path, *vstupy: str):
    return subprocess.run([str(EXE), "spoj", "-o", str(vystup), *vstupy], capture_output=True, text=True,
                          encoding="utf-8")


def pakety(p: Path, typ: str):
    """(časy prezentácie od začiatku, počet kľúčových) pre prvú stopu daného typu."""
    sel = "v:0" if typ == "video" else "a:0"
    r = subprocess.run([FP, "-v", "error", "-select_streams", sel, "-show_entries", "packet=pts_time,flags",
                        "-of", "csv=p=0", str(p)], capture_output=True, text=True, check=True)
    casy, kluce = [], 0
    for riadok in r.stdout.split():
        cas, _, prz = riadok.partition(",")
        if cas not in ("", "N/A"):
            casy.append(float(cas))
        kluce += "K" in prz
    zac = min(casy) if casy else 0
    return sorted(round(c - zac, 4) for c in casy), kluce


def stopy(info, typ):
    return [s for s in info["streams"] if s["codec_type"] == typ]


def zaciatok(s):
    return float(s.get("start_time") or 0)


def over(nazov, vystup: Path, zdroje: list[tuple[Path, str]], ocakavane_jazyky=None, ocakavane_nazvy=None):
    """zdroje: (súbor, typ) v poradí stôp výsledku; porovnajú sa počty paketov a posuny."""
    try:
        info = sonda(vystup)
        st = info["streams"]
        assert len(st) == len(zdroje), f"{len(st)} stôp namiesto {len(zdroje)}"
        prvy_video = None
        for s, (zdroj, typ) in zip(st, zdroje):
            zi = sonda(zdroj)
            zs = stopy(zi, typ)[0]
            assert s["codec_type"] == typ and s["codec_name"] == zs["codec_name"], (s["codec_name"], zs["codec_name"])
            assert s["nb_read_packets"] == zs["nb_read_packets"], (
                f"{typ} {s['codec_name']}: {s['nb_read_packets']} paketov, zdroj {zs['nb_read_packets']}")
            if typ == "video":
                assert (s["width"], s["height"]) == (zs["width"], zs["height"])
                prvy_video = (s, zs)
        # vzájomný posun obrazu a zvuku sa nesmie zmeniť
        if prvy_video:
            v, zv = prvy_video
            for s, (zdroj, typ) in zip(st, zdroje):
                if typ != "audio":
                    continue
                za = stopy(sonda(zdroj), "audio")[0]
                posun_zdroj = zaciatok(za) - zaciatok(zv) if zdroj == zdroje[0][0] else None
                if posun_zdroj is not None:
                    posun = zaciatok(s) - zaciatok(v)
                    assert abs(posun - posun_zdroj) < 0.03, f"posun zvuku {posun:.3f} s, v zdroji {posun_zdroj:.3f} s"
        if ocakavane_jazyky:
            jazyky = [s.get("tags", {}).get("language") for s in stopy(info, "audio")]
            assert jazyky == ocakavane_jazyky, jazyky
            predvolene = [s["disposition"]["default"] for s in stopy(info, "audio")]
            assert predvolene[0] == 1 and sum(predvolene) == 1, predvolene
        if ocakavane_nazvy:
            nazvy = [s.get("tags", {}).get("title") or s.get("tags", {}).get("handler_name") for s in stopy(info, "audio")]
            assert nazvy == ocakavane_nazvy, nazvy
        # presné časy každej vzorky (aj pri B-snímkach) a kľúčové snímky
        for i, (zdroj, typ) in enumerate(zdroje):
            if typ != "video" or i > 0:
                continue
            casy_v, kluce_v = pakety(vystup, "video")
            casy_z, kluce_z = pakety(zdroj, "video")
            assert kluce_v == kluce_z, f"kľúčových snímok {kluce_v}, v zdroji {kluce_z}"
            rozdiel = max((abs(a - b) for a, b in zip(casy_v, casy_z)), default=0)
            assert len(casy_v) == len(casy_z) and rozdiel < 0.0016, f"časy snímok sa líšia až o {rozdiel:.4f} s"
        # pretáčanie: dekódovanie od polovice
        r = subprocess.run([FF, "-hide_banner", "-v", "error", "-ss", "2.5", "-i", str(vystup), "-frames:v", "5",
                            "-f", "null", "-"], capture_output=True, text=True)
        assert r.returncode == 0 and not r.stderr.strip(), f"pretáčanie: {r.stderr[-200:]}"
        trvanie = float(info["format"]["duration"])
        trvanie_zdroja = max(float(sonda(z)["format"]["duration"]) for z, _ in zdroje)
        assert abs(trvanie - trvanie_zdroja) < 0.25, f"trvanie {trvanie:.2f} s, zdroj {trvanie_zdroja:.2f} s"
        chyba = dekoduj(vystup)
        assert not chyba, f"dekódovanie: {chyba[:300]}"
        print(f"  OK  {nazov}")
    except AssertionError as e:
        CHYBY.append(f"{nazov}: {e}")
        print(f"  ZLYHALO  {nazov}: {e}")


def poskladaj(vzor: str, ciel: str, init: str | None = None):
    """Kúsky ako ich poskladá sťahovanie Komety (init + kúsky za sebou)."""
    with open(D / ciel, "wb") as out:
        if init:
            out.write((D / init).read_bytes())
        for k in sorted(D.glob(vzor)):
            out.write(k.read_bytes())


def main() -> int:
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:  # noqa: BLE001
        pass
    if not (FF and FP and EXE.is_file()):
        print("Chýba FFmpeg/ffprobe alebo preložené jadro (cargo build --release).")
        return 1
    obraz = ["-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25"]
    ton = lambda hz: ["-f", "lavfi", "-i", f"sine=frequency={hz}:sample_rate=48000"]  # noqa: E731
    x264 = ["-c:v", "libx264", "-bf", "2", "-g", "50", "-pix_fmt", "yuv420p", "-preset", "veryfast"]
    try:
        print("Príprava vstupov…")
        # A) TS: obraz + zvuk v jednom súbore
        ff(*obraz, *ton(1000), "-t", "5", *x264, "-c:a", "aac", "-f", "mpegts", "a.ts")
        # B) HLS: obraz a dva jazyky ako samostatné kúsky
        hls = ["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"]
        ff(*obraz, "-t", "5", *x264, *hls, "-hls_segment_filename", "hv_%03d.ts", "hv.m3u8")
        ff(*ton(440), "-t", "5", "-c:a", "aac", *hls, "-hls_segment_filename", "hcs_%03d.ts", "hcs.m3u8")
        ff(*ton(660), "-t", "5", "-c:a", "aac", *hls, "-hls_segment_filename", "hsk_%03d.ts", "hsk.m3u8")
        poskladaj("hv_*.ts", "hv.ts")
        poskladaj("hcs_*.ts", "hcs.ts")
        poskladaj("hsk_*.ts", "hsk.ts")
        # C) DASH/fMP4: rôzne kodeky obrazu, AAC aj Opus
        kodeky = {
            "h264": x264,
            "hevc": ["-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error", "-pix_fmt", "yuv420p"],
            "av1": ["-c:v", "libsvtav1", "-preset", "12", "-pix_fmt", "yuv420p"],
            "vp9": ["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-pix_fmt", "yuv420p"],
        }
        for meno, v in kodeky.items():
            zvuk = ["-c:a", "libopus", "-b:a", "64k"] if meno in ("av1", "vp9") else ["-c:a", "aac"]
            (D / meno).mkdir()
            ff(*obraz, *ton(500), "-t", "4", "-map", "0:v", "-map", "1:a", *v, *zvuk, "-f", "dash",
               "-seg_duration", "1", "-use_template", "1", "-use_timeline", "1", "-dash_segment_type", "mp4",
               "manifest.mpd", cwd=D / meno)
            for s in ("0", "1"):
                with open(D / f"{meno}_{s}.mp4", "wb") as out:
                    out.write((D / meno / f"init-stream{s}.m4s").read_bytes())
                    for k in sorted((D / meno).glob(f"chunk-stream{s}-*.m4s")):
                        out.write(k.read_bytes())
        # D) bežné MP4: moov na konci a moov na začiatku
        ff(*obraz, *ton(800), "-t", "5", *x264, "-c:a", "aac", "p_koniec.mp4")
        ff(*obraz, *ton(800), "-t", "5", *x264, "-c:a", "aac", "-movflags", "+faststart", "p_zaciatok.mp4")
        # F) Matroska a WebM ako vstup (YouTube: obraz VP9/AV1 a zvuk Opus vo WebM; vlastné stiahnuté MKV)
        vp9 = ["-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-pix_fmt", "yuv420p", "-g", "50"]
        ff(*obraz, "-t", "5", *vp9, "-an", "yt_vp9.webm")
        ff(*obraz, "-t", "5", "-c:v", "libsvtav1", "-preset", "12", "-pix_fmt", "yuv420p", "-an", "yt_av1.webm")
        ff(*ton(300), "-t", "5", "-c:a", "libopus", "-b:a", "96k", "-vn", "yt_cs.webm")
        ff(*ton(350), "-t", "5", "-c:a", "aac", "-vn", "yt_sk.m4a")
        ff(*obraz, *ton(900), "-t", "5", *x264, "-c:a", "aac", "-metadata:s:a:0", "language=eng",
           "-metadata:s:a:0", "title=Pôvodný zvuk", "moje.mkv")
        ff(*ton(250), "-t", "4", "-c:a", "libopus", "-f", "matroska", "zive.mka")

        # E) HEVC v TS – zatiaľ nepodporované
        ff(*obraz, "-t", "2", *kodeky["hevc"], "-f", "mpegts", "hevc.ts")

        for prip in ("mkv", "mp4"):
            print(f"Výsledok .{prip}:")
            vys = D / "vystup"
            vys.mkdir(exist_ok=True)
            r = spoj(vys / f"a.{prip}", str(D / "a.ts"))
            assert r.returncode == 0, r.stderr
            over("TS: H.264 + AAC v jednom súbore", vys / f"a.{prip}", [(D / "a.ts", "video"), (D / "a.ts", "audio")])

            r = spoj(vys / f"hls.{prip}", f"{D / 'hv.ts'}#video", f"{D / 'hcs.ts'}#zvuk#cs#Čeština",
                     f"{D / 'hsk.ts'}#zvuk#sk#Slovenčina")
            assert r.returncode == 0, r.stderr
            jazyky = ["cze", "slo"] if prip == "mkv" else ["ces", "slk"]
            over("HLS: obraz + CZ + SK z kúskov", vys / f"hls.{prip}",
                 [(D / "hv.ts", "video"), (D / "hcs.ts", "audio"), (D / "hsk.ts", "audio")],
                 ocakavane_jazyky=jazyky, ocakavane_nazvy=["Čeština", "Slovenčina"])

            for meno in kodeky:
                r = spoj(vys / f"{meno}.{prip}", f"{D / f'{meno}_0.mp4'}#video", f"{D / f'{meno}_1.mp4'}#zvuk#en#English")
                if r.returncode != 0:
                    CHYBY.append(f"DASH {meno}: {r.stderr.strip()[-300:]}")
                    print(f"  ZLYHALO  DASH {meno}: {r.stderr.strip()[-300:]}")
                    continue
                over(f"DASH/fMP4: {meno} + {'Opus' if meno in ('av1', 'vp9') else 'AAC'}", vys / f"{meno}.{prip}",
                     [(D / f"{meno}_0.mp4", "video"), (D / f"{meno}_1.mp4", "audio")])

            for zdroj in ("p_koniec.mp4", "p_zaciatok.mp4"):
                r = spoj(vys / f"{zdroj}.{prip}", str(D / zdroj))
                assert r.returncode == 0, r.stderr
                over(f"bežné MP4 ({zdroj})", vys / f"{zdroj}.{prip}", [(D / zdroj, "video"), (D / zdroj, "audio")])

            for meno in ("yt_vp9.webm", "yt_av1.webm"):
                r = spoj(vys / f"{meno}.{prip}", f"{D / meno}#video", f"{D / 'yt_sk.m4a'}#zvuk#sk#Slovenčina",
                         f"{D / 'yt_cs.webm'}#zvuk#cs#Čeština – Opus")
                if r.returncode != 0:
                    CHYBY.append(f"WebM {meno}: {r.stderr.strip()[-300:]}")
                    print(f"  ZLYHALO  WebM {meno}: {r.stderr.strip()[-300:]}")
                    continue
                over(f"WebM ako z YouTube: {meno[3:6].upper()} + AAC + Opus", vys / f"{meno}.{prip}",
                     [(D / meno, "video"), (D / "yt_sk.m4a", "audio"), (D / "yt_cs.webm", "audio")],
                     ocakavane_jazyky=["slo", "cze"] if prip == "mkv" else ["slk", "ces"],
                     ocakavane_nazvy=["Slovenčina", "Čeština – Opus"])
            # doplnenie jazyka do vlastného MKV (H.264 so snímkami B): obraz, nový jazyk, pôvodný zvuk na koniec
            r = spoj(vys / f"moje.{prip}", f"{D / 'moje.mkv'}#video", f"{D / 'yt_cs.webm'}#zvuk#cs#Čeština",
                     f"{D / 'moje.mkv'}#zvuk")
            if r.returncode != 0:
                CHYBY.append(f"vlastné MKV: {r.stderr.strip()[-300:]}")
                print(f"  ZLYHALO  vlastné MKV: {r.stderr.strip()[-300:]}")
            else:
                over("vlastné MKV (H.264 so snímkami B) + nový jazyk, pôvodný zvuk ostane", vys / f"moje.{prip}",
                     [(D / "moje.mkv", "video"), (D / "yt_cs.webm", "audio"), (D / "moje.mkv", "audio")],
                     ocakavane_jazyky=["cze", "eng"] if prip == "mkv" else ["ces", "eng"],
                     ocakavane_nazvy=["Čeština", "Pôvodný zvuk"])
            # vlastný výsledok jadra (MKV) ako vstup – časy dekódovania sa musia dopočítať
            if prip == "mp4":
                r = spoj(vys / "znova.mp4", str(vys / "hls.mkv"))
                if r.returncode != 0:
                    CHYBY.append(f"MKV jadra → MP4: {r.stderr.strip()[-300:]}")
                    print(f"  ZLYHALO  MKV jadra → MP4: {r.stderr.strip()[-300:]}")
                else:
                    over("MKV z jadra znova do MP4", vys / "znova.mp4",
                         [(D / "hv.ts", "video"), (D / "hcs.ts", "audio"), (D / "hsk.ts", "audio")])

            r = spoj(vys / f"hevc_ts.{prip}", str(D / "hevc.ts"))
            if r.returncode != 0 and "Nepodporovaný" in r.stderr and not (vys / f"hevc_ts.{prip}").exists():
                print("  OK  HEVC v TS: jasná chyba, žiadny polovičný súbor")
            else:
                CHYBY.append(f"HEVC v TS sa nezastavilo chybou (kód {r.returncode}: {r.stderr.strip()[-200:]})")
                print("  ZLYHALO  HEVC v TS")
        if CHYBY:
            print(f"\nZLYHALO {len(CHYBY)}:")
            for c in CHYBY:
                print(" -", c)
            return 1
        print("\nVŠETKO OK")
        return 0
    finally:
        shutil.rmtree(D, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
