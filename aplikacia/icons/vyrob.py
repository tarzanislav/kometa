"""Vyrobí ikony aplikácie z ikona.svg (prehliadač Chromium cez Playwright + Pillow).

    python aplikacia/icons/vyrob.py
"""
from pathlib import Path

from PIL import Image
from playwright.sync_api import sync_playwright

D = Path(__file__).resolve().parent


def main() -> None:
    svg = (D / "ikona.svg").read_text(encoding="utf-8")
    with sync_playwright() as p:
        b = p.chromium.launch()
        s = b.new_page(viewport={"width": 1024, "height": 1024})
        s.set_content(f"<html><body style='margin:0;background:transparent'>{svg}</body></html>")
        s.locator("svg").screenshot(path=str(D / "icon.png"), omit_background=True)
        b.close()
    z = Image.open(D / "icon.png").convert("RGBA")
    for meno, r in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
        z.resize((r, r), Image.LANCZOS).save(D / meno)
    z.save(D / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    z.save(D / "icon.icns")


if __name__ == "__main__":
    main()
