"""Build or exercise the isolated CEF/GLES prototype, retaining completed frames."""

import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
import zlib

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))
from pipeline import cef

MANIFEST = ROOT / "tests/rendering/cef/Cargo.toml"
TARGET = ROOT / "target/cef-prototype"
BINARY = TARGET / "debug/ipp-cef-surface-prototype"
# Five 12s frame waits + 8s navigation + 5s browser closure, plus 27s
# initialization/scheduling allowance. Display startup and process cleanup each
# have separate budgets and are outside this browser deadline.
BROWSER_TIMEOUT = 100


def group_alive(group):
    # CEF may outlive its process leader. Linux zombies have already released
    # runtime mappings; waiting for an unrelated reaper would not add safety.
    for path in Path("/proc").glob("[0-9]*/stat"):
        try:
            fields = path.read_text().rsplit(")", 1)[1].split()
            if int(fields[2]) == group and fields[0] not in ("Z", "X"):
                return True
        except FileNotFoundError, ProcessLookupError, PermissionError:
            continue
    return False


def stop(process, grace=5):
    if process is None:
        return
    # Stop the complete group even if wait()/poll() already reaped its leader.
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            process.wait(timeout=grace)
            return
        deadline = time.monotonic() + grace
        while time.monotonic() < deadline:
            process.poll()
            if not group_alive(process.pid):
                process.wait(timeout=grace)
                return
            time.sleep(0.025)
    raise RuntimeError(f"Owned process group {process.pid} survived SIGKILL")


def png(path):
    data = path.read_bytes()
    if len(data) != 800 * 600 * 4:
        raise ValueError(f"Bad capture size: {path}")

    def chunk(kind, payload):
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload))
        )

    rows = b"".join(b"\0" + data[y * 3200 : (y + 1) * 3200] for y in range(600))
    path.with_suffix(".png").write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 800, 600, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )


def weston_config(root, env):
    if not root:
        executable = shutil.which("weston")
        if not executable:
            raise RuntimeError("Weston is unavailable; set IPP_CEF_WESTON_ROOT")
        return executable, []
    root = Path(root)
    libs = root / "usr/lib64"
    env["WESTON_MODULE_MAP"] = ";".join(
        f"{p.name}={p}"
        for p in [*libs.glob("libweston-*/**/*.so"), *(libs / "weston").glob("*.so")]
    )
    return str(root / "usr/bin/weston"), [str(libs), str(libs / "weston")]


def probe(args):
    if sys.platform != "linux":
        raise RuntimeError("CEF DMA-BUF prototype requires Linux")
    env = os.environ.copy()
    executable, libraries = weston_config(args.weston_root, env)
    env["LD_LIBRARY_PATH"] = ":".join([*libraries, env.get("LD_LIBRARY_PATH", "")])
    help_text = subprocess.check_output(
        [executable, "--help"], env=env, text=True, timeout=10
    )
    if "--fake-seat" not in help_text:
        raise RuntimeError("Weston must support --fake-seat for Chromium shutdown")
    print(
        "Weston executable/--fake-seat found; headless GL, kiosk shell and DMA-BUF import are verified only by the scenario"
    )


def cargo(args):
    with cef.installation(offline=args.offline) as runtime:
        package = cef.patched_package(offline=args.offline)
        env = {**os.environ, "CEF_PATH": str(runtime), "IPP_CEF_EXTERNAL_RUNTIME": "1"}
        options = [
            "--config",
            f"patch.crates-io.cef-dll-sys.path={json.dumps(str(package))}",
            "--manifest-path",
            str(MANIFEST),
            *(["--offline"] if args.offline else []),
        ]
        if args.relock:
            subprocess.run(
                ["cargo", "update", "--workspace", *options],
                cwd=ROOT,
                env=env,
                check=True,
            )
            root_versions = {}
            for record in tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]:
                if record.get("source", "").startswith("registry+"):
                    root_versions.setdefault(record["name"], set()).add(
                        record["version"]
                    )
            # Preserve the root's tested resolution for unambiguous shared crates.
            # Incompatible requirements fail explicitly for review, never silently
            # substitute a different root dependency or change the root lockfile.
            for name, versions in root_versions.items():
                if len(versions) != 1:
                    continue
                expected = next(iter(versions))
                records = tomllib.loads(MANIFEST.with_name("Cargo.lock").read_text())[
                    "package"
                ]
                for record in records:
                    if (
                        record["name"] == name
                        and record["version"] != expected
                        and record.get("source", "").startswith("registry+")
                    ):
                        subprocess.run(
                            [
                                "cargo",
                                "update",
                                *options,
                                "--package",
                                f"{name}@{record['version']}",
                                "--precise",
                                expected,
                            ],
                            cwd=ROOT,
                            env=env,
                            check=True,
                        )
            return
        command = [
            "cargo",
            "clippy" if args.clippy else "build",
            "--locked",
            *options,
            "--target-dir",
            str(TARGET),
        ]
        if args.clippy:
            command.extend(["--", "-D", "warnings"])
        subprocess.run(command, cwd=ROOT, env=env, check=True)


def wait_socket(process, path, name):
    deadline = time.monotonic() + 10
    while not path.exists():
        if process.poll() is not None or time.monotonic() > deadline:
            raise RuntimeError(f"{name} failed to start; inspect its retained log")
        time.sleep(0.05)


def launch(command, env, log):
    with log.open("w") as output:
        return subprocess.Popen(
            command,
            cwd=ROOT,
            env=env,
            stdout=output,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )


def exercise(args):
    evidence = (
        args.evidence
        or ROOT
        / "target/cef-surface"
        / f"{time.strftime('%Y%m%d-%H%M%S')}-{time.time_ns() % 1_000_000_000:09d}"
    ).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    print(f"Evidence: {evidence}", flush=True)
    result = {
        "passed": False,
        "stage": "setup",
        "error": None,
        "cleanup_errors": [],
        "evidence": str(evidence),
    }
    processes = []
    stack = ExitStack()
    display_dir = None
    try:
        if not BINARY.is_file():
            raise RuntimeError(
                "Build first: python tools/ipp.py build cef-surface-prototype"
            )
        runtime = stack.enter_context(cef.installation(offline=args.offline))
        display_dir = Path(tempfile.mkdtemp(prefix="ipp-cef-"))
        env = os.environ.copy()
        for key in (
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "DBUS_SESSION_BUS_ADDRESS",
            "DBUS_SYSTEM_BUS_ADDRESS",
        ):
            env.pop(key, None)
        address = f"unix:path={display_dir / 'bus'}"
        env.update(
            IPP_CEF_RUNTIME=str(runtime),
            IPP_CEF_EVIDENCE=str(evidence),
            IPP_EGL_LIBRARY_DIR=args.egl_dir,
            XDG_RUNTIME_DIR=str(display_dir),
            DBUS_SESSION_BUS_ADDRESS=address,
            DBUS_SYSTEM_BUS_ADDRESS=address,
        )
        # No D-Bus services are needed by this local fixture. Explicit private
        # addresses prevent Chromium from discovering the desktop/system bus.
        libraries = []
        if not args.headless:
            executable, libraries = weston_config(args.weston_root, env)
            display_env = {
                **env,
                "LD_LIBRARY_PATH": ":".join(
                    [*libraries, env.get("LD_LIBRARY_PATH", "")]
                ),
            }
            weston = launch(
                [
                    executable,
                    "--backend=headless",
                    "--renderer=gl",
                    "--fake-seat",
                    "--shell=kiosk-shell.so",
                    "--socket=ipp-cef",
                    "--no-config",
                ],
                display_env,
                evidence / "weston.log",
            )
            processes.append(weston)
            wait_socket(weston, display_dir / "ipp-cef", "Weston")
            env["WAYLAND_DISPLAY"] = "ipp-cef"
        env["LD_LIBRARY_PATH"] = ":".join(
            [str(runtime), *libraries, env.get("LD_LIBRARY_PATH", "")]
        )
        command = [
            str(BINARY),
            f"--ozone-platform={'headless' if args.headless else 'wayland'}",
            "--enable-gpu",
            "--use-gl=angle",
            "--use-angle=gl-egl",
            "--password-store=basic",
        ]
        (evidence / "environment.json").write_text(
            json.dumps(
                {
                    "command": command,
                    "cef": str(runtime),
                    "cef_identity": cef.identity(cef.PIN),
                    "egl": args.egl_dir,
                    "viewport": [800, 600],
                    "browser_timeout_seconds": BROWSER_TIMEOUT,
                    "prototype_sha256": hashlib.sha256(
                        b"".join(
                            p.relative_to(MANIFEST.parent).as_posix().encode()
                            + b"\0"
                            + p.read_bytes()
                            for p in sorted(MANIFEST.parent.rglob("*"))
                            if p.is_file() and "__pycache__" not in p.parts
                        )
                    ).hexdigest(),
                    "source": subprocess.check_output(
                        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
                    ).strip(),
                    "dirty": subprocess.check_output(
                        ["git", "status", "--short"], cwd=ROOT, text=True
                    ),
                },
                indent=2,
            )
            + "\n"
        )
        result["stage"] = "browser"
        browser = launch(command, env, evidence / "run.log")
        processes.append(browser)
        code = browser.wait(timeout=BROWSER_TIMEOUT)
        result["exit_code"] = code
        if code:
            raise RuntimeError(f"CEF prototype failed with exit {code}")
        producer = json.loads((evidence / "result.json").read_text())
        result["producer"] = producer
        if (
            not producer["passed"]
            or not producer["closed"]
            or producer["cpu_frames"] != 0
        ):
            raise RuntimeError(f"Invalid producer result: {producer}")
        result["passed"] = True
        result["stage"] = "complete"
    except BaseException as error:
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        for process in reversed(processes):
            try:
                stop(process)
            except Exception as error:
                result["cleanup_errors"].append(f"process cleanup: {error}")
        # Keep the shared runtime lease until every owned consumer has stopped.
        stack.close()
        if display_dir is not None:
            try:
                shutil.rmtree(display_dir)
            except OSError as error:
                result["cleanup_errors"].append(f"display cleanup: {error}")
        for capture in evidence.glob("*.rgba"):
            try:
                png(capture)
            except Exception as error:
                result["cleanup_errors"].append(f"capture conversion: {error}")
        if result["cleanup_errors"]:
            result["passed"] = False
        (evidence / "runner-result.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
        if (evidence / "run.log").exists():
            print((evidence / "run.log").read_text(errors="replace"), flush=True)
        print(f"Evidence: {evidence}", flush=True)
    if not result["passed"]:
        raise RuntimeError(
            f"{result['error'] or result['cleanup_errors']}; evidence: {evidence}"
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--build", action="store_true")
    action.add_argument("--clippy", action="store_true")
    action.add_argument(
        "--relock",
        action="store_true",
        help="Refresh standalone path dependencies and align unambiguous shared registry versions to the root lockfile",
    )
    action.add_argument("--probe", action="store_true")
    action.add_argument("--setup", action="store_true")
    action.add_argument(
        "--prune",
        action="store_true",
        help="Explicitly prune this pinned managed installation if no consumers hold it; adopted data stays intact",
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="Forbid CEF acquisition and ask Cargo for offline crate/index resolution",
    )
    parser.add_argument(
        "--egl-dir", default=os.environ.get("IPP_EGL_LIBRARY_DIR", "/usr/lib64")
    )
    parser.add_argument("--weston-root", default=os.environ.get("IPP_CEF_WESTON_ROOT"))
    parser.add_argument("--evidence", type=Path)
    parser.add_argument(
        "--headless",
        action="store_true",
        help="Diagnose Chromium's headless platform; unavailable is a failure",
    )
    args = parser.parse_args()
    if sys.platform != "linux":
        raise RuntimeError("CEF DMA-BUF prototype requires Linux")
    if args.probe:
        probe(args)
    elif args.prune:
        cef.prune()
    elif args.setup:
        with cef.installation(offline=args.offline) as runtime:
            print(f"CEF runtime: {runtime}")
            print(f"Patched bindings: {cef.patched_package(offline=args.offline)}")
    elif args.build or args.clippy or args.relock:
        cargo(args)
    else:
        exercise(args)


if __name__ == "__main__":
    main()
