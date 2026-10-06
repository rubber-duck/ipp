import assert from "node:assert/strict";
import test from "node:test";
import { decodePng } from "../../tools/shared-host/png.js";
import { runBrowserEnvironment } from "../harness/browser.js";
import {
  exerciseChartRegeneration,
  exerciseChartAdaptiveScope,
  exerciseChartAutomaticRange,
  exerciseChartSmoothChanges,
  exerciseChartStreamSmoothing,
  assertChartFeedback,
} from "./scenarios/gallery-chart-changes.js";
import {
  baselineBarPointer,
  baselineGridPointer,
  assertChartImageChanged,
  focusChart,
  waitForCharts,
  type GalleryChartsDriver,
  type GalleryChartsState,
} from "./scenarios/gallery-charts.js";
import { openGallery, galleryEnvironment } from "./drivers/browser-gallery.js";

/** Await the real pointer action, including its coalesced lane and committed labels. */
async function samplePointer(
  g: Awaited<ReturnType<typeof openGallery>>,
  bounds: { x: number; y: number; width: number; height: number },
  point: { x: number; y: number } | null,
) {
  await g.page.evaluate(
    ({ point, width, height }) => {
      type Action = (name: string, args?: unknown) => Promise<unknown>;
      const chartWindow = window as unknown as {
        ippGalleryScene: { action: Action };
        chartPointerWait?: { wait: Promise<void>; restore(): void };
      };
      const scene = chartWindow.ippGalleryScene;
      const original = scene.action;
      const descriptor = Object.getOwnPropertyDescriptor(scene, "action");
      let resolve!: () => void, reject!: (error: unknown) => void;
      const wait = new Promise<void>((yes, no) => {
        resolve = yes;
        reject = no;
      });
      const timer = setTimeout(
        () => reject(new Error("Chart pointer action was not sampled")),
        10_000,
      );
      let matched = false;
      scene.action = function (name, args) {
        const pending = original.call(this, name, args);
        const sampled = args as { x: number; y: number } | null;
        if (
          !matched &&
          name === "hover" &&
          (point === null
            ? sampled === null
            : sampled !== null &&
              Math.abs(sampled.x - point.x) <= 1 / width &&
              Math.abs(sampled.y - point.y) <= 1 / height)
        ) {
          matched = true;
          void pending.then(resolve, reject);
        }
        return pending;
      };
      chartWindow.chartPointerWait = {
        wait,
        restore() {
          clearTimeout(timer);
          if (descriptor) Object.defineProperty(scene, "action", descriptor);
          else Reflect.deleteProperty(scene, "action");
          delete chartWindow.chartPointerWait;
        },
      };
    },
    { point, width: bounds.width, height: bounds.height },
  );
  try {
    if (point)
      await g.page.mouse.move(
        bounds.x + point.x * bounds.width,
        bounds.y + point.y * bounds.height,
      );
    else await g.page.mouse.move(bounds.x - 8, bounds.y - 8);
    await g.page.evaluate(async () => {
      await (window as unknown as { chartPointerWait: { wait: Promise<void> } })
        .chartPointerWait.wait;
    });
  } finally {
    await g.page.evaluate(() =>
      (
        window as unknown as { chartPointerWait?: { restore(): void } }
      ).chartPointerWait?.restore(),
    );
  }
}

for (const [name, exercise] of [
  [
    "regenerated chart hover follows the stationary pointer and current source lifetime",
    exerciseChartRegeneration,
  ],
  [
    "adaptive gallery axes follow only hovered or selected charts",
    exerciseChartAdaptiveScope,
  ],
  [
    "gallery automatic value ranges fit growing samples inside the fixed physical box",
    exerciseChartAutomaticRange,
  ],
  [
    "gallery existing sample edits render and pick intermediate smoothed values",
    exerciseChartSmoothChanges,
  ],
  [
    "gallery live snapshots glide by window position between stream arrivals",
    exerciseChartStreamSmoothing,
  ],
] as const) {
  test(name, { timeout: 60_000 }, async (context) => {
    await runBrowserEnvironment(
      name,
      galleryEnvironment,
      context.signal,
      async (scenario) => {
        const g = await openGallery(scenario);
        await g.navigate("charts");
        const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
        assert.ok(bounds);
        const driver: GalleryChartsDriver = {
          setSampleInterpolation: (chart, rate) =>
            g.call("galleryChartSampleInterpolation", chart, rate),
          inspect: () => g.call<GalleryChartsState>("gallerySceneState"),
          action: async (name, args) => {
            if (name === "hover") {
              const point = args as { x: number; y: number } | null;
              await samplePointer(g, bounds, point);
              return;
            }
            return g.call("gallerySceneAction", name, args);
          },
          capture: async (label) => {
            const frame = await g.capture(label);
            return decodePng(
              Buffer.from(frame.dataUrl.split(",")[1]!, "base64"),
            );
          },
          record: async (label, value) => {
            await scenario.evidence.writeJson(`${label}.json`, value);
            await scenario.evidence.record(label, {
              artifact: `${label}.json`,
            });
          },
        };
        await exercise(driver, bounds.width / bounds.height);
        assert.deepEqual(g.errors, []);
      },
    );
  });
}

test("gallery source replacement rejects an older successful pick reply", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "gallery delayed source pick",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("charts");
      const inspect = () => g.call<GalleryChartsState>("gallerySceneState");
      const driver: GalleryChartsDriver = {
        inspect,
        action: (name, args) => g.call("gallerySceneAction", name, args),
        capture: async (label) => {
          const frame = await g.capture(label);
          return decodePng(Buffer.from(frame.dataUrl.split(",")[1]!, "base64"));
        },
        record: (label, value) =>
          scenario.evidence.writeJson(`${label}.json`, value),
      };
      await driver.action("streamPlayback", false);
      const state = await focusChart(driver, "bars");
      const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(bounds);
      const point = baselineBarPointer(state, bounds.width / bounds.height);
      await driver.action("hover", point);
      const old = await inspect();
      assert.equal(String(old.hover?.rowId), "2");
      // Delay one real reply after the Host completed its pick; no fabricated result.
      await g.page.evaluate((point) => {
        type Query = (...args: unknown[]) => Promise<unknown>;
        const chartWindow = window as unknown as {
          ippWorldCanvas: { client: { query: Query } };
          ippGalleryScene: {
            action(name: string, args?: unknown): Promise<unknown>;
            options: { dataMode: string };
          };
          chartPickGate?: {
            reached: boolean;
            release(): void;
            restore(): void;
            pending: Promise<unknown>;
            replacement?: Promise<unknown>;
          };
        };
        const client = chartWindow.ippWorldCanvas.client;
        const original = client.query;
        const descriptor = Object.getOwnPropertyDescriptor(client, "query");
        let release!: () => void;
        const barrier = new Promise<void>((resolve) => {
          release = resolve;
        });
        const gate = {
          reached: false,
          release,
          restore() {
            release();
            if (descriptor) Object.defineProperty(client, "query", descriptor);
            else Reflect.deleteProperty(client, "query");
            delete chartWindow.chartPickGate;
          },
          pending: Promise.resolve<unknown>(undefined),
        };
        chartWindow.chartPickGate = gate;
        client.query = async function (...args) {
          const reply = await original.apply(this, args);
          if (
            !gate.reached &&
            (args[0] as { type?: string })?.type === "GeometryPickQuery"
          ) {
            gate.reached = true;
            await barrier;
          }
          return reply;
        };
        gate.pending = chartWindow.ippGalleryScene.action("hover", point);
      }, point);
      try {
        await g.page.waitForFunction(
          () =>
            (window as unknown as { chartPickGate?: { reached: boolean } })
              .chartPickGate?.reached,
        );
        await g.page.evaluate(() => {
          const chartWindow = window as unknown as {
            chartPickGate: { replacement?: Promise<unknown> };
            ippGalleryScene: GalleryChartsDriver;
          };
          chartWindow.chartPickGate.replacement =
            chartWindow.ippGalleryScene.action("dataSource", "streaming");
        });
        await g.page.waitForFunction(
          () =>
            (
              window as unknown as {
                ippGalleryScene: { options: { dataMode: string } };
              }
            ).ippGalleryScene.options.dataMode === "streaming",
        );
        await g.page.evaluate(async () => {
          const gate = (
            window as unknown as {
              chartPickGate: {
                release(): void;
                pending: Promise<unknown>;
                replacement?: Promise<unknown>;
              };
            }
          ).chartPickGate;
          gate.release();
          await Promise.all([gate.pending, gate.replacement]);
        });
        const current = await waitForCharts(
          driver,
          (state) =>
            state.hover?.chart === "bars" && String(state.hover.rowId) === "2",
          "chart-delayed-pick-current-source",
        );
        assert.notEqual(
          String(current.hover?.sourceIncarnation),
          String(old.hover?.sourceIncarnation),
        );
        assertChartFeedback(current);
        await driver.record("chart-old-successful-pick-fenced", {
          old,
          current,
        });
        await g.capture("chart-old-successful-pick-current-highlight");
      } finally {
        await g.page.evaluate(() =>
          (
            window as unknown as { chartPickGate?: { restore(): void } }
          ).chartPickGate?.restore(),
        );
      }
      assert.deepEqual(g.errors, []);
    },
  );
});

test("gallery pointer leave clears feedback across a pending pick and view change", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "gallery delayed pointer leave",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("charts");
      const bounds = await g.page.locator("#ipp-world-canvas").boundingBox();
      assert.ok(bounds);
      const driver: GalleryChartsDriver = {
        inspect: () => g.call<GalleryChartsState>("gallerySceneState"),
        action: (name, args) => g.call("gallerySceneAction", name, args),
        capture: async (label) => {
          const frame = await g.capture(label);
          return decodePng(Buffer.from(frame.dataUrl.split(",")[1]!, "base64"));
        },
        record: (label, value) =>
          scenario.evidence.writeJson(`${label}.json`, value),
      };
      const focused = await focusChart(driver, "grid-bars");
      const point = baselineGridPointer(focused, bounds.width / bounds.height);
      await samplePointer(g, bounds, point);
      await driver.action("adaptiveAxes", true);
      const active = await driver.inspect();
      assert.equal(active.hover?.chart, "grid-bars");
      assertChartFeedback(active);
      const highlighted = await driver.capture("chart-pending-leave-highlight");
      await g.page.evaluate(() => {
        type Query = (...args: unknown[]) => Promise<unknown>;
        type Action = (name: string, args?: unknown) => Promise<unknown>;
        const chartWindow = window as unknown as {
          ippWorldCanvas: { client: { query: Query } };
          ippGalleryScene: { action: Action };
          chartLeaveGate?: {
            reached: boolean;
            leaveQueued: boolean;
            pending: Promise<unknown>[];
            transition?: Promise<unknown>;
            release(): void;
            restore(): void;
          };
        };
        const client = chartWindow.ippWorldCanvas.client;
        const scene = chartWindow.ippGalleryScene;
        const originalQuery = client.query;
        const originalAction = scene.action;
        const queryDescriptor = Object.getOwnPropertyDescriptor(
          client,
          "query",
        );
        const actionDescriptor = Object.getOwnPropertyDescriptor(
          scene,
          "action",
        );
        let release!: () => void;
        const barrier = new Promise<void>((resolve) => {
          release = resolve;
        });
        const gate = {
          reached: false,
          leaveQueued: false,
          pending: [] as Promise<unknown>[],
          release,
          restore() {
            release();
            if (queryDescriptor)
              Object.defineProperty(client, "query", queryDescriptor);
            else Reflect.deleteProperty(client, "query");
            if (actionDescriptor)
              Object.defineProperty(scene, "action", actionDescriptor);
            else Reflect.deleteProperty(scene, "action");
            delete chartWindow.chartLeaveGate;
          },
        };
        chartWindow.chartLeaveGate = gate;
        client.query = async function (...args) {
          const reply = await originalQuery.apply(this, args);
          if (
            !gate.reached &&
            (args[0] as { type?: string })?.type === "GeometryPickQuery"
          ) {
            gate.reached = true;
            await barrier;
          }
          return reply;
        };
        scene.action = function (name, args) {
          const pending = originalAction.call(this, name, args);
          if (name === "hover") {
            gate.pending.push(pending);
            if (args === null) gate.leaveQueued = true;
          }
          return pending;
        };
      });
      try {
        // Real pointer input queues a leave behind the held successful pick.
        await g.page.mouse.move(
          bounds.x + point.x * bounds.width + 2,
          bounds.y + point.y * bounds.height,
        );
        await g.page.waitForFunction(
          () =>
            (window as unknown as { chartLeaveGate?: { reached: boolean } })
              .chartLeaveGate?.reached,
        );
        await g.page.mouse.move(bounds.x - 8, bounds.y - 8);
        await g.page.waitForFunction(
          () =>
            (window as unknown as { chartLeaveGate?: { leaveQueued: boolean } })
              .chartLeaveGate?.leaveQueued,
        );
        await g.page.evaluate(() => {
          const chartWindow = window as unknown as {
            chartLeaveGate: { transition?: Promise<unknown> };
            ippGalleryScene: GalleryChartsDriver;
          };
          chartWindow.chartLeaveGate.transition =
            chartWindow.ippGalleryScene.action("automaticRange", true);
        });
        await g.page.waitForFunction(
          () =>
            (
              window as unknown as {
                ippGalleryScene: { options: { automaticRange: boolean } };
              }
            ).ippGalleryScene.options.automaticRange,
        );
        await g.page.evaluate(async () => {
          const gate = (
            window as unknown as {
              chartLeaveGate: {
                release(): void;
                pending: Promise<unknown>[];
                transition?: Promise<unknown>;
              };
            }
          ).chartLeaveGate;
          gate.release();
          await Promise.all([...gate.pending, gate.transition]);
        });
        const cleared = await waitForCharts(
          driver,
          (state) => state.hover === null,
          "chart-pending-leave-cleared",
        );
        assertChartFeedback(cleared);
        for (const chart of cleared.charts.filter((chart) =>
          chart.component.endsWith("3d"),
        )) {
          const frame = chart.inspection.entities
            .find((entity) => String(entity.id) === String(chart.entity))!
            .components.find(
              (component) => "adaptive_axes" in component.fields,
            )!;
          assert.equal(frame.fields.adaptive_axes, false);
        }
        assertChartImageChanged(
          highlighted,
          await driver.capture("chart-pending-leave-clear"),
          "Pointer leave clears the actual highlighted row",
        );
        await driver.record("chart-pending-leave-view-change", {
          active,
          cleared,
        });
      } finally {
        await g.page.evaluate(() =>
          (
            window as unknown as { chartLeaveGate?: { restore(): void } }
          ).chartLeaveGate?.restore(),
        );
      }
      assert.deepEqual(g.errors, []);
    },
  );
});
