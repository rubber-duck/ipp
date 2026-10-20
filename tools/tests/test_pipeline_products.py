"""Actual HTTP serving and publication boundaries, independent of graphics setup."""

from contextlib import contextmanager
import gzip
import hashlib
from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
import json
from pathlib import Path
import random
import socket
import struct
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.builds import product
from pipeline.server import handler


@contextmanager
def serving(root, server_handler=None):
    with ThreadingHTTPServer(
        ("127.0.0.1", 0), server_handler or handler("gallery", root)
    ) as server:
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server
        finally:
            server.shutdown()
            thread.join(timeout=3)


def request_product(server, method, path, headers=None):
    connection = HTTPConnection("127.0.0.1", server.server_port, timeout=3)
    try:
        connection.request(method, path, headers=headers or {})
        response = connection.getresponse()
        return response.status, response.headers, response.read()
    finally:
        connection.close()


class ProductTests(unittest.TestCase):
    def test_failed_preparation_preserves_previous_product(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "target/product"
            destination.mkdir(parents=True)
            (destination / "runtime").write_text("verified old product")
            with patch("pipeline.builds.ROOT", root):
                with self.assertRaisesRegex(ValueError, "validation failed"):
                    with product(destination) as staging:
                        (staging / "runtime").write_text("unverified")
                        raise ValueError("validation failed")
                self.assertEqual(
                    (destination / "runtime").read_text(), "verified old product"
                )
                with product(destination) as staging:
                    (staging / "runtime").write_text("verified new product")
                    (staging / "build-report.json").write_text(
                        json.dumps({"path": str(staging / "runtime")})
                    )
            self.assertEqual(
                (destination / "runtime").read_text(), "verified new product"
            )
            self.assertEqual(
                json.loads((destination / "build-report.json").read_text())["path"],
                str(destination / "runtime"),
            )
            self.assertEqual(
                [p.name for p in destination.parent.iterdir()], ["product"]
            )

    def test_static_server_serves_complete_artifacts_and_restricts_roots(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "examples/demos").mkdir(parents=True)
            (root / "examples/demos/index.html").write_text("<title>Gallery</title>")
            (root / "target/gallery-fixtures").mkdir(parents=True)
            (root / "target/gallery-fixtures/helper.js").write_text(
                "export const ready = true;"
            )
            for directory in (
                "gallery-trace",
                "browser-build/render-instrumentation",
                "demos-cockpit-assets",
                "demos-drawings",
                "font-assets",
            ):
                path = root / "target" / directory
                path.mkdir(parents=True)
                (path / "helper.js").write_text("export const diagnostic = true;")
            terminal_assets = {
                "terminal-worker.js": (
                    b"self.onmessage = () => {};",
                    "text/javascript",
                ),
                "ghostty-vt.wasm": (b"\x00asm\x01\x00\x00\x00", "application/wasm"),
                "terminal-font.json": (b'{"advance": 10}', "application/json"),
            }
            terminal = root / "target/terminal-build"
            terminal.mkdir(parents=True)
            for name, (content, _) in terminal_assets.items():
                (terminal / name).write_bytes(content)
            (root / "private.txt").write_text("not public")
            with ThreadingHTTPServer(
                ("127.0.0.1", 0), handler("gallery", root)
            ) as server:
                server.daemon_threads = True
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                connection = HTTPConnection("127.0.0.1", server.server_port, timeout=3)
                try:
                    connection.request("GET", "/")
                    response = connection.getresponse()
                    self.assertEqual(response.status, 200)
                    body = response.read()
                    self.assertIn(b"Gallery", body)
                    etag = response.headers["ETag"]
                    connection.request("HEAD", "/")
                    response = connection.getresponse()
                    self.assertEqual(response.headers["ETag"], etag)
                    self.assertEqual(int(response.headers["Content-Length"]), len(body))
                    self.assertEqual(response.read(), b"")
                    connection.request("GET", "/target/gallery-fixtures/helper.js")
                    response = connection.getresponse()
                    self.assertEqual(response.status, 200)
                    self.assertIn("javascript", response.headers["Content-Type"])
                    response.read()
                    for path in (
                        "/target/gallery-trace/helper.js",
                        "/target/browser-build/render-instrumentation/helper.js",
                        "/target/demos-cockpit-assets/helper.js",
                        "/target/demos-drawings/helper.js",
                        "/target/font-assets/helper.js",
                    ):
                        connection.request("GET", path)
                        response = connection.getresponse()
                        self.assertEqual(response.status, 200)
                        response.read()
                    for name, (content, mime_type) in terminal_assets.items():
                        connection.request("GET", f"/target/terminal-build/{name}")
                        response = connection.getresponse()
                        self.assertEqual(response.status, 200)
                        self.assertIn(mime_type, response.headers["Content-Type"])
                        self.assertEqual(response.read(), content)
                    for path in (
                        "/private.txt",
                        "/target/gallery-fixtures/%2e%2e/%2e%2e/private.txt",
                    ):
                        connection.request("GET", path)
                        response = connection.getresponse()
                        self.assertEqual(response.status, 404)
                        response.read()
                    connection.request("POST", "/", body=b"")
                    response = connection.getresponse()
                    self.assertEqual(response.status, 405)
                    response.read()
                finally:
                    connection.close()
                    server.shutdown()
                    thread.join(timeout=3)

    def test_static_server_negotiates_lossless_gzip_and_head_metadata(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            asset = root / "target/demos-cockpit-assets/1/8"
            asset.parent.mkdir(parents=True)
            original = bytes(range(256)) * 1024
            asset.write_bytes(original)
            with serving(root) as server:
                identity_etag = None
                gzip_etag = None
                for accepted, compressed in (
                    (None, False),
                    ("", False),
                    ("identity", False),
                    ("br", False),
                    ("gzip", True),
                    ("br, GZip; q=0.5", True),
                    ("*;q=0.5", True),
                    ("gzip;q=0, *;q=1", False),
                    ("gzip;q=0.000", False),
                    ("gzip;q=0, gzip", False),
                    ("gzip;q=0.5, identity;q=1", False),
                    ("gzip;q=0.5, identity;q=0.1", True),
                    ("gzip;q=invalid", False),
                    ("gzip;q=NaN", False),
                    ("gzip;q=1.1", False),
                ):
                    with self.subTest(accepted=accepted):
                        headers = (
                            {"Accept-Encoding": accepted}
                            if accepted is not None
                            else {}
                        )
                        status, metadata, body = request_product(
                            server, "GET", "/target/demos-cockpit-assets/1/8", headers
                        )
                        self.assertEqual(status, 200)
                        self.assertEqual(metadata["Vary"], "Accept-Encoding")
                        self.assertEqual(metadata["Cache-Control"], "no-cache")
                        self.assertEqual(
                            metadata["Content-Type"], "application/octet-stream"
                        )
                        self.assertEqual(int(metadata["Content-Length"]), len(body))
                        self.assertEqual(
                            metadata["ETag"], f'"{hashlib.sha256(body).hexdigest()}"'
                        )
                        if compressed:
                            self.assertEqual(metadata["Content-Encoding"], "gzip")
                            self.assertEqual(gzip.decompress(body), original)
                            self.assertLess(len(body), len(original) // 10)
                            if gzip_etag is not None:
                                self.assertEqual(metadata["ETag"], gzip_etag)
                            gzip_etag = metadata["ETag"]
                        else:
                            self.assertIsNone(metadata["Content-Encoding"])
                            self.assertEqual(body, original)
                            identity_etag = metadata["ETag"]
                        status, head_metadata, head_body = request_product(
                            server, "HEAD", "/target/demos-cockpit-assets/1/8", headers
                        )
                        self.assertEqual(status, 200)
                        self.assertEqual(head_body, b"")
                        for name in (
                            "Content-Type",
                            "Content-Length",
                            "Content-Encoding",
                            "Cache-Control",
                            "ETag",
                            "Vary",
                        ):
                            self.assertEqual(head_metadata[name], metadata[name])
                self.assertNotEqual(identity_etag, gzip_etag)
                for accepted, other_etag in (
                    ("identity", gzip_etag),
                    ("gzip", identity_etag),
                ):
                    status, metadata, body = request_product(
                        server,
                        "GET",
                        "/target/demos-cockpit-assets/1/8",
                        {"Accept-Encoding": accepted, "If-None-Match": other_etag},
                    )
                    self.assertEqual(status, 200)
                    self.assertNotEqual(metadata["ETag"], other_etag)
                    self.assertEqual(
                        gzip.decompress(body) if accepted == "gzip" else body,
                        original,
                    )
                status, metadata, body = request_product(
                    server,
                    "GET",
                    "/target/demos-cockpit-assets/1/8",
                    {"Accept-Encoding": "gzip", "If-Match": gzip_etag},
                )
                self.assertEqual(status, 200)
                self.assertEqual(metadata["ETag"], gzip_etag)
                self.assertEqual(gzip.decompress(body), original)
            self.assertEqual(asset.read_bytes(), original)

    def test_static_server_keeps_small_and_incompressible_products_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "target/gallery-fixtures"
            directory.mkdir(parents=True)
            fixtures = {
                "small.js": b"export const ready = true;",
                "noise": random.Random(0).randbytes(64 * 1024),
            }
            for name, body in fixtures.items():
                (directory / name).write_bytes(body)
            with serving(root) as server:
                for name, original in fixtures.items():
                    status, metadata, body = request_product(
                        server,
                        "GET",
                        f"/target/gallery-fixtures/{name}",
                        {"Accept-Encoding": "gzip"},
                    )
                    self.assertEqual(status, 200)
                    self.assertIsNone(metadata["Content-Encoding"])
                    self.assertEqual(int(metadata["Content-Length"]), len(original))
                    self.assertEqual(body, original)

    def test_static_server_revalidates_selected_representation_and_rebuilds(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            font = root / "target/font-assets/font.ippf"
            font.parent.mkdir(parents=True)
            original = b"font fixture contours\x00" * 4096
            with serving(root) as server:
                for encoding in ("identity", "gzip"):
                    with self.subTest(encoding=encoding):
                        font.write_bytes(original)
                        accepted = {"Accept-Encoding": encoding}
                        status, metadata, body = request_product(
                            server, "GET", "/target/font-assets/font.ippf", accepted
                        )
                        self.assertEqual(status, 200)
                        etag = metadata["ETag"]
                        for method in ("GET", "HEAD"):
                            for condition in (etag, f'"other", W/{etag}', "*"):
                                status, cached, body = request_product(
                                    server,
                                    method,
                                    "/target/font-assets/font.ippf",
                                    {**accepted, "If-None-Match": condition},
                                )
                                self.assertEqual(status, 304)
                                self.assertEqual(body, b"")
                                self.assertEqual(cached["ETag"], etag)
                                self.assertEqual(cached["Cache-Control"], "no-cache")
                                self.assertEqual(cached["Vary"], "Accept-Encoding")
                                self.assertEqual(
                                    cached["Content-Encoding"],
                                    metadata["Content-Encoding"],
                                )
                                self.assertIsNone(cached["Content-Length"])
                        status, _, body = request_product(
                            server,
                            "GET",
                            "/target/font-assets/font.ippf",
                            {**accepted, "If-None-Match": '"different"'},
                        )
                        self.assertEqual(status, 200)
                        self.assertEqual(
                            gzip.decompress(body) if encoding == "gzip" else body,
                            original,
                        )
                        changed = original + b"changed outlines"
                        font.write_bytes(changed)
                        status, rebuilt, body = request_product(
                            server,
                            "GET",
                            "/target/font-assets/font.ippf",
                            {**accepted, "If-None-Match": etag},
                        )
                        self.assertEqual(status, 200)
                        self.assertNotEqual(rebuilt["ETag"], etag)
                        self.assertEqual(
                            gzip.decompress(body) if encoding == "gzip" else body,
                            changed,
                        )

    def test_static_server_client_disconnect_does_not_send_another_response(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            asset = root / "target/gallery-fixtures/large"
            asset.parent.mkdir(parents=True)
            asset.write_bytes(b"payload" * 100_000)
            disconnected = threading.Event()
            finished = threading.Event()
            statuses = []
            errors = []

            class GatedHandler(handler("gallery", root)):
                def send_response(self, code, message=None):
                    statuses.append(code)
                    super().send_response(code, message)

                def end_headers(self):
                    super().end_headers()
                    if not disconnected.wait(timeout=3):
                        raise TimeoutError("client did not disconnect")

                def finish(self):
                    try:
                        super().finish()
                    finally:
                        finished.set()

            with serving(root, GatedHandler) as server:
                server.handle_error = lambda request, address: errors.append(
                    sys.exc_info()[1]
                )
                client = socket.create_connection(
                    ("127.0.0.1", server.server_port), timeout=3
                )
                try:
                    client.sendall(
                        b"GET /target/gallery-fixtures/large HTTP/1.1\r\n"
                        b"Host: localhost\r\nConnection: close\r\n\r\n"
                    )
                    received = b""
                    while b"\r\n\r\n" not in received:
                        fragment = client.recv(4096)
                        self.assertTrue(fragment, "server closed before headers")
                        received += fragment
                    self.assertTrue(received.startswith(b"HTTP/1.1 200 "))
                    # Reset the real connection before the gated body write resumes.
                    client.setsockopt(
                        socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0)
                    )
                finally:
                    client.close()
                    disconnected.set()
                self.assertTrue(finished.wait(timeout=3), "request did not finish")
                self.assertEqual(statuses, [200])
                self.assertEqual(errors, [])
                status, _, body = request_product(
                    server, "GET", "/target/gallery-fixtures/large"
                )
                self.assertEqual(status, 200)
                self.assertEqual(body, asset.read_bytes())


if __name__ == "__main__":
    unittest.main()
