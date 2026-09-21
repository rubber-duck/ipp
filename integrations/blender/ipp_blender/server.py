"""Cooperative main-thread HTTPS/WSS; no executors or Blender references in I/O."""

import asyncio
import html
import json
import secrets
import threading
import time
from urllib.parse import urlencode, urlsplit

from aiohttp import WSMsgType, web

from .asset_store import AssetStore
from .certificates import credentials
from .streaming import ExportStream


MAX_PENDING_UPDATES = 4


class SceneServer:
    def __init__(
        self,
        export,
        *,
        directory,
        port=8118,
        certificate="",
        private_key="",
        allowed_origins=(),
        viewer_url="",
    ):
        self.export = export
        self.port = port
        self.viewer_url = viewer_url
        self.allowed_origins = set(allowed_origins)
        if viewer_url:
            parsed = urlsplit(viewer_url)
            if parsed.scheme not in {"http", "https"} or not parsed.netloc:
                raise ValueError("Viewer URL must be an HTTP or HTTPS URL")
            self.allowed_origins.add(f"{parsed.scheme}://{parsed.netloc}")
        self.context, self.certificate_paths = credentials(
            directory, certificate, private_key
        )
        self.token = secrets.token_urlsafe(32)
        self.session = secrets.token_hex(16)
        self.revision = 0
        self.issued_revision = 0
        self.store = AssetStore(session=self.session)
        self.clients = {}
        self.asset_watchers = {}
        self.store.changed = self._asset_changed
        self.active_reads = 0
        self.pending_asset_reads = 0
        self.snapshot = None
        self.origin = ""
        self.loop = asyncio.new_event_loop()
        self.runner = None
        self.exporting = False
        self.last_export_seconds = 0.0
        self.closed = False
        self.pending_exports = []
        self.transfer = None

    def assert_main_thread(self):
        if threading.current_thread() is not threading.main_thread():
            raise RuntimeError("IPP Blender server must run on Blender's main thread")

    def start(self):
        self.assert_main_thread()
        try:
            self.loop.run_until_complete(self._start())
            self.sync()
        except BaseException:
            self.close()
            raise
        return self

    async def _start(self):
        app = web.Application(middlewares=[self._guard], client_max_size=1024)
        app.router.add_get("/", self._onboarding)
        app.router.add_get("/v1/scene", self._scene)
        app.router.add_get("/v1/updates", self._updates)
        app.router.add_get("/v1/assets", self._asset_updates)
        app.router.add_post("/v1/sync", self._sync)
        app.router.add_get("/assets/{session}/{revision}/{asset}", self._asset)
        app.router.add_route("OPTIONS", "/{path:.*}", self._options)
        self.runner = web.AppRunner(app, access_log=None, shutdown_timeout=0.5)
        await self.runner.setup()
        # Numeric loopback avoids asyncio's default threaded DNS resolver.
        site = web.TCPSite(
            self.runner, "127.0.0.1", self.port, ssl_context=self.context
        )
        await site.start()
        self.port = self.runner.addresses[0][1]
        self.origin = f"https://127.0.0.1:{self.port}"

    def pump(self):
        self.assert_main_thread()
        if not self.closed:
            self.loop.stop()
            self.loop.run_forever()
            if self.pending_exports and not self.exporting:
                future, stream = self.pending_exports.pop(0)
                if future.cancelled():
                    return
                try:
                    self.sync(stream=stream)
                    if not future.done():
                        future.set_result(self.revision)
                except Exception as error:
                    if not future.done():
                        future.set_exception(error)

    def sync(self, *, stream=None):
        self.assert_main_thread()
        if self.exporting or self.closed:
            raise RuntimeError("Export is already running or the server is closed")
        if any(queue.full() for queue in self.clients.values()):
            raise RuntimeError(
                "A viewer is too slow; wait or disconnect it before syncing"
            )
        before = set(self.store.entries)
        started = time.monotonic()
        self.exporting = True
        self.issued_revision += 1
        revision = self.issued_revision
        self.store.revision = revision
        try:
            if stream:
                socket, batch_size = stream
                self.transfer = ExportStream(self, socket, batch_size, revision)
                scene = self.export(self.store.publish, stream=self.transfer)
                extraction_seconds = time.monotonic() - started
                profile = self.transfer.finish()
                profile["extractionSeconds"] = extraction_seconds
            else:
                scene = self.export(self.store.publish)
                profile = None
            snapshot = {
                "type": "snapshot",
                "session": self.session,
                "revision": revision,
                "scene": scene,
                "exportSeconds": time.monotonic() - started,
                "streamProfile": profile,
            }
            encoded = json.dumps(snapshot, separators=(",", ":"), allow_nan=False)
            if self.transfer:
                # As with ordinary snapshots, publication completes at the final
                # marker. The viewer owns its independently applied revision;
                # slow controller setup must not time out an already sent scene.
                self.transfer.send("commit", acknowledged=False, snapshot=snapshot)
        except BaseException as error:
            if self.transfer:
                # Acknowledged partial effects can still reference these assets.
                queue = self.clients.get(self.transfer.socket)
                if queue is not None and not self.transfer.socket.closed:
                    queue.put_nowait(
                        json.dumps(
                            {
                                "type": "abort",
                                "transfer": self.transfer.transfer,
                                "error": str(error),
                            }
                        )
                    )
            else:
                self.store.discard(self.store.entries.keys() - before)
            raise
        finally:
            self.exporting = False
            self.last_export_seconds = time.monotonic() - started
            self.transfer = None
        self.snapshot = encoded
        self.revision = revision
        for socket, queue in self.clients.items():
            if not stream or socket is not stream[0]:
                queue.put_nowait(encoded)

    @web.middleware
    async def _guard(self, request, handler):
        origin = request.headers.get("Origin")
        allowed = (
            origin is None or origin in self.allowed_origins or origin == self.origin
        )
        if not allowed:
            raise web.HTTPForbidden(text="Viewer origin is not allowed")
        if request.headers.get("Host") != urlsplit(self.origin).netloc:
            raise web.HTTPForbidden(text="Use the endpoint printed by Blender")
        if request.method != "OPTIONS" and request.path != "/":
            if not secrets.compare_digest(request.query.get("token", ""), self.token):
                raise web.HTTPUnauthorized(
                    text="Open the viewer from Blender to obtain a session"
                )
        try:
            response = await handler(request)
        except web.HTTPException as error:
            response = error
        self._headers(response, origin)
        return response

    def _headers(self, response, origin):
        response.headers["Cache-Control"] = "no-store"
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["Referrer-Policy"] = "no-referrer"
        if origin:
            response.headers["Access-Control-Allow-Origin"] = origin
            response.headers["Vary"] = "Origin"
            response.headers["Access-Control-Expose-Headers"] = "ETag"

    async def _options(self, request):
        response = web.Response(status=204)
        response.headers["Access-Control-Allow-Methods"] = "GET, HEAD, POST, OPTIONS"
        response.headers["Access-Control-Allow-Headers"] = "If-Match, Content-Type"
        return response

    @property
    def onboarding_url(self):
        return f"{self.origin}/?{urlencode({'token': self.token})}"

    async def _onboarding(self, request):
        # A cross-origin page cannot read the token from an unauthenticated root.
        authenticated = secrets.compare_digest(
            request.query.get("token", ""), self.token
        )
        target = ""
        if self.viewer_url and authenticated:
            parsed = urlsplit(self.viewer_url)
            fragment = urlencode({"endpoint": self.origin, "token": self.token})
            target = parsed._replace(fragment=fragment).geturl()
        redirect = (
            f'<meta http-equiv="refresh" content="0;url={html.escape(target, quote=True)}">'
            if target
            else ""
        )
        body = f"<!doctype html><meta charset=utf-8><title>IPP Blender</title>{redirect}<h1>IPP Blender is ready</h1><p>You can now connect your viewer to {html.escape(self.origin)}.</p>"
        return web.Response(text=body, content_type="text/html")

    async def _scene(self, request):
        return web.Response(text=self.snapshot, content_type="application/json")

    async def _sync(self, request):
        try:
            # Extraction runs outside asyncio callbacks so checkpoints can service
            # the same loop without nesting event loops or starting worker threads.
            if self.exporting or self.pending_exports:
                raise RuntimeError("Export is already pending")
            future = self.loop.create_future()
            self.pending_exports.append((future, None))
            await future
        except (ValueError, RuntimeError, TimeoutError) as error:
            raise web.HTTPConflict(text=str(error)) from error
        return web.json_response({"session": self.session, "revision": self.revision})

    def _asset_changed(self, name):
        for wanted, changed, event in self.asset_watchers.values():
            if name in wanted:
                changed.add(name)
                event.set()

    async def _asset_updates(self, request):
        if request.query.get("session") != self.session:
            raise web.HTTPGone(text="Export session expired")
        socket = web.WebSocketResponse(heartbeat=15, max_msg_size=64 * 1024)
        await socket.prepare(request)
        wanted, changed, event = set(), set(), asyncio.Event()
        self.asset_watchers[socket] = (wanted, changed, event)

        async def send():
            while not socket.closed:
                await event.wait()
                event.clear()
                while changed:
                    names = sorted(changed)[:128]
                    changed.difference_update(names)
                    await socket.send_json(
                        {
                            "session": self.session,
                            "assets": [
                                self.store.states.get(
                                    name,
                                    {
                                        "source": f"/assets/{name}",
                                        "state": "failed",
                                        "error": "Immutable export is unavailable",
                                    },
                                )
                                for name in names
                            ],
                        }
                    )

        sender = asyncio.create_task(send())
        try:
            async for message in socket:
                if message.type != web.WSMsgType.TEXT:
                    break
                value = json.loads(message.data)
                source = value.get("source", "")
                if (
                    not isinstance(source, str)
                    or not source.startswith(f"/assets/{self.session}/")
                    or len(source) > 4096
                ):
                    await socket.close(
                        code=1008, message=b"Invalid source subscription"
                    )
                    break
                name = source.removeprefix("/assets/")
                if value.get("watch") is True:
                    wanted.add(name)
                    changed.add(name)
                    event.set()
                else:
                    wanted.discard(name)
                    changed.discard(name)
        finally:
            self.asset_watchers.pop(socket, None)
            sender.cancel()
            await asyncio.gather(sender, return_exceptions=True)
        return socket

    async def _asset(self, request):
        name = "/".join(
            request.match_info[key] for key in ("session", "revision", "asset")
        )
        state = self.store.states.get(name)
        if state and state["state"] == "pending":
            self.pending_asset_reads += 1
            return web.json_response(
                {
                    "source": state["source"],
                    "monitor": f"/v1/assets?session={self.session}&token={self.token}",
                },
                status=202,
            )
        if state and state["state"] == "failed":
            raise web.HTTPGone(text=state["error"])
        entry = self.store.entries.get(name)
        if entry is None:
            raise web.HTTPNotFound(text="Immutable export is unavailable")
        etag = f'"{name}"'
        if request.headers.get("If-Match", etag) != etag:
            raise web.HTTPPreconditionFailed(text="Immutable export identity differs")
        path, size, content_type = entry
        response = web.StreamResponse(
            headers={
                "ETag": etag,
                "Content-Type": content_type,
                "Content-Length": str(size),
            }
        )
        self._headers(response, request.headers.get("Origin"))
        self.active_reads += 1
        try:
            async with asyncio.timeout(30):
                await response.prepare(request)
                # FileResponse and file payloads use worker executors. These small,
                # explicit reads stay on the main thread; writes apply backpressure.
                if request.method != "HEAD":
                    with path.open("rb") as stream:
                        while chunk := stream.read(64 * 1024):
                            await response.write(chunk)
                await response.write_eof()
        finally:
            self.active_reads -= 1
        return response

    async def _updates(self, request):
        try:
            batch_size = int(request.query.get("stream", "0"))
            if not 0 <= batch_size <= 1000:
                raise ValueError()
        except ValueError as error:
            raise web.HTTPBadRequest(
                text="Stream batch size must be 0..1000"
            ) from error
        refresh = batch_size > 0 or request.query.get("refresh") == "1"
        if refresh and (self.exporting or self.pending_exports):
            raise web.HTTPConflict(text="Export is already pending")
        socket = web.WebSocketResponse(compress=False, max_msg_size=1024, heartbeat=15)
        await socket.prepare(request)
        queue = asyncio.Queue(maxsize=MAX_PENDING_UPDATES)
        if not refresh:
            queue.put_nowait(self.snapshot)
        self.clients[socket] = queue
        export_future = None
        if refresh:
            export_future = self.loop.create_future()
            self.pending_exports.append(
                (
                    export_future,
                    (socket, batch_size) if batch_size else None,
                )
            )

            def export_finished(future):
                if future.cancelled():
                    return
                error = future.exception()
                # Stream failures already carry an abort with their transfer ID.
                if error is not None and not batch_size and not socket.closed:
                    queue.put_nowait(json.dumps({"type": "error", "error": str(error)}))

            export_future.add_done_callback(export_finished)

        async def write_updates():
            while True:
                update = await queue.get()
                async with asyncio.timeout(10):
                    await socket.send_str(update)

        writer = asyncio.create_task(write_updates())
        writer.add_done_callback(
            lambda task: (
                self.loop.create_task(socket.close()) if not task.cancelled() else None
            )
        )
        try:
            async for message in socket:
                if message.type == WSMsgType.ERROR:
                    break
                if (
                    message.type == WSMsgType.TEXT
                    and self.transfer
                    and self.transfer.socket is socket
                ):
                    try:
                        acknowledgement = json.loads(message.data)
                        if (
                            isinstance(acknowledgement, dict)
                            and acknowledgement.get("type") == "chunk-applied"
                        ):
                            self.transfer.acknowledge(acknowledgement)
                    except (ValueError, TypeError):
                        await socket.close(
                            code=1008, message=b"Invalid acknowledgement"
                        )
                # Acknowledgements are observations only; they never drive Blender
                # or the runtime clock and do not mutate the latest scene.
        finally:
            self.clients.pop(socket, None)
            if export_future and not export_future.done():
                export_future.cancel()
            writer.cancel()
            await asyncio.gather(writer, return_exceptions=True)
        return socket

    def close(self):
        self.assert_main_thread()
        if self.closed:
            return
        self.closed = True

        async def cleanup():
            await asyncio.gather(
                *(socket.close() for socket in [*self.clients, *self.asset_watchers]),
                return_exceptions=True,
            )
            if self.runner:
                await self.runner.cleanup()
            pending = [
                task
                for task in asyncio.all_tasks()
                if task is not asyncio.current_task()
            ]
            for task in pending:
                task.cancel()
            await asyncio.gather(*pending, return_exceptions=True)

        try:
            self.loop.run_until_complete(cleanup())
        finally:
            self.loop.close()
            self.store.close()
