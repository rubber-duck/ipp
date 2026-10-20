"""Real shared-cache publication, concurrent setup, and offline Chrome reuse."""

from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pipeline import browser


def chrome_archive(version="155.0.8059.84"):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        executable = zipfile.ZipInfo("chrome-linux64/chrome")
        executable.external_attr = 0o100755 << 16
        archive.writestr(
            executable, f'#!/bin/sh\necho "Google Chrome for Testing {version}"\n'
        )
        for name in ("resources.pak", "icudtl.dat"):
            archive.writestr(f"chrome-linux64/{name}", b"fixture" * 20000)
    return output.getvalue()


@contextmanager
def installation_fixture(archive=None, gate=None):
    requests = []
    requested = threading.Event()
    payload = archive if archive is not None else chrome_archive()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            requested.set()
            if gate is not None and not gate.wait(10):
                self.send_error(504)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *_args):
            pass

    with (
        tempfile.TemporaryDirectory() as temporary,
        ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server,
    ):
        root = Path(temporary)
        tools = root / "tools/pipeline"
        tools.mkdir(parents=True)
        pin = {
            "version": "155.0.8059.84",
            "revision": "1697595",
            "resolvedAt": "original feed",
            "source": "authoritative feed",
            "downloads": {
                "linux64": f"http://127.0.0.1:{server.server_port}/chrome.zip"
            },
        }
        (tools / "toolchains.json").write_text(json.dumps({"chromeForTesting": pin}))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with (
                patch.object(browser, "ROOT", root),
                patch.object(browser, "browser_cache", return_value=root / "cache"),
                patch.object(browser, "browser_platform", return_value="linux64"),
            ):
                yield root, pin, requests, requested
        finally:
            if gate is not None:
                gate.set()
            server.shutdown()
            thread.join(3)


@unittest.skipUnless(
    sys.platform == "linux", "Executable fixture and process locking run on Linux"
)
class BrowserInstallationTests(unittest.TestCase):
    def test_warm_offline_reuse_ignores_unrelated_feed_updates(self):
        with installation_fixture() as (root, pin, requests, _):
            executable = browser.install_browser()
            self.assertTrue((executable.parent.parent / "chrome.zip").is_file())
            pin["resolvedAt"] = "new feed timestamp"
            pin["downloads"]["mac-arm64"] = (
                "https://example.invalid/another-platform.zip"
            )
            (root / "tools/pipeline/toolchains.json").write_text(
                json.dumps({"chromeForTesting": pin})
            )
            with patch.object(
                browser.urllib.request,
                "urlopen",
                side_effect=AssertionError("Warm installation accessed the network"),
            ):
                self.assertEqual(browser.install_browser(), executable)
            self.assertEqual(len(requests), 1)
            resource = executable.parent / "resources.pak"
            with resource.open("r+b") as stream:
                stream.seek(70000)
                stream.write(b"changed")
            os.utime(
                resource,
                ns=(resource.stat().st_atime_ns, resource.stat().st_mtime_ns + 1),
            )
            with self.assertRaisesRegex(ValueError, "Unverified Chrome installation"):
                browser.install_browser()
            self.assertEqual(len(requests), 1)

    def test_interrupted_validation_does_not_publish_partial_install(self):
        with installation_fixture(archive=chrome_archive("wrong-version")) as (
            root,
            _,
            requests,
            _,
        ):
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                browser.install_browser()
            self.assertFalse((root / "cache/155.0.8059.84-linux64").exists())
            self.assertEqual(
                [path.suffix for path in (root / "cache").iterdir()], [".lock"]
            )
            self.assertEqual(len(requests), 1)

    def test_two_processes_wait_then_reuse_one_download(self):
        gate = threading.Event()
        with installation_fixture(gate=gate) as (root, _, requests, requested):
            module_path = str(Path(browser.__file__).resolve().parents[1])
            script = (
                "import sys;from pathlib import Path;"
                f"sys.path.insert(0,{module_path!r});from pipeline import browser;"
                f"browser.ROOT=Path({str(root)!r});"
                f"browser.browser_cache=lambda:Path({str(root / 'cache')!r});"
                "browser.browser_platform=lambda:'linux64';browser.install_browser()"
            )
            processes = []
            try:
                first = subprocess.Popen(
                    [sys.executable, "-c", script],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                processes.append(first)
                self.assertTrue(
                    requested.wait(5), "First process did not start downloading"
                )
                second = subprocess.Popen(
                    [sys.executable, "-c", script],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                )
                processes.append(second)
                readable, _, _ = select.select([second.stdout], [], [], 5)
                self.assertTrue(readable, "Second process did not report waiting")
                self.assertIn(
                    "Waiting for the shared Chrome installation",
                    second.stdout.readline(),
                )
                gate.set()
                for process in processes:
                    stdout, stderr = process.communicate(timeout=10)
                    self.assertEqual(process.returncode, 0, stdout + stderr)
                self.assertEqual(len(requests), 1)
                self.assertTrue(
                    browser.verified_installation(
                        root / "cache/155.0.8059.84-linux64",
                        json.loads(
                            (root / "tools/pipeline/toolchains.json").read_text()
                        )["chromeForTesting"],
                        "linux64",
                    )
                )
            finally:
                gate.set()
                for process in processes:
                    if process.poll() is None:
                        process.kill()
                    process.communicate(timeout=5)

    def test_archive_preserves_bundle_symlinks_and_rejects_escape(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "fixture.zip"
            with zipfile.ZipFile(archive, "w") as source:
                source.writestr("bundle/Versions/155/data", b"actual data")
                link = zipfile.ZipInfo("bundle/Versions/Current")
                link.external_attr = 0o120777 << 16
                source.writestr(link, "155")
            browser.extract_browser(archive, root)
            self.assertEqual(
                (root / "bundle/Versions/Current/data").read_bytes(), b"actual data"
            )
            with zipfile.ZipFile(archive, "w") as source:
                link = zipfile.ZipInfo("escape")
                link.external_attr = 0o120777 << 16
                source.writestr(link, "../outside")
            with self.assertRaisesRegex(ValueError, "Unsafe Chrome archive symlink"):
                browser.extract_browser(archive, root)

    def test_shared_lock_wait_is_bounded(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "install.lock"
            with browser.installation_lock(path):
                with self.assertRaisesRegex(ValueError, "Timed out waiting"):
                    with browser.installation_lock(path, timeout=0.02):
                        self.fail("A second lock must not be admitted")


if __name__ == "__main__":
    unittest.main()
