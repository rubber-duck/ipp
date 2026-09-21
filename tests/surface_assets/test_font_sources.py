"""Exercise verified downloads and offline cache reuse over real local HTTP."""

from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading
import unittest

from tools.font_sources import FontSource, fetch_font


@contextmanager
def serve(data: bytes):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/font.ttf", requests
    finally:
        server.shutdown()
        worker.join()
        server.server_close()


class FontSourceTests(unittest.TestCase):
    def test_download_cache_repair_and_offline_reuse(self):
        data = b"pinned font download"
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "font.ttf"
            with serve(data) as (url, requests):
                source = FontSource("fixture", url, hashlib.sha256(data).hexdigest())
                self.assertEqual(fetch_font(source, destination).read_bytes(), data)
                self.assertEqual(fetch_font(source, destination).read_bytes(), data)
                self.assertEqual(len(requests), 1)
                destination.write_bytes(b"corrupt cache")
                self.assertEqual(fetch_font(source, destination).read_bytes(), data)
                self.assertEqual(len(requests), 2)
            # The server is stopped: this read must use verified local content.
            self.assertEqual(fetch_font(source, destination).read_bytes(), data)

    def test_checksum_failure_preserves_previous_file_and_cleans_download(self):
        with (
            tempfile.TemporaryDirectory() as directory,
            serve(b"bad content") as (
                url,
                _,
            ),
        ):
            destination = Path(directory) / "font.ttf"
            destination.write_bytes(b"previous content")
            source = FontSource("fixture", url, hashlib.sha256(b"expected").hexdigest())
            with self.assertRaisesRegex(ValueError, "source checksum mismatch"):
                fetch_font(source, destination)
            self.assertEqual(destination.read_bytes(), b"previous content")
            self.assertEqual(list(Path(directory).iterdir()), [destination])
