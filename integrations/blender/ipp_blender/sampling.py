"""Merge synchronous sample jobs into one ordered pass through scene time."""

import heapq
import math


class SamplingSchedule:
    """Jobs yield their next frame, then capture detached data when resumed.

    Each job must request increasing frames. Only one pending frame per job is
    retained; action lengths do not grow the scheduling queue. Blender references
    remain on the main thread, and every exit restores the authoring timeline.
    """

    def __init__(self, exporter):
        self.exporter = exporter
        self.jobs = []
        self.simulation = False

    def add(self, job, *, fractional=False, simulation=False):
        self.jobs.append((job, fractional))
        self.simulation |= simulation

    def run(self):
        scene = self.exporter.scene
        original = (scene.frame_current, scene.frame_subframe)
        try:
            if self.simulation:
                # Subframe evaluations change Blender simulation integration
                # (notably Brownian forces). Preserve the integer bake cadence.
                self._run([job for job, fractional in self.jobs if not fractional])
                self._run([job for job, fractional in self.jobs if fractional])
            else:
                self._run([job for job, _ in self.jobs])
        finally:
            for job, _ in self.jobs:
                job.close()
            if self.jobs:
                scene.frame_set(original[0], subframe=original[1])

    def _run(self, jobs):
        pending = []
        for order, job in enumerate(jobs):
            value = next(job, None)
            if value is not None:
                heapq.heappush(pending, (value, order, job))
        while pending:
            value = pending[0][0]
            self.exporter.scene.frame_set(math.floor(value), subframe=value % 1)
            self.exporter.checkpoint()
            while pending and pending[0][0] == value:
                _, order, job = heapq.heappop(pending)
                following = next(job, None)
                if following is not None:
                    if following <= value:
                        raise ValueError("Sampling jobs must advance scene time")
                    heapq.heappush(pending, (following, order, job))


def action_frames(start, end):
    count = max(2, math.ceil(end - start) + 1)
    for index in range(count):
        yield start + (end - start) * index / (count - 1)
