"""Loopback development serving for explicitly built gallery/viewer products."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import signal
import threading
from urllib.parse import unquote, urlsplit

from .model import ROOT


TYPES = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".wasm": "application/wasm",
    ".json": "application/json",
}


def handler(name: str, root: Path = ROOT) -> type[BaseHTTPRequestHandler]:
    """Keep serving roots explicit; neighboring source and credentials are never public."""
    if name == "gallery":
        base = root
        allowed = [
            root / path
            for path in (
                "examples/world-gallery",
                "target/gallery-build",
                "target/gallery-fixtures",
                "target/browser-build/render-expanded",
                "target/gallery-gui-assets",
                "target/gallery-platformer-assets",
                "target/font-assets",
                "target/surface-assets",
            )
        ]
        index = "/examples/world-gallery/"
    elif name == "blender-viewer":
        base = root / "target/blender-viewer"
        allowed = [base]
        index = "/index.html"
    else:
        raise ValueError(f"Unknown development server: {name}")

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, format: str, *args: object) -> None:
            pass

        def respond(self, head: bool = False) -> None:
            try:
                path = unquote(urlsplit(self.path).path)
                if path == "/":
                    path = index
                if path.endswith("/"):
                    path += "index.html"
                candidate = (base / path.lstrip("/")).resolve()
                if not candidate.is_relative_to(root.resolve()) or not any(
                    candidate.is_relative_to(directory.resolve())
                    for directory in allowed
                ):
                    raise FileNotFoundError(path)
                content = candidate.read_bytes()
                self.send_response(200)
                self.send_header(
                    "Content-Type",
                    TYPES.get(candidate.suffix, "application/octet-stream"),
                )
                self.send_header("Content-Length", str(len(content)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("ETag", f'"{hashlib.sha256(content).hexdigest()}"')
                self.send_header("Cross-Origin-Resource-Policy", "same-origin")
                self.end_headers()
                if not head:
                    self.wfile.write(content)
            except OSError, ValueError:
                self.send_error(404, "Product file unavailable")

        def do_GET(self) -> None:
            self.respond()

        def do_HEAD(self) -> None:
            self.respond(head=True)

        def do_POST(self) -> None:
            self.send_error(405, "Only GET and HEAD are supported")

    return Handler


def serve(name: str, port: int | None = None) -> None:
    selected_port = (5178 if name == "blender-viewer" else 0) if port is None else port
    if not 0 <= selected_port <= 65535:
        raise ValueError("Port must be between 0 and 65535")
    with ThreadingHTTPServer(("127.0.0.1", selected_port), handler(name)) as server:
        server.daemon_threads = True
        stopped = threading.Event()

        def stop(_signal: int, _frame: object) -> None:
            if not stopped.is_set():
                stopped.set()
                threading.Thread(target=server.shutdown, daemon=True).start()

        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, stop)
        origin = f"http://127.0.0.1:{server.server_port}"
        print(
            f"IPP scene gallery: {origin}/examples/world-gallery/"
            if name == "gallery"
            else f"IPP Blender viewer: {origin}/",
            flush=True,
        )
        server.serve_forever(poll_interval=0.1)
