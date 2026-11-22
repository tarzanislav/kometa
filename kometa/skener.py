"""Skener stránok – nájde videá, streamy a súbory na akejkoľvek webovej stránke.

Postup (všetko vlastný kód, bez prehliadača):
  1. stiahne HTML stránky (voliteľne s cookies z prehliadača – stránky po prihlásení),
  2. prejde značky <video>/<source>/<audio>/<iframe>/<embed>/<a>/<meta>, atribúty data-*,
     JSON-LD (VideoObject) a OpenGraph/Twitter metadáta,
  3. v celom texte (aj v JavaScripte a JSON) hľadá adresy médií – aj zakódované
     (\\/, \\u002F, &amp;, percentá),
  4. rozpozná vložené prehrávače (Vimeo, YouTube, Wistia, Bunny, JW Player, Brightcove,
     Cloudflare Stream, Dailymotion, Loom, Streamable, Facebook, Instagram, TikTok …)
     a prevedie ich na kanonickú adresu; pri Vimeo si pamätá aj tajný kľúč ?h=…,
  5. vnorené iframe (napr. prehrávač v rámčeku v rámčeku) prehľadá rekurzívne,
  6. každému nálezu priradí skóre (hlavné video > metadáta > stream > súbor; reklamy dole)
     a Referer – adresu stránky, na ktorej je prehrávač vložený. Vimeo s obmedzením
     na doménu sa bez správneho Referera nedá prehrať ani stiahnuť.
"""

from __future__ import annotations

import html
import json
import re
from dataclasses import dataclass, field
from html.parser import HTMLParser
from urllib.parse import parse_qs, unquote, urljoin, urlparse

import requests

from .common import EXTRA_YDL_OPTS, USER_AGENT

MEDIA_EXT = {
    "m3u8": ("stream", "HLS"), "mpd": ("stream", "DASH"), "ism": ("stream", "Smooth"),
    "mp4": ("video", "MP4"), "m4v": ("video", "MP4"), "webm": ("video", "WebM"), "mov": ("video", "MOV"),
    "mkv": ("video", "MKV"), "avi": ("video", "AVI"), "flv": ("video", "FLV"), "ts": ("video", "TS"),
    "mp3": ("audio", "MP3"), "m4a": ("audio", "M4A"), "aac": ("audio", "AAC"), "ogg": ("audio", "OGG"),
    "opus": ("audio", "Opus"), "flac": ("audio", "FLAC"), "wav": ("audio", "WAV"),
    "zip": ("subor", "ZIP"), "rar": ("subor", "RAR"), "7z": ("subor", "7z"), "pdf": ("subor", "PDF"),
    "iso": ("subor", "ISO"), "srt": ("titulky", "SRT"), "vtt": ("titulky", "VTT"),
}
_EXT_ALT = "|".join(sorted(MEDIA_EXT, key=len, reverse=True))
# adresa média kdekoľvek v texte (HTML, JS, JSON); končí príponou, voliteľne s ?parametrami
_MEDIA_URL_RE = re.compile(
    r"""(?P<url>(?:https?:)?//[^\s"'<>\\(){}\[\]]+?\.(?P<ext>%s)(?:\?[^\s"'<>\\(){}\[\]]*)?)(?=[\s"'<>\\(){}\[\],;]|$)"""
    % _EXT_ALT, re.I)
_AD_RE = re.compile(r"doubleclick|googlesyndication|googleads|adservice|adnxs|/ads?/|[?&/_-]ad[sv]?[_-]|preroll|"
                    r"vast\b|\bvpaid|imasdk|taboola|outbrain|teads|spotx|springserve|/pixel|analytics", re.I)

KIND_TEXT = {"embed": "Prehrávač", "stream": "Stream", "video": "Video", "audio": "Zvuk", "subor": "Súbor",
             "titulky": "Titulky", "stranka": "Stránka"}


@dataclass
class Nalez:
    """Jeden nájdený zdroj na stránke."""
    url: str
    kind: str           # embed | stream | video | audio | subor | titulky | stranka
    provider: str       # Vimeo, YouTube, HLS, MP4 …
    referer: str = ""
    title: str = ""
    score: int = 0
    note: str = ""
    key: str = field(default="", repr=False)

    @property
    def downloadable(self) -> bool:
        return self.kind in ("embed", "stream", "video", "audio", "subor")


# --------------------------------------------------------------------------- #
#  Vložené prehrávače → kanonická adresa (provider, url, kľúč na odstránenie duplicít)
# --------------------------------------------------------------------------- #

def _vimeo(m: re.Match, raw: str):
    vid = m.group("id")
    h = m.groupdict().get("h") or ""
    if not h:
        q = parse_qs(urlparse(raw).query)
        h = (q.get("h") or [""])[0]
    url = f"https://player.vimeo.com/video/{vid}" + (f"?h={h}" if h else "")
    return "Vimeo", url, f"vimeo:{vid}"


EMBEDS: list[tuple[re.Pattern, object]] = [
    (re.compile(r"player\.vimeo\.com/video/(?P<id>\d+)", re.I), _vimeo),
    (re.compile(r"(?<![\w.])vimeo\.com/(?:video/|channels/[\w-]+/|groups/[\w-]+/videos/|album/\d+/video/)?"
                r"(?P<id>\d{5,})(?:/(?P<h>[0-9a-f]{6,}))?", re.I), _vimeo),
    (re.compile(r"vimeo\.com/(?P<kind>event|showcase)/(?P<id>\d+)", re.I),
     lambda m, r: ("Vimeo", f"https://vimeo.com/{m.group('kind')}/{m.group('id')}", f"vimeo:{m.group(0)}")),
    (re.compile(r"embed\.vhx\.tv/videos/(?P<id>\d+)", re.I),
     lambda m, r: ("Vimeo OTT", f"https://embed.vhx.tv/videos/{m.group('id')}", f"vhx:{m.group('id')}")),
    (re.compile(r"(?:youtube(?:-nocookie)?\.com/(?:embed|v|shorts|live)/|youtu\.be/|youtube\.com/watch\?(?:[^\"'\s]*&)?v=)"
                r"(?P<id>[\w-]{11})", re.I),
     lambda m, r: ("YouTube", f"https://www.youtube.com/watch?v={m.group('id')}", f"yt:{m.group('id')}")),
    (re.compile(r"(?:fast\.wistia\.(?:net|com)/embed/(?:iframe|medias)/|wistia_async_|wistia\.com/medias/)"
                r"(?P<id>[a-z0-9]{8,})", re.I),
     lambda m, r: ("Wistia", f"https://fast.wistia.net/embed/iframe/{m.group('id')}", f"wistia:{m.group('id')}")),
    (re.compile(r"(?:iframe|player)\.mediadelivery\.net/(?:embed|play)/(?P<lib>\d+)/(?P<id>[0-9a-f-]{36})", re.I),
     lambda m, r: ("Bunny Stream", f"https://iframe.mediadelivery.net/embed/{m.group('lib')}/{m.group('id')}",
                   f"bunny:{m.group('id')}")),
    (re.compile(r"(?:cdn\.jwplayer\.com|content\.jwplatform\.com)/(?:players|previews|videos|manifests)/"
                r"(?P<id>[a-zA-Z0-9]{8})", re.I),
     lambda m, r: ("JW Player", f"https://cdn.jwplayer.com/v2/media/{m.group('id')}", f"jw:{m.group('id')}")),
    (re.compile(r"players\.brightcove\.net/(?P<acc>\d+)/(?P<pl>[\w-]+)_\w+/index\.html\?[^\"'\s]*videoId=(?P<id>[\w:-]+)",
                re.I),
     lambda m, r: ("Brightcove", html.unescape(m.group(0)).split('"')[0] if m.group(0).startswith("http")
                   else "https://" + html.unescape(m.group(0)), f"bc:{m.group('id')}")),
    (re.compile(r"(?:customer-[\w]+\.cloudflarestream\.com|iframe\.videodelivery\.net|watch\.cloudflarestream\.com)"
                r"/(?P<id>[0-9a-f]{32})", re.I),
     lambda m, r: ("Cloudflare Stream", f"https://iframe.videodelivery.net/{m.group('id')}", f"cf:{m.group('id')}")),
    (re.compile(r"dailymotion\.com/(?:embed/)?video/(?P<id>[a-z0-9]+)|dai\.ly/(?P<id2>[a-z0-9]+)|"
                r"geo\.dailymotion\.com/player[^\"'\s]*[?&]video=(?P<id3>[a-z0-9]+)", re.I),
     lambda m, r: ("Dailymotion", "https://www.dailymotion.com/video/" + (m.group("id") or m.group("id2") or m.group("id3")),
                   "dm:" + (m.group("id") or m.group("id2") or m.group("id3")))),
    (re.compile(r"loom\.com/(?:embed|share)/(?P<id>[0-9a-f]{32})", re.I),
     lambda m, r: ("Loom", f"https://www.loom.com/share/{m.group('id')}", f"loom:{m.group('id')}")),
    (re.compile(r"streamable\.com/(?:e/|o/|s/)?(?P<id>[a-z0-9]{5,8})(?![\w.])", re.I),
     lambda m, r: ("Streamable", f"https://streamable.com/{m.group('id')}", f"stb:{m.group('id')}")),
    (re.compile(r"rumble\.com/embed/(?P<id>v[a-z0-9]+)", re.I),
     lambda m, r: ("Rumble", f"https://rumble.com/embed/{m.group('id')}/", f"rumble:{m.group('id')}")),
    (re.compile(r"player\.twitch\.tv/\?[^\"'\s]*video=(?P<id>v?\d+)|clips\.twitch\.tv/embed\?clip=(?P<clip>[\w-]+)",
                re.I),
     lambda m, r: ("Twitch", f"https://www.twitch.tv/videos/{m.group('id').lstrip('v')}" if m.group("id")
                   else f"https://clips.twitch.tv/{m.group('clip')}", f"tw:{m.group(0)}")),
    (re.compile(r"instagram\.com/(?P<t>p|reel|tv)/(?P<id>[\w-]+)", re.I),
     lambda m, r: ("Instagram", f"https://www.instagram.com/{m.group('t')}/{m.group('id')}/", f"ig:{m.group('id')}")),
    (re.compile(r"tiktok\.com/(?:embed/v2/|@[\w.-]+/video/|player/v1/)(?P<id>\d{15,})", re.I),
     lambda m, r: ("TikTok", f"https://www.tiktok.com/embed/v2/{m.group('id')}", f"tt:{m.group('id')}")),
    (re.compile(r"facebook\.com/plugins/video\.php\?[^\"'\s]*href=(?P<href>[^&\"'\s]+)", re.I),
     lambda m, r: ("Facebook", unquote(html.unescape(m.group("href"))), "fb:" + unquote(m.group("href")))),
    (re.compile(r"w\.soundcloud\.com/player/\?[^\"'\s]*url=(?P<u>[^&\"'\s]+)", re.I),
     lambda m, r: ("SoundCloud", unquote(html.unescape(m.group("u"))), "sc:" + unquote(m.group("u")))),
]


def match_embed(url: str):
    """Ak adresa patrí známemu prehrávaču, vráti (provider, kanonická_url, kľúč)."""
    for rx, fn in EMBEDS:
        m = rx.search(url)
        if m:
            return fn(m, url)
    return None


# --------------------------------------------------------------------------- #
#  Čítanie HTML
# --------------------------------------------------------------------------- #

class _Collector(HTMLParser):
    URL_ATTRS = ("src", "href", "data-src", "data-url", "data-video", "data-video-url", "data-video-src",
                 "data-file", "data-stream", "data-hls", "data-dash", "data-mp4", "data-embed", "data-iframe",
                 "data-lazy-src", "data-vimeo-url", "content", "value", "poster")

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.items: list[tuple[str, str, dict]] = []   # (tag, url, attrs)
        self.meta: dict[str, str] = {}
        self.ldjson: list[str] = []
        self.title = ""
        self._in_title = False
        self._in_ld = False
        self._buf: list[str] = []

    def handle_starttag(self, tag, attrs):
        a = {k.lower(): (v or "") for k, v in attrs}
        if tag == "meta":
            key = (a.get("property") or a.get("name") or a.get("itemprop") or "").lower()
            if key and a.get("content"):
                self.meta.setdefault(key, a["content"])
            return
        if tag == "title":
            self._in_title = True
        if tag == "script" and "ld+json" in a.get("type", "").lower():
            self._in_ld, self._buf = True, []
        if tag == "param" and a.get("name", "").lower() in ("movie", "src", "flashvars"):
            self.items.append(("param", a.get("value", ""), a))
        for k in self.URL_ATTRS:
            if a.get(k) and k not in ("content", "value"):
                self.items.append((tag, a[k], a))
        for k in ("data-vimeo-id", "data-vimeo-video-id", "data-vimeo"):
            if a.get(k, "").isdigit():
                self.items.append(("vimeo-id", f"https://player.vimeo.com/video/{a[k]}"
                                   + (f"?h={a['data-vimeo-hash']}" if a.get("data-vimeo-hash") else ""), a))
        for k in ("data-youtube-id", "data-yt-id", "data-video-id"):
            v = a.get(k, "")
            if re.fullmatch(r"[\w-]{11}", v) and (k != "data-video-id" or "youtube" in json.dumps(a).lower()):
                self.items.append(("yt-id", f"https://www.youtube.com/watch?v={v}", a))
        if "srcset" in a and tag == "source":
            for part in a["srcset"].split(","):
                self.items.append((tag, part.strip().split(" ")[0], a))

    def handle_endtag(self, tag):
        if tag == "title":
            self._in_title = False
        if tag == "script" and self._in_ld:
            self._in_ld = False
            self.ldjson.append("".join(self._buf))

    def handle_data(self, data):
        if self._in_title:
            self.title += data
        if self._in_ld:
            self._buf.append(data)


def _decode_text(text: str) -> str:
    """Odhalí adresy schované v JS/JSON: \\/ → /, \\u002F → /, &amp; → &."""
    t = text.replace("\\/", "/")
    t = re.sub(r"\\u00([0-9a-fA-F]{2})", lambda m: chr(int(m.group(1), 16)), t)
    return html.unescape(t)


def _walk_json(obj):
    if isinstance(obj, dict):
        yield obj
        for v in obj.values():
            yield from _walk_json(v)
    elif isinstance(obj, list):
        for v in obj:
            yield from _walk_json(v)


# --------------------------------------------------------------------------- #
#  Skener
# --------------------------------------------------------------------------- #

class Skener:
    def __init__(self, session: requests.Session | None = None, *, max_frames: int = 6, max_depth: int = 2,
                 log=lambda m: None, cookies_browser: str | None = None):
        self.session = session or requests.Session()
        self.session.headers.update({"User-Agent": USER_AGENT,
                                     "Accept-Language": "sk,cs;q=0.9,en;q=0.8"})
        self.page_error = ""
        self.max_frames = max_frames
        self.max_depth = max_depth
        self.log = log
        browser = cookies_browser if cookies_browser is not None else \
            (EXTRA_YDL_OPTS.get("cookiesfrombrowser") or (None,))[0]
        if browser:
            self._load_browser_cookies(browser)

    def _load_browser_cookies(self, browser: str) -> None:
        try:
            from yt_dlp.cookies import extract_cookies_from_browser
            jar = extract_cookies_from_browser(browser)
            for c in jar:
                self.session.cookies.set_cookie(c)
            self.log(f"🍪 Načítané cookies z prehliadača {browser}")
        except Exception as e:  # noqa: BLE001 – stránka môže fungovať aj bez prihlásenia
            self.log(f"⚠ Cookies z prehliadača {browser} sa nepodarilo načítať: {e}")

    def fetch(self, url: str, referer: str | None = None) -> tuple[str, str]:
        headers = {"Referer": referer} if referer else {}
        r = self.session.get(url, headers=headers, timeout=(15, 30), allow_redirects=True)
        r.raise_for_status()
        ctype = (r.headers.get("Content-Type") or "").lower()
        if "html" not in ctype and "xml" not in ctype and "text" not in ctype and "json" not in ctype:
            raise ValueError("not-html:" + ctype)
        r.encoding = r.encoding or r.apparent_encoding
        return r.text[:4_000_000], r.url

    def scan(self, url: str) -> list[Nalez]:
        """Prehľadá stránku (a vnorené rámčeky) a vráti nálezy zoradené od najlepšieho."""
        found: dict[str, Nalez] = {}
        frames_left = [self.max_frames]
        page_title = self._scan_page(url, None, 0, found, frames_left)
        out = sorted(found.values(), key=lambda n: (-n.score, n.url))
        for n in out:
            if not n.title and page_title:
                n.title = page_title
        return [n for n in out if n.score > 0]

    # ---------- jedna stránka ----------
    def _scan_page(self, url: str, referer: str | None, depth: int, found: dict, frames_left: list) -> str:
        try:
            text, final_url = self.fetch(url, referer)
        except ValueError as e:
            if str(e).startswith("not-html:"):   # samotná adresa je médium/súbor
                self._add_media(found, url, referer or url, "", base_score=70, force=True)
            return ""
        except requests.RequestException as e:
            self.log(f"⚠ Stránku {url} sa nepodarilo načítať: {e}")
            if depth == 0:
                self.page_error = str(e)
            return ""
        self.log(f"🔍 Prehľadávam {final_url}" + (f" (rámček, úroveň {depth})" if depth else ""))

        p = _Collector()
        try:
            p.feed(text)
            p.close()
        except Exception:  # noqa: BLE001 – pokazené HTML nevadí, máme aj textové hľadanie
            pass
        title = html.unescape((p.meta.get("og:title") or p.title or "").strip())[:200]
        bonus = 5 if depth == 0 else 0
        frames: list[str] = []
        link_keys: set[str] = set()   # videá, na ktoré stránka len odkazuje (<a href>), nie sú hlavný obsah

        # 1) značky a atribúty
        for tag, raw, attrs in p.items:
            raw = html.unescape(raw.strip())
            if not raw or raw.startswith(("data:", "blob:", "javascript:", "#", "mailto:")):
                continue
            abs_url = urljoin(final_url, raw)
            emb = match_embed(abs_url)
            if emb:
                # vložený prehrávač je hlavný obsah; obyčajný odkaz <a> na iné video len možnosť
                if tag == "a":
                    link_keys.add(emb[2])
                self._add_embed(found, emb, final_url, title, (40 if tag == "a" else 90) + bonus)
                continue
            if tag in ("video", "source", "audio") and tag != "img":
                if attrs.get("poster") == raw:
                    continue
                if self._add_media(found, abs_url, final_url, title, 85 + bonus, force=tag in ("video", "source")):
                    continue
            if tag in ("iframe", "frame", "embed") and abs_url.startswith("http"):
                frames.append(abs_url)
                continue
            if tag in ("a", "link", "param", "div", "button", "span", "vimeo-id", "yt-id", "object"):
                self._add_media(found, abs_url, final_url, title, 45 + bonus)

        # 2) OpenGraph / Twitter / itemprop
        for key in ("og:video:secure_url", "og:video:url", "og:video", "twitter:player:stream", "contenturl",
                    "embedurl", "twitter:player"):
            v = p.meta.get(key)
            if not v:
                continue
            v = urljoin(final_url, html.unescape(v))
            emb = match_embed(v)
            if emb:
                self._add_embed(found, emb, final_url, title, 82 + bonus)
            elif not self._add_media(found, v, final_url, title, 80 + bonus, force=key != "twitter:player") \
                    and key == "twitter:player":
                frames.append(v)

        # 3) JSON-LD VideoObject
        for block in p.ldjson:
            try:
                data = json.loads(block)
            except (ValueError, TypeError):
                continue
            for obj in _walk_json(data):
                types = obj.get("@type")
                types = types if isinstance(types, list) else [types]
                if not any(str(t).lower() in ("videoobject", "audioobject", "clip", "movie") for t in types):
                    continue
                name = html.unescape(str(obj.get("name") or title))[:200]
                for k in ("contentUrl", "embedUrl", "url"):
                    v = obj.get(k)
                    if isinstance(v, str) and v.startswith(("http", "/")):
                        v = urljoin(final_url, v)
                        emb = match_embed(v)
                        if emb:
                            self._add_embed(found, emb, final_url, name, 88 + bonus)
                        else:
                            self._add_media(found, v, final_url, name, 84 + bonus, force=k == "contentUrl")

        # 4) celý text vrátane JavaScriptu/JSON (aj zakódované adresy)
        decoded = _decode_text(text)
        for m in _MEDIA_URL_RE.finditer(decoded):
            u = m.group("url")
            u = ("https:" + u) if u.startswith("//") else u
            self._add_media(found, u, final_url, title, 60 + bonus)
        for rx, fn in EMBEDS:
            for m in rx.finditer(decoded):
                start = decoded.rfind("http", max(0, m.start() - 60), m.start() + 1)
                raw = decoded[start:m.end() + 80] if start >= 0 else m.group(0)
                emb = fn(m, raw)
                if emb[2] not in link_keys:
                    self._add_embed(found, emb, final_url, title, 66 + bonus)

        # 5) vnorené rámčeky (prehrávač býva v iframe v iframe)
        if depth < self.max_depth:
            for f in dict.fromkeys(frames):
                if frames_left[0] <= 0:
                    break
                if _AD_RE.search(f) or match_embed(f):
                    continue
                frames_left[0] -= 1
                self._scan_page(f, final_url, depth + 1, found, frames_left)
        return title

    # ---------- pridanie nálezu ----------
    def _put(self, found: dict, n: Nalez) -> None:
        if _AD_RE.search(n.url):
            n.score -= 70
            n.note = (n.note + " · pravdepodobne reklama").strip(" ·")
        old = found.get(n.key)
        if old is None or n.score > old.score:
            if old and not n.title:
                n.title = old.title
            found[n.key] = n
        elif old and n.score == old.score and not old.title:
            old.title = n.title

    def _add_embed(self, found: dict, emb, referer: str, title: str, score: int) -> None:
        provider, url, key = emb
        note = "obmedzené na doménu – použije sa Referer stránky" if provider.startswith("Vimeo") else ""
        if provider == "Vimeo" and "?h=" in url:
            note = "neverejné video (kľúč h=…) · " + note
        self._put(found, Nalez(url=url, kind="embed", provider=provider, referer=referer, title=title,
                               score=score, note=note.strip(" ·"), key=key))

    def _add_media(self, found: dict, url: str, referer: str, title: str, base_score: int,
                   force: bool = False) -> bool:
        if not url.startswith("http"):
            return False
        path = urlparse(url).path.lower()
        m = re.search(r"\.(%s)$" % _EXT_ALT, path)
        if not m and not force:
            return False
        ext = m.group(1) if m else "mp4"
        kind, provider = MEDIA_EXT.get(ext, ("video", ext.upper()))
        score = base_score + {"stream": 4, "video": 0, "audio": -25, "titulky": -40, "subor": -30}[kind]
        if kind == "stream":
            low = url.lower()
            if re.search(r"master|playlist|manifest|index\.m3u8", low):
                score += 3   # hlavný zoznam (všetky kvality) je lepší ako jedna kvalita
            if re.search(r"/(?:\d{3,4}p?|chunklist|media_\d|segment)", low):
                score -= 6
        key = re.sub(r"[?#].*$", "", url) if kind in ("video", "audio", "subor", "titulky") else url.split("#")[0]
        self._put(found, Nalez(url=url, kind=kind, provider=provider, referer=referer, title=title,
                               score=score, key=key))
        return True


def best_candidates(found: list[Nalez], limit: int = 10) -> list[Nalez]:
    """Výber na automatické stiahnutie: hlavné videá/prehrávače, bez reklám a duplicít."""
    media = [n for n in found if n.kind in ("embed", "stream", "video") and n.score >= 50]
    if not media:
        media = [n for n in found if n.kind == "audio" and n.score >= 30]
    # ak je na stránke prehrávač (napr. Vimeo), streamy z toho istého prehrávača sú zvyčajne duplicitné
    if any(n.kind == "embed" for n in media):
        top = max(n.score for n in media)
        media = [n for n in media if n.kind == "embed" or n.score >= top - 3]
    return media[:limit]
