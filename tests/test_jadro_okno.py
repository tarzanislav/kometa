"""Skúška služby jadra v Ruste cez `kometa okno --bez-okna` – rovnaké volania, aké posiela rozhranie.

Overí: ochranu servera (kľúč, Host), nastavenia (súbor z Kometa pre Python ostane platný), front (súbor, HLS,
pauza a pokračovanie s rovnakým výsledkom, odstránenie s upratovaním, ukončenie a pokračovanie po
reštarte), skener a stránku s viacerými videami, yt-dlp (vlastnou cestou bez FFmpeg aj samotným
yt-dlp), doinštalovanie yt-dlp s kontrolou SHA-256, Webshare a Fastshare na simulovaných serveroch
a – ak je nainštalovaný Playwright – aj rozhranie v Chromiu.

    python tests/test_jadro_okno.py          (po `cargo build --release` v koreni repozitára)
"""
import hashlib
import http.server
import json
import re
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

EXE = KOREN / "target" / "release" / ("kometa.exe" if os.name == "nt" else "kometa")
FF, FP, YT = shutil.which("ffmpeg"), shutil.which("ffprobe"), shutil.which("yt-dlp")

# referenčný výpočet hesla Webshare (MD5-crypt, ako `openssl passwd -1`)
_ITOA64 = "./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def md5_crypt(password: str, salt: str) -> str:
    """Klasický algoritmus MD5-crypt (rovnaký ako `openssl passwd -1`)."""
    pw = password.encode("utf-8")
    salt = salt[:8]
    sb = salt.encode("ascii")
    magic = b"$1$"

    alt = hashlib.md5(pw + sb + pw).digest()
    ctx = hashlib.md5(pw + magic + sb)
    n = len(pw)
    while n > 0:
        ctx.update(alt[:min(16, n)])
        n -= 16
    n = len(pw)
    while n:
        ctx.update(b"\x00" if n & 1 else pw[:1])
        n >>= 1
    final = ctx.digest()

    for i in range(1000):
        c = hashlib.md5()
        c.update(pw if i & 1 else final)
        if i % 3:
            c.update(sb)
        if i % 7:
            c.update(pw)
        c.update(final if i & 1 else pw)
        final = c.digest()

    def to64(v: int, count: int) -> str:
        out = ""
        for _ in range(count):
            out += _ITOA64[v & 0x3F]
            v >>= 6
        return out

    f = final
    encoded = (to64((f[0] << 16) | (f[6] << 8) | f[12], 4) + to64((f[1] << 16) | (f[7] << 8) | f[13], 4)
               + to64((f[2] << 16) | (f[8] << 8) | f[14], 4) + to64((f[3] << 16) | (f[9] << 8) | f[15], 4)
               + to64((f[4] << 16) | (f[10] << 8) | f[5], 4) + to64(f[11], 2))
    return f"$1${salt}${encoded}"

D = Path(tempfile.mkdtemp(prefix="kometa_okno_"))
CHYBY: list[str] = []
BASE = ""
VELKY = bytes((i * 7 + (i >> 9)) & 0xFF for i in range(24 << 20))
FALOSNY_YTDLP = b"#!/bin/sh\necho 2099.01.01\n" + b"#" * 4096
FALOSNE_DENO = b"#!/bin/sh\necho deno 2.5.6\n"
# vydania pomocníkov na „GitHube“: verzie a Deno v ZIP (súčet vo formáte PowerShellu)
GH = {"ytdlp": "2099.01.01", "deno": "v2.5.6"}
LMS_STRANKA = """<html><head><title>Online kurz AI Grafika | Akadémia</title></head><body>
<header><nav class="main-menu"><a href="/">Home</a><a href="/moje-bonusy">Moje bonusy</a>
<a href="/kategoria/tituly/">Online Profesijné Tituly archívy</a><a href="/kategoria/administracia/">Administrácia IT archívy</a></nav></header>
<main><h1>Online kurz AI Grafika, Dizajn a Multimédia</h1>
<iframe src="https://player.vimeo.com/video/1"></iframe>
<a class="tl" href="/portal/lms/ai-v-adobe">Pokračovať na ďalšiu časť</a>
<div class="taby"><a href="/portal/lms/prehlad">Prehľad</a><a href="/portal/lms/materialy">Materiály</a><a href="/portal/lms/ulohy">Domáce úlohy</a></div>
<h2>Materiály</h2><p><a href="/kurz/material/skratky.pdf">Klávesové skratky (PDF)</a></p></main>
<aside class="osnova">
<section><h3>I. Základy AI grafiky</h3><ul>
<li><a href="/portal/lms/ako-funguje">Ako funguje AI grafika?</a> <span>23:10</span></li>
<li><a href="/portal/lms/ai-v-adobe">AI grafika v Adobe</a> <span>20 minút</span></li>
<li><a href="/portal/lms/firefly">Ako používať Adobe Firefly</a> <span>02:14:15</span></li></ul></section>
<section><h3>II. AI nástroje ChatGPT a DALL·E</h3><ul>
<li><a href="/portal/lms/zorientovat">Ako sa zorientovať</a> <span>01:03:14</span></li>
<li><a href="/portal/lms/dalle">Obrázky v DALL·E</a> <span>35:57</span></li></ul></section></aside>
<footer><a href="/o-nas">O nás</a><a href="/kontakt">Kontakt</a><a href="/subory/vop.pdf">Obchodné podmienky</a></footer>
</body></html>"""
PRIHLASENIE_STRANKA = """<html><body><form action="/portal/hotovo" method="post">
<input id="hladat" type="search" name="q"><label>E-mail <input id="email" type="text" name="email" autocomplete="username"></label>
<label>Heslo <input id="pass" type="password" name="password"></label><button>Prihlásiť</button></form></body></html>"""
SPA_STRANKA = """<html><head><title>Kurz – priebeh | Akadémia</title></head><body>
<header><nav class="menu"><a href="/">Home</a><a href="/moje-kurzy">Moje kurzy</a></nav></header>
<main><h1>Online kurz AI Nástroje Umelej Inteligencie v Praxi</h1><div id="obsah" class="lesson-content"></div></main>
<aside class="sidebar">
<h3>I. Prehľad AI nástrojov a ich praktické využitie</h3>
<table><tr class="riadok" data-l="0"><td><i class="icon-video"></i> Ako používať AI nástroje Whisper AI a Claude AI?</td><td>26:40</td><td>Videné</td></tr>
<tr class="riadok" data-l="1"><td><i class="icon-text"></i> Ktoré chatovacie AI nástroje patria medzi najpoužívanejšie</td><td>20<br>minút</td></tr></table>
<h3>II. Google Gemini AI, používateľské rozhranie a pokročilé funkcie</h3>
<table><tr class="riadok" data-l="2"><td><i class="icon-video"></i> Ako začať používať Google Gemini AI?</td><td>44:11</td></tr>
<tr class="riadok" data-l="3"><td><i class="icon-test"></i> Priebežný test</td><td>20 minút</td></tr></table>
</aside>
<footer><a href="/o-nas">O nás</a></footer>
<script>
const lekcie = [
  '<iframe src="https://player.vimeo.com/video/111?h=aa" width="640" height="360"></iframe><p>Úvod do nástrojov.</p>',
  '<h2>Chatovacie nástroje</h2><p>ChatGPT, Claude a Gemini – porovnanie.</p><p><a href="/kurz/material/skratky.pdf">Prehľad nástrojov (PDF)</a></p>',
  '<iframe src="https://player.vimeo.com/video/222?h=bb" width="640" height="360"></iframe>',
  '<h2>Priebežný test</h2><form><p>Otázka 1: Čo je prompt?</p><label><input type="radio" name="q1"> Pokyn pre AI</label></form>',
];
const ukaz = (i) => {
  document.getElementById('obsah').innerHTML = lekcie[i];
  document.querySelectorAll('[data-l]').forEach((r) => r.classList.toggle('active', +r.dataset.l === i));
};
document.querySelectorAll('[data-l]').forEach((r) => r.addEventListener('click', () => setTimeout(() => ukaz(+r.dataset.l), 300)));
ukaz(0);
</script></body></html>"""
MATERIAL_PDF = b"%PDF-1.4\n% material kurzu\n" + bytes(range(256)) * 40


def deno_zip() -> bytes:
    import io
    import zipfile
    b = io.BytesIO()
    with zipfile.ZipFile(b, "w") as z:
        i = zipfile.ZipInfo("deno")
        i.external_attr = 0o755 << 16
        z.writestr(i, FALOSNE_DENO)
    return b.getvalue()
WS_SUBOR = os.urandom(3 << 20)
# súbor z Datoidu bez názvu so službou: začiatok MKV, aby sa prípona dala určiť z obsahu
DT_MKV = bytes([0x1A, 0x45, 0xDF, 0xA3, 0x9F, 0x42, 0x82, 0x88]) + b"matroska" + os.urandom(200_000)
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
        if u.path.startswith("/aktualizacia/"):
            meno = u.path.rsplit("/", 1)[1]
            if meno == "latest":
                return self.posli(200, (D / "akt" / "latest.json").read_text(encoding="utf-8"), "application/json")
            f = D / "akt" / meno
            return self.posli(200, f.read_bytes(), "application/octet-stream") if f.is_file() else self.posli(404, "")
        if u.path == "/gh/ytdlp-latest":
            return self.posli(200, json.dumps({"tag_name": GH["ytdlp"]}), "application/json")
        if u.path == "/gh/deno-latest":
            return self.posli(200, json.dumps({"tag_name": GH["deno"]}), "application/json")
        if u.path == "/deno/deno-x86_64-unknown-linux-gnu.zip":
            return self.posli(200, deno_zip(), "application/zip")
        if u.path == "/deno/deno-x86_64-unknown-linux-gnu.zip.sha256sum":
            h = hashlib.sha256(deno_zip()).hexdigest().upper()
            return self.posli(200, f"\r\nAlgorithm : SHA256\r\nHash      : {h}\r\nPath      : C:\\a\\deno.zip\r\n")
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
            bezmena = "bezmena" in q.get("url", "")
            if m == "get-file-details":
                if bezmena:  # služba názov nepovie
                    return j({"filesize_bytes": len(DT_MKV)})
                return j({"filename": "Datoid súbor.mkv" if not zamknuty else "Datoid zamknutý.mkv",
                          "filesize_bytes": len(WS_SUBOR)})
            if m == "get-download-link":
                if q.get("token") != "dt-tok":
                    return j({"error": "Bad token"})
                if bezmena:
                    return j({"download_link": f"{BASE}/datoid-subor/stiahni".replace("/", "\\/")})
                return j({"download_link": f"{BASE}/ws/subor.bin".replace("/", "\\/")})
        if u.path == "/datoid-subor/stiahni":
            return self.rozsah(DT_MKV)
        if u.path == "/media/Archiv.rar":
            return self.posli(200, (KOREN / "jadro/testdata/zaheslovany.rar").read_bytes(), "application/octet-stream")
        if u.path.startswith("/archivy/"):
            return self.posli(200, ARCHIVY[u.path.rsplit("/", 1)[1]], "application/octet-stream")
        if u.path == "/oembed":
            return self.posli(200, json.dumps({"title": "Pieseň o kométe", "author_name": "Kanál"}), "application/json")
        if u.path == "/portal/spa/course-progress":
            return self.posli(200, SPA_STRANKA)
        if u.path.startswith("/portal/lms/"):
            return self.posli(200, LMS_STRANKA)
        if u.path == "/portal/prihlasenie":
            return self.posli(200, PRIHLASENIE_STRANKA)
        if u.path.startswith("/kurz/"):
            prihlaseny = "sid=tajne" in (self.headers.get("Cookie") or "")
            if not prihlaseny:
                return self.posli(403, "<p>Prihlás sa</p>")
            if u.path == "/kurz/material/skratky.pdf":
                return self.posli(200, MATERIAL_PDF, "application/pdf")
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
        self.env = env
        zaklad = dict(KOMETA_PRIECINOK=str(priecinok), KOMETA_BEZ_TREZORU="1", NO_PROXY="127.0.0.1,localhost",
                      KOMETA_POMOCNICI="nie", KOMETA_YTDLP_API=BASE + "/gh/ytdlp-latest",
                      KOMETA_DENO_API=BASE + "/gh/deno-latest", KOMETA_DENO_ZAKLAD=BASE + "/deno",
                      KOMETA_WEBSHARE_ZAKLAD=BASE + "/ws/api", KOMETA_FASTSHARE_ZAKLAD=BASE + "/fastshare",
                      KOMETA_DATOID_ZAKLAD=BASE + "/datoid")
        e = dict(os.environ, **dict(zaklad, **env))
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
    d3, d4 = "https://datoid.cz/def456/film-bezmena-1080p-mkv", "https://datoid.cz/ghi789/bezmena-bez-pripony"
    o.data("pridat", d3 + "\n" + d4, "video")
    s = o.cakaj(lambda s: all(polozka(s, x)["stav"] in ("hotovo", "chyba") for x in (d3, d4)), 30)
    skontroluj("Datoid bez názvu: prípona z odkazu („…-1080p-mkv“ → „….mkv“), nie .bin",
               polozka(s, d3)["subor"] == "film-bezmena-1080p.mkv" and (out / "film-bezmena-1080p.mkv").read_bytes() == DT_MKV,
               polozka(s, d3))
    skontroluj("Datoid bez názvu aj prípony: prípona podľa obsahu súboru (MKV)",
               polozka(s, d4)["subor"] == "bezmena-bez-pripony.mkv" and polozka(s, d4)["nazov"] == "bezmena-bez-pripony.mkv"
               and not (out / "bezmena-bez-pripony.bin").exists(), polozka(s, d4))


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
    skontroluj("RAR s heslom: vypýta si heslo (heslá iných súborov vo fronte nesedia)",
               polozka(s, ur)["rozbalenie"]["stav"] == "heslo", polozka(s, ur)["rozbalenie"])
    ur2 = ur + "?druhy"
    o.data("pridat", ur2, "video")
    s = o.cakaj(lambda s: polozka(s, ur2)["stav"] == "hotovo"
                and (polozka(s, ur2).get("rozbalenie") or {}).get("stav") in ("hotovo", "heslo", "chyba"), 30)
    n = o.data("nastavit_heslo", [polozka(s, ur)["id"], polozka(s, ur2)["id"]], "unrar")
    s = o.cakaj(lambda s: all(polozka(s, x)["rozbalenie"]["stav"] in ("hotovo", "chyba") for x in (ur, ur2)), 30)
    r, r2 = polozka(s, ur)["rozbalenie"], polozka(s, ur2)["rozbalenie"]
    skontroluj("jedno heslo pre viac archívov naraz: oba rozbalené bez nového sťahovania", n == 2
               and r["stav"] == r2["stav"] == "hotovo" and r["ciel"] != r2["ciel"]
               and (out / r["ciel"]).read_text() == (out / r2["ciel"]).read_text() == "target\nCargo.lock\n", (n, r, r2))
    ur3 = ur + "?treti"
    o.data("pridat", ur3, "video")
    s = o.cakaj(lambda s: polozka(s, ur3)["stav"] == "hotovo"
                and (polozka(s, ur3).get("rozbalenie") or {}).get("stav") in ("hotovo", "heslo", "chyba"), 30)
    r3 = polozka(s, ur3)["rozbalenie"]
    skontroluj("ďalší archív sa rozbalí sám heslom zadaným pri inom súbore", r3["stav"] == "hotovo"
               and "heslo z iného súboru" in r3["text"] and polozka(s, ur3)["s_heslom"], r3)
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
        # heslo v skopírovanom texte: priradí sa odkazom z tej istej kópie, archívy sa rozbalia samé
        ra, rb = BASE + "/media/Archiv.rar?schranka-a", BASE + "/media/Archiv.rar?schranka-b"
        schr.write_text(f"Archív z fóra:\n{ra}\n{rb}\nHeslo: unrar\n", encoding="utf-8")
        s = o.cakaj(lambda s: len([z for z in s["zachytene"] if "schranka-" in z["url"]]) == 2, 15)
        zz = [z for z in s["zachytene"] if "schranka-" in z["url"]]
        skontroluj("heslo zo skopírovaného textu: obom odkazom, jedna skupina",
                   all(z["heslo"] == "unrar" and z["heslo_zo_schranky"] for z in zz) and len({z["skupina"] for z in zz}) == 1,
                   zz)
        o.data("schranka_heslo", [zz[0]["id"]], "zlé")
        n = o.data("schranka_heslo", [z["id"] for z in zz], "unrar")
        zz = [z for z in o.data("stav")["zachytene"] if "schranka-" in z["url"]]
        skontroluj("jedno heslo pre označené odkazy v schránke", n == 2
                   and all(z["heslo"] == "unrar" and not z["heslo_zo_schranky"] for z in zz), zz)
        o.data("schranka_stiahnut", [z["id"] for z in zz], "video")
        s = o.cakaj(lambda s: all((polozka(s, u) or {}).get("rozbalenie", {}) and polozka(s, u)["rozbalenie"]["stav"]
                                  in ("hotovo", "heslo", "chyba") for u in (ra, rb)), 30)
        skontroluj("zo schránky s heslom: stiahnuté a rozbalené bez pýtania",
                   all(polozka(s, u)["rozbalenie"]["stav"] == "hotovo" and polozka(s, u)["s_heslom"] for u in (ra, rb)),
                   [polozka(s, u) for u in (ra, rb)])
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


def strazca_okno():
    """Okno sa po zachytení prepne na kartu Zo schránky – no nie, keď človek práve píše v Komete."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("Strážca v okne: Playwright nie je nainštalovaný – preskakujem")
        return
    print("Strážca schránky v okne")
    schr = D / "schranka-okno.txt"
    schr.write_text("nič", encoding="utf-8")
    o = Okno(D / "cfg-strazca-okno", KOMETA_SKUSKA_SCHRANKA=str(schr))
    try:
        o.data("ulozit_nastavenia", {"strazca": True})
        with sync_playwright() as p:
            b = p.chromium.launch()
            s = b.new_page(viewport={"width": 1280, "height": 860})
            s.goto(o.url)
            s.get_by_role("heading", name="Sťahovanie").wait_for(timeout=15000)
            time.sleep(1.5)
            # píše do poľa na odkazy → okno ostane, odkaz sa len zachytí
            s.locator("textarea").first.click()
            schr.write_text("https://webshare.cz/#/file/okno1/Diel.1.rar", encoding="utf-8")
            o.cakaj(lambda st: len(st["zachytene"]) == 1, 10)
            time.sleep(1.5)
            skontroluj("pri písaní v Komete sa okno neprepne", s.get_by_role("heading", name="Sťahovanie").count() == 1)
            s.locator("body").click(position={"x": 5, "y": 5})
            s.evaluate("document.activeElement && document.activeElement.blur()")
            schr.write_text("https://webshare.cz/#/file/okno2/Diel.2.rar\nHeslo: x", encoding="utf-8")
            s.get_by_role("heading", name="Zo schránky").wait_for(timeout=10000)
            nova = s.locator("li.nova")
            skontroluj("po zachytení sa okno prepne na Zo schránky a nový odkaz zvýrazní",
                       nova.count() == 1 and "Diel.2.rar" in nova.first.inner_text(), nova.count())
            s.get_by_role("switch", name="Ukázať zachytené odkazy").click()
            o.cakaj(lambda st: o.data("init")["nastavenia"].get("strazca_ukazat") is False, 10)
            s.get_by_role("button", name="Sťahovanie").first.click()
            schr.write_text("https://webshare.cz/#/file/okno3/Diel.3.rar", encoding="utf-8")
            o.cakaj(lambda st: len(st["zachytene"]) == 3, 10)
            time.sleep(1.5)
            skontroluj("vypnuté „Ukázať zachytené odkazy“: okno ostane", s.get_by_role("heading", name="Sťahovanie").count() == 1)
            b.close()
    finally:
        o.zatvor()


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
        kurzy_zalozky_a_osnova(o, pr, out)
    finally:
        o.zatvor()
    o2 = Okno(D / "cfg-kurz2")
    try:
        skontroluj("bez aplikácie (okno v prehliadači) kurzy nie sú", o2.data("init")["funkcie"]["kurzy"] is False
                   and not o2.volaj("kurz_otvorit", "https://example.com")["ok"])
    finally:
        o2.zatvor()


def kurzy_zalozky_a_osnova(o: Okno, pr: Path, out: Path):
    """Záložky stránok s kurzami, prihlásenie v trezore a jeho vyplnenie; osnova v ozajstnom prehliadači."""
    heslo = 'tajné "heslo"'
    z = o.data("kurz_zalozka_ulozit", {"id": "", "nazov": "", "adresa": BASE.replace("http://", "") + "/kurz/obsah",
                                       "meno": "ja@example.sk"}, heslo)
    skontroluj("záložka stránky s kurzami s uloženým prihlásením", len(z) == 1 and z[0]["nazov"] == "127.0.0.1"
               and z[0]["s_heslom"] and z[0]["meno"] == "ja@example.sk" and z[0]["host"] == "127.0.0.1", z)
    subor = (D / "cfg-kurz" / "kurzy.json").read_text(encoding="utf-8")
    skontroluj("heslo nie je v súbore so záložkami", "tajné" not in subor and "heslo" not in json.loads(subor)["zalozky"][0],
               subor)
    z = o.data("kurz_zalozka_ulozit", dict(z[0], nazov="Môj portál"), None)
    skontroluj("úprava záložky bez hesla heslo nechá", z[0]["nazov"] == "Môj portál" and z[0]["s_heslom"], z)
    (pr / "prihlasenie.json").write_text(json.dumps({"meno": True, "heslo": True}))
    o.data("kurz_otvorit", BASE + "/portal/prihlasenie")
    r = o.data("kurz_prihlasit")
    js = (pr / "skript.js").read_text(encoding="utf-8")
    skontroluj("vyplnenie prihlásenia pre otvorenú stránku", r["meno"] and r["heslo"] and "kometaPrihlasenie" in js
               and json.dumps("ja@example.sk") in js, r)
    try:
        from playwright.sync_api import sync_playwright
    except ImportError:
        print("Kurzy v prehliadači: Playwright nie je nainštalovaný – preskakujem")
        sync_playwright = None
    if sync_playwright:
        with sync_playwright() as p:
            b = p.chromium.launch()
            s = b.new_page()
            s.goto(BASE + "/portal/prihlasenie")
            r = s.evaluate(js)
            hodnoty = s.evaluate("['#email', '#pass', '#hladat'].map((x) => document.querySelector(x).value)")
            skontroluj("prihlasovací formulár: meno aj heslo vyplnené, nič iné a nič neodoslané",
                       r == {"meno": True, "heslo": True} and hodnoty == ["ja@example.sk", heslo, ""]
                       and s.url.endswith("/portal/prihlasenie"), (r, hodnoty))
            s.goto(BASE.replace("127.0.0.1", "localhost") + "/portal/prihlasenie")
            r2 = s.evaluate(js)
            skontroluj("na inej stránke sa prihlásenie nevyplní",
                       r2.get("chyba") == "ina-stranka" and s.evaluate("document.querySelector('#pass').value") == "", r2)
            # zberový skript na stránke lekcie v štýle LMS: osnova s dĺžkami, menu s inými kurzami
            o.data("kurz_nacitat")
            zber_js = (pr / "skript.js").read_text(encoding="utf-8")
            s.goto(BASE + "/portal/lms/ako-funguje")
            (pr / "zber.json").write_text(json.dumps(s.evaluate(zber_js)))
            b.close()
        k = o.data("kurz_nacitat")
        prehlad = [(x["nazov"], x.get("menu", False), [(l["nazov"], l["odporucana"], l.get("material", False))
                                                      for l in x["lekcie"]]) for x in k["kapitoly"]]
        skontroluj("osnova kurzu: lekcie s dĺžkami, materiál, menu a päta zvlášť a neoznačené", prehlad == [
            ("I. Základy AI grafiky", False, [("Ako funguje AI grafika?", True, False), ("AI grafika v Adobe", True, False),
                                              ("Ako používať Adobe Firefly", True, False)]),
            ("II. AI nástroje ChatGPT a DALL·E", False, [("Ako sa zorientovať", True, False), ("Obrázky v DALL·E", True, False)]),
            ("Materiály", False, [("Klávesové skratky (PDF)", True, True)]),
            ("Online kurz AI Grafika, Dizajn a Multimédia", False,
             [("Prehľad", False, False), ("Materiály", False, False), ("Domáce úlohy", False, False)]),
            ("Odkazy z menu a päty stránky", True,
             [("Moje bonusy", False, False), ("Online Profesijné Tituly archívy", False, False),
              ("Administrácia IT archívy", False, False), ("O nás", False, False), ("Obchodné podmienky", False, True)]),
        ] and [l["dlzka"] for x in k["kapitoly"][:2] for l in x["lekcie"]]
            == ["23:10", "20 min", "02:14:15", "01:03:14", "35:57"], prehlad)
        material = dict(k, kapitoly=[x for x in k["kapitoly"] if x["nazov"] == "Materiály"])
        o.data("kurz_stiahnut", material, "video")
        s = o.cakaj(lambda s: any("skratky.pdf" in x["url"] and x["stav"] in ("hotovo", "chyba") for x in s["polozky"]), 30)
        q = next(x for x in s["polozky"] if "skratky.pdf" in x["url"])
        f = next(iter(out.glob(f"*/{q.get('subor') or '-'}")), out / "-")
        skontroluj("materiál kurzu stiahnutý s prihlásením a so svojou príponou",
                   q["stav"] == "hotovo" and f.name == "01 Klávesové skratky (PDF).pdf" and f.is_file()
                   and f.read_bytes() == MATERIAL_PDF, q)
        prechod_osnovy(o, pr, out)
    z = o.data("kurz_zalozka_zmazat", z[0]["id"])
    z2 = o.data("kurz_zalozka_ulozit", {"id": "", "nazov": "", "adresa": BASE + "/kurz/obsah", "meno": "ja@example.sk"}, None)
    skontroluj("odstránená záložka zmaže aj heslo z trezoru", z == [] and not z2[0]["s_heslom"], (z, z2))
    o.data("kurz_zalozka_zmazat", z2[0]["id"])


def prechod_osnovy(o: Okno, pr: Path, out: Path):
    """Lekcie, ktoré sa prepínajú kliknutím na tej istej stránke: Kometa prejde osnovu v prehliadači."""
    import threading
    from playwright.sync_api import sync_playwright

    def na_pozadi(metoda, *args):
        vysledok = {}
        t = threading.Thread(target=lambda: vysledok.update(r=o.data(metoda, *args)), daemon=True)
        t.start()
        return t, vysledok

    def most(s, kym, limit=90):
        koniec = time.time() + limit
        while not kym() and time.time() < koniec:
            for f in sorted(pr.glob("poziadavka-*.js")):
                n = f.stem.split("-")[1]
                js = f.read_text(encoding="utf-8")
                f.unlink()
                try:
                    r = s.evaluate(js)
                except Exception as e:  # noqa: BLE001 – chyba stránky ide späť ako odpoveď
                    r = {"chyba": str(e)}
                (pr / f"odpoved-{n}.tmp").write_text(json.dumps(r), encoding="utf-8")
                (pr / f"odpoved-{n}.tmp").rename(pr / f"odpoved-{n}.json")
            time.sleep(0.03)

    adresa = BASE + "/portal/spa/course-progress"
    (pr / "adresa.txt").write_text(adresa)
    (pr / "most").write_text("1")
    try:
        with sync_playwright() as p:
            b = p.chromium.launch()
            s = b.new_page(viewport={"width": 1280, "height": 900})
            s.goto(adresa)
            t, r = na_pozadi("kurz_nacitat")
            most(s, lambda: not t.is_alive())
            k = r.get("r") or {}
            skontroluj("bežné načítanie spozná osnovu, ktorá nie je z odkazov (4 riadky s dĺžkou)",
                       k.get("riadkov_osnovy") == 4, k.get("riadkov_osnovy"))
            o.data("kurz_prejst", k.get("nazov", ""))
            most(s, lambda: not o.data("kurz_prechod")["stav"]["bezi"])
            st = o.data("kurz_prechod")
            k = st["kurz"] or {}
            prehlad = [(x["nazov"], [(l["nazov"], l["druh"], l["odporucana"]) for l in x["lekcie"]]) for x in k.get("kapitoly", [])]
            skontroluj("prechod osnovy: video, text s materiálom a test v kapitolách", prehlad == [
                ("I. Prehľad AI nástrojov a ich praktické využitie", [
                    ("Ako používať AI nástroje Whisper AI a Claude AI?", "video", True),
                    ("Ktoré chatovacie AI nástroje patria medzi najpoužívanejšie", "text", True),
                    ("Prehľad nástrojov (PDF)", "material", True)]),
                ("II. Google Gemini AI, používateľské rozhranie a pokročilé funkcie", [
                    ("Ako začať používať Google Gemini AI?", "video", True),
                    ("Priebežný test", "test", True)]),
            ] and st["stav"]["hotovo"] == 4 and not st["stav"]["chyba"], (prehlad, st["stav"]))
            l = [x for kap in k.get("kapitoly", []) for x in kap["lekcie"]]
            skontroluj("video lekcie: adresa prehrávača z otvorenej lekcie, dĺžky z osnovy",
                       [x["zdroj"] for x in l if x["druh"] == "video"]
                       == ["https://player.vimeo.com/video/111?h=aa", "https://player.vimeo.com/video/222?h=bb"]
                       and [x["dlzka"] for x in l if x["druh"] != "material"] == ["26:40", "20 minút", "44:11", "20 minút"], l)
            b.close()
        # texty, test a materiál (videá Vimea sa v skúške nesťahujú)
        bez_videi = dict(k, kapitoly=[dict(x, lekcie=[y for y in x["lekcie"] if y["druh"] != "video"]) for x in k["kapitoly"]])
        r = o.data("kurz_stiahnut", bez_videi, "video")
        koren = out / "Online kurz AI Nástroje Umelej Inteligencie v Praxi"
        text = koren / "01 I. Prehľad AI nástrojov a ich praktické využitie" / "01 Ktoré chatovacie AI nástroje patria medzi najpoužívanejšie.html"
        test = koren / "02 II. Google Gemini AI, používateľské rozhranie a pokročilé funkcie" / "01 Priebežný test.html"
        skontroluj("text a test lekcie uložené ako stránky HTML v priečinkoch kapitol", r["texty"] == 2 and r["pridane"] == 1
                   and "ChatGPT, Claude a Gemini" in text.read_text(encoding="utf-8")
                   and "Otázka 1" in test.read_text(encoding="utf-8") and "<script" not in text.read_text(encoding="utf-8"),
                   (r, sorted(str(x.relative_to(out)) for x in koren.rglob("*"))) if koren.exists() else r)
        meno = "02 Prehľad nástrojov (PDF).pdf"
        s2 = o.cakaj(lambda s: any(x["nazov"] == meno and x["stav"] in ("hotovo", "chyba") for x in s["polozky"]), 30)
        mat = next((x for x in s2["polozky"] if x["nazov"] == meno), {})
        skontroluj("materiál z textovej lekcie stiahnutý s prihlásením", mat["stav"] == "hotovo"
                   and (koren / "01 I. Prehľad AI nástrojov a ich praktické využitie" / "02 Prehľad nástrojov (PDF).pdf").read_bytes() == MATERIAL_PDF, mat)
    finally:
        (pr / "most").unlink(missing_ok=True)


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
                   and (n / "ffprobe").exists()
                   and sorted(x.name for x in n.iterdir()) == ["ffmpeg", "ffprobe", "yt-dlp", "yt-dlp.verzia"],
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


def sonda(p: Path) -> dict:
    """Celý prehľad zo ffprobe: stopy (aj príznaky), kapitoly a údaje o súbore."""
    r = subprocess.run([FP, "-v", "error", "-show_streams", "-show_chapters", "-show_format", "-of", "json", str(p)],
                       capture_output=True, text=True)
    return json.loads(r.stdout or "{}")


def doplnky(p: Path) -> tuple:
    """(kapitoly, údaje o súbore malými písmenami, obrázok vložený, roly zvukov [(jazyk, dabing, originál)])."""
    j = sonda(p)
    kap = [(round(float(c["start_time"]), 2), c.get("tags", {}).get("title")) for c in j.get("chapters", [])]
    tagy = {k.lower(): v for k, v in j.get("format", {}).get("tags", {}).items()}
    obal = any(x.get("disposition", {}).get("attached_pic") or x.get("tags", {}).get("filename", "").startswith("cover.")
               for x in j.get("streams", []))
    roly = [(x.get("tags", {}).get("language"), x["disposition"].get("dub", 0), x["disposition"].get("original", 0))
            for x in j.get("streams", []) if x["codec_type"] == "audio"]
    return kap, tagy, obal, roly


def prud(p: Path):
    """[(typ, kodek, jazyk, názov, predvolená)] zo ffprobe (bez vloženého obrázka videa)."""
    r = subprocess.run([FP, "-v", "error", "-show_entries",
                        "stream=codec_type,codec_name:stream_tags=language,title,handler_name,filename"
                        ":stream_disposition=default,attached_pic",
                        "-of", "json", str(p)], capture_output=True, text=True)
    out = []
    for x in json.loads(r.stdout or "{}").get("streams", []):
        t = x.get("tags", {})
        if (x.get("disposition", {}).get("attached_pic") or t.get("filename", "").startswith("cover.")
                or x["codec_type"] == "data"):   # obrázok a kapitolová stopa MP4 sa kontrolujú zvlášť
            continue
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
    ff("-f", "lavfi", "-i", "color=c=orange:size=160x90", "-frames:v", "1", "obal.jpg")
    (m / "obal.webp").write_bytes(b"RIFF\x00\x00\x00\x00WEBPVP8 ")
    ff("-f", "lavfi", "-i", "testsrc2=size=320x240:rate=25", "-f", "lavfi", "-i", "sine=frequency=900", "-t", "3",
       "-c:v", "libx264", "-bf", "2", "-pix_fmt", "yuv420p", "-c:a", "aac", "-metadata:s:a:0", "language=eng",
       "-metadata:s:a:0", "title=Pôvodný zvuk", str(D / "moje.mkv"))
    u = lambda n: f"{BASE}/media/yt/{n}"  # noqa: E731
    zvuk = lambda i, jazyk, kodek, n, poznamka, pref: {  # noqa: E731
        "format_id": i, "ext": n.rsplit(".", 1)[1], "vcodec": "none", "acodec": kodek, "abr": 64, "language": jazyk,
        "language_preference": pref, "format_note": poznamka, "protocol": "http", "url": u(n)}
    info = {"id": "dQw4w9WgXcQ", "title": "Jazyková skúška", "uploader": "Kanál", "duration": 3,
            "upload_date": "20240517", "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "description": "Popis skúšky", "thumbnail": u("obal.webp"),
            "thumbnails": [{"url": u("obal.webp"), "preference": 0}, {"url": u("obal.jpg"), "preference": -1}],
            "chapters": [{"start_time": 0.0, "end_time": 1.5, "title": "Začiatok"},
                         {"start_time": 1.5, "end_time": 3.0, "title": "Koniec"}], "formats": [
        {"format_id": "248", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 240, "tbr": 300,
         "protocol": "http", "url": u("v.webm")},
        {"format_id": "137", "ext": "mp4", "vcodec": "avc1.4d400d", "acodec": "none", "height": 240, "tbr": 200,
         "protocol": "http", "url": u("v.mp4")},
        zvuk("140-0", "en", "mp4a.40.2", "en.m4a", "English original (default), medium", 10),
        zvuk("251-0", "en", "opus", "en.webm", "English original (default), medium", 10),
        zvuk("140-1", "cs", "mp4a.40.2", "cs.m4a", "Czech, medium", -1),
        zvuk("251-1", "cs", "opus", "cs.webm", "Czech, medium", -1),
        zvuk("251-2", "sk", "opus", "sk.webm", "Slovak, medium", -1)]}
    # titulky: CZ od autora (json3), SK od autora (len WebVTT), EN len automatické (rozpoznaná reč)
    (m / "cs.json3").write_text(json.dumps({"events": [
        {"tStartMs": 0, "dDurationMs": 3000, "wWinId": 1},
        {"tStartMs": 500, "dDurationMs": 1000, "segs": [{"utf8": "Ahoj, "}, {"utf8": "svet"}]},
        {"tStartMs": 1600, "dDurationMs": 1200, "segs": [{"utf8": "Druhý titulok"}]}]}), encoding="utf-8")
    (m / "sk.vtt").write_text("WEBVTT\n\n00:00:00.500 --> 00:00:01.500\nAhoj, <i>svet</i>\n\n"
                              "00:00:01.600 --> 00:00:02.800\nDruhý titulok\n", encoding="utf-8")
    (m / "en.json3").write_text(json.dumps({"events": [
        {"tStartMs": 400, "dDurationMs": 2000, "segs": [{"utf8": "hello"}, {"utf8": " world"}]}]}), encoding="utf-8")
    info["subtitles"] = {"cs": [{"ext": "json3", "url": u("cs.json3")}, {"ext": "vtt", "url": u("chyba.vtt")}],
                         "sk": [{"ext": "vtt", "url": u("sk.vtt")}], "live_chat": [{"ext": "json", "url": u("x")}]}
    info["automatic_captions"] = {"en-orig": [{"ext": "json3", "url": u("en.json3")}],
                                  "de": [{"ext": "json3", "url": u("en.json3")}]}
    (D / "info.json").write_text(json.dumps(info), encoding="utf-8")
    nastroj = D / "falosny-yt-dlp"
    nastroj.write_text(f"#!/bin/sh\ncat '{D / 'info.json'}'\n", encoding="utf-8")
    nastroj.chmod(0o755)
    return nastroj, "https://www.youtube.com/watch?v=dQw4w9WgXcQ"


def jazyky_rozhranie(o: Okno, yt):
    """Karta Jazykové stopy: viac odkazov naraz a kanál; okno playlistu s prepínačom jazykov."""
    from playwright.sync_api import sync_playwright
    pl = {"_type": "playlist", "title": "Kanál o histórii", "uploader": "Kanál", "entries": [
        {"id": c * 11, "url": yt(c), "title": f"Diel {i + 1}", "duration": 600, "ie_key": "Youtube"}
        for i, c in enumerate("fghijklmnopq")]}
    (D / "pl.json").write_text(json.dumps(pl), encoding="utf-8")
    nastroj = Path(o.env["KOMETA_YTDLP"])
    povodny = nastroj.read_text(encoding="utf-8")
    nastroj.write_text(f"#!/bin/sh\ncase \"$*\" in *--flat-playlist*) cat '{D / 'pl.json'}';; *) cat '{D / 'info.json'}';; esac\n",
                       encoding="utf-8")
    pred = {x["url"] for x in o.data("stav")["polozky"]}
    try:
        with sync_playwright() as p:
            b = p.chromium.launch()
            s = b.new_page(viewport={"width": 1280, "height": 1000})
            s.route("**/i.ytimg.com/**", lambda r: r.fulfill(status=204, body=""))
            s.goto(o.url)
            s.get_by_role("button", name="Jazykové stopy").first.click()
            s.locator("#yt-url").fill(f"{yt('r')}\n{yt('s')}\n{yt('t')}")
            s.get_by_role("button", name="Načítať jazyky").click()
            s.get_by_text("Jazykové stopy pre 3 videá").wait_for(timeout=20000)
            s.get_by_text("Jazyky vo videách").wait_for(timeout=30000)
            skontroluj("viac odkazov: každé video ukáže svoje jazyky", s.locator(".zoznam .video").count() == 3
                       and s.locator(".zoznam .jazyk").count() == 9)
            s.get_by_role("checkbox", name="Stiahnuť Jazyková skúška").nth(2).uncheck()
            s.get_by_role("button", name="Stiahnuť 2 videá").click()
            s.get_by_role("heading", name="Sťahovanie", exact=True).wait_for(timeout=10000)
            s.wait_for_timeout(500)
            st = o.data("stav")
            nove = [x for x in st["polozky"] if x["url"] not in pred]
            skontroluj("viac odkazov: 2 vybrané videá s jazykmi do frontu",
                       sorted(x["url"] for x in nove) == [yt("r"), yt("s")] and all(x["format"] == "jazyky" for x in nove),
                       [(x["url"], x["format"]) for x in nove])
            # kanál v karte Jazykové stopy → výber videí s jazykmi
            s.get_by_role("button", name="Jazykové stopy").first.click()
            s.locator("#yt-url").fill("https://www.youtube.com/@historia")
            s.get_by_role("button", name="Načítať jazyky").click()
            s.get_by_text("Kanál o histórii").first.wait_for(timeout=20000)
            prep = s.get_by_role("switch", name="S jazykovými stopami")
            skontroluj("kanál z karty Jazykové stopy: okno s výberom videí a zapnutými jazykmi",
                       prep.get_attribute("aria-checked") == "true")
            s.get_by_text("podľa 10 z 12 videí", exact=False).wait_for(timeout=30000)
            skontroluj("kanál: jazyky zistené zo vzorky videí", s.locator(".vyskyt").first.inner_text() == "10/10")
            s.get_by_role("button", name="Žiadne").click()
            s.get_by_role("button", name="Najnovších").click()
            s.get_by_role("button", name="Stiahnuť 10 videí").click()
            s.wait_for_timeout(1500)
            st = o.data("stav")
            z_kanala = [x for x in st["polozky"] if x["url"] in {e["url"] for e in pl["entries"]}]
            skontroluj("kanál: 10 najnovších videí s jazykmi do frontu", len(z_kanala) == 10
                       and all(x["format"] == "jazyky" for x in z_kanala), len(z_kanala))
            b.close()
    finally:
        nastroj.write_text(povodny, encoding="utf-8")
    o.cakaj(lambda s: all(x["stav"] in ("hotovo", "chyba") for x in s["polozky"]), 240)


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
        kap, tagy, obal, roly = doplnky(f)
        skontroluj("MKV: kapitoly, údaje o videu, obrázok (JPEG, nie WebP) a roly (dabing, originál)",
                   kap == [(0.0, "Začiatok"), (1.5, "Koniec")] and tagy.get("artist") == "Kanál"
                   and tagy.get("date_released") == "2024-05-17" and tagy.get("url", "").endswith("dQw4w9WgXcQ")
                   and obal and roly == [("cze", 1, 0), ("cze", 1, 0), ("slo", 1, 0), ("slo", 1, 0),
                                         ("eng", 0, 1), ("eng", 0, 1)]
                   and any("Do súboru: údaje o videu, kapitoly (2), obrázok" in r for r in s["log"]),
                   (kap, tagy, obal, roly))
        skontroluj("načítanie: titulky od autora a automatické", n["titulky"] == ["cs", "sk"] and n["auto_titulky"] is True, n)
        s, f = uloha(o, {"poradie": ["cs", "sk"], "rezim": "video", "original": True, "zvuk": "aac",
                         "kontajner": "mkv", "titulky": "jazyky"})
        p = prud(f) if f else []
        tit = [x for x in p if x[0] == "subtitle"]
        skontroluj("titulky ku každému jazyku zvuku, vložené do MKV (bez automatických len od autora)",
                   s["text"] == "Hotovo" and tit == [("subtitle", "subrip", "cze", "Čeština", 0),
                                                     ("subtitle", "subrip", "slo", "Slovenčina", 0)]
                   and any("Titulky nie sú k dispozícii: EN (automatické sú vypnuté)" in r for r in s["log"]), (s, p))
        r = subprocess.run([FF, "-v", "error", "-i", str(f), "-map", "0:s:1", "-f", "srt", "-"], capture_output=True, text=True)
        # časy sa posunú spolu s obrazom (spoločný začiatok všetkých stôp – Opus začína o pár ms skôr)
        skontroluj("obsah titulkov (WebVTT → SRT, bez značiek a s časmi)",
                   re.search(r"00:00:00,5[0-4]\d --> 00:00:01,5[0-4]\d\nAhoj, svet\n", r.stdout) is not None
                   and "Druhý titulok" in r.stdout, r.stdout + r.stderr)
        s, f = uloha(o, {"poradie": ["cs"], "rezim": "video", "original": True, "zvuk": "aac", "kontajner": "mkv",
                         "titulky": "jazyky", "titulky_auto": True})
        tit = [x for x in (prud(f) if f else []) if x[0] == "subtitle"]
        skontroluj("automatické titulky k originálu (rozpoznaná reč)", tit == [
            ("subtitle", "subrip", "cze", "Čeština", 0), ("subtitle", "subrip", "eng", "Angličtina (automatické)", 0)], tit)
        s, f = uloha(o, {"poradie": ["cs", "sk"], "rezim": "video", "original": False, "zvuk": "aac",
                         "kontajner": "mp4", "titulky": "jazyky"})
        srt = sorted(x.name for x in f.parent.iterdir() if x.name.startswith(f.stem + ".") and x.suffix == ".srt") if f else []
        tit = [x for x in (prud(f) if f else []) if x[0] == "subtitle"]
        r = subprocess.run([FF, "-v", "error", "-i", str(f), "-map", "0:s:0", "-f", "srt", "-"], capture_output=True,
                           text=True) if f else None
        skontroluj("MP4: titulky vložené do súboru (tx3g), nie vedľa videa",
                   s["text"] == "Hotovo" and srt == [] and tit == [("subtitle", "mov_text", "ces", "Čeština", 0),
                                                                   ("subtitle", "mov_text", "slk", "Slovenčina", 0)]
                   and r is not None and "Ahoj, svet" in r.stdout and "Druhý titulok" in r.stdout, (s, srt, tit))
        kap, tagy, obal, roly = doplnky(f)
        skontroluj("MP4: kapitoly, údaje o videu, obrázok a roly (dabing)",
                   kap == [(0.0, "Začiatok"), (1.5, "Koniec")] and tagy.get("artist") == "Kanál"
                   and tagy.get("date") == "2024-05-17" and tagy.get("comment", "").endswith("dQw4w9WgXcQ")
                   and tagy.get("title") == "Jazyková skúška" and obal and roly == [("ces", 1, 0), ("slk", 1, 0)],
                   (kap, tagy, obal, roly))
        o.data("ulozit_nastavenia", {"vlozit_udaje": False, "vlozit_kapitoly": False, "vlozit_obal": False})
        s, f = uloha(o, {"poradie": ["cs"], "rezim": "audio", "zvuk": "aac", "kontajner": "mp4"})
        kap, tagy, obal, _ = doplnky(f) if f else ([1], {}, True, [])
        skontroluj("nastavenia: bez údajov, kapitol a obrázka", s["text"] == "Hotovo" and kap == [] and not obal
                   and "artist" not in tagy and tagy.get("title") == "Jazyková skúška", (kap, tagy, obal))
        o.data("ulozit_nastavenia", {"vlozit_udaje": True, "vlozit_kapitoly": True, "vlozit_obal": True})
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
        z = o.data("jazyky_zistit", [yt("a"), yt("b"), yt("c"), "https://vimeo.com/123"])
        skontroluj("zistenie jazykov vo viacerých videách (iné ako YouTube sa vynechá)",
                   z["preskumanych"] == 3 and [(j["kod"], j["pocet"], j["original"]) for j in z["jazyky"]]
                   == [("cs", 3, 0), ("sk", 3, 0), ("en", 3, 3)]
                   and [v["jazyky"] for v in z["videa"]] == [["en", "cs", "sk"]] * 3
                   and z["videa"][0]["nazov"] == "Jazyková skúška", z)
        jazyky_rozhranie(o, yt)
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
        # bežné sťahovanie bez FFmpeg: najlepší obraz (VP9) + AAC aj Opus → MKV; „MP4“ → H.264 + AAC
        hotova = lambda s, u: (polozka(s, u) or {}).get("stav") in ("hotovo", "chyba")  # noqa: E731
        o.data("ulozit_nastavenia", {"audio_mode": "oboje"})
        uv, um = "https://www.youtube.com/watch?v=bezffmpeg01", "https://www.youtube.com/watch?v=bezffmpeg02"
        o.data("pridat", uv, "video")
        o.data("pridat", um, "mp4")
        s = o.cakaj(lambda s: hotova(s, uv) and hotova(s, um), 120)
        a, b = polozka(s, uv), polozka(s, um)
        pa = prud(out / "jz2" / a["subor"]) if a.get("subor") else []
        pb = prud(out / "jz2" / b["subor"]) if b.get("subor") else []
        skontroluj("bez FFmpeg: najlepšia kvalita = VP9 + AAC + Opus v MKV",
                   a["stav"] == "hotovo" and a["subor"].endswith(".mkv")
                   and [(x[0], x[1], x[3]) for x in pa] == [("video", "vp9", ""), ("audio", "aac", "AAC"),
                                                             ("audio", "opus", "Opus")], (a, pa))
        skontroluj("bez FFmpeg: MP4 pre TV = H.264 + AAC", b["stav"] == "hotovo" and b["subor"].endswith(".mp4")
                   and [(x[0], x[1]) for x in pb] == [("video", "h264"), ("audio", "aac")], (b, pb))
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


def pomocnici():
    """Kometa si sama doinštaluje vlastný yt-dlp a Deno, aktualizuje ich a Deno dá yt-dlp-u."""
    print("Pomocníci: vlastný yt-dlp a Deno, aktualizácie")
    cfg = D / "cfg-pom"
    n = cfg / "nastroje"
    env = dict(PATH="/usr/bin:/bin", KOMETA_POMOCNICI="hned", KOMETA_YTDLP_ZAKLAD=BASE + "/vydania")
    GH.update(ytdlp="2099.01.01", deno="v2.5.6")
    o = Okno(cfg, **env)
    try:
        s = o.cakaj(lambda s: s["nastroje"]["ytdlp"] and s["nastroje"]["deno"] and not s["nastroje"]["instaluje"], 40)
        skontroluj("po štarte sám doinštaluje yt-dlp a Deno (overené, s verziou)",
                   (n / "yt-dlp").read_bytes() == FALOSNY_YTDLP and (n / "deno").read_bytes() == FALOSNE_DENO
                   and os.access(n / "deno", os.X_OK) and (n / "yt-dlp.verzia").read_text() == "2099.01.01"
                   and (n / "deno.verzia").read_text() == "2.5.6" and s["nastroje"]["deno_vlastne"]
                   and not [x for x in n.iterdir() if "rozbal" in x.name or "stahuje" in x.name], s["nastroje"])
    finally:
        o.zatvor()
    cas_deno = (n / "deno").stat().st_mtime_ns
    GH.update(ytdlp="2099.02.02", deno="v2.6.0")
    o = Okno(cfg, **env)
    try:
        o.cakaj(lambda s: (n / "yt-dlp.verzia").read_text() == "2099.02.02" and not s["nastroje"]["instaluje"], 40)
        time.sleep(1)
        skontroluj("nový yt-dlp sa stiahne sám; Deno len pri novej hlavnej verzii",
                   (n / "yt-dlp.verzia").read_text() == "2099.02.02" and (n / "deno").stat().st_mtime_ns == cas_deno)
    finally:
        o.zatvor()
    # yt-dlp dostane cestu k Deno
    zaznam = D / "ytdlp-args.txt"
    nastroj = D / "ytdlp-zaznam"
    nastroj.write_text(f"#!/bin/sh\necho \"$@\" >> {zaznam}\nexit 1\n")
    nastroj.chmod(0o755)
    o = Okno(D / "cfg-pom2", KOMETA_YTDLP=str(nastroj), KOMETA_DENO=str(n / "deno"))
    try:
        u = "https://www.youtube.com/watch?v=jsruntime01"
        o.data("pridat", u, "video")
        o.cakaj(lambda s: (polozka(s, u) or {}).get("stav") in ("hotovo", "chyba"), 30)
        t = zaznam.read_text() if zaznam.exists() else ""
        skontroluj("yt-dlp dostane Deno: --js-runtimes deno:…", f"--js-runtimes deno:{n / 'deno'}" in t, t[:300])
    finally:
        o.zatvor()


def vydanie(verzia: str, obsah: bytes, zly_sucet: bool = False):
    """Falošné vydanie na GitHube: latest.json, inštalátor a SHA256SUMS.txt."""
    a = D / "akt"
    a.mkdir(exist_ok=True)
    meno = f"Kometa-{verzia}-windows-x64-setup.exe"
    (a / meno).write_bytes(obsah)
    sucet = "0" * 64 if zly_sucet else hashlib.sha256(obsah).hexdigest()
    (a / "SHA256SUMS.txt").write_text(f"# Kometa {verzia}\n{sucet}  {meno}\n", encoding="utf-8")
    url = lambda n: f"{BASE}/aktualizacia/{n}"  # noqa: E731
    (a / "latest.json").write_text(json.dumps({
        "tag_name": f"v{verzia}", "html_url": "https://github.com/tarzanislav/kometa/releases/tag/v" + verzia,
        "assets": [{"name": meno, "size": len(obsah), "browser_download_url": url(meno),
                    "digest": "sha256:" + hashlib.sha256(obsah).hexdigest()},
                   {"name": "SHA256SUMS.txt", "browser_download_url": url("SHA256SUMS.txt")}]}), encoding="utf-8")


def aktualizacie():
    """Nová verzia: stiahnutie a overenie súčtom, inštalácia hneď aj pri ďalšom spustení (raz), ochrana."""
    print("Automatická aktualizácia")
    cfg = D / "cfg-akt"
    env = dict(KOMETA_AKTUALIZACIE_ZAKLAD=BASE + "/aktualizacia/latest", KOMETA_AKTUALIZACIE_SKUSKA="1",
               KOMETA_AKTUALIZACIE_BALIK="instalator")
    spustene = cfg / "aktualizacia" / "spustene.txt"
    vydanie("99.0.0", b"MZ falosny instalator")
    o = Okno(cfg, **env)
    try:
        s = o.cakaj(lambda s: (s.get("aktualizacia") or {}).get("pripravena"), 30)["aktualizacia"]
        skontroluj("nová verzia stiahnutá a overená (SHA256SUMS aj odtlačok GitHubu), pri ďalšom spustení sama",
                   s["najnovsia"] == "99.0.0" and s["pri_starte"] and s["balik"] == "instalator"
                   and (cfg / "aktualizacia" / "Kometa-99.0.0-windows-x64-setup.exe").read_bytes() == b"MZ falosny instalator"
                   and s["stranka"].endswith("v99.0.0"), s)
        r = o.volaj("aktualizacia_instaluj")
        skontroluj("Nainštalovať teraz: spustí overený inštalátor", r["ok"] and spustene.is_file()
                   and spustene.read_text().startswith("Instalator") and "99.0.0" in spustene.read_text(), r)
    finally:
        o.zatvor()
    spustene.unlink()
    o = Okno(cfg, **env)
    try:
        s = o.cakaj(lambda s: (s.get("aktualizacia") or {}).get("chyba"), 15)["aktualizacia"]
        time.sleep(2)
        skontroluj("nedokončená inštalácia sa pri štarte neopakuje dookola, okno to povie",
                   not spustene.exists() and "minule nedokončila" in s["chyba"] and s["pripravena"], s)
        o.data("aktualizacia_pri_starte")
    finally:
        o.zatvor()
    o = Okno(cfg, **env)
    try:
        time.sleep(3)
        skontroluj("Pri ďalšom spustení: nainštaluje sa sama", spustene.is_file(), list((cfg / "aktualizacia").iterdir()))
    finally:
        o.zatvor()
    # poškodený stiahnutý súbor sa nepoužije – stiahne sa znova a overí
    spustene.unlink()
    (cfg / "aktualizacia" / "Kometa-99.0.0-windows-x64-setup.exe").write_bytes(b"podvrh")
    o = Okno(cfg, **env)
    try:
        s = o.cakaj(lambda s: (s.get("aktualizacia") or {}).get("pripravena"), 30)["aktualizacia"]
        time.sleep(2)
        skontroluj("zmenený súbor aktualizácie sa nespustí, stiahne sa znova",
                   not spustene.exists() and (cfg / "aktualizacia" / "Kometa-99.0.0-windows-x64-setup.exe").read_bytes()
                   == b"MZ falosny instalator", s)
    finally:
        o.zatvor()
    # zlý súčet vo vydaní: nič sa nepripraví
    vydanie("99.0.1", b"MZ iny", zly_sucet=True)
    cfg2 = D / "cfg-akt2"
    o = Okno(cfg2, **env)
    try:
        s = o.cakaj(lambda s: (s.get("aktualizacia") or {}).get("chyba"), 30)["aktualizacia"]
        skontroluj("súbor, ktorý nesedí so súčtom, sa odmietne", not s["pripravena"]
                   and ("nesedí" in s["chyba"] or "líšia" in s["chyba"])
                   and not (cfg2 / "aktualizacia" / "Kometa-99.0.1-windows-x64-setup.exe").exists(), s)
    finally:
        o.zatvor()
    # rovnaká alebo staršia verzia: nič
    vydanie("0.0.1", b"MZ stara")
    o = Okno(D / "cfg-akt3", **env)
    try:
        s = o.cakaj(lambda s: (s.get("aktualizacia") or {}).get("kontrola"), 30)["aktualizacia"]
        skontroluj("staršia verzia na GitHube: bez oznamu", not s["najnovsia"] and not s["pripravena"], s)
    finally:
        o.zatvor()


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
        strazca_okno()
        kurzy(out)
        if sys.platform.startswith("linux"):
            doinstalovanie()
            priecinky()
            jazyky(out)
        aktualizacie()
        pomocnici()
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
