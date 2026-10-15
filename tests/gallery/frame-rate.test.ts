import assert from "node:assert/strict";
import test from "node:test";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import type { PresentedFrame } from "@ipp/client";
import {
  DemoDiagnostics,
  FRAME_RATE_INTERVAL_MS,
} from "../../examples/demos/frame-rate.js";

/** Settle producer continuations without allowing an unrelated real timer to run. */
async function settle() {
  for (let turn = 0; turn < 12; turn++) await Promise.resolve();
}

function fixture(statistics?: () => Promise<unknown>) {
  const frames: ((frame: PresentedFrame) => void)[] = [];
  const writes: unknown[] = [];
  const destroyed: unknown[] = [];
  let created = 0;
  const view = {
    selection: 1n,
    binding: { generation: { host: 1n, serial: 1n } },
  };
  const host = {
    datasets: {
      async create() {
        created++;
        return { id: created };
      },
      async update(_producer: unknown, operations: unknown) {
        writes.push(operations);
        return {};
      },
      async destroy(producer: unknown) {
        destroyed.push(producer);
      },
    },
    presentation: {
      frame: () =>
        new Promise<PresentedFrame>((resolve) => frames.push(resolve)),
      capture: () => assert.fail("Monitoring must never read back pixels"),
    },
  };
  if (statistics)
    Object.defineProperty(host, Symbol.for("ipp.presentation"), {
      value: { statistics },
    });
  const canvas = {
    host,
    client: { session: 1n },
    view,
    viewport: { width: 960, height: 640, devicePixelRatio: 1 },
  } as unknown as IppCanvasHandle;
  const monitor = new DemoDiagnostics(canvas, (error) => {
    throw error;
  });
  return {
    monitor,
    frames,
    writes,
    destroyed,
    get created() {
      return created;
    },
    frame(sequence: bigint) {
      assert.ok(frames.length, "A real presentation observation is pending");
      frames.shift()!({
        view,
        sequence,
        drawCalls: 17,
        triangles: 1234,
        failedDrawCalls: 2,
      } as PresentedFrame);
    },
  };
}

test("overlay and scene share one FPS producer until the final consumer closes", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  let time = 1000;
  context.mock.method(performance, "now", () => time);
  const fixture_ = fixture();
  const overlay = await fixture_.monitor.acquire();
  const scene = await fixture_.monitor.acquire();
  try {
    assert.equal(fixture_.created, 1);
    assert.equal(overlay.source, scene.source);
    assert.equal(scene.snapshot, scene.getSnapshot());
    assert.equal(scene.snapshot.fps, null, "No fabricated initial FPS");
    assert.equal(scene.snapshot.rendererStatus, "unavailable");
    assert.equal(scene.snapshot.renderer, null);
    assert.equal(scene.snapshot.drawCalls, null);
    assert.equal(scene.snapshot.triangles, null);
    assert.equal(scene.snapshot.failedDrawCalls, null);
    fixture_.frame(10n);
    await settle();
    assert.equal(scene.snapshot.drawCalls, 17);
    assert.equal(scene.snapshot.triangles, 1234);
    assert.equal(scene.snapshot.failedDrawCalls, 2);
    assert.equal(FRAME_RATE_INTERVAL_MS, 100);
    time = 1099;
    context.mock.timers.tick(99);
    assert.equal(
      fixture_.frames.length,
      0,
      "No observation before the report interval",
    );
    time = 1100;
    context.mock.timers.tick(1);
    fixture_.frame(16n);
    await settle();
    assert.equal(scene.snapshot.fps, 60);
    assert.equal(scene.snapshot.frameMs, 1000 / 60);
    assert.equal(scene.snapshot.presentedFrames, 16n);
    assert.equal(fixture_.writes.length, 1);
    let retiredNotifications = 0;
    overlay.subscribe(() => retiredNotifications++);
    await overlay.close();
    await overlay.close();
    assert.equal(fixture_.destroyed.length, 0);
    time = 1200;
    context.mock.timers.tick(FRAME_RATE_INTERVAL_MS);
    fixture_.frame(22n);
    await settle();
    assert.equal(
      retiredNotifications,
      0,
      "A released lease loses its subscriptions",
    );
    assert.equal(
      fixture_.writes.length,
      2,
      "The remaining scene keeps sampling",
    );
    await scene.close();
    assert.equal(fixture_.destroyed.length, 1);
    context.mock.timers.tick(5000);
    assert.equal(
      fixture_.frames.length,
      0,
      "No more observations after last release",
    );
    const replacement = await fixture_.monitor.acquire();
    assert.notEqual(replacement.source, scene.source);
    assert.equal(fixture_.created, 2);
    await replacement.close();
  } finally {
    await fixture_.monitor.close();
  }
});

test("closing fences late presentation and renderer responses without waiting for a surface", async () => {
  let resolveStatistics!: (value: unknown) => void;
  const fixture_ = fixture(
    () =>
      new Promise((resolve) => {
        resolveStatistics = resolve;
      }),
  );
  const scene = await fixture_.monitor.acquire();
  const initial = scene.getSnapshot();
  let notifications = 0;
  scene.subscribe(() => notifications++);
  await fixture_.monitor.close();
  await scene.close();
  fixture_.frame(20n);
  resolveStatistics({ device: { renderer: "Late renderer" } });
  await settle();
  assert.equal(scene.getSnapshot(), initial);
  assert.equal(notifications, 0);
  assert.equal(fixture_.writes.length, 0);
  assert.equal(fixture_.destroyed.length, 1);
  await assert.rejects(fixture_.monitor.acquire(), /closed/);
});

test("renderer observations keep actual Host data and expose failures as unavailable", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  let time = 1000;
  context.mock.method(performance, "now", () => time);
  const renderer = {
    device: { renderer: "Observed GPU" },
    gui: { guiBatches: 7 },
  };
  let queries = 0;
  const fixture_ = fixture(async () => {
    if (queries++) throw new Error("Renderer disconnected");
    return renderer;
  });
  const scene = await fixture_.monitor.acquire();
  try {
    await settle();
    assert.equal(scene.snapshot.rendererStatus, "available");
    assert.equal(scene.snapshot.renderer, renderer);
    fixture_.frame(1n);
    await settle();
    time = 3500;
    context.mock.timers.tick(FRAME_RATE_INTERVAL_MS);
    fixture_.frame(151n);
    await settle();
    assert.equal(scene.snapshot.fps, 60);
    assert.equal(scene.snapshot.rendererStatus, "unavailable");
    assert.equal(
      scene.snapshot.renderer,
      null,
      "A failed refresh does not retain misleading live data",
    );
    assert.equal(scene.snapshot.rendererError, "Renderer disconnected");
  } finally {
    await scene.close();
    await fixture_.monitor.close();
  }
});
