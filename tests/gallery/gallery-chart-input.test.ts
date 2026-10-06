import assert from "node:assert/strict";
import test from "node:test";
import type { Page } from "playwright";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  baselineBarPointer,
  assertBoundedChartInput,
  assertCenterCamera,
  assertNoChartAnimation,
  focusChart,
  waitForCharts,
  type ChartPose,
  type GalleryChartsDriver,
  type GalleryChartsState,
} from "./scenarios/gallery-charts.js";
import { decodePng } from "../../tools/shared-host/png.js";
import {
  assertUndistortedChartSamples,
  type StreamingChartsState,
} from "./scenarios/gallery-chart-streaming.js";
import { openGallery, galleryEnvironment } from "./drivers/browser-gallery.js";
import {
  observeChartWork,
  waitForChartUpdates,
} from "./support/gallery-chart-observer.js";

/** Hold page input sampling callbacks; the worker Host keeps its own clock. */
async function holdSamplingFrames(page: Page) {
  await page.evaluate(() => {
    const chartWindow = window as Window & { releaseChartFrames?: () => void };
    const request = window.requestAnimationFrame;
    const cancel = window.cancelAnimationFrame;
    const callbacks = new Map<number, FrameRequestCallback>();
    let next = -1;
    window.requestAnimationFrame = (callback) => {
      const id = next--;
      callbacks.set(id, callback);
      return id;
    };
    window.cancelAnimationFrame = (id) => {
      if (id < 0) callbacks.delete(id);
      else cancel.call(window, id);
    };
    chartWindow.releaseChartFrames = () => {
      window.requestAnimationFrame = request;
      window.cancelAnimationFrame = cancel;
      delete chartWindow.releaseChartFrames;
      for (const callback of callbacks.values()) request.call(window, callback);
    };
  });
}

async function releaseSamplingFrames(page: Page) {
  await page.evaluate(() => {
    (
      window as Window & { releaseChartFrames?: () => void }
    ).releaseChartFrames?.();
  });
}

function assertYawTurn(before: ChartPose, after: ChartPose, yaw: number) {
  const sine = Math.sin(yaw / 2),
    cosine = Math.cos(yaw / 2);
  const expected = [
    cosine * before.qx + sine * before.qz,
    cosine * before.qy + sine * before.qw,
    cosine * before.qz - sine * before.qx,
    cosine * before.qw - sine * before.qy,
  ];
  const actual = [after.qx, after.qy, after.qz, after.qw];
  assert.ok(
    Math.abs(
      Math.abs(
        expected.reduce((dot, value, i) => dot + value * actual[i]!, 0),
      ) - 1,
    ) < 0.00001,
    "The complete intended horizontal displacement reaches the camera orientation",
  );
}

async function assertReleasedCamera(
  driver: GalleryChartsDriver,
  label: string,
) {
  await driver.action("endNavigation");
  const released = await driver.inspect();
  const later = await waitForCharts(
    driver,
    (state) => state.world.time >= released.world.time + 0.4,
    `${label}-stationary`,
  );
  assert.deepEqual(
    later.camera.transform,
    released.camera.transform,
    "Released input leaves no delayed camera motion",
  );
  assert.equal(later.focus, null);
  assertBoundedChartInput(later);
  await driver.record(label, { released, later });
  return released;
}

test("chart input stays bounded, keeps the newest intent and stops after release", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "gallery chart input",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("charts");
      const driver: GalleryChartsDriver = {
        inspect: () => g.call<GalleryChartsState>("gallerySceneState"),
        action: (name, args) => g.call("gallerySceneAction", name, args),
        capture: async (label) => {
          const frame = await g.capture(label);
          return decodePng(Buffer.from(frame.dataUrl.split(",")[1]!, "base64"));
        },
        record: async (label, value) => {
          await scenario.evidence.writeJson(`${label}.json`, value);
          await scenario.evidence.record(label, { artifact: `${label}.json` });
        },
      };
      // Observation mode runs on both revisions, without asserting the changed contract.
      // It counts real calls only; inspection and pixel capture stay outside each window.
      if (process.env.IPP_CHART_WORK_ONLY === "1") {
        const initial = await driver.inspect();
        await driver.record("chart-default-controller-observation", {
          playing:
            (initial as unknown as { playing?: boolean }).playing ?? false,
          worlds: [
            ...new Map(
              initial.charts.map((chart) => [
                String(chart.world.id),
                chart.inspection,
              ]),
            ),
          ].map(([world, inspection]) => ({
            world,
            time: inspection.time,
            tick: inspection.tick,
            controllers: inspection.controllers,
          })),
        });
        // The diagnostic deliberately supports the older animated revision too.
        if ((initial as unknown as { playing?: boolean }).playing)
          await driver.action("playback", { playing: false, time: 0 });
        const state = await focusChart(driver, "bars");
        const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
        assert.ok(bounds);
        const point = baselineBarPointer(state, bounds.width / bounds.height);
        const hover = await observeChartWork(g.page, async () => {
          await driver.action("hover", point);
        });
        await focusChart(driver, "center");
        const turn = await observeChartWork(g.page, async () => {
          await driver.action("navigate", {
            kind: "rotate",
            yaw: 0.1,
            pitch: 0,
          });
        });
        await driver.action("dataSource", "streaming");
        await driver.action("streamPlayback", false);
        const stream = await observeChartWork(g.page, async () => {
          await driver.action("streamPlayback", true);
          await waitForChartUpdates(g.page, 18);
          await driver.action("streamPlayback", false);
        });
        await driver.record("chart-public-call-observation", {
          diagnostic:
            "Public method counts; no timing or hardware FPS inference",
          hover,
          turn,
          stream,
        });
        await g.capture("chart-observation-completed-frame");
        assert.deepEqual(g.errors, []);
        return;
      }
      await exerciseChartBrowserInput(g, driver);
    },
  );
});

async function exerciseChartBrowserInput(
  g: Awaited<ReturnType<typeof openGallery>>,
  driver: GalleryChartsDriver,
) {
  const initial = await driver.inspect();
  assertNoChartAnimation(initial);
  assertCenterCamera(initial);
  const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
  assert.ok(bounds);
  const from = [
    bounds.x + bounds.width * 0.1,
    bounds.y + bounds.height * 0.12,
  ] as const;
  const to = [from[0] + bounds.width * 0.12, from[1]] as const;

  // Trusted mouse input with page RAF held proves release keeps a short gesture.
  await holdSamplingFrames(g.page);
  try {
    await g.page.mouse.move(...from);
    await g.page.mouse.down();
    await g.page.mouse.move(...to);
    await g.page.mouse.up();
  } finally {
    await releaseSamplingFrames(g.page);
  }
  const quick = await assertReleasedCamera(driver, "chart-quick-drag-release");
  assertCenterCamera(quick);
  assertYawTurn(
    initial.camera.transform,
    quick.camera.transform,
    (-(to[0] - from[0]) / bounds.height) * Math.PI,
  );

  await g.page.mouse.move(...from);
  await g.page.mouse.down();
  await g.page.mouse.move(from[0] + bounds.width * 0.35, from[1], {
    steps: 64,
  });
  await g.page.mouse.up();
  const burstDrag = await assertReleasedCamera(
    driver,
    "chart-pointer-burst-release",
  );
  assertCenterCamera(burstDrag);
  assertYawTurn(
    quick.camera.transform,
    burstDrag.camera.transform,
    ((-bounds.width * 0.35) / bounds.height) * Math.PI,
  );

  const beforeBurst = await driver.inspect();
  const navigation = await observeChartWork(g.page, async () => {
    await g.page.evaluate(async () => {
      const scene = (
        window as unknown as { ippGalleryScene: GalleryChartsDriver }
      ).ippGalleryScene;
      await Promise.all(
        Array.from({ length: 64 }, () =>
          scene.action("navigate", { kind: "rotate", yaw: 0.005, pitch: 0 }),
        ),
      );
    });
  });
  const afterBurst = await driver.inspect();
  assertBoundedChartInput(afterBurst);
  assertYawTurn(
    beforeBurst.camera.transform,
    afterBurst.camera.transform,
    0.32,
  );
  assert.equal(
    afterBurst.input.camera.submitted - beforeBurst.input.camera.submitted,
    64,
  );
  assert.ok(
    afterBurst.input.camera.executed - beforeBurst.input.camera.executed <= 2,
    "A same-turn burst executes at most active and coalesced camera work",
  );
  assert.ok(
    afterBurst.input.camera.coalesced - beforeBurst.input.camera.coalesced >=
      62,
  );
  assert.equal(
    navigation.calls["world.inspect"] ?? 0,
    0,
    "Camera input avoids full World inspection",
  );
  assert.ok(
    (navigation.calls["scene.notify"] ?? 0) <= 2,
    "Coalesced camera callers publish only committed semantic changes",
  );
  await driver.record("chart-camera-burst-work", {
    navigation,
    before: beforeBurst.input,
    after: afterBurst.input,
  });

  // New focus is admitted while older camera work is still pending.
  await g.page.evaluate(async () => {
    const scene = (
      window as unknown as { ippGalleryScene: GalleryChartsDriver }
    ).ippGalleryScene;
    const stale = Array.from({ length: 48 }, () =>
      scene.action("navigate", { kind: "rotate", yaw: 0.02, pitch: 0 }),
    );
    const first = scene.action("focus", "grid-bars");
    const newest = scene.action("focus", "bars");
    await Promise.all([...stale, first, newest]);
  });
  const latest = await waitForCharts(
    driver,
    (state) => state.focus?.chart === "bars" && state.focus.time >= 1.999,
    "latest-focus-completed",
  );
  assertBoundedChartInput(latest);
  assert.equal(
    latest.world.controllers?.length,
    1,
    "Superseded focus leaves one camera controller",
  );
  const latestTarget = latest.focus!.to;
  for (const axis of ["x", "y", "z"] as const)
    assert.ok(
      Math.abs(latest.camera.transform[axis] - latestTarget[axis]) < 0.01,
    );
  await driver.record("chart-focus-supersedes-camera-burst", latest);

  // Focus buttons remain newest when blur discards wheel input held before them.
  await holdSamplingFrames(g.page);
  try {
    await g.page.evaluate(() => {
      const canvas =
        document.querySelector<HTMLCanvasElement>("#ipp-world-canvas")!;
      canvas.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: 120,
          bubbles: true,
          cancelable: true,
        }),
      );
      document
        .querySelector<HTMLButtonElement>("#charts-focus-grid-bars")!
        .click();
      document.querySelector<HTMLButtonElement>("#charts-focus-bars")!.click();
      window.dispatchEvent(new Event("blur"));
    });
  } finally {
    await releaseSamplingFrames(g.page);
  }
  const buttonFocus = await waitForCharts(
    driver,
    (state) => state.focus?.chart === "bars" && state.focus.time >= 1.999,
    "focus-button-survives-stale-wheel-blur",
  );
  assertBoundedChartInput(buttonFocus);
  assert.equal(buttonFocus.world.controllers?.length, 1);
  await driver.record(
    "chart-focus-button-survives-stale-wheel-blur",
    buttonFocus,
  );

  const point = baselineBarPointer(buttonFocus, bounds.width / bounds.height);
  const clientPoint = [
    bounds.x + point.x * bounds.width,
    bounds.y + point.y * bounds.height,
  ] as const;
  await g.page.mouse.move(...from);
  await g.page.mouse.move(...clientPoint, { steps: 48 });
  const movedHover = await waitForCharts(
    driver,
    (state) => state.hover?.chart === "bars" && state.hover.rowId === 2n,
    "latest-pointer-hover",
  );
  assertBoundedChartInput(movedHover);
  await driver.record("chart-pointer-latest-hover", movedHover);
  const hoverWork = await observeChartWork(g.page, async () => {
    await g.page.evaluate(async (point) => {
      const scene = (
        window as unknown as { ippGalleryScene: GalleryChartsDriver }
      ).ippGalleryScene;
      await Promise.all(
        Array.from({ length: 48 }, (_, index) =>
          scene.action("hover", index % 2 ? point : null),
        ),
      );
    }, point);
  });
  const newestHover = await driver.inspect();
  assert.equal(newestHover.hover?.rowId, 2n);
  assertBoundedChartInput(newestHover);
  assert.equal(
    hoverWork.calls["datasets.read"] ?? 0,
    0,
    "Hover reads no unrelated raw datasets",
  );
  assert.ok(
    (hoverWork.calls["datasets.bindingView"] ?? 0) <= 2,
    "Hover fetches only the hit row's binding",
  );
  assert.ok(
    (hoverWork.calls["world.query.GeometryPickQuery"] ?? 0) <= 2,
    "A same-turn hover burst executes only active and latest work",
  );
  assert.ok(
    (hoverWork.calls["scene.notify"] ?? 0) <= 2,
    "Hover burst notifications follow semantic changes, not caller count",
  );
  await driver.record("chart-hover-burst-work", {
    hoverWork,
    input: newestHover.input,
  });
  const repeatedHover = await observeChartWork(g.page, async () => {
    await g.page.evaluate(async (point) => {
      const scene = (
        window as unknown as { ippGalleryScene: GalleryChartsDriver }
      ).ippGalleryScene;
      await Promise.all(
        Array.from({ length: 32 }, () => scene.action("hover", point)),
      );
    }, point);
  });
  assert.equal(
    repeatedHover.calls["scene.notify"] ?? 0,
    0,
    "Repeated hover on the same mark emits no UI notification",
  );
  await driver.record("chart-same-mark-hover-work", repeatedHover);
  const clearedWork = await observeChartWork(g.page, async () => {
    await g.page.evaluate(async (point) => {
      const scene = (
        window as unknown as { ippGalleryScene: GalleryChartsDriver }
      ).ippGalleryScene;
      await Promise.all([
        scene.action("hover", point),
        scene.action("hover", null),
      ]);
    }, point);
  });
  assert.equal(
    (await driver.inspect()).hover,
    null,
    "A stale hit cannot restore hover after the pointer leaves",
  );
  await driver.record("chart-hover-clear-wins", clearedWork);
  await exerciseHoverDuringSourceReplacement(g, driver, from, clientPoint);

  await g.page.locator("#charts-focus-grid-bars").click();
  await waitForCharts(
    driver,
    (state) =>
      state.focus?.chart === "grid-bars" &&
      state.focus.time >= 0.2 &&
      state.focus.time < 1.8,
    "pointer-interrupts-active-focus",
  );
  await g.page.mouse.move(...from);
  await g.page.mouse.down();
  await g.page.mouse.move(...to, { steps: 16 });
  await g.page.mouse.up();
  const interrupted = await assertReleasedCamera(
    driver,
    "chart-pointer-focus-interruption",
  );
  assert.equal(
    interrupted.world.controllers?.length ?? 0,
    0,
    "Manual browser input releases the canceled camera controller",
  );
  await g.capture("chart-input-final-frame");
  assert.deepEqual(g.errors, []);
}

async function exerciseHoverDuringSourceReplacement(
  g: Awaited<ReturnType<typeof openGallery>>,
  driver: GalleryChartsDriver,
  from: readonly [number, number],
  point: readonly [number, number],
) {
  await driver.action("streamPlayback", false);
  await g.page.mouse.move(...from);
  // Gate one actual successful source-create reply, preserving all real transport work.
  await g.page.evaluate(() => {
    const chartWindow = window as unknown as {
      ippWorldCanvas: {
        host: { datasets: { create(...args: unknown[]): Promise<unknown> } };
      };
      chartSourceGate?: { reached: boolean; release(): void; restore(): void };
    };
    const datasets = chartWindow.ippWorldCanvas.host.datasets;
    const original = datasets.create;
    const descriptor = Object.getOwnPropertyDescriptor(datasets, "create");
    let release!: () => void;
    const admitted = new Promise<void>((resolve) => {
      release = resolve;
    });
    const gate = {
      reached: false,
      release,
      restore() {
        release();
        if (descriptor) Object.defineProperty(datasets, "create", descriptor);
        else Reflect.deleteProperty(datasets, "create");
        delete chartWindow.chartSourceGate;
      },
    };
    chartWindow.chartSourceGate = gate;
    datasets.create = async function (...args: unknown[]) {
      const result = await original.apply(this, args);
      if (!gate.reached && String(args[0]).includes("/streaming/")) {
        gate.reached = true;
        await admitted;
      }
      return result;
    };
  });
  try {
    await g.page.locator("#charts-data-source").selectOption("streaming");
    await g.page.waitForFunction(
      () =>
        (window as unknown as { chartSourceGate?: { reached: boolean } })
          .chartSourceGate?.reached === true,
    );
    await g.page.mouse.move(...point, { steps: 12 });
    await g.page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
        }),
    );
    await g.page.evaluate(() => {
      (
        window as unknown as { chartSourceGate: { release(): void } }
      ).chartSourceGate.release();
    });
    const replaced = (await driver.inspect()) as StreamingChartsState;
    assert.equal(replaced.data.mode, "streaming");
    assert.equal(replaced.selection, null);
    assertUndistortedChartSamples(replaced);
    if (replaced.hover) {
      const chart = replaced.charts.find(
        (chart) => chart.id === replaced.hover!.chart,
      )!;
      const row = chart.binding.rows.find(
        (row) => row.id === replaced.hover!.rowId,
      );
      assert.ok(
        row,
        "Hover during replacement resolves against current binding membership",
      );
      assert.deepEqual(
        replaced.hover.values,
        row.values,
        "Hover during replacement never publishes stale source values",
      );
    }
    assertBoundedChartInput(replaced);
    await driver.record("chart-hover-during-source-replacement", replaced);
  } finally {
    await g.page.evaluate(() => {
      (
        window as unknown as { chartSourceGate?: { restore(): void } }
      ).chartSourceGate?.restore();
    });
  }
  // No hover point or selection remains, so no row feedback needs following.
  // Feedback on a positional live chart would read its binding view after
  // arrivals and pauses to follow the glide; this measures the path without it.
  await driver.action("hover", null);
  const streamWork = await observeChartWork(g.page, async () => {
    await driver.action("streamPlayback", true);
    await waitForChartUpdates(g.page, 18);
    await driver.action("streamPlayback", false);
  });
  assert.equal(
    streamWork.calls["world.inspect"] ?? 0,
    0,
    "Ordinary stream arrivals avoid whole-World readiness inspection",
  );
  assert.equal(
    streamWork.calls["datasets.read"] ?? 0,
    0,
    "Stream arrival processing avoids raw-source diagnostic enumeration",
  );
  assert.equal(
    streamWork.calls["datasets.bindingView"] ?? 0,
    0,
    "Stream arrival processing reads no binding when no row feedback needs reconciliation",
  );
  assert.equal(
    Object.entries(streamWork.calls)
      .filter(([name]) => name.startsWith("world.inspectPage."))
      .reduce((count, [, calls]) => count + calls, 0),
    0,
    "Ordinary stream arrivals use frame events without readiness polling",
  );
  await driver.record("chart-stream-arrival-work", streamWork);
  await driver.action("dataSource", "buffer");
}
