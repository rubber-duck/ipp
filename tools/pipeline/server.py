"""Loopback development serving for explicitly built gallery/viewer products."""

import gzip
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import re
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


def accepts_gzip(value: str) -> bool:
    """An explicit exclusion overrides a wildcard; honor identity preferences."""
    qualities: dict[str, float] = {}
    for entry in value.split(","):
        coding, *parameters = entry.strip().lower().split(";")
        if not coding:
            continue
        quality = 1.0
        for parameter in parameters:
            name, separator, setting = parameter.strip().partition("=")
            if (
                name.strip() != "q"
                or not separator
                or not re.fullmatch(
                    r"0(?:\.[0-9]{0,3})?|1(?:\.0{0,3})?", setting.strip()
                )
            ):
                quality = 0.0
                break
            quality = float(setting)
        qualities[coding] = min(qualities.get(coding, quality), quality)
    quality = qualities.get("gzip", qualities.get("*", 0.0))
    return quality > 0 and quality >= qualities.get("identity", 0.0)


def matches_etag(value: str, etag: str) -> bool:
    """GET/HEAD revalidation uses weak comparison against the selected bytes."""
    return any(
        item.strip() == "*" or item.strip().removeprefix("W/") == etag
        for item in value.split(",")
    )


def handler(name: str, root: Path = ROOT) -> type[BaseHTTPRequestHandler]:
    """Keep serving roots explicit; neighboring source and credentials are never public."""
    if name == "gallery":
        base = root
        allowed = [
            root / path
            for path in (
                "examples/demos",
                "target/gallery-build",
                "target/gallery-assets",
                "target/gallery-fixtures",
                "target/gallery-trace",
                "target/gui-glass-skin",
                "target/browser-build/render",
                "target/browser-build/render-instrumentation",
                "target/gallery-platformer-assets",
                "target/demos-cockpit-assets",
                "target/demos-drawings",
                "target/terminal-build",
                "target/font-assets",
                "target/surface-assets",
            )
        ]
        index = "/examples/demos/"
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
            except OSError, ValueError:
                try:
                    self.send_error(404, "Product file unavailable")
                except ConnectionError:
                    self.close_connection = True
                return

            encoding = None
            if len(content) >= 16 * 1024 and accepts_gzip(
                ",".join(self.headers.get_all("Accept-Encoding", []))
            ):
                compressed = gzip.compress(content, compresslevel=6, mtime=0)
                if len(compressed) <= len(content) * 0.9:
                    content = compressed
                    encoding = "gzip"
            etag = f'"{hashlib.sha256(content).hexdigest()}"'
            unchanged = matches_etag(
                ",".join(self.headers.get_all("If-None-Match", [])), etag
            )

            try:
                self.send_response(304 if unchanged else 200)
                self.send_header("Cache-Control", "no-cache")
                self.send_header("ETag", etag)
                self.send_header("Vary", "Accept-Encoding")
                self.send_header("Cross-Origin-Resource-Policy", "same-origin")
                if encoding:
                    self.send_header("Content-Encoding", encoding)
                if unchanged:
                    self.end_headers()
                    return
                self.send_header(
                    "Content-Type",
                    TYPES.get(candidate.suffix, "application/octet-stream"),
                )
                self.send_header("Content-Length", str(len(content)))
                self.end_headers()
                if not head:
                    self.wfile.write(content)
            except ConnectionError:
                # Browser cancellation can disconnect after headers or partial bytes.
                self.close_connection = True

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
            f"IPP scene gallery: {origin}/examples/demos/"
            if name == "gallery"
            else f"IPP Blender viewer: {origin}/",
            flush=True,
        )
        server.serve_forever(poll_interval=0.1)
