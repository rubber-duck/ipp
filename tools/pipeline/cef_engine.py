"""Package the optional macOS CEF engine against one verified shared runtime."""

from pathlib import Path
import plistlib
import shutil
import sys

from . import cef
from .model import ROOT
from .processes import run


def build(directory: Path) -> None:
    if sys.platform != "darwin":
        raise RuntimeError("The native CEF engine currently implements macOS only")
    with cef.installation() as runtime:
        build = ROOT / "target/cef-engine-build"
        run(
            [
                "cmake",
                "-S",
                str(ROOT / "crates/ipp-server/cef"),
                "-B",
                str(build),
                "-G",
                "Ninja",
                f"-DCEF_ROOT={runtime}",
                "-DCMAKE_BUILD_TYPE=Release",
            ]
        )
        run(["cmake", "--build", str(build), "-j", "8"])
        shutil.copy2(build / "libipp-cef.dylib", directory / "libipp-cef.dylib")
        bundle = directory / "IppCEF.app"
        for suffix in (
            "",
            " Helper",
            " Helper (GPU)",
            " Helper (Renderer)",
            " Helper (Plugin)",
        ):
            app = (
                bundle
                if not suffix
                else bundle / "Contents/Frameworks" / f"IppCEF{suffix}.app"
            )
            executable = "IppCEF" + suffix
            (app / "Contents/MacOS").mkdir(parents=True, exist_ok=True)
            identifier = (
                "org.ipp.cef"
                + suffix.replace(" ", "").replace("(", "").replace(")", "").lower()
            )
            (app / "Contents/Info.plist").write_bytes(
                plistlib.dumps(
                    {
                        "CFBundleIdentifier": identifier,
                        "CFBundleName": executable,
                        "CFBundleExecutable": executable,
                        "CFBundlePackageType": "APPL",
                        "NSHighResolutionCapable": True,
                        "LSUIElement": bool(suffix),
                    }
                )
            )
            if suffix:
                shutil.copy2(
                    build / "ipp-cef-helper", app / "Contents/MacOS" / executable
                )
        framework = bundle / "Contents/Frameworks/Chromium Embedded Framework.framework"
        framework.symlink_to(runtime / framework.name)
        (directory / "runtime-path").write_text(str(runtime) + "\n")
        (directory / "runtime-lease").write_text(
            str(cef.cache_root() / "locks" / f"{cef.identity(cef.PIN)}.lock") + "\n"
        )
