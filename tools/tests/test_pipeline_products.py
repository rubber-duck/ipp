"""Actual HTTP serving and publication boundaries, independent of graphics setup."""

from http.client import HTTPConnection
from http.server import ThreadingHTTPServer
import json
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from pipeline.builds import product
from pipeline.server import handler


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
            (root / "examples/world-gallery").mkdir(parents=True)
            (root / "examples/world-gallery/index.html").write_text(
                "<title>Gallery</title>"
            )
            (root / "target/gallery-fixtures").mkdir(parents=True)
            (root / "target/gallery-fixtures/helper.js").write_text(
                "export const ready = true;"
            )
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


if __name__ == "__main__":
    unittest.main()
