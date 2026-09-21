"""Reserve immutable names before producing detached export recipes."""

from dataclasses import dataclass
from functools import partial
import hashlib
import pickle
from collections.abc import Callable


@dataclass(frozen=True)
class ProductionJob:
    priority: int
    order: int
    source: str
    content_type: str
    produce: Callable[[], bytes]


class AssetPlan:
    """Own one export's declarations until every source has settled.

    Private recipe keys let the store reuse unchanged immutable names between
    revisions. They never leave the producer and do not become URL semantics.
    """

    def __init__(self, store, available, checkpoint):
        self.store = store
        self.available = available
        self.checkpoint = checkpoint
        self.sources = {}
        self.jobs = []
        self.producing = False
        self.finished = False
        self.next_job = 0

    def _check_open(self):
        if self.finished:
            raise RuntimeError("Asset plan is already finished")

    def reserve(
        self, key, content_type="application/octet-stream", *, reuse_private=False
    ):
        self._check_open()
        if key not in self.sources:
            self.sources[key] = self.store.reserve(
                content_type,
                private_key=("recipe", key) if reuse_private else None,
            )
        return self.sources[key]

    def defer(
        self, encode, args, *, priority=0, content_type="application/octet-stream"
    ):
        self._check_open()
        # Recipes contain detached Python values only. The digest remains private
        # and allows an unchanged recipe to reuse its prior immutable source name.
        recipe = (
            encode.__module__,
            encode.__qualname__,
            hashlib.sha256(pickle.dumps(args)).digest(),
        )
        key = ("recipe", recipe)
        if key in self.sources:
            return self.sources[key]
        source = self.reserve(key, content_type, reuse_private=True)
        if self.store.status(source) == "pending":
            self._add_job(priority, source, content_type, partial(encode, *args))
        return source

    def publish(self, data, content_type, key=None):
        self._check_open()
        if not isinstance(data, bytes):
            raise TypeError("Asset publication requires immutable bytes")
        if key is None:
            key = ("content", content_type, hashlib.sha256(data).digest())
            reuse_private = True
        else:
            reuse_private = False
        source = self.reserve(key, content_type, reuse_private=reuse_private)
        if self.store.status(source) != "pending":
            return source
        if self.producing:
            self.complete(source, data, content_type)
        else:
            priority = 2 if content_type == "application/json" else 0
            self._add_job(priority, source, content_type, lambda: data)
        return source

    def _add_job(self, priority, source, content_type, produce):
        self.jobs.append(
            ProductionJob(priority, self.next_job, source, content_type, produce)
        )
        self.next_job += 1

    def complete(self, source, data, content_type):
        self.store.complete(source, data)
        self.available(source, content_type)

    def produce(self):
        self._check_open()
        self.producing = True
        jobs, self.jobs = self.jobs, []
        for job in sorted(jobs, key=lambda value: (value.priority, value.order)):
            if self.store.status(job.source) != "pending":
                continue
            try:
                self.complete(job.source, job.produce(), job.content_type)
            except BaseException as error:
                self.store.fail(job.source, error)
                raise
            self.checkpoint()

    def finish(self, error="No supported output for this declaration"):
        if self.finished:
            return
        self.finished = True
        self.jobs.clear()
        for source in self.sources.values():
            self.store.fail(source, error)

    def index(self):
        return [self.store.describe(source) for source in self.sources.values()]
