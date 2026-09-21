#!/usr/bin/env python3
"""Build, launch and obtain reproducible inputs for the Blender integration."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import zipfile

from pipeline.processes import blender

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target/blender"
SOURCE = ROOT / "integrations/blender/ipp_blender"


def prepare():
    if (platform.system(), platform.machine()) != ("Linux", "x86_64"):
        raise SystemExit(
            "The initial wheel lock targets Linux x64 / Blender Python 3.13"
        )
    wheels = OUTPUT / "wheels"
    wheels.mkdir(parents=True, exist_ok=True)
    lock = json.loads((ROOT / "integrations/blender/wheels-linux-x64.json").read_text())
    if not all((wheels / name).is_file() for name in lock):
        subprocess.run(
            [
                sys.executable,
                "-m",
                "pip",
                "download",
                "--dest",
                str(wheels),
                "--python-version",
                "3.13",
                "--only-binary=:all:",
                "-r",
                str(ROOT / "integrations/blender/requirements.txt"),
            ],
            check=True,
        )
    for name, digest in lock.items():
        path = wheels / name
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise SystemExit(f"Wheel checksum mismatch: {name}")
    dependencies = OUTPUT / "dependencies"
    dependencies.mkdir(parents=True, exist_ok=True)
    for name in lock:
        with zipfile.ZipFile(wheels / name) as archive:
            archive.extractall(dependencies)
    return lock


def package():
    lock = prepare()
    manifest = (SOURCE / "blender_manifest.toml").read_text()
    # Top-level wheel/platform keys must precede the permissions table.
    metadata = (
        'platforms = ["linux-x64"]\nwheels = [\n'
        + "".join(f'  "./wheels/{name}",\n' for name in lock)
        + "]\n\n"
    )
    manifest = manifest.replace("[permissions]", metadata + "[permissions]")
    destination = OUTPUT / "ipp_blender-0.1.0-linux-x64.zip"
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in SOURCE.rglob("*"):
            if (
                path.is_file()
                and "__pycache__" not in path.parts
                and path.name != "blender_manifest.toml"
            ):
                archive.write(path, path.relative_to(SOURCE))
        archive.writestr("blender_manifest.toml", manifest)
        for name in lock:
            archive.write(OUTPUT / "wheels" / name, f"wheels/{name}")
    print(destination)


def certificate(args):
    executable = os.environ.get("MKCERT_BIN") or shutil.which("mkcert")
    if not executable:
        raise SystemExit(
            "Install mkcert (https://github.com/FiloSottile/mkcert), then rerun python tools/ipp.py setup certificates. Artists use the addon's automatic certificate instead."
        )
    directory = Path(args.directory).resolve()
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    environment = dict(os.environ)
    environment.setdefault("CAROOT", str(Path.home() / ".local/share/ipp/blender-ca"))
    cert, key = directory / "localhost.pem", directory / "localhost-key.pem"
    if args.install_trust:
        subprocess.run([executable, "-install"], env=environment, check=True)
    if not cert.exists() and not key.exists():
        subprocess.run(
            [
                executable,
                "-cert-file",
                str(cert),
                "-key-file",
                str(key),
                "localhost",
                "127.0.0.1",
                "::1",
            ],
            env=environment,
            check=True,
        )
        key.chmod(0o600)
    if not cert.is_file() or not key.is_file():
        raise SystemExit(
            "Incomplete configured credentials; restore the missing file or select a new directory"
        )
    print(json.dumps({"certificate": str(cert), "private_key": str(key)}, indent=2))
    print(
        "Configure these paths in the addon, or pass --certificate and --private-key after python tools/ipp.py dev blender --args."
    )
    print(
        "For browser trust, rerun python tools/ipp.py setup certificates --install-trust once in the browser's environment."
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("prepare")
    commands.add_parser("package")
    cert = commands.add_parser("certificate")
    cert.add_argument("--directory", default=str(OUTPUT / "certificates"))
    cert.add_argument("--install-trust", action="store_true")
    commands.add_parser("serve", add_help=False)
    args, remaining = parser.parse_known_args()
    if args.command == "serve":
        if not (OUTPUT / "dependencies/aiohttp").is_dir():
            raise SystemExit("Run python tools/ipp.py setup blender first")
        os.execv(
            blender(),
            [
                blender(),
                "--background",
                "--factory-startup",
                "--python-exit-code",
                "1",
                "--python",
                str(ROOT / "tools/blender_runner.py"),
                "--",
                *remaining,
            ],
        )
    if remaining:
        parser.error(f"Unexpected arguments: {remaining}")
    if args.command == "prepare":
        prepare()
    elif args.command == "package":
        package()
    elif args.command == "certificate":
        certificate(args)


if __name__ == "__main__":
    main()
