"""Fetch the shared, checksum-pinned fonts into the ignored build cache."""

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class FontSource:
    version: str
    url: str
    sha256: str
    axes: dict[str, float] | None = None
    instance_sha256: str | None = None


def sources() -> dict[str, FontSource]:
    manifest = json.loads((ROOT / "assets/fonts/sources.json").read_text())
    return {name: FontSource(**entry) for name, entry in manifest.items()}


def checksum(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fetch_font(source: FontSource, destination: Path) -> Path:
    expected = source.instance_sha256 if source.axes else source.sha256
    if not expected:
        raise ValueError("A variable font instance requires a pinned checksum")
    if destination.is_file() and checksum(destination) == expected:
        return destination

    destination.parent.mkdir(parents=True, exist_ok=True)
    print(f"Fetching {destination.name} ({source.version})")
    with tempfile.TemporaryDirectory(
        prefix="font-", dir=destination.parent
    ) as temporary:
        downloaded = Path(temporary) / "source.ttf"
        with (
            urllib.request.urlopen(source.url, timeout=60) as response,
            downloaded.open("wb") as output,
        ):
            shutil.copyfileobj(response, output)
        if checksum(downloaded) != source.sha256:
            raise ValueError(f"Font source checksum mismatch: {source.url}")

        prepared = downloaded
        if source.axes:
            prepared = Path(temporary) / "instance.ttf"
            with TTFont(downloaded, recalcTimestamp=False) as font:
                instantiateVariableFont(font, source.axes, inplace=True)
                font.save(prepared)
        if checksum(prepared) != expected:
            raise ValueError(f"Font instance checksum mismatch: {source.url}")
        prepared.replace(destination)
    return destination


def font_source(name: str) -> Path:
    return fetch_font(sources()[name], ROOT / "target/font-sources" / f"{name}.ttf")
