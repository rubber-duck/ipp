"""Immutable export names; content deduplication stays private to the store."""

import hashlib
from pathlib import Path
import secrets
import tempfile


class AssetStore:
    def __init__(self, directory=None, *, session=None):
        self.temporary = (
            tempfile.TemporaryDirectory(prefix="ipp-blender-assets-")
            if directory is None
            else None
        )
        self.directory = Path(self.temporary.name if self.temporary else directory)
        self.session = session or secrets.token_hex(16)
        self.revision = 1
        self.next_id = 0
        self.entries = {}
        self.states = {}
        self.changed = lambda name: None
        self.content_names = {}
        self.private_names = {}
        self.total = 0

    @staticmethod
    def _name(source):
        if not isinstance(source, str) or not source.startswith("/assets/"):
            raise ValueError("Invalid asset source")
        return source.removeprefix("/assets/")

    def status(self, source):
        return self.states[self._name(source)]["state"]

    def describe(self, source):
        return dict(self.states[self._name(source)])

    def has_pending(self):
        return any(state["state"] == "pending" for state in self.states.values())

    def reserve(self, content_type="application/octet-stream", *, private_key=None):
        if private_key is not None:
            name = self.private_names.get(private_key)
            state = self.states.get(name)
            if state is not None and state["state"] != "failed":
                if state["contentType"] != content_type:
                    raise ValueError("Private asset key changed content type")
                return f"/assets/{name}"
        self.next_id += 1
        name = f"{self.session}/{self.revision}/{self.next_id}"
        self.states[name] = {
            "source": f"/assets/{name}",
            "state": "pending",
            "contentType": content_type,
        }
        if private_key is not None:
            self.private_names[private_key] = name
        self.changed(name)
        return f"/assets/{name}"

    def complete(self, source, data):
        if not isinstance(data, bytes):
            raise TypeError("Asset publication requires immutable bytes")
        name = self._name(source)
        state = self.states[name]
        if state["state"] != "pending":
            raise ValueError("An immutable source can only be completed once")
        content_type = state["contentType"]
        path = self.directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        self.entries[name] = (path, len(data), content_type)
        self.content_names.setdefault(
            (content_type, hashlib.sha256(data).digest()), name
        )
        self.total += len(data)
        state.update(state="ready", bytes=len(data))
        self.changed(name)
        return source

    def fail(self, source, error):
        name = self._name(source)
        state = self.states[name]
        if state["state"] == "pending":
            state.update(state="failed", error=str(error)[:4096])
            self.private_names = {
                key: value for key, value in self.private_names.items() if value != name
            }
            self.changed(name)

    def publish(self, data, content_type="application/octet-stream"):
        if not isinstance(data, bytes):
            raise TypeError("Asset publication requires immutable bytes")
        content = (content_type, hashlib.sha256(data).digest())
        name = self.content_names.get(content)
        if name is not None:
            return f"/assets/{name}"
        return self.complete(self.reserve(content_type), data)

    def discard(self, names):
        for name in list(names):
            self.states.pop(name, None)
            entry = self.entries.pop(name, None)
            if entry:
                path, size, _ = entry
                path.unlink()
                self.total -= size
        self.content_names = {
            content: name
            for content, name in self.content_names.items()
            if name in self.entries
        }
        self.private_names = {
            key: name for key, name in self.private_names.items() if name in self.states
        }

    def close(self):
        if self.temporary:
            self.temporary.cleanup()
        self.entries.clear()
        self.states.clear()
        self.content_names.clear()
        self.private_names.clear()
        self.total = 0
