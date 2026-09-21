"""Install the built extension and start it using only its bundled dependencies."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main():
    binary = os.environ.get("BLENDER_BIN") or shutil.which("blender")
    if not binary:
        raise SystemExit("Blender 5.2 LTS is required")
    archive = ROOT / "target/blender/ipp_blender-0.1.0-linux-x64.zip"
    evidence = ROOT / "target/integration-artifacts/blender-package"
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ipp-blender-package-") as temporary:
        directory = Path(temporary)
        environment = dict(
            os.environ,
            BLENDER_USER_CONFIG=str(directory / "config"),
            BLENDER_USER_EXTENSIONS=str(directory / "extensions"),
        )
        environment.pop("PYTHONPATH", None)
        code = "\n".join(
            [
                "import json, pathlib, aiohttp, cryptography",
                "import bl_ext.ipp_test.ipp_blender as addon",
                f"directory = pathlib.Path({str(directory)!r})",
                "assert pathlib.Path(aiohttp.__file__).is_relative_to(directory)",
                "assert pathlib.Path(cryptography.__file__).is_relative_to(directory)",
                "server = addon.start(port=0, directory=directory / 'credentials')",
                "assert json.loads(server.snapshot)['scene']['entities']",
                "addon.stop()",
                "assert server.closed and server.loop.is_closed()",
                "print('INSTALLED_EXTENSION_PASS')",
            ]
        )
        commands = [
            [
                "--background",
                "--factory-startup",
                "--command",
                "extension",
                "validate",
                str(archive),
            ],
            [
                "--background",
                "--factory-startup",
                "--command",
                "extension",
                "repo-add",
                "--directory",
                str(directory / "repo"),
                "ipp_test",
            ],
            [
                "--background",
                "--command",
                "extension",
                "install-file",
                "--repo",
                "ipp_test",
                "--enable",
                str(archive),
            ],
            ["--background", "--python-exit-code", "1", "--python-expr", code],
        ]
        for index, arguments in enumerate(commands):
            result = subprocess.run(
                [binary, *arguments],
                env=environment,
                capture_output=True,
                text=True,
                timeout=45,
            )
            (evidence / f"{index}.log").write_text(result.stdout + result.stderr)
            if result.returncode:
                raise AssertionError(
                    f"Extension validation failed: {evidence / f'{index}.log'}"
                )
        assert "INSTALLED_EXTENSION_PASS" in result.stdout
    report = {
        "installed_extension": "passed",
        "bundled_dependencies": "passed",
        "cleanup": "passed",
        "archive_bytes": archive.stat().st_size,
    }
    (evidence / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report))


if __name__ == "__main__":
    main()
