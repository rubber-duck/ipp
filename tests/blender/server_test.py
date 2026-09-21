"""Real TLS/socket tests inside Blender; complements the full browser sync suite."""

import asyncio
import json
import ssl
import sys
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [
    str(ROOT / "target/blender/dependencies"),
    str(ROOT / "integrations/blender"),
]

from aiohttp import ClientSession
from ipp_blender.asset_store import AssetStore
from ipp_blender.certificates import credentials
from ipp_blender.server import SceneServer


def check_private_asset_reuse():
    store = AssetStore(session="private-reuse")
    try:
        first = store.reserve(private_key=("mesh", b"detached-recipe"))
        store.complete(first, b"stable geometry")
        store.revision = 2
        assert store.reserve(private_key=("mesh", b"detached-recipe")) == first, (
            "An unchanged private recipe should retain its immutable source"
        )

        failed = store.reserve(private_key=("mesh", b"failed-recipe"))
        store.fail(failed, "producer failed")
        replacement = store.reserve(private_key=("mesh", b"failed-recipe"))
        assert replacement != failed
        assert store.states[replacement.removeprefix("/assets/")]["state"] == "pending"
    finally:
        store.close()


async def exercise(server, current):
    trusted = ssl.create_default_context(cafile=str(server.certificate_paths[0]))
    origin = "https://viewer.example"
    token = {"token": server.token}
    async with ClientSession() as client:
        async with client.get(server.origin + "/v1/scene", ssl=trusted) as response:
            assert response.status == 401
        async with client.get(
            server.origin + "/v1/scene",
            ssl=trusted,
            params=token,
            headers={"Origin": "https://denied.example"},
        ) as response:
            assert response.status == 403
        async with client.get(
            server.origin + "/v1/scene",
            ssl=trusted,
            params=token,
            headers={"Origin": origin},
        ) as response:
            assert response.status == 200
            assert response.headers["Access-Control-Allow-Origin"] == origin
            first = await response.json()
        resource = first["scene"]["entities"][0]["mesh"]["source"]
        assert resource == f"/assets/{server.session}/1/1"
        assert server.store.publish(current[0]) == resource
        pending = server.store.reserve()
        async with client.get(
            server.origin + pending, ssl=trusted, params=token
        ) as response:
            assert response.status == 202
            declaration = await response.json()
        async with client.ws_connect(
            server.origin + declaration["monitor"], ssl=trusted, origin=origin
        ) as status:
            await status.send_json({"source": pending, "watch": True})
            assert (await status.receive_json())["assets"][0]["state"] == "pending"
            server.store.complete(pending, b"later bytes")
            assert (await status.receive_json())["assets"][0]["state"] == "ready"
            failed = server.store.reserve()
            await status.send_json({"source": failed, "watch": True})
            assert (await status.receive_json())["assets"][0]["state"] == "pending"
            server.store.fail(failed, "production cancelled")
            assert (await status.receive_json())["assets"][0][
                "error"
            ] == "production cancelled"
        async with client.get(
            server.origin + pending, ssl=trusted, params=token
        ) as response:
            assert await response.read() == b"later bytes"
        async with client.get(
            server.origin + failed, ssl=trusted, params=token
        ) as response:
            assert response.status == 410
        async with client.ws_connect(
            server.origin + "/v1/updates",
            ssl=trusted,
            params=token,
            origin=origin,
            compress=0,
        ) as socket:
            assert (await socket.receive_json())["revision"] == 1
            current[0] = b"second immutable revision"
            server.sync()
            assert (await socket.receive_json())["revision"] == 2
        async with client.get(
            server.origin + resource,
            ssl=trusted,
            params=token,
            headers={"Origin": origin},
        ) as response:
            assert await response.read() == b"first immutable revision"
            etag = response.headers["ETag"]
            assert response.headers["Access-Control-Expose-Headers"] == "ETag"
        async with client.options(
            server.origin + resource,
            ssl=trusted,
            headers={"Origin": origin, "Access-Control-Request-Headers": "if-match"},
        ) as response:
            assert response.status == 204
            assert "If-Match" in response.headers["Access-Control-Allow-Headers"]
        async with client.get(
            server.origin + resource,
            ssl=trusted,
            params=token,
            headers={"If-Match": etag},
        ) as response:
            assert response.status == 200
            assert await response.read() == b"first immutable revision"
        async with client.get(
            server.origin + resource,
            ssl=trusted,
            params=token,
            headers={"If-Match": '"wrong"', "Origin": origin},
        ) as response:
            assert response.status == 412
            assert response.headers["Access-Control-Allow-Origin"] == origin
        async with client.head(
            server.origin + resource, ssl=trusted, params=token
        ) as response:
            assert response.status == 200
            assert await response.read() == b""
        async with client.get(server.onboarding_url, ssl=trusted) as response:
            body = await response.text()
            assert "endpoint=" in body and "token=" in body
        async with client.get(server.origin + "/", ssl=trusted) as response:
            assert server.token not in await response.text()
        async with client.get(
            server.origin + f"/assets/{server.session}/1/missing",
            ssl=trusted,
            params=token,
        ) as response:
            assert response.status == 404
        prior = server.snapshot
        # Real immutable publication/reads above the former per-asset, snapshot,
        # and retained-storage budgets; no fake server or constant patching.
        current[0] = b"x" * (4 * 1024 * 1024 + 1)
        server.sync()
        large = json.loads(server.snapshot)["scene"]["entities"][0]["mesh"]["source"]
        async with client.get(
            server.origin + large, ssl=trusted, params=token
        ) as response:
            assert response.status == 200
            assert await response.read() == current[0]
        for index in range(32):
            server.store.publish(bytes([index]) + current[0])
        assert server.store.total > 128 * 1024 * 1024
        current.append("padding" * (650 * 1024))
        server.sync()
        async with client.ws_connect(
            server.origin + "/v1/updates",
            ssl=trusted,
            params=token,
            origin=origin,
            max_msg_size=0,
        ) as socket:
            snapshot = await socket.receive_json()
            assert len(snapshot["scene"]["metadata"]) > 4 * 1024 * 1024
        current.pop()
        prior = server.snapshot
        before = set(server.store.entries)
        current[0] = b"new asset before a deliberate extraction failure"
        current.append(float("nan"))
        async with client.post(
            server.origin + "/v1/sync", ssl=trusted, params=token
        ) as response:
            assert response.status == 409
        assert server.snapshot == prior and server.revision == 4
        assert set(server.store.entries) == before
        current.pop()
        # Failed full exports cannot leave deduplication pointing at deleted bytes.
        server.sync()
        recovered = json.loads(server.snapshot)["scene"]["entities"][0]["mesh"][
            "source"
        ]
        async with client.get(
            server.origin + recovered, ssl=trusted, params=token
        ) as response:
            assert response.status == 200
            assert await response.read() == current[0]
        assert server.active_reads == 0


def check_stream_lifecycle(directory):
    mode = ["complete"]
    published = []

    def export(publish, *, stream=None):
        source = publish(f"immutable {mode[0]}".encode())
        published.append(source)
        if mode[0] == "reject-full":
            raise RuntimeError("full export rejected")
        entities = [
            {"id": "child", "parent": "parent", "mesh": {"source": source}},
            {"id": "parent"},
            {"id": "last"},
        ]
        if stream:
            for entity in entities:
                stream.entities([entity])
            stream.flush_entities()
        return {"entities": entities}

    server = SceneServer(export, directory=directory, port=0).start()

    async def exercise_stream():
        trusted = ssl.create_default_context(cafile=str(server.certificate_paths[0]))
        async with ClientSession() as client:
            for attempt in ("complete", "reject", "disconnect"):
                mode[0] = attempt
                previous = server.snapshot
                async with client.ws_connect(
                    server.origin + "/v1/updates",
                    ssl=trusted,
                    params={"token": server.token, "stream": "1"},
                ) as socket:
                    seen = []
                    sequence = 0
                    while True:
                        message = await socket.receive_json()
                        if message["type"] == "abort":
                            assert attempt == "reject"
                            break
                        assert message["sequence"] == sequence
                        sequence += 1
                        if message["type"] == "commit":
                            assert not server.exporting
                            assert server.revision == message["revision"]
                        if message["type"] == "chunk":
                            for entity in message["entities"]:
                                assert (
                                    entity.get("parent") is None
                                    or entity["parent"] in seen
                                )
                                seen.append(entity["id"])
                            if attempt == "disconnect":
                                await socket.close()
                                break
                        await socket.send_json(
                            {
                                "type": "chunk-applied",
                                "session": message["session"],
                                "revision": message["revision"],
                                "transfer": message["transfer"],
                                "sequence": message["sequence"],
                                "ok": not (
                                    attempt == "reject" and message["type"] == "chunk"
                                ),
                            }
                        )
                        if message["type"] == "commit":
                            assert seen == ["parent", "child", "last"]
                            # A completed acknowledgement wins a subsequent close.
                            await socket.close()
                            while server.exporting:
                                await asyncio.sleep(0.001)
                            break
                while server.exporting:
                    await asyncio.sleep(0.001)
                if attempt != "complete":
                    assert server.snapshot == previous
                    # Even rejected/disconnected streams may have committed effects.
                    assert (
                        published[-1].removeprefix("/assets/") in server.store.entries
                    )
            assert server.revision == 2
            assert server.issued_revision == 4
            mode[0] = "reject-full"
            async with client.ws_connect(
                server.origin + "/v1/updates",
                ssl=trusted,
                params={"token": server.token, "refresh": "1"},
            ) as socket:
                assert await socket.receive_json() == {
                    "type": "error",
                    "error": "full export rejected",
                }
            assert server.revision == 2
            assert published[-1].removeprefix("/assets/") not in server.store.entries
            mode[0] = "repaired"
            server.sync()
            assert server.revision == 6
            assert server.active_reads == 0

    try:
        task = server.loop.create_task(asyncio.wait_for(exercise_stream(), 15))
        while not task.done():
            server.pump()
        task.result()
        assert server.loop._default_executor is None
    finally:
        server.close()


def main():
    check_private_asset_reuse()
    threads = {thread.ident for thread in threading.enumerate()}
    current = [b"first immutable revision"]

    def export(publish):
        assert threading.current_thread() is threading.main_thread()
        return {
            "entities": [{"id": "fixture", "mesh": {"source": publish(current[0])}}],
            "metadata": current[1] if len(current) > 1 else "",
        }

    with tempfile.TemporaryDirectory(prefix="ipp-blender-server-test-") as directory:
        server = SceneServer(
            export,
            directory=directory,
            port=0,
            allowed_origins=["https://viewer.example"],
            viewer_url="https://viewer.example/viewer",
        ).start()
        certificate = server.certificate_paths[0].read_bytes()
        session = server.session
        token = server.token
        store_path = server.store.directory
        original_source = json.loads(server.snapshot)["scene"]["entities"][0]["mesh"][
            "source"
        ]
        try:
            task = server.loop.create_task(
                asyncio.wait_for(exercise(server, current), 20)
            )
            while not task.done():
                server.pump()
            task.result()
            assert {thread.ident for thread in threading.enumerate()} == threads
            assert server.loop._default_executor is None
        finally:
            server.close()
        assert not store_path.exists()
        server.close()
        current[0] = b"restart"
        replacement = SceneServer(export, directory=directory, port=server.port).start()
        try:
            assert replacement.session != session and replacement.token != token
            assert replacement.certificate_paths[0].read_bytes() == certificate
            assert replacement.revision == 1

            async def check_old_source():
                trusted = ssl.create_default_context(
                    cafile=str(replacement.certificate_paths[0])
                )
                async with ClientSession() as client:
                    async with client.get(
                        replacement.origin + original_source,
                        ssl=trusted,
                        params={"token": replacement.token},
                    ) as response:
                        assert response.status == 404

            task = replacement.loop.create_task(asyncio.wait_for(check_old_source(), 5))
            while not task.done():
                replacement.pump()
            task.result()
        finally:
            replacement.close()
        try:
            credentials(
                directory, certificate="missing.pem", private_key="missing-key.pem"
            )
            raise AssertionError("Invalid supplied pair was replaced silently")
        except FileNotFoundError:
            pass
        check_stream_lifecycle(directory)
        assert {thread.ident for thread in threading.enumerate()} == threads
    print(
        json.dumps(
            {
                "result": "passed",
                "real_tls": True,
                "real_websocket": True,
                "python_threads_added": 0,
                "immutable_recovery": True,
                "restart_cleanup": True,
            }
        )
    )


if __name__ == "__main__":
    main()
