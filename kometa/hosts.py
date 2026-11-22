"""Úložiská súborov: Webshare.cz (oficiálne API) a Fastshare.cz (prihlásenie + stránka súboru).

Sťahuje sa vždy cez tvoj vlastný účet. Program neobchádza obmedzenia
bezplatných účtov (čakanie, captcha, rýchlosť) – na rýchle sťahovanie
treba VIP (Webshare) alebo kredit (Fastshare).
"""

from __future__ import annotations

import hashlib
import re
import threading
import time
import xml.etree.ElementTree as ET

import requests

from .common import USER_AGENT, get_password, save_config


class HostError(Exception):
    pass


WEBSHARE_RE = re.compile(r"https?://(?:[\w-]+\.)?webshare\.cz/(?:#/)?file/(?P<id>\w+)", re.I)
FASTSHARE_RE = re.compile(r"https?://(?:www\.)?fastshare\.(?:cz|cloud)/\S+", re.I)


def detect_host(url: str) -> str | None:
    if WEBSHARE_RE.match(url.strip()):
        return "webshare"
    if FASTSHARE_RE.match(url.strip()):
        return "fastshare"
    return None


# --------------------------------------------------------------------------- #
#  md5-crypt ($1$) – Webshare ho používa na hashovanie hesla pri prihlásení
# --------------------------------------------------------------------------- #

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


# --------------------------------------------------------------------------- #
#  Webshare.cz
# --------------------------------------------------------------------------- #

class WebshareClient:
    API = "https://webshare.cz/api/"

    def __init__(self, token: str = "", session: requests.Session | None = None):
        self.token = token or ""
        self.session = session or requests.Session()
        self.session.headers.update({"User-Agent": USER_AGENT})

    def _call(self, method: str, **data) -> ET.Element:
        try:
            r = self.session.post(self.API + method + "/", data=data, timeout=30,
                                  headers={"Accept": "text/xml; charset=UTF-8"})
            r.raise_for_status()
            root = ET.fromstring(r.content)
        except requests.RequestException as e:
            raise HostError(f"Webshare nie je dostupný: {e}") from e
        except ET.ParseError as e:
            raise HostError("Webshare vrátil neočakávanú odpoveď.") from e
        if (root.findtext("status") or "").upper() != "OK":
            msg = root.findtext("message") or root.findtext("code") or "neznáma chyba"
            raise HostError(f"Webshare: {msg}")
        return root

    def login(self, user: str, password: str) -> str:
        salt = self._call("salt", username_or_email=user).findtext("salt") or ""
        hashed = hashlib.sha1(md5_crypt(password, salt).encode()).hexdigest()
        digest = hashlib.md5(f"{user}:Webshare:{password}".encode("utf-8")).hexdigest()
        try:
            root = self._call("login", username_or_email=user, password=hashed, digest=digest,
                              keep_logged_in=1)
        except HostError as e:
            raise HostError(f"Prihlásenie na Webshare zlyhalo – skontroluj meno a heslo. ({e})") from e
        self.token = root.findtext("token") or ""
        if not self.token:
            raise HostError("Webshare neposlal prihlasovací token.")
        return self.token

    def user_info(self) -> dict:
        root = self._call("user_data", wst=self.token)
        vip = (root.findtext("vip") or "0").strip() == "1"
        days = root.findtext("vip_days") or ""
        until = root.findtext("vip_until") or ""
        return {"user": root.findtext("username") or "", "vip": vip,
                "text": (f"VIP do {until[:10]}" + (f" ({days} dní)" if days else "")) if vip
                else "bez VIP – sťahovanie bude pomalé"}

    def file_info(self, ident: str) -> dict:
        root = self._call("file_info", ident=ident, wst=self.token)
        size = root.findtext("size") or ""
        return {"name": root.findtext("name") or ident, "size": int(size) if size.isdigit() else None}

    def file_link(self, ident: str) -> str:
        root = self._call("file_link", ident=ident, wst=self.token, download_type="file_download",
                          force_https=1)
        link = root.findtext("link")
        if not link:
            raise HostError("Webshare nevrátil odkaz na stiahnutie.")
        return link


# --------------------------------------------------------------------------- #
#  Fastshare.cz
# --------------------------------------------------------------------------- #

class FastshareClient:
    BASE = "https://fastshare.cz"
    NAME_RE = re.compile(r'<h2 title="(.+?)" class="section_title')
    SIZE_RE = re.compile(r'<i class="fa fa-bars"></i>\s*([\d.,]+)\s*(?:&nbsp;)?\s*([KMGT]?B)', re.I)
    LINK_RE = re.compile(r"(https?://\w+\.fastshare\.(?:cz|cloud)/download\.php\?id=\d+)&")
    OFFLINE_RE = re.compile(r">(The file has been deleted|Requested page not found|This file is no longer available)")
    CREDIT_ERROR = " credit for "
    LOGGED_IN = 'href="/logout.php"'

    def __init__(self, session: requests.Session | None = None):
        self.session = session or requests.Session()
        self.session.headers.update({"User-Agent": USER_AGENT})
        for dom in ("fastshare.cz", "fastshare.cloud"):
            self.session.cookies.set("lang", "en", domain=dom)
        self.logged_in = False

    def _get(self, url: str, **kw) -> requests.Response:
        try:
            r = self.session.get(url, timeout=30, **kw)
            r.raise_for_status()
            return r
        except requests.RequestException as e:
            raise HostError(f"Fastshare nie je dostupný: {e}") from e

    def login(self, user: str, password: str) -> None:
        if self.LOGGED_IN in self._get(self.BASE + "/user").text:
            self.logged_in = True
            return
        try:
            r = self.session.post(self.BASE + "/sql.php", data={"login": user, "heslo": password}, timeout=30)
        except requests.RequestException as e:
            raise HostError(f"Fastshare nie je dostupný: {e}") from e
        if self.LOGGED_IN not in r.text and self.LOGGED_IN not in self._get(self.BASE + "/user").text:
            raise HostError("Prihlásenie na Fastshare zlyhalo – skontroluj meno a heslo.")
        self.logged_in = True

    def user_info(self) -> dict:
        html = self._get(self.BASE + "/user").text
        if self.LOGGED_IN not in html:
            raise HostError("Na Fastshare nie si prihlásený.")
        m = re.search(r">Active until ([\d.]+)<", html)
        if m:
            return {"vip": True, "text": f"neobmedzené sťahovanie do {m.group(1)}"}
        if ">Unlimited downloading<" in html:
            return {"vip": True, "text": "neobmedzené sťahovanie"}
        m = re.search(r"<td>([\d.,]+) ([KMGT]B)\s*</td>", html)
        if m:
            return {"vip": True, "text": f"kredit {m.group(1)} {m.group(2)}"}
        return {"vip": False, "text": "bez kreditu – sťahovanie nebude možné"}

    def resolve(self, url: str) -> dict:
        """Vráti {name, size, link} pre stránku súboru."""
        r = self._get(url.split("#")[0], stream=True)
        ctype = (r.headers.get("Content-Type") or "").lower()
        if "text/html" not in ctype:  # s kreditom niekedy presmeruje rovno na súbor
            from .httpdl import filename_from_response
            size = r.headers.get("Content-Length")
            r.close()
            return {"name": filename_from_response(r.headers, r.url), "link": r.url,
                    "size": int(size) if size and size.isdigit() else None}
        html = r.text
        if self.OFFLINE_RE.search(html):
            raise HostError("Súbor na Fastshare už neexistuje.")
        m = self.NAME_RE.search(html)
        name = m.group(1) if m else None
        size = None
        m = self.SIZE_RE.search(html)
        if m:
            mult = {"B": 1, "KB": 1024, "MB": 1024 ** 2, "GB": 1024 ** 3, "TB": 1024 ** 4}[m.group(2).upper()]
            size = int(float(m.group(1).replace(",", ".")) * mult)  # len približne – stránka zaokrúhľuje
        m = self.LINK_RE.search(html)
        if m:
            return {"name": name, "size": size, "link": m.group(1)}
        if self.CREDIT_ERROR in html:
            raise HostError("Na Fastshare nemáš dosť kreditu na tento súbor.")
        if self.LOGGED_IN not in html:
            raise HostError("Na Fastshare nie si prihlásený – zadaj účet v karte Účty.")
        raise HostError("Na stránke Fastshare sa nenašiel odkaz na stiahnutie (stránka sa mohla zmeniť).")


# --------------------------------------------------------------------------- #
#  Správa účtov (prihlásenie podľa uložených údajov)
# --------------------------------------------------------------------------- #

class Accounts:
    def __init__(self, cfg: dict):
        self.cfg = cfg
        self._lock = threading.Lock()
        self._ws: WebshareClient | None = None
        self._fs: FastshareClient | None = None
        self._fs_time = 0.0

    def reset(self, service: str | None = None) -> None:
        with self._lock:
            if service in (None, "webshare"):
                self._ws = None
            if service in (None, "fastshare"):
                self._fs = None

    def webshare(self) -> WebshareClient:
        with self._lock:
            if self._ws:
                return self._ws
            user = self.cfg.get("webshare_user") or ""
            client = WebshareClient(self.cfg.get("webshare_token") or "")
            if client.token:
                try:
                    client.user_info()
                    self._ws = client
                    return client
                except HostError:
                    client.token = ""
            password = get_password("webshare", user)
            if not user or not password:
                raise HostError("Na sťahovanie z Webshare sa prihlás v karte „Účty“.")
            self.cfg["webshare_token"] = client.login(user, password)
            save_config(self.cfg)
            self._ws = client
            return client

    def fastshare(self) -> FastshareClient:
        with self._lock:
            if self._fs and time.time() - self._fs_time < 1800:
                return self._fs
            user = self.cfg.get("fastshare_user") or ""
            password = get_password("fastshare", user)
            if not user or not password:
                raise HostError("Na sťahovanie z Fastshare sa prihlás v karte „Účty“.")
            client = FastshareClient()
            client.login(user, password)
            self._fs, self._fs_time = client, time.time()
            return client
