"""Skúška skenera stránok a prehrávačov jadra v Ruste na simulovanom webe.

Simulovaný server ponúka: kurz s Vimeo v rámčeku v rámčeku (obmedzené na doménu, s kľúčom h),
reklamy, odkaz na iné video, článok s JSON-LD, stránky s JW Playerom, Wistiou a Cloudflare Stream,
verejné Vimeo s priamym MP4 (cez /config), Vimeo s heslom a s DRM a prázdnu stránku.
Médiá vyrobí FFmpeg.

Spustenie (z koreňa repozitára, po `cargo build --release` v priečinku jadro):
    python tests/test_jadro_web.py
"""
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path
from urllib.parse import parse_qs, urlparse

KOREN = Path(__file__).resolve().parents[1]
EXE = KOREN / "jadro" / "target" / "release" / ("kometa.exe" if os.name == "nt" else "kometa")
FF = shutil.which("ffmpeg")
FP = shutil.which("ffprobe")
D = Path(tempfile.mkdtemp(prefix="kometa_web_"))
CHYBY = []
BASE = ""


def priprav():
    m = D / "media"
    m.mkdir()
    ff = lambda *a: subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *a], cwd=m, check=True)  # noqa: E731
    vod = ["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"]
    ff("-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p",
       *vod, "-hls_segment_filename", "v_%03d.ts", "v.m3u8")
    ff("-f", "lavfi", "-i", "sine=frequency=500", "-t", "3", "-c:a", "aac", *vod, "-hls_segment_filename",
       "a_%03d.ts", "a.m3u8")
    (m / "master.m3u8").write_text(
        "#EXTM3U\n"
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="a",LANGUAGE="sk",NAME="Slovenčina",DEFAULT=YES,URI="a.m3u8"\n'
        '#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,AUDIO="a"\nv.m3u8\n', encoding="utf-8")
    # ako skutočné Vimeo: fragmentované MP4 v jednom súbore, kúsky ako rozsahy bajtov (EXT-X-BYTERANGE)
    fmp4 = ["-hls_segment_type", "fmp4", "-hls_flags", "single_file"]
    ff("-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25", "-t", "3", "-c:v", "libx264", "-pix_fmt", "yuv420p",
       "-g", "25", *vod, *fmp4, "-hls_fmp4_init_filename", "vf_init.mp4", "vf.m3u8")
    ff("-f", "lavfi", "-i", "sine=frequency=700", "-t", "3", "-c:a", "aac", *vod, *fmp4,
       "-hls_fmp4_init_filename", "af_init.mp4", "af.m3u8")
    (m / "vimeo.m3u8").write_text(
        "#EXTM3U\n#EXT-X-INDEPENDENT-SEGMENTS\n"
        '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio-high",NAME="Original",DEFAULT=YES,AUTOSELECT=YES,CHANNELS="2",URI="af.m3u8"\n'
        '#EXT-X-STREAM-INF:BANDWIDTH=900000,AVERAGE-BANDWIDTH=800000,RESOLUTION=640x360,FRAME-RATE=25.000,'
        'CODECS="avc1.64001E,mp4a.40.2",AUDIO="audio-high"\nvf.m3u8\n', encoding="utf-8")
    ff("-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=300", "-t", "2",
       "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart", "video.mp4")


ZIP = b"PK\x03\x04" + bytes(range(256)) * 64


def vimeo_config(hls=None, mp4=None, drm=False, nazov="Výhľad z Lomnického štítu"):
    files = {"dash": {"cdns": {}}}
    if hls:
        files["hls"] = {"default_cdn": "akfire_interconnect_quic", "separate_av": True,
                        "cdns": {"akfire_interconnect_quic": {"url": hls, "avc_url": hls},
                                 "fastly_skyfire": {"url": hls + "?zaloha=1"}}}
        if drm:
            files["hls"]["drm"] = {"widevine": {"license": "x"}}
    if mp4:
        files["progressive"] = [{"url": mp4, "quality": "240p", "width": 320, "height": 240}]
    return {"request": {"files": files, "text_tracks": []},
            "video": {"id": 1, "title": nazov, "duration": 3, "owner": {"name": "Autor"}}}


STRANKY = {
    "/kurz/lekcia": """<!doctype html><html><head><title>Lekcia 1 – Tatry</title>
        <script>var reklama = "https://pubads.g.doubleclick.net/gampad/ads/preroll.mp4";</script></head>
        <body><h1>Lekcia 1</h1>
        <iframe src="/ramec/1" width="640"></iframe>
        <iframe src="https://securepubads.g.doubleclick.net/ads/frame.html"></iframe>
        <a href="https://vimeo.com/111111">Iné video autora</a></body></html>""",
    "/ramec/1": """<html><body><div class="obal">
        <iframe src="https://player.vimeo.com/video/76979871?h=8272103f6e&amp;badge=0&amp;autopause=0"
            allow="autoplay; fullscreen"></iframe></div></body></html>""",
    "/clanok": """<html><head><meta property="og:title" content="Článok s videom">
        <script type="application/ld+json">{"@context":"https://schema.org","@type":"VideoObject",
        "name":"Prechádzka mestom","contentUrl":"/media/video.mp4","thumbnailUrl":"/obr.jpg"}</script></head>
        <body><p>Text článku</p></body></html>""",
    "/jwstranka": """<html><head><title>JW ukážka</title>
        <script src="https://cdn.jwplayer.com/players/AbCd1234-XyZ98765.js"></script></head><body></body></html>""",
    "/wistiastranka": """<html><head><title>Wistia stránka</title></head><body>
        <div class="wistia_responsive_padding"><div class="wistia_embed wistia_async_abcd1234ef videoFoam=true">
        </div></div><script src="https://fast.wistia.com/assets/external/E-v1.js" async></script></body></html>""",
    "/cfstranka": """<html><head><title>Cloudflare ukážka</title></head><body>
        <iframe src="https://iframe.videodelivery.net/0123456789abcdef0123456789abcdef?autoplay=false"
            allowfullscreen></iframe></body></html>""",
    "/prazdna": "<html><head><title>Nič</title></head><body><p>Žiadne video.</p></body></html>",
}


class Tichy(http.server.ThreadingHTTPServer):
    def handle_error(self, request, client_address):  # jadro pri ochutnaní obsahu spojenie zavrie skôr
        if not isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            super().handle_error(request, client_address)


class Server(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(D), **k)

    def log_message(self, *a):
        pass

    def posli(self, kod, telo, typ="text/html; charset=utf-8"):
        b = telo.encode("utf-8") if isinstance(telo, str) else telo
        self.send_response(kod)
        self.send_header("Content-Type", typ)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):  # noqa: N802
        u = urlparse(self.path)
        q = parse_qs(u.query)
        ref = self.headers.get("Referer") or ""
        if u.path in STRANKY:
            return self.posli(200, STRANKY[u.path])
        if u.path == "/vimeo/video/76979871":
            if q.get("h") != ["8272103f6e"]:
                return self.posli(404, "<html>Sorry, we couldn’t find that page</html>")
            if not ref.startswith(BASE):
                return self.posli(403, "<html><h1>Sorry</h1>Because of its privacy settings, this video cannot be played here.</html>")
            cfg = vimeo_config(hls=BASE + "/media/vimeo.m3u8")
            return self.posli(200, f"<html><script>window.playerConfig = {json.dumps(cfg)}; var dalsie = {{}};</script></html>")
        if u.path == "/vimeo/video/555555":
            return self.posli(200, "<html><body>prehrávač bez nastavení</body></html>")
        if u.path == "/vimeo/video/555555/config":
            return self.posli(200, json.dumps(vimeo_config(mp4=BASE + "/media/video.mp4", nazov="Verejné video")),
                              "application/json")
        if u.path == "/vimeo/video/444444":
            return self.posli(200, '<html><form><input type="password" name="password"></form></html>')
        if u.path == "/vimeo/video/333333":
            cfg = vimeo_config(hls=BASE + "/media/master.m3u8", drm=True)
            return self.posli(200, f"<script>window.playerConfig = {json.dumps(cfg)};</script>")
        if u.path == "/jw/v2/media/AbCd1234":
            return self.posli(200, json.dumps({"title": "JW", "playlist": [{"title": "Ukážka JW", "sources": [
                {"file": BASE + "/media/master.m3u8", "type": "application/vnd.apple.mpegurl"}]}]}), "application/json")
        if u.path == "/bez_pripony/zoznam":  # playlist bez prípony a so zlým typom
            m = (D / "media" / "master.m3u8").read_text(encoding="utf-8")
            m = m.replace('URI="a.m3u8"', f'URI="{BASE}/media/a.m3u8"').replace("\nv.m3u8", f"\n{BASE}/media/v.m3u8")
            return self.posli(200, m, "application/octet-stream")
        if u.path == "/archiv/balik.zip":  # súbor so zlým typom
            return self.posli(200, ZIP, "text/plain")
        if u.path == "/wistia/embed/medias/abcd1234ef.json":
            return self.posli(200, json.dumps({"media": {"name": "Wistia návod", "assets": [
                {"type": "iphone_video", "container": "mp4", "height": 240, "url": BASE + "/wistia/deliveries/nizka.bin"},
                {"type": "hd_mp4_video", "container": "mp4", "height": 720, "url": BASE + "/wistia/deliveries/hd.bin"},
                {"type": "still_image", "url": BASE + "/obr.jpg"}]}}), "application/json")
        if u.path == "/wistia/deliveries/hd.bin":
            return self.posli(200, (D / "media" / "video.mp4").read_bytes(), "application/octet-stream")
        if u.path == "/cf/0123456789abcdef0123456789abcdef/manifest/video.m3u8":
            m = (D / "media" / "master.m3u8").read_text(encoding="utf-8")
            m = m.replace('URI="a.m3u8"', f'URI="{BASE}/media/a.m3u8"').replace("\nv.m3u8", f"\n{BASE}/media/v.m3u8")
            return self.posli(200, m, "application/vnd.apple.mpegurl")
        return super().do_GET()


def kometa(*args):
    env = dict(os.environ, KOMETA_VIMEO_ZAKLAD=BASE + "/vimeo", KOMETA_JW_ZAKLAD=BASE + "/jw",
               KOMETA_WISTIA_ZAKLAD=BASE + "/wistia", KOMETA_CF_ZAKLAD=BASE + "/cf",
               NO_PROXY="127.0.0.1,localhost")
    r = subprocess.run([str(EXE), *args], capture_output=True, text=True, encoding="utf-8", env=env)
    return r.returncode, r.stdout, r.stderr


def skontroluj(nazov, podmienka, detail=""):
    if podmienka:
        print(f"  OK  {nazov}")
    else:
        CHYBY.append(f"{nazov}: {detail}")
        print(f"  ZLYHALO  {nazov}: {detail}")


def trvanie(p: Path) -> float:
    r = subprocess.run([FP, "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(p)],
                       capture_output=True, text=True)
    try:
        return float(r.stdout.strip())
    except ValueError:
        return -1.0


def stopy(p: Path):
    r = subprocess.run([FP, "-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0", str(p)],
                       capture_output=True, text=True)
    return r.stdout.split()


def main() -> int:
    global BASE
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:  # noqa: BLE001
        pass
    if not (FF and FP and EXE.is_file()):
        print("Chýba FFmpeg/ffprobe alebo preložené jadro (cargo build --release).")
        return 1
    priprav()
    srv = Tichy(("127.0.0.1", 0), Server)
    BASE = f"http://127.0.0.1:{srv.server_address[1]}"
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    out = D / "vysledky"
    try:
        # 1) skener: Vimeo v rámčeku v rámčeku, reklamy a odkaz bokom
        kod, so, se = kometa("skenuj", BASE + "/kurz/lekcia", "--json")
        n = json.loads(so) if kod == 0 else []
        prvy = n[0] if n else {}
        skontroluj("skener: Vimeo z vnoreného rámčeka je prvé a odporúčané",
                   prvy.get("poskytovatel") == "Vimeo" and prvy.get("odporucane")
                   and prvy.get("url") == "https://player.vimeo.com/video/76979871?h=8272103f6e", prvy or se)
        skontroluj("skener: Referer je rámček, v ktorom je prehrávač", prvy.get("referer") == BASE + "/ramec/1",
                   prvy.get("referer"))
        skontroluj("skener: reklamy vynechané", not any("doubleclick" in x["url"] for x in n), [x["url"] for x in n])
        bokom = [x for x in n if x["url"].endswith("111111")]
        skontroluj("skener: odkaz na iné video nie je odporúčaný", bokom and not bokom[0]["odporucane"], bokom)

        # 2) celá stránka → Vimeo → HLS → MKV s názvom z Vimeo
        kod, so, se = kometa("stiahni", BASE + "/kurz/lekcia", "-o", str(out))
        f = out / "Výhľad z Lomnického štítu.mkv"
        skontroluj("stiahni stránku s Vimeo (Referer doplnený sám, fMP4 s rozsahmi bajtov)",
                   kod == 0 and f.exists() and stopy(f) == ["video", "audio"] and abs(trvanie(f) - 3.0) < 0.15,
                   (kod, se[-300:], list(out.glob("*")) if out.exists() else []))

        # 3) Vimeo bez Referera → zrozumiteľná rada, s Refererom → ide
        kod, so, se = kometa("stiahni", "https://player.vimeo.com/video/76979871?h=8272103f6e", "-o", str(out))
        skontroluj("Vimeo obmedzené na doménu bez Referera: rada", kod == 1 and "--referer" in se, (kod, se[-200:]))
        kod, so, se = kometa("stiahni", "https://player.vimeo.com/video/76979871?h=8272103f6e", "--referer",
                             BASE + "/kurz/lekcia", "-o", str(out), "-n", "s_refererom")
        skontroluj("Vimeo s --referer", kod == 0 and (out / "s_refererom.mkv").exists(), (kod, se[-300:]))

        # 4) verejné Vimeo bez nastavení na stránke → /config → priame MP4
        kod, so, se = kometa("stiahni", "https://vimeo.com/555555", "-o", str(out))
        f = out / "Verejné video.mp4"
        skontroluj("Vimeo cez /config s priamym MP4", kod == 0 and f.exists()
                   and f.read_bytes() == (D / "media" / "video.mp4").read_bytes(), (kod, se[-300:]))

        # 5) heslo a DRM
        kod, so, se = kometa("stiahni", "https://vimeo.com/444444", "-o", str(out))
        skontroluj("Vimeo s heslom: jasná správa", kod == 1 and "heslom" in se, (kod, se[-200:]))
        kod, so, se = kometa("stiahni", "https://vimeo.com/333333", "-o", str(out))
        skontroluj("Vimeo s DRM: zastavené (kód 2)", kod == 2, (kod, se[-200:]))

        # 6) JW Player na stránke
        kod, so, se = kometa("stiahni", BASE + "/jwstranka", "-o", str(out))
        f = out / "Ukážka JW.mkv"
        skontroluj("JW Player → HLS → MKV", kod == 0 and f.exists() and stopy(f) == ["video", "audio"], (kod, se[-300:]))

        # 6b) Wistia vložená triedou → najvyššie MP4 (adresa .bin) s príponou .mp4
        kod, so, se = kometa("stiahni", BASE + "/wistiastranka", "-o", str(out))
        f = out / "Wistia návod.mp4"
        skontroluj("Wistia → najlepšie MP4 s názvom", kod == 0 and f.exists()
                   and f.read_bytes() == (D / "media" / "video.mp4").read_bytes(), (kod, se[-300:], list(out.glob("*"))))

        # 6c) Cloudflare Stream v rámčeku → HLS, názov zo stránky
        kod, so, se = kometa("stiahni", BASE + "/cfstranka", "-o", str(out))
        f = out / "Cloudflare ukážka.mkv"
        skontroluj("Cloudflare Stream → HLS → MKV s názvom stránky", kod == 0 and f.exists()
                   and stopy(f) == ["video", "audio"], (kod, se[-300:], list(out.glob("*"))))

        # 7) JSON-LD VideoObject → priamy súbor s názvom videa
        kod, so, se = kometa("stiahni", BASE + "/clanok", "-o", str(out))
        f = out / "Prechádzka mestom.mp4"
        skontroluj("JSON-LD: súbor s názvom videa", kod == 0 and f.exists(), (kod, se[-300:], list(out.glob("*"))))

        # 7b) obsah rozhodne, nie Content-Type
        kod, so, se = kometa("stiahni", BASE + "/bez_pripony/zoznam", "-o", str(out), "-n", "zoznam")
        f = out / "zoznam.mkv"
        skontroluj("playlist bez prípony ako octet-stream → stream", kod == 0 and f.exists()
                   and stopy(f) == ["video", "audio"], (kod, se[-300:]))
        kod, so, se = kometa("stiahni", BASE + "/archiv/balik.zip", "-o", str(out))
        f = out / "balik.zip"
        skontroluj("ZIP označený ako text → stiahnutý bez zmeny", kod == 0 and f.exists() and f.read_bytes() == ZIP,
                   (kod, se[-300:]))

        # 7c) YouTube jadro zatiaľ nesťahuje – jasná správa bez sieťovej požiadavky
        kod, so, se = kometa("stiahni", "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "-o", str(out))
        skontroluj("YouTube: odkaz na okno Komety (kód 5)", kod == 5 and "okno Komety" in se, (kod, se[-200:]))

        # 8) prázdna stránka
        kod, so, se = kometa("stiahni", BASE + "/prazdna", "-o", str(out))
        skontroluj("prázdna stránka: jasná správa", kod == 1 and "nenašlo" in se, (kod, se[-200:]))

        # 9) info o stránke
        kod, so, se = kometa("info", BASE + "/kurz/lekcia")
        i = json.loads(so) if kod == 0 else {}
        skontroluj("info: cieľ Vimeo stream a plán so stopami",
                   i.get("ciel", {}).get("poskytovatel") == "Vimeo" and i.get("ciel", {}).get("druh") == "stream"
                   and len((i.get("plan") or {}).get("stopy", [])) == 2, (kod, so[:300], se[-200:]))
    finally:
        srv.shutdown()
        shutil.rmtree(D, ignore_errors=True)
    if CHYBY:
        print(f"\nZLYHALO {len(CHYBY)}")
        return 1
    print("\nVŠETKO OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
