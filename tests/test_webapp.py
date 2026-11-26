"""Rýchla skúška nového rozhrania bez siete: jadro, lokálny server s kľúčom, API a súbory rozhrania.

Spustenie:  python tests/test_webapp.py
"""
import json
import subprocess
import shutil
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from kometa import common  # noqa: E402

TMP = Path(tempfile.mkdtemp(prefix="kometa_test_"))
common.CONFIG_DIR = TMP / "cfg"
common.CONFIG_PATH = common.CONFIG_DIR / "nastavenia.json"
common.SECRETS_PATH = common.CONFIG_DIR / "ucty.dat"
common.QUEUE_PATH = common.CONFIG_DIR / "fronta.json"
common.default_outdir = lambda: TMP / "Kometa"

from kometa import webapp  # noqa: E402

urllib.request.install_opener(urllib.request.build_opener(urllib.request.ProxyHandler({})))


def main() -> int:
    try:
        sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]
    except Exception:  # noqa: BLE001
        pass
    assert (webapp.WEBUI / "index.html").is_file(), "chýba preložené rozhranie kometa/webui"
    jadro = webapp.Jadro()
    srv, url = webapp.serve(jadro)
    base, token = url.split("#t=")

    def api(name, *args, kluc=token):
        req = urllib.request.Request(base + "api/" + name, data=json.dumps(list(args)).encode(),
                                     headers={"X-Kometa": kluc, "Host": "127.0.0.1"}, method="POST")
        return json.loads(urllib.request.urlopen(req, timeout=10).read())

    try:
        init = api("init")
        assert init["ok"] and init["data"]["verzia"], init
        assert {f["id"] for f in init["data"]["formaty"]} >= {"video", "mp4", "mp3"}
        stav = api("stav")["data"]
        assert stav["polozky"] == [] and len(stav["graf"]) == 90
        r = api("rozpoznat", "https://www.youtube.com/watch?v=uL9-e9OlLfw\nhttps://webshare.cz/#/file/abc\n"
                             "https://www.youtube.com/@kanal")["data"]
        assert [x["zdroj"] for x in r] == ["YouTube", "Webshare", "YouTube"] and r[2]["kolekcia"], r
        assert [x["ulozisko"] for x in r] == [False, True, False], r
        assert not api("pridat", "bez odkazu")["ok"]

        # úložisko: hotový súbor (nie zvolený formát), názov z odkazu Fastshare
        fs = "https://fastshare.cloud/35565309/a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv"
        jadro.dq.pause_all()
        api("pridat", fs, "mp3")
        p = next(x for x in api("stav")["data"]["polozky"] if x["url"] == fs)
        assert p["format"] == "subor" and p["nazov"] == "a.loose.end.web-dl.cz.5.1.es.5.1.2024.1080p.mkv", p
        assert api("vycistit_front")["ok"] and api("stav")["data"]["polozky"] == []

        # jazyky vo viacerých videách (yt-dlp nahradený – formáty s jazykmi)
        def falosne_info(url, log=None):
            if "zle" in url:
                raise RuntimeError("Video is unavailable")
            f = lambda lang, note="": {"format_id": lang, "vcodec": "none", "acodec": "mp4a.40.2", "ext": "m4a",  # noqa: E731
                                       "language": lang, "abr": 128, "format_note": note, "url": "https://x"}
            return {"title": f"Video {url[-1]}", "duration": 60,
                    "formats": [f("en-US", "original, default"), f("cs"), f("sk")] if url.endswith("a") else [f("en", "original"), f("cs")]}
        povodne = webapp.fetch_info
        webapp.fetch_info = falosne_info
        try:
            z = api("jazyky_zistit", ["https://youtu.be/aaaaaaaaaaa", "https://youtu.be/bbbbbbbbbbb",
                                      "https://youtu.be/zlezlezlezl", "https://vimeo.com/1"])["data"]
        finally:
            webapp.fetch_info = povodne
        assert z["preskumanych"] == 2 and [(j["kod"], j["pocet"]) for j in z["jazyky"]] == [("cs", 2), ("en", 2), ("sk", 1)], z
        assert len(z["videa"]) == 3 and "chyba" in z["videa"][2] and z["videa"][0]["jazyky"] == ["en", "cs", "sk"], z

        # titulky: výber podľa jazyka, json3/WebVTT → SRT, vloženie cez FFmpeg
        from kometa import jazyky as jz
        info = {"subtitles": {"cs": [{"ext": "vtt", "url": "u-cs"}], "en-GB": [{"ext": "json3", "url": "u-en"}]},
                "automatic_captions": {"ru-orig": [{"ext": "json3", "url": "u-ru"}], "sk": [{"ext": "vtt", "url": "u-sk"}]}}
        assert jz.find_subtitles(info, "cs", False, False) == ("u-cs", "vtt", False)
        assert jz.find_subtitles(info, "en-US", False, False) == ("u-en", "json3", False)
        assert jz.find_subtitles(info, "sk", False, False) is None
        assert jz.find_subtitles(info, "sk", False, True) == ("u-sk", "vtt", True)
        assert jz.find_subtitles(info, "ru", True, True) == ("u-ru", "json3", True)
        c = jz.subs_from_json3('{"events":[{"tStartMs":0,"dDurationMs":900,"wWinId":1},'
                               '{"tStartMs":100,"dDurationMs":800,"segs":[{"utf8":"Ahoj "},{"utf8":"svet"}]}]}')
        assert jz.to_srt(c) == "1\n00:00:00,100 --> 00:00:00,900\nAhoj svet\n\n", c
        c = jz.subs_from_vtt("WEBVTT\n\n00:01.000 --> 00:02.000 align:start\n<c>Prvý</c> &amp; druhý\n")
        assert c == [[1000, 2000, "Prvý & druhý"]], c
        if shutil.which("ffmpeg") and shutil.which("ffprobe"):
            d = TMP / "tit"
            d.mkdir()
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=160x120:rate=10", "-t", "2",
                            "-c:v", "libx264", str(d / "v.mp4")], check=True)
            (d / "cs.srt").write_text(jz.to_srt([[200, 1500, "Ahoj"]]), encoding="utf-8")
            for ext in ("mkv", "mp4"):
                jz.mux(d / f"o.{ext}", [{"path": d / "v.mp4", "kind": "video"},
                                        {"path": d / "cs.srt", "kind": "subtitle", "lang3": "cze", "title": "Čeština"}], "t")
                r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "stream=codec_type,codec_name:stream_tags=language",
                                    "-of", "csv=p=0", str(d / f"o.{ext}")], capture_output=True, text=True)
                assert ("subtitle" in r.stdout and "cze" in r.stdout
                        and ("subrip" if ext == "mkv" else "mov_text") in r.stdout), (ext, r.stdout)
            # údaje o videu, kapitoly, obrázok a roly (dabing/originál) – ako vo vlastnom spájaní jadra
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=500", "-t", "2", "-c:a", "aac",
                            str(d / "a.m4a")], check=True)
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "color=c=red:size=64x36", "-frames:v", "1",
                            str(d / "obal.jpg")], check=True)
            ex = jz.extras_from_info({"uploader": "Kanál", "upload_date": "20240517", "webpage_url": "https://e.org/v",
                                      "description": "Popis", "duration": 2,
                                      "chapters": [{"start_time": 0, "title": "Úvod"}, {"start_time": 1, "title": "B=c"}]},
                                     {"udaje": True, "kapitoly": True, "obal": False})
            ex["cover"] = d / "obal.jpg"
            for ext in ("mkv", "mp4"):
                jz.mux(d / f"m.{ext}", [{"path": d / "v.mp4", "kind": "video"},
                                        {"path": d / "a.m4a", "kind": "audio", "lang3": "cze", "title": "Čeština",
                                         "povodny": False}], "Názov", ex)
                j = json.loads(subprocess.run(["ffprobe", "-v", "error", "-show_streams", "-show_chapters",
                                               "-show_format", "-of", "json", str(d / f"m.{ext}")],
                                              capture_output=True, text=True).stdout)
                tagy = {k.lower(): v for k, v in j["format"].get("tags", {}).items()}
                kap = [c["tags"]["title"] for c in j["chapters"]]
                obal = [x for x in j["streams"] if x["disposition"].get("attached_pic")
                        or x.get("tags", {}).get("filename") == "cover.jpg"]
                dub = [x["disposition"]["dub"] for x in j["streams"] if x["codec_type"] == "audio"]
                assert kap == ["Úvod", "B=c"] and tagy.get("artist") == "Kanál" and tagy.get("title") == "Názov" \
                    and len(obal) == 1 and dub == [1] and not list(d.glob("*.kapitoly.txt")), (ext, kap, tagy, obal, dub)

        # aktualizácia Kometa pre Python: porovnanie verzií, súčty, inštalácia raz, nič mimo priečinka
        import hashlib
        import io as io_
        import zipfile as zf
        from kometa import aktualizacia as akt
        assert akt.newer("1.10.0", "1.9.0") and not akt.newer("1.9", "1.9.0") and akt.newer("v2", "1.99.9")
        assert akt.sum_from_list("# Kometa 2\n" + "a" * 64 + "  Kometa-2.zip\n", "Kometa-2.zip") == "a" * 64
        ak = TMP / "akt"
        akt.ROOT, akt.DIR = ak / "program", ak / "cfg"
        (ak / "program").mkdir(parents=True)
        (ak / "program" / "app.py").write_text("stary")
        buf = io_.BytesIO()
        with zf.ZipFile(buf, "w") as z:
            z.writestr("Kometa/app.py", "novy")
            z.writestr("Kometa/kometa/x.py", "x")
            z.writestr("Kometa/../von.txt", "zlo")
        akt.DIR.mkdir()
        (akt.DIR / "Kometa-99.0.zip").write_bytes(buf.getvalue())
        akt.STATE["balik"] = "zip"
        akt._save({"verzia": "99.0", "subor": "Kometa-99.0.zip", "sha256": hashlib.sha256(buf.getvalue()).hexdigest(),
                   "pri_starte": True, "pokusy": 0})
        assert akt.apply_pending(log=lambda *_: None) is True
        assert (ak / "program" / "app.py").read_text() == "novy" and (ak / "program" / "kometa" / "x.py").exists()
        assert not (ak / "von.txt").exists() and not (TMP / "von.txt").exists()
        assert akt.apply_pending(log=lambda *_: None) is False and "nedokončila" in akt.status()["chyba"]
        (akt.DIR / "Kometa-99.0.zip").write_bytes(b"podvrh")
        assert akt.apply_pending(log=lambda *_: None) is False and not akt.DIR.exists()

        # archívy: rozpoznanie častí, rozbalenie ZIP (aj rozdeleného), nič mimo priečinka
        import io
        import zipfile
        from kometa import archivy
        assert archivy.recognize(Path("Film.part2.rar")) == ("rar", "Film", 2)
        assert archivy.recognize(Path("Film.7z.003")) == ("7z", "Film", 3)
        assert archivy.recognize(Path("Film.r00")) == ("rar", "Film", 2) and archivy.recognize(Path("a.mkv")) is None
        ad = TMP / "archivy"
        ad.mkdir()
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("Kurz/a.txt", "a")
            z.writestr("Kurz/b.txt", "b")
            z.writestr("../von.txt", "zlo")
        (ad / "Balik.zip").write_bytes(buf.getvalue())
        cesta, n = archivy.extract(ad / "Balik.zip")
        assert cesta == ad / "Kurz" and n == 2 and (ad / "Kurz" / "b.txt").read_text() == "b", (cesta, n)
        assert not (TMP / "von.txt").exists() and not (ad / "von.txt").exists()
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w") as z:
            z.writestr("Film.mkv", b"x" * 5000)
        d = buf.getvalue()
        (ad / "Velky.zip.001").write_bytes(d[:3000])
        try:
            archivy.extract(ad / "Velky.zip.001")
            raise AssertionError("rozbalil neúplný archív")
        except archivy.ArchiveError:
            pass
        (ad / "Velky.zip.002").write_bytes(d[3000:])
        cesta, n = archivy.extract(ad / "Velky.zip.002")
        assert cesta == ad / "Film.mkv" and cesta.read_bytes() == b"x" * 5000, cesta

        # vymazať stiahnuté súbory: len súbory z frontu, do Koša; prázdny priečinok kanála zmizne
        out = TMP / "Kometa" / "Kanál"
        out.mkdir(parents=True, exist_ok=True)
        (out / "video.mp4").write_bytes(b"x" * 1000)
        (TMP / "Kometa" / "cudzi.txt").write_text("ostane")
        jadro.dq.items["h1"] = {"id": "h1", "url": "https://example.com/v", "source": "Web", "preset": "video",
                                "outdir": str(out), "title": "video", "status": "hotovo", "done": 1000, "total": 1000,
                                "speed": 0, "file": str(out / "video.mp4"), "dest": "", "tmp": [], "error": ""}
        jadro.dq.order.append("h1")
        assert api("subory_na_zmazanie", None)["data"] == {"pocet": 1, "velkost": 1000}
        r = api("zmazat_subory", ["h1"])["data"]
        assert r == {"zmazane": 1, "chyby": []}, r
        assert not out.exists() and (TMP / "Kometa" / "cudzi.txt").exists() and api("stav")["data"]["polozky"] == []
        jadro.dq.start_all()
        nast = api("ulozit_nastavenia", {"tema": "les", "rezim": "noc", "parallel": 99, "neznamy": 1})["data"]
        assert nast["tema"] == "les" and nast["rezim"] == "noc" and nast["parallel"] == 6 and "neznamy" not in nast
        assert api("ulozit_nastavenia", {"tema": "neexistuje"})["data"]["tema"] == "kometa"
        assert not api("neexistujuca_akcia")["ok"]
        try:
            api("init", kluc="zly")
            raise AssertionError("server prijal zlý kľúč")
        except urllib.error.HTTPError as e:
            assert e.code == 403
        assert urllib.request.urlopen(base + "index.html").status == 200
        try:
            urllib.request.urlopen(base + "../webapp.py")
            raise AssertionError("server vydal súbor mimo rozhrania")
        except urllib.error.HTTPError as e:
            assert e.code == 404
        assert webapp.yt_thumb("https://youtu.be/uL9-e9OlLfw").endswith("/uL9-e9OlLfw/mqdefault.jpg")
        api_obj = webapp.Api(jadro)
        verejne = {m for m in dir(api_obj) if not m.startswith("_")}
        assert verejne == set(webapp.API_METHODS), verejne ^ set(webapp.API_METHODS)
        print("VŠETKO OK")
        return 0
    finally:
        jadro.zavriet()
        srv.shutdown()


if __name__ == "__main__":
    sys.exit(main())
