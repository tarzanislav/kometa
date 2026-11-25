"""Rozbaľovanie archívov po stiahnutí (Kometa pre Python).

ZIP (aj zaheslovaný ZipCrypto) zvládne sama cez `zipfile`. 7z, RAR a ZIP šifrovaný AES rozbalí
cez 7-Zip, ak je nainštalovaný (Instalacia.bat ho doinštaluje). Rozdelené archívy: `.partN.rar`,
`.rNN`, `.7z.001`, `.zip.001`. Nič sa nezapíše mimo cieľového priečinka.
"""
from __future__ import annotations

import os
import re
import shutil
import subprocess
import zipfile
from pathlib import Path, PurePosixPath


class PasswordError(Exception):
    """Archív je zaheslovaný: wrong=False – heslo chýba, wrong=True – heslo nesedí."""

    def __init__(self, wrong: bool):
        super().__init__("Heslo k archívu nesedí – skús ho znova." if wrong else "Archív je chránený heslom – zadaj ho.")
        self.wrong = wrong


class ArchiveError(Exception):
    pass


_PART_RAR = re.compile(r"^(?P<name>.+)\.part(?P<n>\d+)\.rar$", re.I)
_OLD_RAR = re.compile(r"^(?P<name>.+)\.r(?P<n>\d\d)$", re.I)
_SPLIT = re.compile(r"^(?P<name>.+)\.(?P<kind>7z|zip)\.(?P<n>\d{3})$", re.I)


def recognize(path: Path) -> tuple[str, str, int] | None:
    """(druh, meno skupiny, číslo časti) – druh je „zip“, „7z“ alebo „rar“."""
    n = path.name
    if m := _PART_RAR.match(n):
        return "rar", m["name"], max(1, int(m["n"]))
    if m := _OLD_RAR.match(n):
        return "rar", m["name"], int(m["n"]) + 2
    if m := _SPLIT.match(n):
        return m["kind"].lower(), m["name"], int(m["n"])
    low = n.lower()
    for ext, kind in ((".rar", "rar"), (".zip", "zip"), (".7z", "7z")):
        if low.endswith(ext):
            return kind, n[: -len(ext)], 1
    return None


def is_split(path: Path) -> bool:
    return bool(_PART_RAR.match(path.name) or _OLD_RAR.match(path.name) or _SPLIT.match(path.name))


def parts(path: Path) -> list[Path]:
    r = recognize(path)
    if not r:
        return []
    found = {}
    for p in path.parent.iterdir():
        x = recognize(p)
        if x and x[0] == r[0] and x[1] == r[1] and p.is_file():
            found.setdefault(x[2], p)
    return [found[k] for k in sorted(found)]


def safe_rel(name: str) -> Path | None:
    """Bezpečná relatívna cesta z archívu – žiadne `..`, koreň ani písmeno disku."""
    p = PurePosixPath(name.replace("\\", "/"))
    if p.is_absolute() or any(x in ("..", "") or ":" in x for x in p.parts if x != "."):
        return None
    out = [x for x in p.parts if x != "."]
    return Path(*out) if out else None


def free_name(folder: Path, name: str) -> Path:
    c = folder / name
    if not c.exists():
        return c
    stem, ext = (Path(name).stem, Path(name).suffix) if "." in name and not c.is_dir() else (name, "")
    i = 2
    while (folder / f"{stem} ({i}){ext}").exists():
        i += 1
    return folder / f"{stem} ({i}){ext}"


def seven_zip() -> str | None:
    for name in ("7z", "7zz", "7za"):
        if exe := shutil.which(name):
            return exe
    for base in (os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)")):
        if base and (p := Path(base) / "7-Zip" / "7z.exe").is_file():
            return str(p)
    return None


class _Joined:
    """Časti .zip.001, .zip.002 … čítané ako jeden súbor (pre zipfile)."""

    def __init__(self, files: list[Path]):
        self.fs = [open(f, "rb") for f in files]
        self.sizes = [f.stat().st_size for f in files]
        self.total, self.pos = sum(self.sizes), 0

    def seekable(self):
        return True

    def seek(self, off, whence=0):
        self.pos = off if whence == 0 else self.pos + off if whence == 1 else self.total + off
        return self.pos

    def tell(self):
        return self.pos

    def read(self, n=-1):
        out = bytearray()
        want = self.total - self.pos if n is None or n < 0 else n
        while want > 0 and self.pos < self.total:
            start = 0
            for f, size in zip(self.fs, self.sizes):
                if self.pos < start + size:
                    f.seek(self.pos - start)
                    chunk = f.read(min(want, start + size - self.pos))
                    out += chunk
                    self.pos += len(chunk)
                    want -= len(chunk)
                    break
                start += size
        return bytes(out)

    def close(self):
        for f in self.fs:
            f.close()


def _zip(files: list[Path], dest: Path, password: str) -> int:
    src = _Joined(files) if len(files) > 1 else open(files[0], "rb")
    try:
        try:
            z = zipfile.ZipFile(src)
        except zipfile.BadZipFile as e:
            raise ArchiveError(f"ZIP sa nedá otvoriť: {e}") from e
        count = 0
        with z:
            for info in z.infolist():
                rel = safe_rel(info.filename)
                if rel is None:
                    continue
                target = dest / rel
                if info.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                if info.flag_bits & 0x1 and not password:
                    raise PasswordError(False)
                target.parent.mkdir(parents=True, exist_ok=True)
                try:
                    with z.open(info, pwd=password.encode() if password else None) as r, open(target, "wb") as w:
                        shutil.copyfileobj(r, w, 1 << 20)
                except RuntimeError as e:   # zlé heslo / nepodporované šifrovanie
                    if "password" in str(e).lower() or "encrypted" in str(e).lower():
                        raise PasswordError(bool(password)) from e
                    raise ArchiveError(f"ZIP: {e}") from e
                except NotImplementedError as e:   # AES – zvládne 7-Zip
                    raise _Need7z() from e
                except zipfile.BadZipFile as e:
                    if info.flag_bits & 0x1:
                        raise PasswordError(True) from e
                    raise ArchiveError(f"ZIP je poškodený: {e}") from e
                count += 1
        return count
    finally:
        src.close()


class _Need7z(Exception):
    pass


def _7z(first: Path, dest: Path, password: str) -> int:
    exe = seven_zip()
    if not exe:
        raise ArchiveError("Na tento archív treba 7-Zip – nainštaluj ho (alebo spusti Instalacia.bat).")
    # -p bez hesla: 7-Zip sa nepýta interaktívne; -snl zakáže symbolické odkazy (nevyjdú von)
    cmd = [exe, "x", "-y", "-bd", f"-p{password}", f"-o{dest}", str(first)]
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    out = (r.stdout + r.stderr).lower()
    if r.returncode != 0:
        if "wrong password" in out or "can not open encrypted archive" in out:
            raise PasswordError(bool(password))
        if "missing volume" in out or "unexpected end of archive" in out:
            raise ArchiveError("Chýba časť rozdeleného archívu – stiahni všetky časti.")
        raise ArchiveError("7-Zip: " + ((r.stderr or r.stdout).strip().splitlines() or ["chyba"])[-1])
    # pre istotu: nič mimo cieľa (7-Zip cesty s .. sám odstraňuje)
    return sum(1 for p in dest.rglob("*") if p.is_file())


def extract(archive: Path, password: str = "") -> tuple[Path, int]:
    """Rozbalí archív vedľa neho; jediný súbor/priečinok dá rovno vedľa archívu, inak do priečinka
    s menom archívu. Vráti (výsledok, počet súborov)."""
    r = recognize(archive)
    if not r:
        raise ArchiveError("Toto nie je archív.")
    kind, name, _ = r
    files = parts(archive)
    if not files:
        raise ArchiveError("Archív sa nenašiel.")
    if kind != "rar":
        for i, p in enumerate(files, 1):
            if (recognize(p) or ("", "", 0))[2] != i:
                raise ArchiveError(f"Chýba časť {i} rozdeleného archívu – stiahni všetky časti.")
    folder = archive.parent
    tmp = free_name(folder, f"{name}.rozbaluje")
    tmp.mkdir(parents=True)
    try:
        if kind == "zip":
            try:
                count = _zip(files, tmp, password)
            except _Need7z:
                shutil.rmtree(tmp, ignore_errors=True)
                tmp.mkdir(parents=True)
                count = _7z(files[0], tmp, password)
        else:
            count = _7z(files[0], tmp, password)
    except BaseException:
        shutil.rmtree(tmp, ignore_errors=True)
        raise
    content = list(tmp.iterdir())
    if len(content) == 1:
        target = free_name(folder, content[0].name)
        content[0].rename(target)
        tmp.rmdir()
    else:
        target = free_name(folder, name)
        tmp.rename(target)
    return target, count
