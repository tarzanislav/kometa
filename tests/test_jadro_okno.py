"""Skúška služby jadra v Ruste cez `kometa okno --bez-okna` – rovnaké volania, aké posiela rozhranie.

Overí: ochranu servera (kľúč, Host), nastavenia (spoločný súbor s Pythonom), front (súbor, HLS,
pauza a pokračovanie s rovnakým výsledkom, odstránenie s upratovaním, ukončenie a pokračovanie po
reštarte), skener a stránku s viacerými videami, yt-dlp (vlastnou cestou bez FFmpeg aj samotným
yt-dlp), doinštalovanie yt-dlp s kontrolou SHA-256, Webshare a Fastshare na simulovaných serveroch
a – ak je nainštalovaný Playwright – aj rozhranie v Chromiu.

    python tests/test_jadro_okno.py          (po `cargo build --release` v koreni repozitára)
"""
import hashlib
import http.server
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

KOREN = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(KOREN))
from kometa.hosts import md5_crypt  # noqa: E402 – referenčný výpočet hesla Webshare

EXE = KOREN / "target" / "release" / ("kometa.exe" if os.name == "nt" else "kometa")
FF, FP, YT = shutil.which("ffmpeg"), shutil.which("ffprobe"), shutil.which("yt-dlp")
D = Path(tempfile.mkdtemp(prefix="kometa_okno_"))
CHYBY: list[str] = []
BASE = ""
VELKY = bytes((i * 7 + (i >> 9)) & 0xFF for i in range(24 << 20))
FALOSNY_YTDLP = b"#!/bin/sh\necho 2099.01.01\n" + b"#" * 4096
WS_SUBOR = os.urandom(3 << 20)
FF_ARCHIV: list[bytes] = []
ARCHIVY: dict[str, bytes] = {}
UNIKY: list[str] = []            # cookies kurzu, ktoré by odišli na cudziu doménu   # názov → obsah (vytvorí priprav_archivy())   # [archív tar.xz, program ffmpeg] – vytvorí priprav_ffmpeg()
WS_SOL, WS_HESLO = "AbCd1234", "tajné heslo"
op = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def skontroluj(nazov, podmienka, detail=""):
    if podmienka:
        print(f"  OK  {nazov}")
    else:
        CHYBY.append(f"{nazov}: {detail}")
        print(f"  ZLYHALO  {nazov}: {str(detail)[:600]}")


class Server(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def __init__(self, *a, **k):
        super().__init__(*a, directory=str(D / "web"), **k)

    def log_message(self, *a):
        pass

    def posli(self, kod, telo, typ="text/html; charset=utf-8", hlavicky=()):
        b = telo.encode() if isinstance(telo, str) else telo
        self.send_response(kod)
        self.send_header("Content-Type", typ)
        self.send_header("Content-Length", str(len(b)))
        for k, v in hlavicky:
            self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(b)

    def rozsah(self, data, pomaly=False):
        """Súbor s podporou Range; `pomaly` = ~6 MB/s, aby sa dalo pozastaviť uprostred."""
        r = self.headers.get("Range")
        zac, kon = 0, len(data) - 1
        if r and r.startswith("bytes="):
            a, _, b = r[6:].partition("-")
            zac, kon = int(a or 0), int(b) if b else len(data) - 1
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {zac}-{kon}/{len(data)}")
        else:
            self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(kon - zac + 1))
        self.end_headers()
        try:
            for i in range(zac, kon + 1, 64 << 10):
                self.wfile.write(data[i:min(i + (64 << 10), kon + 1)])
                if pomaly:
                    time.sleep(0.01)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_HEAD(self):  # noqa: N802
        self.do_GET()

    def do_GET(self):  # noqa: N802
        u = urllib.parse.urlparse(self.path)
        if u.path == "/velky.bin":
            return self.rozsah(VELKY, pomaly=True)
        if u.path == "/ws/subor.bin":
            return self.rozsah(WS_SUBOR)
        if u.path == "/stranka-viac":
            return self.posli(200, f"""<html><head><title>Dve videá</title></head><body>
                <video src="{BASE}/media/a.mp4"></video><video src="{BASE}/media/b.mp4"></video></body></html>""")
        if u.path == "/vydania/SHA2-256SUMS":
            return self.posli(200, f"{hashlib.sha256(FALOSNY_YTDLP).hexdigest()}  yt-dlp_linux\n"
                              f"{'0' * 64}  yt-dlp.exe\n", "text/plain")
        if u.path == "/vydania/yt-dlp_linux":
            return self.posli(200, FALOSNY_YTDLP, "application/octet-stream")
        if u.path == "/zle-vydania/SHA2-256SUMS":
            return self.posli(200, f"{'1' * 64}  yt-dlp_linux\n", "text/plain")
        if u.path == "/zle-vydania/yt-dlp_linux":
            return self.posli(200, FALOSNY_YTDLP, "application/octet-stream")
        # Fastshare: stránka súboru rovno presmeruje na súbor (s kreditom) – s názvom v hlavičke aj bez neho
        if u.path == "/fastshare/1/a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv":
            return self.posli(302, "", hlavicky=[("Location", "/fastshare/dl/1")])
        if u.path == "/fastshare/2/druhy.film.2024.mkv":
            return self.posli(302, "", hlavicky=[("Location", "/fastshare/download.php?id=2")])
        if u.path == "/fastshare/dl/1":
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Disposition", 'attachment; filename="A.Loose.End.2024.1080p.mkv"')
            self.send_header("Content-Length", str(len(WS_SUBOR)))
            self.end_headers()
            if self.command != "HEAD":
                self.wfile.write(WS_SUBOR)
            return
        if u.path == "/fastshare/download.php":
            return self.rozsah(WS_SUBOR)
        if u.path.startswith("/datoid/"):
            q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
            m = u.path[len("/datoid/"):]
            j = lambda x: self.posli(200, json.dumps(x), "application/json")  # noqa: E731
            if m == "login":
                ok = q.get("email") == "ja@example.sk" and q.get("password") == "dt-heslo"
                return j({"success": True, "token": "dt-tok"} if ok else {"success": False})
            if m == "get-user-details":
                return j({"credits": 2048} if q.get("token") == "dt-tok" else {"error": "Bad token"})
            zamknuty = "zamknuty" in q.get("url", "")
            if zamknuty and q.get("password") != "dt-subor":
                return j({"error": "File is password protected"})
            if m == "get-file-details":
                return j({"filename": "Datoid súbor.mkv" if not zamknuty else "Datoid zamknutý.mkv",
                          "filesize_bytes": len(WS_SUBOR)})
            if m == "get-download-link":
                if q.get("token") != "dt-tok":
                    return j({"error": "Bad token"})
                return j({"download_link": f"{BASE}/ws/subor.bin".replace("/", "\\/")})
        if u.path == "/media/Archiv.rar":
            return self.posli(200, (KOREN / "jadro/testdata/zaheslovany.rar").read_bytes(), "application/octet-stream")
        if u.path.startswith("/archivy/"):
            return self.posli(200, ARCHIVY[u.path.rsplit("/", 1)[1]], "application/octet-stream")
        if u.path == "/oembed":
            return self.posli(200, json.dumps({"title": "Pieseň o kométe", "author_name": "Kanál"}), "application/json")
        if u.path.startswith("/kurz/"):
            prihlaseny = "sid=tajne" in (self.headers.get("Cookie") or "")
            if not prihlaseny:
                return self.posli(403, "<p>Prihlás sa</p>")
            if u.path == "/kurz/lekcia-1":
                return self.posli(200, f'<html><h1>Úvod</h1><video src="{BASE}/media/a.mp4"></video></html>')
            if u.path == "/kurz/lekcia-2":
                cudzi = BASE.replace("127.0.0.1", "localhost") + "/cudzie/b.mp4"
                return self.posli(200, f'<html><h1>Ďalej</h1><video src="{cudzi}"></video></html>')
        if u.path == "/cudzie/b.mp4":
            if "sid=" in (self.headers.get("Cookie") or ""):
                UNIKY.append(self.headers.get("Cookie"))
            return self.rozsah((D / "web/media/b.mp4").read_bytes())
        if u.path == "/ff-vydanie":
            return self.posli(200, json.dumps({"assets": [
                {"name": n, "browser_download_url": f"{BASE}/ff/{n}"}
                for n in ("checksums.sha256", "ffmpeg-master-latest-linux64-gpl-shared.tar.xz",
                          "ffmpeg-master-latest-linux64-gpl.tar.xz")]}), "application/json")
        if u.path == "/ff/checksums.sha256":
            return self.posli(200, f"{hashlib.sha256(FF_ARCHIV[0]).hexdigest()}  ffmpeg-master-latest-linux64-gpl.tar.xz\n",
                              "text/plain")
        if u.path == "/ff/ffmpeg-master-latest-linux64-gpl.tar.xz":
            return self.posli(200, FF_ARCHIV[0], "application/octet-stream")
        if u.path == "/fastshare/user":
            prihlaseny = "fs=ok" in (self.headers.get("Cookie") or "")
            telo = '<a href="/logout.php">x</a><table><td>12,5 GB</td></table>' if prihlaseny else "<form></form>"
            return self.posli(200, telo)
        return super().do_GET()

    def do_POST(self):  # noqa: N802
        u = urllib.parse.urlparse(self.path)
        n = int(self.headers.get("Content-Length") or 0)
        f = {k: v[0] for k, v in urllib.parse.parse_qs(self.rfile.read(n).decode()).items()}
        if u.path.startswith("/ws/api/"):
            m = u.path[8:].strip("/")
            ok = lambda x="": f"<?xml version='1.0'?><response><status>OK</status>{x}</response>"  # noqa: E731
            if m == "salt":
                return self.posli(200, ok(f"<salt>{WS_SOL}</salt>"), "text/xml")
            if m == "login":
                cakane = hashlib.sha1(md5_crypt(WS_HESLO, WS_SOL).encode()).hexdigest()
                if f.get("password") != cakane or f.get("username_or_email") != "ja":
                    return self.posli(200, "<response><status>FATAL</status><message>Bad login</message></response>",
                                      "text/xml")
                return self.posli(200, ok("<token>tok-123</token>"), "text/xml")
            if f.get("wst") != "tok-123":
                return self.posli(200, "<response><status>FATAL</status><code>BAD_TOKEN</code></response>", "text/xml")
            if m == "user_data":
                return self.posli(200, ok("<username>ja</username><vip>1</vip><vip_days>30</vip_days>"
                                          "<vip_until>2027-01-01 10:00:00</vip_until>"), "text/xml")
            if m == "file_info":
                if f.get("ident") == "zamok1":
                    return self.posli(200, ok(f"<name>Zamknutý film.mkv</name><size>{len(WS_SUBOR)}</size>"
                                              "<password>1</password>"), "text/xml")
                return self.posli(200, ok(f"<name>Film &amp; titulky.mkv</name><size>{len(WS_SUBOR)}</size>"), "text/xml")
            if m == "file_password_salt":
                return self.posli(200, ok("<salt>SoLsUbOr</salt>"), "text/xml")
            if m == "file_link":
                if f.get("ident") == "zamok1":
                    spravne = hashlib.sha1(md5_crypt("tajné heslo", "SoLsUbOr").encode()).hexdigest()
                    if f.get("password") != spravne:
                        return self.posli(200, "<response><status>FATAL</status><code>FILE_LINK_FATAL_5</code>"
                                               "<message>Bad password</message></response>", "text/xml")
                return self.posli(200, ok(f"<link>{BASE}/ws/subor.bin</link>"), "text/xml")
        if u.path == "/fastshare/sql.php":
            if f.get("login") == "ja" and f.get("heslo") == "fs-heslo":
                return self.posli(200, '<a href="/logout.php">x</a>', hlavicky=[("Set-Cookie", "fs=ok; Path=/")])
            return self.posli(200, "<p>Zlé meno</p>")
        self.posli(404, "")


class Tichy(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        if not isinstance(sys.exc_info()[1], (BrokenPipeError, ConnectionResetError)):
            super().handle_error(request, client_address)


class Okno:
    """Spustená `kometa okno --bez-okna` s vlastným priečinkom nastavení."""

    def __init__(self, priecinok: Path, **env):
        self.priecinok = priecinok
        e = dict(os.environ, KOMETA_PRIECINOK=str(priecinok), KOMETA_BEZ_TREZORU="1", NO_PROXY="127.0.0.1,localhost",
                 KOMETA_WEBSHARE_ZAKLAD=BASE + "/ws/api", KOMETA_FASTSHARE_ZAKLAD=BASE + "/fastshare", KOMETA_DATOID_ZAKLAD=BASE + "/datoid", **env)
        self.p = subprocess.Popen([str(EXE), "okno", "--bez-okna"], stdout=subprocess.PIPE, text=True, env=e)
        self.url = ""
        for _ in range(20):
            r = self.p.stdout.readline()
            if r.startswith("KOMETA_ADRESA "):
                self.url = r.split()[1]
                break
        self.base, self.kluc = self.url.split("/#t=")

    def volaj(self, metoda, *args, kluc=None, host=None):
        h = {"X-Kometa": self.kluc if kluc is None else kluc, "Content-Type": "application/json"}
        if host:
            h["Host"] = host
        r = urllib.request.Request(f"{self.base}/api/{metoda}", data=json.dumps(list(args)).encode(), headers=h)
        return json.loads(op.open(r, timeout=60).read())

    def data(self, metoda, *args):
        o = self.volaj(metoda, *args)
        if not o.get("ok"):
            raise AssertionError(f"{metoda}: {o}")
        return o.get("data")

    def cakaj(self, podmienka, sekund=60):
        koniec = time.time() + sekund
        while time.time() < koniec:
            s = self.data("stav")
            if podmienka(s):
                return s
            time.sleep(0.25)
        return self.data("stav")

    def zatvor(self):
        if self.p.poll() is None:
            self.p.send_signal(signal.SIGINT)
            try:
                return self.p.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.p.kill()
        return self.p.returncode


def polozka(s, url):
    return next((x for x in s["polozky"] if x["url"] == url), None)


def stopy(p: Path):
    r = subprocess.run([FP, "-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0", str(p)],
                       capture_output=True, text=True)
    return r.stdout.split()


def priprav_ffmpeg():
    """Archív ako zo zostavení BtbN: ffmpeg-master-latest-linux64-gpl/bin/{ffmpeg,ffprobe}."""
    import io
    import tarfile
    program = b"#!/bin/sh\necho falosny ffmpeg\n"
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:xz") as t:
        for meno in ("ffmpeg", "ffprobe", "ffplay"):
            ti = tarfile.TarInfo(f"ffmpeg-master-latest-linux64-gpl/bin/{meno}")
            ti.size, ti.mode = len(program), 0o755
            t.addfile(ti, io.BytesIO(program))
        ti = tarfile.TarInfo("ffmpeg-master-latest-linux64-gpl/LICENSE.txt")
        ti.size = 3
        t.addfile(ti, io.BytesIO(b"GPL"))
    FF_ARCHIV[:] = [buf.getvalue(), program]


def priprav_archivy():
    """ZIP s viacerými súbormi a ZIP rozdelený na .zip.001 + .zip.002 (ako ho delí 7-Zip)."""
    import io
    import zipfile
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("Kurz/01 Úvod.txt", "úvod")
        z.writestr("Kurz/02 Druhá.txt", "druhá" * 1000)
        z.writestr("../mimo.txt", "nesmie von")
    ARCHIVY["Balik.zip"] = buf.getvalue()
    film = os.urandom(200_000)
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_STORED) as z:
        z.writestr("Film.mkv", film)
    d = buf.getvalue()
    ARCHIVY["Velky.zip.001"], ARCHIVY["Velky.zip.002"] = d[:120_000], d[120_000:]
    ARCHIVY["film-obsah"] = film


def priprav():
    priprav_ffmpeg()
    priprav_archivy()
    m = D / "web" / "media"
    m.mkdir(parents=True)
    ff = lambda *a: subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *a], cwd=m, check=True)  # noqa: E731
    src = ["-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=400"]
    for meno, t in (("a.mp4", 2), ("b.mp4", 1)):
        ff(*src, "-t", str(t), "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart", meno)
    vod = ["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"]
    ff("-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-t", "2", "-c:v", "libx264", "-pix_fmt", "yuv420p",
       *vod, "-hls_segment_filename", "v_%03d.ts", "v.m3u8")
    ff("-f", "lavfi", "-i", "sine=frequency=600", "-t", "2", "-c:a", "aac", *vod, "-hls_segment_filename", "z_%03d.ts",
       "z.m3u8")
    (m / "master.m3u8").write_text(
        '#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="a",LANGUAGE="sk",NAME="Slovenčina",DEFAULT=YES,URI="z.m3u8"\n'
        '#EXT-X-STREAM-INF:BANDWIDTH=600000,RESOLUTION=320x240,AUDIO="a"\nv.m3u8\n', encoding="utf-8")


def zakladne(o: Okno, out: Path):
    print("Server a nastavenia")
    r = op.open(o.base + "/", timeout=10)
    skontroluj("rozhranie je vložené v programe", r.status == 200 and b"<html" in r.read().lower()
               and "default-src 'self'" in (r.headers.get("Content-Security-Policy") or ""))
    for nazov, kw in (("bez kľúča", {"kluc": ""}), ("zlý kľúč", {"kluc": "0" * 32}), ("cudzí Host", {"host": "zly.sk"})):
        try:
            o.volaj("init", **kw)
            skontroluj(f"API odmietne volanie – {nazov}", False, "prešlo")
        except urllib.error.HTTPError as e:
            skontroluj(f"API odmietne volanie – {nazov}", e.code == 403, e.code)
    skontroluj("neznáma akcia", o.volaj("zmaz_disk") == {"ok": False, "chyba": "Neznáma akcia."})
    i = o.data("init")
    skontroluj("init: jadro v Ruste a čo vie", i["jadro"] == "rust" and i["funkcie"]["jazyky"] is True
               and i["funkcie"]["ucty"] and len(i["formaty"]) == 7 and i["nastavenia"]["tema"] == "les", i)
    n = o.data("ulozit_nastavenia", {"outdir": str(out), "parallel": 99, "connections": "3", "heslo": "x"})
    ulozene = json.loads((o.priecinok / "nastavenia.json").read_text(encoding="utf-8"))
    skontroluj("nastavenia: kontrola hodnôt, neznáme kľúče okna zahodené, vlastné ostanú",
               n["parallel"] == 6 and n["connections"] == 3 and "heslo" not in ulozene
               and ulozene.get("vlastny") == "z Pythonu" and ulozene["outdir"] == str(out), ulozene)
    r = o.data("rozpoznat", "https://youtu.be/dQw4w9WgXcQ a https://www.youtube.com/@ucitel a https://webshare.cz/#/file/x")
    skontroluj("rozpoznanie odkazov", [(x["zdroj"], x["kolekcia"]) for x in r]
               == [("YouTube", False), ("YouTube", True), ("Webshare", False)], r)
    chyba = o.volaj("pridat", "bez odkazu")
    skontroluj("pridanie bez odkazu: jasná veta", not chyba["ok"] and "http" in chyba["chyba"], chyba)


def front(o: Okno, out: Path):
    print("Front")
    u1, u2 = BASE + "/media/a.mp4", BASE + "/media/master.m3u8"
    p = o.data("pridat", f"{u1}\n{u2}\n{u1}", "video")
    skontroluj("pridanie: opakovaný odkaz sa vynechá", p == {"pridane": 2, "uz_vo_fronte": 0, "kolekcie": []}, p)
    s = o.cakaj(lambda s: all(x["stav"] in ("hotovo", "chyba") for x in s["polozky"]))
    a, h = polozka(s, u1), polozka(s, u2)
    skontroluj("priamy súbor stiahnutý bez zmeny", a["stav"] == "hotovo"
               and (out / "a.mp4").read_bytes() == (D / "web/media/a.mp4").read_bytes(), a)
    skontroluj("HLS → MKV s obrazom aj zvukom", h["stav"] == "hotovo" and h["subor"].endswith(".mkv")
               and stopy(out / h["subor"]) == ["video", "audio"], h)

    # pauza uprostred a pokračovanie – výsledok musí byť bajt po bajte rovnaký
    uv = BASE + "/velky.bin"
    o.data("pridat", uv, "video")
    s = o.cakaj(lambda s: (polozka(s, uv) or {}).get("hotovo", 0) > (3 << 20), 30)
    iid = polozka(s, uv)["id"]
    o.data("pozastavit", [iid])
    s = o.cakaj(lambda s: polozka(s, uv)["stav"] == "pauza", 15)
    rozpracovane = [x.name for x in out.iterdir() if x.name.startswith("velky.bin.")]
    skontroluj("pauza uprostred: rozpracované časti ostali", polozka(s, uv)["stav"] == "pauza" and rozpracovane,
               (polozka(s, uv), rozpracovane))
    o.data("spustit", [iid])
    s = o.cakaj(lambda s: polozka(s, uv)["stav"] in ("hotovo", "chyba"), 60)
    f = out / "velky.bin"
    skontroluj("pokračovanie po pauze: súbor je celý a správny", polozka(s, uv)["stav"] == "hotovo" and f.exists()
               and hashlib.sha256(f.read_bytes()).digest() == hashlib.sha256(VELKY).digest(), polozka(s, uv))
    skontroluj("po dokončení žiadne pomocné súbory", not [x for x in out.iterdir() if x.name.startswith("velky.bin.")])

    # odstránenie počas sťahovania uprace rozpracované časti
    f.unlink()
    o.data("vycistit_hotove")
    o.data("pridat", uv, "video")
    s = o.cakaj(lambda s: (polozka(s, uv) or {}).get("hotovo", 0) > (2 << 20), 30)
    o.data("odstranit", [polozka(s, uv)["id"]])
    time.sleep(2)
    zvysky = [x.name for x in out.iterdir() if x.name.startswith("velky.bin")]
    skontroluj("odstránenie počas sťahovania: nič nezostalo", not zvysky and polozka(o.data("stav"), uv) is None, zvysky)

    # stránka s dvoma videami: skener a rozbalenie do frontu
    us = BASE + "/stranka-viac"
    n = o.data("skenovat", us)
    skontroluj("skener: dve odporúčané videá", sum(1 for x in n if x["odporucane"] and x["da_sa"]) == 2
               and {x["druh"] for x in n} == {"video"}, n)
    o.data("pridat", us, "video")
    s = o.cakaj(lambda s: polozka(s, us) and polozka(s, us)["stav"] in ("rozbalene", "chyba")
                and all(x["stav"] in ("hotovo", "chyba", "rozbalene") for x in s["polozky"]), 40)
    nove = [x for x in s["polozky"] if x["url"].endswith(("/a.mp4", "/b.mp4")) and x["nazov"].startswith("Dve videá")]
    skontroluj("stránka s viacerými videami sa rozbalí do frontu", polozka(s, us)["stav"] == "rozbalene"
               and len(nove) == 2 and all(x["stav"] == "hotovo" for x in nove), [(x["nazov"], x["stav"]) for x in s["polozky"]])


def ucty(o: Okno, out: Path):
    print("Webshare a Fastshare")
    zle = o.volaj("prihlasit", "webshare", "ja", "zlé heslo")
    skontroluj("Webshare: zlé heslo – jasná správa", not zle["ok"] and "skontroluj meno a heslo" in zle["chyba"], zle)
    u = o.data("prihlasit", "webshare", "ja", WS_HESLO)
    skontroluj("Webshare: prihlásenie s heslom solenom MD5-crypt + SHA-1", u["stav"] == "ok"
               and u["text"].startswith("VIP do 2027-01-01") and "(30 dní)" in u["text"], u)
    skontroluj("heslo uložené mimo nastavení", "ucty.dat" in os.listdir(o.priecinok)
               and WS_HESLO not in (o.priecinok / "nastavenia.json").read_text(encoding="utf-8"))
    uw = "https://webshare.cz/#/file/abc123/film"
    o.data("pridat", uw, "video")
    s = o.cakaj(lambda s: polozka(s, uw)["stav"] in ("hotovo", "chyba"), 40)
    f = out / "Film & titulky.mkv"
    skontroluj("Webshare: súbor s názvom zo služby", polozka(s, uw)["stav"] == "hotovo" and f.exists()
               and f.read_bytes() == WS_SUBOR, polozka(s, uw))
    fs = o.data("prihlasit", "fastshare", "ja", "fs-heslo")
    skontroluj("Fastshare: prihlásenie a kredit", fs["stav"] == "ok" and fs["text"] == "kredit 12,5 GB", fs)
    f1 = "https://fastshare.cloud/1/a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv"
    f2 = "https://fastshare.cz/2/druhy.film.2024.mkv"
    o.data("pridat", f"{f1}\n{f2}", "mp3")
    s = o.data("stav")
    skontroluj("úložisko: položka je „súbor“ (nie zvolený formát) a má názov z odkazu",
               polozka(s, f1)["format"] == "subor" and polozka(s, f2)["format"] == "subor"
               and polozka(s, f2)["nazov"] == "druhy.film.2024.mkv", [polozka(s, f1), polozka(s, f2)])
    s = o.cakaj(lambda s: all(polozka(s, x)["stav"] in ("hotovo", "chyba") for x in (f1, f2)), 40)
    skontroluj("Fastshare: názov od servera (Content-Disposition)", polozka(s, f1)["stav"] == "hotovo"
               and (out / "A.Loose.End.2024.1080p.mkv").read_bytes() == WS_SUBOR, polozka(s, f1))
    skontroluj("Fastshare: bez názvu od servera – názov z odkazu, prípona ostane", polozka(s, f2)["stav"] == "hotovo"
               and (out / "druhy.film.2024.mkv").read_bytes() == WS_SUBOR, polozka(s, f2))
    r = o.data("rozpoznat", f"{f1} https://youtu.be/dQw4w9WgXcQ")
    skontroluj("rozpoznanie: úložisko vs. videostránka", [x["ulozisko"] for x in r] == [True, False], r)

    # Webshare: súbor chránený heslom – zastaví sa hneď na začiatku a čaká na heslo
    uz = "https://webshare.cz/#/file/zamok1/film"
    o.data("prihlasit", "webshare", "ja", WS_HESLO)
    o.data("pridat", uz, "video")
    s = o.cakaj(lambda s: polozka(s, uz)["stav"] in ("hotovo", "chyba"), 30)
    skontroluj("Webshare: zamknutý súbor si vypýta heslo", polozka(s, uz)["treba_heslo"] == "subor"
               and "heslom" in polozka(s, uz)["chyba"], polozka(s, uz))
    o.data("zadat_heslo", polozka(s, uz)["id"], "zlé")
    s = o.cakaj(lambda s: polozka(s, uz)["stav"] in ("hotovo", "chyba"), 30)
    skontroluj("Webshare: zlé heslo – znova si ho vypýta", polozka(s, uz)["stav"] == "chyba"
               and polozka(s, uz)["treba_heslo"] == "subor" and "nesedí" in polozka(s, uz)["chyba"], polozka(s, uz))
    o.data("zadat_heslo", polozka(s, uz)["id"], "tajné heslo")
    s = o.cakaj(lambda s: polozka(s, uz)["stav"] in ("hotovo", "chyba"), 30)
    skontroluj("Webshare: so správnym heslom stiahnutý", polozka(s, uz)["stav"] == "hotovo"
               and (out / "Zamknutý film.mkv").read_bytes() == WS_SUBOR, polozka(s, uz))

    # Datoid
    zle = o.volaj("prihlasit", "datoid", "ja@example.sk", "zlé")
    skontroluj("Datoid: zlé heslo – jasná správa", not zle["ok"] and "e-mail a heslo" in zle["chyba"], zle)
    d = o.data("prihlasit", "datoid", "ja@example.sk", "dt-heslo")
    skontroluj("Datoid: prihlásenie a kredit", d["stav"] == "ok" and d["text"] == "kredit 2048", d)
    d1, d2 = "https://datoid.cz/abc123/datoid-subor-mkv", "https://datoid.sk/xyz789/zamknuty-mkv"
    o.data("pridat", d1, "video")
    o.data("pridat", d2, "video", "dt-subor")
    s = o.cakaj(lambda s: all(polozka(s, x)["stav"] in ("hotovo", "chyba") for x in (d1, d2)), 30)
    skontroluj("Datoid: súbor s názvom zo služby", polozka(s, d1)["stav"] == "hotovo" and polozka(s, d1)["format"] == "subor"
               and (out / "Datoid súbor.mkv").read_bytes() == WS_SUBOR, polozka(s, d1))
    skontroluj("Datoid: heslo zadané vopred pri odkaze", polozka(s, d2)["stav"] == "hotovo"
               and (out / "Datoid zamknutý.mkv").exists(), polozka(s, d2))


def archivy(o: Okno, out: Path):
    print("Rozbaľovanie archívov")
    ua = BASE + "/archivy/Balik.zip"
    o.data("pridat", ua, "video")
    s = o.cakaj(lambda s: (polozka(s, ua) or {}).get("rozbalenie") and polozka(s, ua)["rozbalenie"]["stav"]
                not in ("bezi", "caka"), 30)
    r = polozka(s, ua)["rozbalenie"]
    skontroluj("ZIP sa po stiahnutí sám rozbalí (jediný priečinok rovno vedľa archívu)", r["stav"] == "hotovo"
               and r["ciel"] == "Kurz" and (out / "Kurz" / "02 Druhá.txt").read_text() == "druhá" * 1000
               and (out / "Balik.zip").exists(), r)
    skontroluj("archív nezapíše nič mimo priečinka", not (out.parent / "mimo.txt").exists()
               and not (out / "mimo.txt").exists())
    u1, u2 = BASE + "/archivy/Velky.zip.001", BASE + "/archivy/Velky.zip.002"
    o.data("pridat", u1, "video")
    s = o.cakaj(lambda s: polozka(s, u1)["stav"] == "hotovo" and (polozka(s, u1).get("rozbalenie") or {}).get("stav"), 30)
    skontroluj("rozdelený archív čaká na ďalšie časti", polozka(s, u1)["rozbalenie"]["stav"] == "caka",
               polozka(s, u1)["rozbalenie"])
    o.data("pridat", u2, "video")
    s = o.cakaj(lambda s: (polozka(s, u1).get("rozbalenie") or {}).get("stav") in ("hotovo", "chyba", "heslo"), 30)
    skontroluj("po poslednej časti sa rozbalí celý (rozbaľuje prvá časť)", polozka(s, u1)["rozbalenie"]["stav"] == "hotovo"
               and (out / "Film.mkv").read_bytes() == ARCHIVY["film-obsah"],
               (polozka(s, u1)["rozbalenie"], polozka(s, u2).get("rozbalenie")))
    skontroluj("druhá časť je označená ako časť archívu", (polozka(s, u2).get("rozbalenie") or {}).get("stav") == "cast",
               polozka(s, u2).get("rozbalenie"))
    ur = BASE + "/media/Archiv.rar"
    o.data("pridat", ur, "video")
    s = o.cakaj(lambda s: polozka(s, ur)["stav"] == "hotovo"
                and (polozka(s, ur).get("rozbalenie") or {}).get("stav") in ("hotovo", "heslo", "chyba"), 30)
    skontroluj("RAR s heslom: vypýta si heslo k archívu", polozka(s, ur)["rozbalenie"]["stav"] == "heslo",
               polozka(s, ur)["rozbalenie"])
    o.data("zadat_heslo", polozka(s, ur)["id"], "unrar")
    s = o.cakaj(lambda s: polozka(s, ur)["rozbalenie"]["stav"] in ("hotovo", "chyba"), 30)
    r = polozka(s, ur)["rozbalenie"]
    skontroluj("RAR so správnym heslom rozbalený bez nového sťahovania", r["stav"] == "hotovo"
               and (out / r["ciel"]).read_text() == "target\nCargo.lock\n", r)
    o.data("ulozit_nastavenia", {"rozbalovat": False})
    ub = BASE + "/archivy/Balik.zip?druhy"
    o.data("pridat", ub, "video")
    s = o.cakaj(lambda s: polozka(s, ub)["stav"] == "hotovo" and polozka(s, ub).get("rozbalenie"), 30)
    skontroluj("rozbaľovanie vypnuté: dá sa spustiť ručne", polozka(s, ub)["rozbalenie"]["stav"] == "vypnute",
               polozka(s, ub)["rozbalenie"])
    o.data("ulozit_nastavenia", {"rozbalovat": True})
    o.data("odhlasit", "webshare")
    s = o.data("stav")
    skontroluj("odhlásenie", s["ucty"]["webshare"]["stav"] == "neprihlaseny" and s["ucty"]["fastshare"]["stav"] == "ok", s["ucty"])


def ytdlp_bez_ffmpeg(out: Path):
    print("yt-dlp bez FFmpeg (formáty vyberie a spojí jadro)")
    o = Okno(D / "cfg-yt", KOMETA_VZDY_YTDLP="1", KOMETA_FFMPEG="nie")
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out / "yt1")})
        u = BASE + "/media/a.mp4"
        o.data("pridat", u, "video")
        s = o.cakaj(lambda s: polozka(s, u)["stav"] in ("hotovo", "chyba"), 60)
        p = polozka(s, u)
        f = out / "yt1" / p["subor"] if p["subor"] else None
        skontroluj("yt-dlp len zistí formáty, stiahne jadro", p["stav"] == "hotovo" and p["subor"] == "a [a].mp4"
                   and f.read_bytes() == (D / "web/media/a.mp4").read_bytes(), p)
        mp3 = o.volaj("pridat", BASE + "/media/b.mp4", "mp3")
        s = o.cakaj(lambda s: polozka(s, BASE + "/media/b.mp4")["stav"] in ("hotovo", "chyba"), 30)
        skontroluj("MP3 bez FFmpeg: jasná veta", mp3["ok"] and "FFmpeg" in polozka(s, BASE + "/media/b.mp4")["chyba"],
                   polozka(s, BASE + "/media/b.mp4"))
    finally:
        o.zatvor()


def ytdlp_plny(out: Path):
    print("yt-dlp s FFmpeg (sťahuje a spája samotný yt-dlp)")
    o = Okno(D / "cfg-yt2", KOMETA_VZDY_YTDLP="1")
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out / "yt2")})
        u = BASE + "/media/b.mp4"
        o.data("pridat", u, "video")
        s = o.cakaj(lambda s: polozka(s, u)["stav"] in ("hotovo", "chyba"), 60)
        p = polozka(s, u)
        skontroluj("yt-dlp: priebeh, názov a výsledný súbor", p["stav"] == "hotovo" and p["nazov"] == "b"
                   and p["subor"].startswith("b [b]") and (out / "yt2" / p["subor"]).exists(), p)
    finally:
        o.zatvor()


def mazanie(o: Okno, out: Path):
    print("Vyčistenie frontu a mazanie stiahnutých súborov")
    s = o.data("stav")
    hotove = [x for x in s["polozky"] if x["stav"] == "hotovo" and x["subor"]]
    jeden = next(x for x in hotove if x["subor"] == "druhy.film.2024.mkv")
    i = o.data("subory_na_zmazanie", [jeden["id"]])
    skontroluj("jedna položka: 1 súbor a jeho veľkosť", i == {"pocet": 1, "velkost": len(WS_SUBOR)}, i)
    (out / "cudzi.txt").write_text("nie je z Komety")
    r = o.data("zmazat_subory", [jeden["id"]])
    s = o.data("stav")
    skontroluj("odstrániť aj súbor: súbor v Koši, položka preč, ostatné ostali", r == {"zmazane": 1, "chyby": []}
               and not (out / "druhy.film.2024.mkv").exists() and polozka(s, jeden["url"]) is None
               and (out / "A.Loose.End.2024.1080p.mkv").exists(), (r, [x["subor"] for x in s["polozky"]]))
    vsetky = o.data("subory_na_zmazanie", None)
    na_disku = [x for x in s["polozky"] if x["stav"] == "hotovo" and x["subor"] and (out / x["subor"]).exists()]
    skontroluj("všetko: počet súborov z frontu", vsetky["pocet"] == len({x["subor"] for x in na_disku}) > 0,
               (vsetky, [x["subor"] for x in na_disku]))
    r = o.data("zmazat_subory", None)
    s = o.data("stav")
    zostalo = [x for x in na_disku if (out / x["subor"]).exists()]
    skontroluj("vymazať stiahnuté súbory: front prázdny, súbory v Koši, cudzí súbor ostal",
               not s["polozky"] and not zostalo and r["chyby"] == [] and (out / "cudzi.txt").exists(),
               (r, zostalo, s["polozky"]))
    o.data("pridat", BASE + "/velky.bin", "video")
    o.cakaj(lambda s: s["polozky"] and s["polozky"][0]["hotovo"] > (1 << 20), 30)
    o.data("vycistit_front")
    time.sleep(2)
    s = o.data("stav")
    skontroluj("vyčistiť front: všetko preč aj počas sťahovania, rozpracované upratané",
               not s["polozky"] and not [x for x in out.iterdir() if x.name.startswith("velky.bin")]
               and (out / "cudzi.txt").exists(), s["polozky"])


def strazca(out: Path):
    print("Strážca schránky")
    schr = D / "schranka.txt"
    schr.write_text("https://youtu.be/uL9-e9OlLfw – bolo tu pred zapnutím", encoding="utf-8")
    o = Okno(D / "cfg-strazca", KOMETA_SKUSKA_SCHRANKA=str(schr), KOMETA_OEMBED_ZAKLAD=BASE + "/oembed")
    try:
        skontroluj("strážca: aplikácia ho ponúka", o.data("init")["funkcie"]["strazca"] is True)
        o.data("ulozit_nastavenia", {"strazca": True, "outdir": str(out)})
        time.sleep(2.5)
        skontroluj("to, čo bolo v schránke pred zapnutím, sa nezachytí", o.data("stav")["zachytene"] == [])
        schr.write_text("pozri https://youtu.be/dQw4w9WgXcQ a https://www.sme.sk/clanok a "
                        "https://webshare.cz/#/file/abc123/film", encoding="utf-8")
        s = o.cakaj(lambda s: len(s["zachytene"]) == 2 and all(z["nazov"] for z in s["zachytene"] if z["zdroj"] == "YouTube"), 15)
        z = {x["zdroj"]: x for x in s["zachytene"]}
        skontroluj("zachytí YouTube (s názvom) a Webshare, článok nie", set(z) == {"YouTube", "Webshare"}
                   and z["YouTube"]["nazov"] == "Pieseň o kométe" and z["Webshare"]["ulozisko"], s["zachytene"])
        schr.write_text("iný text", encoding="utf-8")
        time.sleep(1.5)
        schr.write_text("pozri https://youtu.be/dQw4w9WgXcQ znova", encoding="utf-8")
        time.sleep(2.5)
        skontroluj("ten istý odkaz sa nezachytí dvakrát", len(o.data("stav")["zachytene"]) == 2)
        o.data("schranka_zahodit", [z["Webshare"]["id"]])
        r = o.data("schranka_stiahnut", None, "video")
        s = o.data("stav")
        skontroluj("stiahnuť zachytené: do frontu, zoznam prázdny", r["pridane"] == 1 and s["zachytene"] == []
                   and polozka(s, "https://youtu.be/dQw4w9WgXcQ") is not None, (r, s["zachytene"]))
        o.data("ulozit_nastavenia", {"strazca_rezim": "stiahnut"})
        ua = BASE + "/media/a.mp4?zo-schranky"
        schr.write_text(f"{ua}\nhttps://www.youtube.com/playlist?list=PL123", encoding="utf-8")
        s = o.cakaj(lambda s: polozka(s, ua) is not None and s["zachytene"], 15)
        skontroluj("„hneď stiahnuť“: video rovno do frontu, playlist čaká na výber",
                   polozka(s, ua) is not None and [x["kolekcia"] for x in s["zachytene"]] == [True], s["zachytene"])
    finally:
        o.zatvor()
    o2 = Okno(D / "cfg-strazca", KOMETA_SKUSKA_SCHRANKA=str(schr))
    try:
        skontroluj("zachytené odkazy ostanú aj po reštarte", len(o2.data("stav")["zachytene"]) == 1)
    finally:
        o2.zatvor()


def kurzy(out: Path):
    print("Kurzy po prihlásení")
    pr = D / "prehliadac"
    pr.mkdir()
    (pr / "cookies.json").write_text(json.dumps([[BASE + "/kurz/obsah", "sid=tajne; Path=/; HttpOnly"]]))
    (pr / "zber.json").write_text(json.dumps({
        "url": BASE + "/kurz/obsah", "titulok": "Kurz | Portál", "h1": "Testovací kurz",
        "odkazy": [{"href": BASE + "/prihlasenie", "text": "Prihlásiť", "kapitola": ""},
                   {"href": BASE + "/kurz/lekcia-1", "text": "Úvod do kurzu", "kapitola": "Začíname"},
                   {"href": BASE + "/kurz/lekcia-2", "text": "Pokračovanie", "kapitola": "Ďalej"}],
        "ramce": [], "videa": []}))
    o = Okno(D / "cfg-kurz", KOMETA_SKUSKA_PREHLIADAC=str(pr))
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out)})
        skontroluj("kurzy: aplikácia ich ponúka", o.data("init")["funkcie"]["kurzy"] is True)
        o.data("kurz_otvorit", BASE.replace("http://", "") + "/kurz/obsah")
        skontroluj("adresa bez https:// sa doplní a prehliadač je v stave",
                   (o.data("stav")["prehliadac"] or {}).get("adresa") == BASE.replace("http://", "https://") + "/kurz/obsah",
                   o.data("stav")["prehliadac"])
        k = o.data("kurz_nacitat")
        skontroluj("lekcie z otvorenej stránky podľa kapitol", k["nazov"] == "Testovací kurz"
                   and [(x["nazov"], [l["nazov"] for l in x["lekcie"]]) for x in k["kapitoly"]]
                   == [("Začíname", ["Úvod do kurzu"]), ("Ďalej", ["Pokračovanie"])], k)
        k["kapitoly"][1]["lekcie"][0]["nazov"] = "Pokračovanie (premenované)"
        r = o.data("kurz_stiahnut", k, "video")
        skontroluj("kurz do sťahovania", r["pridane"] == 2, r)
        s = o.cakaj(lambda s: all(x["stav"] in ("hotovo", "chyba") for x in s["polozky"]) and s["polozky"], 40)
        d = out / "Testovací kurz"
        skontroluj("lekcie stiahnuté s prihlásením do priečinkov kapitol",
                   (d / "01 Začíname" / "01 Úvod do kurzu.mp4").read_bytes() == (D / "web/media/a.mp4").read_bytes()
                   and (d / "02 Ďalej" / "01 Pokračovanie (premenované).mp4").exists(),
                   [(x["nazov"], x["stav"], x["chyba"]) for x in s["polozky"]])
        skontroluj("prihlásenie neodišlo na cudziu doménu (vložené video)", not UNIKY, UNIKY)
    finally:
        o.zatvor()
    o2 = Okno(D / "cfg-kurz2")
    try:
        skontroluj("bez aplikácie (okno v prehliadači) kurzy nie sú", o2.data("init")["funkcie"]["kurzy"] is False
                   and not o2.volaj("kurz_otvorit", "https://example.com")["ok"])
    finally:
        o2.zatvor()


def doinstalovanie():
    print("Doinštalovanie yt-dlp")
    cesta = "/usr/bin:/bin"  # bez /usr/local/bin – yt-dlp „chýba“
    zly = Okno(D / "cfg-inst-zly", PATH=cesta, KOMETA_YTDLP_ZAKLAD=BASE + "/zle-vydania")
    try:
        skontroluj("bez yt-dlp: okno to vie", zly.data("init")["chyba_ytdlp"] is True)
        zly.data("doinstalovat_ytdlp")
        s = zly.cakaj(lambda s: not s["nastroje"]["instaluje"], 20)
        skontroluj("zlý kontrolný súčet: nič sa neuloží", "kontrolným súčtom" in s["nastroje"]["text"]
                   and not (D / "cfg-inst-zly/nastroje/yt-dlp").exists(), s["nastroje"])
    finally:
        zly.zatvor()
    o = Okno(D / "cfg-inst", PATH=cesta, KOMETA_YTDLP_ZAKLAD=BASE + "/vydania", KOMETA_FFMPEG_ZAKLAD=BASE + "/ff-vydanie")
    try:
        o.data("doinstalovat_ytdlp")
        s = o.cakaj(lambda s: not s["nastroje"]["instaluje"], 20)
        f = D / "cfg-inst/nastroje/yt-dlp"
        skontroluj("doinštalovaný a overený yt-dlp", s["nastroje"]["ytdlp"] and f.read_bytes() == FALOSNY_YTDLP
                   and os.access(f, os.X_OK), s["nastroje"])
        i = o.data("init")
        skontroluj("FFmpeg sa dá doinštalovať", i["funkcie"]["doinstalovat_ffmpeg"] is True, i["funkcie"])
        o.data("doinstalovat_ffmpeg")
        s = o.cakaj(lambda s: not s["nastroje"]["instaluje"], 30)
        n = D / "cfg-inst/nastroje"
        skontroluj("doinštalovaný a overený FFmpeg (aj ffprobe), bez zvyškov archívu",
                   s["nastroje"]["ffmpeg"] and s["nastroje"]["co"] == "FFmpeg"
                   and (n / "ffmpeg").read_bytes() == FF_ARCHIV[1] and os.access(n / "ffmpeg", os.X_OK)
                   and (n / "ffprobe").exists() and sorted(x.name for x in n.iterdir()) == ["ffmpeg", "ffprobe", "yt-dlp"],
                   (s["nastroje"], sorted(x.name for x in n.iterdir())))
    finally:
        o.zatvor()


def priecinky():
    print("Systémový priečinok Videá (presmerovaný / preložený)")
    domov = D / "domov"
    (domov / ".config").mkdir(parents=True)
    (domov / "Videos").mkdir()
    (domov / "Videá").mkdir()
    (domov / ".config/user-dirs.dirs").write_text('XDG_VIDEOS_DIR="$HOME/Videá"\n', encoding="utf-8")
    cfg = D / "cfg-priecinky"
    cfg.mkdir()
    stary = domov / "Videos" / "Kometa"
    (cfg / "nastavenia.json").write_text(json.dumps({"outdir": str(stary), "tema": "more"}), encoding="utf-8")
    o = Okno(cfg, HOME=str(domov), XDG_CONFIG_HOME=str(domov / ".config"))
    try:
        i = o.data("init")
        s = o.data("stav")
        skontroluj("starý predvolený priečinok sa opraví na systémový (Videá)",
                   i["nastavenia"]["outdir"] == str(domov / "Videá" / "Kometa") and i["nastavenia"]["tema"] == "more"
                   and any("Skôr stiahnuté súbory ostali" in z for z in s["zaznam"]), (i["nastavenia"], s["zaznam"]))
    finally:
        o.zatvor()
    vlastny = D / "moje-filmy"
    (cfg / "nastavenia.json").write_text(json.dumps({"outdir": str(vlastny)}), encoding="utf-8")
    o = Okno(cfg, HOME=str(domov), XDG_CONFIG_HOME=str(domov / ".config"))
    try:
        skontroluj("vlastný priečinok používateľa ostane", o.data("init")["nastavenia"]["outdir"] == str(vlastny))
    finally:
        o.zatvor()


def prud(p: Path):
    """[(typ, kodek, jazyk, názov, predvolená)] zo ffprobe."""
    r = subprocess.run([FP, "-v", "error", "-show_entries",
                        "stream=codec_type,codec_name:stream_tags=language,title,handler_name:stream_disposition=default",
                        "-of", "json", str(p)], capture_output=True, text=True)
    out = []
    for x in json.loads(r.stdout or "{}").get("streams", []):
        t = x.get("tags", {})
        out.append((x["codec_type"], x["codec_name"], t.get("language", ""), t.get("title") or t.get("handler_name", ""),
                    x.get("disposition", {}).get("default", 0)))
    return out


def priprav_jazyky() -> tuple[Path, str]:
    """Médiá a simulovaný yt-dlp, ktorý vráti údaje o videe s jazykmi EN (originál), CZ a SK."""
    m = D / "web" / "media" / "yt"
    m.mkdir()
    ff = lambda *a: subprocess.run([FF, "-hide_banner", "-loglevel", "error", "-y", *a], cwd=m, check=True)  # noqa: E731
    obraz = ["-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-t", "3", "-an"]
    ff(*obraz, "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "v.webm")
    ff(*obraz, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "v.mp4")
    for jazyk, hz in (("en", 300), ("cs", 400), ("sk", 500)):
        ton = ["-f", "lavfi", "-i", f"sine=frequency={hz}:sample_rate=48000", "-t", "3", "-vn"]
        ff(*ton, "-c:a", "libopus", "-b:a", "64k", f"{jazyk}.webm")
        if jazyk != "sk":  # slovenčina len v Opus – AAC treba dorobiť (alebo ostane Opus)
            ff(*ton, "-c:a", "aac", f"{jazyk}.m4a")
    ff("-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=900", "-t", "3",
       "-c:v", "libx264", "-bf", "2", "-pix_fmt", "yuv420p", "-c:a", "aac", "-metadata:s:a:0", "language=eng",
       "-metadata:s:a:0", "title=Pôvodný zvuk", str(D / "moje.mkv"))
    u = lambda n: f"{BASE}/media/yt/{n}"  # noqa: E731
    zvuk = lambda i, jazyk, kodek, n, poznamka, pref: {  # noqa: E731
        "format_id": i, "ext": n.rsplit(".", 1)[1], "vcodec": "none", "acodec": kodek, "abr": 64, "language": jazyk,
        "language_preference": pref, "format_note": poznamka, "protocol": "http", "url": u(n)}
    info = {"id": "dQw4w9WgXcQ", "title": "Jazyková skúška", "uploader": "Kanál", "duration": 3, "formats": [
        {"format_id": "248", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 240, "tbr": 300,
         "protocol": "http", "url": u("v.webm")},
        {"format_id": "137", "ext": "mp4", "vcodec": "avc1.4d400d", "acodec": "none", "height": 240, "tbr": 200,
         "protocol": "http", "url": u("v.mp4")},
        zvuk("140-0", "en", "mp4a.40.2", "en.m4a", "English original (default), medium", 10),
        zvuk("251-0", "en", "opus", "en.webm", "English original (default), medium", 10),
        zvuk("140-1", "cs", "mp4a.40.2", "cs.m4a", "Czech, medium", -1),
        zvuk("251-1", "cs", "opus", "cs.webm", "Czech, medium", -1),
        zvuk("251-2", "sk", "opus", "sk.webm", "Slovak, medium", -1)]}
    (D / "info.json").write_text(json.dumps(info), encoding="utf-8")
    nastroj = D / "falosny-yt-dlp"
    nastroj.write_text(f"#!/bin/sh\ncat '{D / 'info.json'}'\n", encoding="utf-8")
    nastroj.chmod(0o755)
    return nastroj, "https://www.youtube.com/watch?v=dQw4w9WgXcQ"


def jazyky(out: Path):
    print("Jazykové stopy z YouTube (simulovaný yt-dlp, sťahuje a spája jadro)")
    nastroj, url = priprav_jazyky()

    def uloha(o: Okno, volby: dict):
        o.data("jazyky_nacitat", url)
        o.data("jazyky_stiahnut", volby)
        s = o.cakaj(lambda s: not s["jazyky"]["busy"], 90)["jazyky"]
        return s, Path(s["file"]) if s["file"] else None

    o = Okno(D / "cfg-jz", KOMETA_YTDLP=str(nastroj))
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out / "jz")})
        n = o.data("jazyky_nacitat", url)
        skontroluj("načítanie jazykov: originál prvý, výber CZ → SK → EN podľa poradia",
                   [(x["kod"], x["nazov"], x["original"], x["aac"]) for x in n["stopy"]]
                   == [("en", "Angličtina", True, True), ("cs", "Čeština", False, True), ("sk", "Slovenčina", False, False)]
                   and n["vybrane"] == ["cs", "sk"] and n["original"] == "Angličtina" and n["nazov"] == "Jazyková skúška", n)
        # pozn.: angličtina je originál, preto nie je vo „vybrane“ – pridá sa prepínačom „Pôvodná stopa na koniec“
        s, f = uloha(o, {"poradie": ["cs", "sk"], "rezim": "video", "original": True, "kvalita": "Najlepšia",
                         "zvuk": "oboje", "kontajner": "mkv"})
        p = prud(f) if f else []
        cakane = [("video", "vp9", "", "", 1),
                  ("audio", "aac", "cze", "Čeština – AAC", 1), ("audio", "opus", "cze", "Čeština – Opus", 0),
                  ("audio", "aac", "slo", "Slovenčina – AAC", 0), ("audio", "opus", "slo", "Slovenčina – Opus", 0),
                  ("audio", "aac", "eng", "Angličtina – originál – AAC", 0),
                  ("audio", "opus", "eng", "Angličtina – originál – Opus", 0)]
        skontroluj("MKV: obraz VP9, stopy CZ (AAC), CZ (Opus), SK (AAC), SK (Opus), originál (AAC, Opus)",
                   s["text"] == "Hotovo" and f.name == "Jazyková skúška [cs+sk+en].mkv" and p == cakane, (s, p))
        s, f = uloha(o, {"poradie": ["cs", "sk", "en"], "rezim": "audio", "zvuk": "oboje", "kontajner": "mkv",
                         "zvuky": {"cs": "aac", "sk": "aac", "en": "opus"}})
        p = prud(f) if f else []
        skontroluj("voľba pri jazyku: CZ len AAC, SK len AAC (prevodom, bez Opus), EN len Opus",
                   s["text"] == "Hotovo" and p == [("audio", "aac", "cze", "Čeština", 1), ("audio", "aac", "slo", "Slovenčina", 0),
                                                   ("audio", "opus", "eng", "Angličtina – originál", 0)]
                   and any("prevedené do AAC" in r for r in s["log"]), (s, p))
        s, f = uloha(o, {"poradie": ["cs", "sk"], "rezim": "video", "original": False, "zvuk": "oboje", "kontajner": "mp4"})
        p = prud(f) if f else []
        skontroluj("MP4: obraz H.264 a len AAC", s["text"] == "Hotovo" and [x[:3] for x in p]
                   == [("video", "h264", "und"), ("audio", "aac", "ces"), ("audio", "aac", "slk")], (s, p))
        s, f = uloha(o, {"poradie": ["cs"], "rezim": "existing", "subor": str(D / "moje.mkv"), "zvuk": "aac",
                         "kontajner": "mkv"})
        p = prud(f) if f else []
        skontroluj("doplnenie do vlastného videa: nový jazyk prvý, pôvodný zvuk ostane s názvom",
                   s["text"] == "Hotovo" and f.name == "moje [cs].mkv"
                   and [x[:4] for x in p] == [("video", "h264", "", ""), ("audio", "aac", "cze", "Čeština"),
                                               ("audio", "aac", "eng", "Pôvodný zvuk")], (s, p))
        s, f = uloha(o, {"poradie": ["sk"], "rezim": "audio", "zvuk": "opus", "kontajner": "mkv"})
        p = prud(f) if f else []
        skontroluj("len zvuk: Slovenčina v Opus do MKA", s["text"] == "Hotovo" and f.suffix == ".mka"
                   and p == [("audio", "opus", "slo", "Slovenčina", 1)], (s, p))

        # playlist alebo kanál: jazyky pre viac videí naraz – každé video samostatne vo fronte
        yt = lambda i: f"https://www.youtube.com/watch?v={i * 11}"  # noqa: E731
        volby = {"poradie": ["sk", "cs"], "original": False, "zvuk": "oboje", "zvuky": {"sk": "aac"},
                 "kontajner": "mkv", "kvalita": "Najlepšia", "rezim": "video", "bez_jazykov": "povodny"}
        r = o.data("pridat_vybrane", [{"url": yt("a"), "title": "Prvé"}, {"url": yt("b"), "title": "Druhé"}],
                   "video", "Kanál", volby)
        hotove = lambda s, u: (polozka(s, u) or {}).get("stav") in ("hotovo", "chyba")  # noqa: E731
        s = o.cakaj(lambda s: hotove(s, yt("a")) and hotove(s, yt("b")), 120)
        a, b = polozka(s, yt("a")), polozka(s, yt("b"))
        cakane = [("video", "vp9", "", "", 1), ("audio", "aac", "slo", "Slovenčina", 1),
                  ("audio", "aac", "cze", "Čeština – AAC", 0), ("audio", "opus", "cze", "Čeština – Opus", 0)]
        pa = prud(out / "jz" / "Kanál" / a["subor"]) if a["subor"] else []
        pb = prud(out / "jz" / "Kanál" / b["subor"]) if b["subor"] else []
        skontroluj("playlist s jazykmi: každé video SK (len AAC) → CZ (AAC, Opus), do priečinka kanála",
                   r["pridane"] == 2 and a["stav"] == b["stav"] == "hotovo" and pa == cakane and pb == cakane
                   and a["format"] == "jazyky" and a["subor"] != b["subor"], (r, a, b, pa))
        skontroluj("položka ukáže voľby jazykov", (a.get("jazyky") or "").startswith("SK, CS · AAC + Opus · MKV"), a)
        o.data("pridat_vybrane", [{"url": yt("c"), "title": "Tretie"}], "video", "", dict(volby, poradie=["de"],
                                                                                         bez_jazykov="preskocit"))
        s = o.cakaj(lambda s: hotove(s, yt("c")), 60)
        skontroluj("video bez vybraných jazykov sa dá preskočiť", polozka(s, yt("c"))["stav"] == "chyba"
                   and "preskočené" in polozka(s, yt("c"))["chyba"], polozka(s, yt("c")))
        o.data("pridat_vybrane", [{"url": yt("d"), "title": "Štvrté"}], "video", "", dict(volby, poradie=["de"]))
        s = o.cakaj(lambda s: hotove(s, yt("d")), 60)
        d = polozka(s, yt("d"))
        pd = prud(out / "jz" / d["subor"]) if d["subor"] else []
        skontroluj("video bez vybraných jazykov: s pôvodným zvukom", d["stav"] == "hotovo"
                   and [x[2:4] for x in pd[1:]] == [("eng", "Angličtina – originál – AAC"),
                                                    ("eng", "Angličtina – originál – Opus")], (d, pd))
        o.data("pridat", yt("e"), "jazyky")
        s = o.cakaj(lambda s: hotove(s, yt("e")), 60)
        e = polozka(s, yt("e"))
        skontroluj("bežné sťahovanie „Video s jazykmi“ podľa nastavení", e["stav"] == "hotovo" and e["format"] == "jazyky"
                   and len(prud(out / "jz" / e["subor"])) >= 2, e)
    finally:
        o.zatvor()
    o = Okno(D / "cfg-jz2", KOMETA_YTDLP=str(nastroj), KOMETA_FFMPEG="nie")
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out / "jz2")})
        s, f = uloha(o, {"poradie": ["sk"], "rezim": "audio", "zvuk": "aac", "kontajner": "mkv"})
        p = prud(f) if f else []
        skontroluj("bez FFmpeg: jazyk bez AAC ostane v Opus a okno to povie", s["text"] == "Hotovo"
                   and p == [("audio", "opus", "slo", "Slovenčina", 1)]
                   and any("bez FFmpeg ostane Opus" in r for r in s["log"]), (s, p))
    finally:
        o.zatvor()


def restart(out: Path):
    print("Zatvorenie počas sťahovania a pokračovanie po spustení")
    cfg = D / "cfg-restart"
    o = Okno(cfg)
    uv = BASE + "/velky.bin"
    try:
        o.data("ulozit_nastavenia", {"outdir": str(out / "re")})
        o.data("pridat", uv, "video")
        o.cakaj(lambda s: (polozka(s, uv) or {}).get("hotovo", 0) > (2 << 20), 30)
    finally:
        kod = o.zatvor()
    ulozeny = json.loads((cfg / "fronta-jadro.json").read_text(encoding="utf-8"))["polozky"][0]
    skontroluj("po zatvorení: položka čaká na pokračovanie", kod == 0 and ulozeny["stav"] == "caka", (kod, ulozeny))
    o = Okno(cfg)
    try:
        s = o.cakaj(lambda s: polozka(s, uv)["stav"] in ("hotovo", "chyba"), 60)
        f = out / "re" / "velky.bin"
        skontroluj("po novom spustení sa dokončí správne", polozka(s, uv)["stav"] == "hotovo"
                   and hashlib.sha256(f.read_bytes()).digest() == hashlib.sha256(VELKY).digest(), polozka(s, uv))
    finally:
        o.zatvor()


def rozhranie(o: Okno, out: Path):
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("Rozhranie: Playwright nie je nainštalovaný – preskakujem")
        return
    print("Rozhranie v Chromiu nad jadrom v Ruste")
    with sync_playwright() as p:
        b = p.chromium.launch()
        s = b.new_page(viewport={"width": 1280, "height": 860})
        chyby_js = []
        s.on("pageerror", lambda e: chyby_js.append(str(e)))
        s.goto(o.url)
        s.get_by_role("heading", name="Sťahovanie").wait_for(timeout=15000)
        pole = s.locator("textarea, input[type=url], input[type=text]").first
        pole.fill(BASE + "/media/b.mp4")
        s.get_by_role("button", name="Stiahnuť").click()
        s.get_by_text("b.mp4").first.wait_for(timeout=20000)
        o.cakaj(lambda st: (polozka(st, BASE + "/media/b.mp4") or {}).get("stav") == "hotovo", 20)
        s.get_by_role("button", name="Nastavenia").first.click()
        s.get_by_role("heading", name="Pomocníci").wait_for(timeout=10000)
        skontroluj("rozhranie: Nastavenia ukazujú pomocníkov yt-dlp a FFmpeg", s.get_by_text("yt-dlp", exact=True).count() >= 1)
        s.get_by_role("button", name="Jazykové stopy").first.click()
        s.get_by_role("heading", name="Jazykové stopy").first.wait_for(timeout=10000)
        skontroluj("rozhranie: Jazykové stopy sú dostupné aj v aplikácii",
                   s.get_by_text("Pribudne v ďalšej verzii aplikácie").count() == 0)
        skontroluj("rozhranie bez chýb JavaScriptu", not chyby_js, chyby_js)
        b.close()


def main() -> int:
    global BASE
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:  # noqa: BLE001
        pass
    if not (FF and FP and YT and EXE.is_file()):
        print("Chýba FFmpeg, ffprobe, yt-dlp alebo preložené jadro (cargo build --release).")
        return 1
    priprav()
    srv = Tichy(("127.0.0.1", 0), Server)
    BASE = f"http://127.0.0.1:{srv.server_address[1]}"
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    out = D / "vystup"
    cfg = D / "cfg"
    cfg.mkdir()
    (cfg / "nastavenia.json").write_text(json.dumps({"tema": "les", "vlastny": "z Pythonu"}), encoding="utf-8")
    o = Okno(cfg)
    try:
        zakladne(o, out)
        front(o, out)
        ucty(o, out)
        archivy(o, out)
        rozhranie(o, out)
        mazanie(o, out)
    finally:
        o.zatvor()
    try:
        ytdlp_bez_ffmpeg(out)
        ytdlp_plny(out)
        strazca(out)
        kurzy(out)
        if sys.platform.startswith("linux"):
            doinstalovanie()
            priecinky()
            jazyky(out)
        restart(out)
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
