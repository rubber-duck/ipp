"""Bounded, acknowledged export streaming on the server's main-thread event loop."""

import asyncio
import json
import secrets
import time


class ExportStream:
    """Send only detached entities whose references have already been sent.

    The final snapshot remains authoritative. Missing/unsupported ancestors stay
    pending and are omitted by the exporter's existing pruning pass.
    """

    def __init__(self, server, socket, batch_size, revision):
        self.server = server
        self.asset_store = server.store
        self.socket = socket
        self.batch_size = batch_size
        self.revision = revision
        self.transfer = secrets.token_hex(16)
        self.sequence = 0
        self.pending = {}
        self.waiting_entities = []
        self.entity_buffer = []
        self.entity_batch_open = False
        self.entity_batches = 0
        self.first_entity_batch_seconds = None
        self.known = set()
        self.clip_buffer = []
        self.clips = set()
        self.pending_reads_before = server.pending_asset_reads
        self.started = time.monotonic()
        self.last_pump = self.started
        self.wait_seconds = 0.0
        self.first_ack_seconds = None
        self.assets_after_first_ack = 0
        self.cancelled = None
        self.send("begin")

    def send(self, kind, *, acknowledged=True, **payload):
        self.checkpoint(force=True)
        if len(self.pending) >= 2:
            self.wait(all_pending=False)
        sequence = self.sequence
        self.sequence += 1
        if acknowledged:
            self.pending[sequence] = self.server.loop.create_future()
        self.server.clients[self.socket].put_nowait(
            json.dumps(
                {
                    "type": kind,
                    "session": self.server.session,
                    "revision": self.revision,
                    "transfer": self.transfer,
                    "sequence": sequence,
                    **payload,
                },
                separators=(",", ":"),
                allow_nan=False,
            )
        )

    def acknowledge(self, message):
        if (
            message.get("transfer") != self.transfer
            or message.get("session") != self.server.session
            or message.get("revision") != self.revision
        ):
            return
        future = self.pending.get(message.get("sequence"))
        if future is not None and not future.done():
            if message.get("ok") is not True:
                self.cancelled = "Viewer rejected the streamed batch"
            elif self.first_ack_seconds is None and message["sequence"] > 0:
                self.first_ack_seconds = time.monotonic() - self.started
            future.set_result(None)

    def checkpoint(self, *, force=False):
        now = time.monotonic()
        if force or now - self.last_pump >= 0.01:
            # Yield only detached I/O callbacks, never Blender UI/evaluation work.
            self.server.loop.run_until_complete(asyncio.sleep(0.001))
            self.last_pump = time.monotonic()
        for sequence, future in list(self.pending.items()):
            if future.done():
                del self.pending[sequence]
        if self.socket.closed or self.socket not in self.server.clients:
            raise RuntimeError("Viewer disconnected during export")
        if self.cancelled:
            raise RuntimeError(self.cancelled)

    def wait(self, *, all_pending):
        started = time.monotonic()

        async def drain():
            async with asyncio.timeout(60):
                while self.pending:
                    for sequence, future in list(self.pending.items()):
                        if future.done():
                            del self.pending[sequence]
                    if not self.pending:
                        break
                    if self.socket.closed or self.cancelled:
                        break
                    await asyncio.wait(
                        list(self.pending.values()),
                        timeout=0.05,
                        return_when=asyncio.FIRST_COMPLETED,
                    )
                    for sequence, future in list(self.pending.items()):
                        if future.done():
                            del self.pending[sequence]
                    if not all_pending and len(self.pending) < 2:
                        break

        self.server.loop.run_until_complete(drain())
        self.wait_seconds += time.monotonic() - started
        if self.cancelled:
            raise RuntimeError(self.cancelled)
        # Once every acknowledgement arrived, a subsequent disconnect cannot
        # undo the completed effect. The next send still checks connection state.
        if self.pending or not all_pending:
            self.checkpoint(force=True)

    def entities(self, entities, *, chained=False):
        self.waiting_entities.extend(entities)
        while True:
            remaining = []
            for entity in self.waiting_entities:
                dependencies = [
                    entity.get("parent"),
                    entity.get("skin", {}).get("skeleton"),
                ]
                if any(
                    value is not None and value not in self.known
                    for value in dependencies
                ):
                    remaining.append(entity)
                    continue
                self.known.add(entity["id"])
                self.entity_buffer.append(entity)
                if len(self.entity_buffer) >= self.batch_size:
                    if chained:
                        self.flush_entities()
                    else:
                        self.complete_entities()
            if len(remaining) == len(self.waiting_entities):
                break
            self.waiting_entities = remaining
        self.checkpoint()

    def flush_entities(self):
        if self.entity_buffer:
            self.entity_batch_open = True
            self.send("chunk", entities=self.entity_buffer)
            self.entity_buffer = []

    def complete_entities(self):
        self.flush_entities()
        if self.entity_batch_open:
            self.send("entities-end")
            self.wait(all_pending=True)
            self.entity_batch_open = False
            self.entity_batches += 1
            if self.first_entity_batch_seconds is None:
                self.first_entity_batch_seconds = time.monotonic() - self.started

    def asset_index(self, entries):
        for offset in range(0, len(entries), self.batch_size):
            self.send("asset-index", assets=entries[offset : offset + self.batch_size])

    def asset(self, source, content_type):
        if self.first_ack_seconds is not None:
            self.assets_after_first_ack += 1
        if content_type == "application/json" and source not in self.clips:
            self.clips.add(source)
            self.clip_buffer.append(source)
            if len(self.clip_buffer) >= self.batch_size:
                self.flush_clips()
        self.checkpoint()

    def flush_clips(self):
        if self.clip_buffer:
            self.complete_entities()
            self.send("chunk", clips=self.clip_buffer)
            self.clip_buffer = []

    def finish(self):
        self.complete_entities()
        self.flush_clips()
        self.wait(all_pending=True)
        return {
            "chunks": self.sequence - 1,
            "pendingAssetReads": self.server.pending_asset_reads
            - self.pending_reads_before,
            "entities": len(self.known),
            "entityBatches": self.entity_batches,
            "firstEntityBatchSeconds": self.first_entity_batch_seconds,
            "clips": len(self.clips),
            "firstAckSeconds": self.first_ack_seconds,
            "assetsPublishedAfterFirstAck": self.assets_after_first_ack,
            "backpressureSeconds": self.wait_seconds,
        }
