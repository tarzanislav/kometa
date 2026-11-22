"""Rýchla skúška nového rozhrania bez siete: jadro, lokálny server s kľúčom, API a súbory rozhrania.

Spustenie:  python tests/test_webapp.py
"""
import json
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
        assert not api("pridat", "bez odkazu")["ok"]
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
