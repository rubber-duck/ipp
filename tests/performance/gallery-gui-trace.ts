/**
 * Trace the gallery GUI demo's frame cost in named interaction states.
 *
 * Each state gets one counter window with a Chrome trace of every thread,
 * then one CPU-profile window of the runtime worker and the page's main
 * thread. Counters come from the renderer's statistics totals, page-side
 * message and React commit counts; nothing here asserts. Run the compiled
 * file with `node --test`; `IPP_TRACE_OUTPUT` names the output directory and
 * `IPP_BROWSER_ANGLE` selects a hardware ANGLE backend as in other browser runs.
 *
 * States: 1 idle at load (SCAN on), 2 idle with SCAN off and reduced motion,
 * 3 pointer sweeping the panel, 4 dragging GAIN, 5 layers exploded, 6 camera
 * orbit with everything static. `IPP_TRACE_STATES`, `IPP_TRACE_VIEWPORT`
 * (`WxH`), `IPP_TRACE_DPR` and `IPP_TRACE_ISOLATE=1` (panel Surface only)
 * select the run. Headless Vulkan composites in software unless
 * `IPP_BROWSER_GPU_COMPOSITING=1`: its per-frame canvas readback and software
 * compositor then pace frames at about 36 per second whatever the content.
 * Statistics are read every frame, which costs the worker about half a
 * millisecond of synchronous device queries per frame. Worker profiles carry
 * function names only from a runtime built without symbol stripping.
 */
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import type { Page } from "playwright";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "../render/gallery-driver.js";
import {
  CANVAS_SIZE,
  controlPoint,
  gainFraction,
  projectContent,
} from "../render/gallery-gui-panel.js";
import {
  awaitStationIdle,
  toggleLayers,
  type Gallery,
} from "../render/gallery-gui-support.js";
import { sampleWorkerCpu } from "./worker-profiling.js";

const OUTPUT = resolve(
  process.env.IPP_TRACE_OUTPUT ?? "target/performance/gallery-gui-trace",
);
const WINDOW_MS = Number(process.env.IPP_TRACE_WINDOW_MS ?? 3_000);
const [viewportWidth, viewportHeight] = (
  process.env.IPP_TRACE_VIEWPORT ?? "1920x1080"
)
  .split("x")
  .map(Number);
const VIEWPORT = { width: viewportWidth!, height: viewportHeight! };
const DEVICE_SCALE = Number(process.env.IPP_TRACE_DPR ?? 1);
/** Unmount the projector scene first, so the panel's Surface is the whole workload. */
const ISOLATE = process.env.IPP_TRACE_ISOLATE === "1";
/** Comma-separated state numbers to trace; every state by default. */
const STATES = new Set(
  (process.env.IPP_TRACE_STATES ?? "1,2,3,4,5,6")
    .split(",")
    .map((state) => state.trim()),
);
const selected = (name: string) => STATES.has(name.split("-")[0]!);

/** Page-side counters: messages to and from the runtime, React commits. */
function installPageCounters() {
  type Counters = {
    sent: Record<string, { count: number; bytes: number }>;
    received: { count: number; bytes: number };
    commits: Record<string, number>;
  };
  const counters: Counters = {
    sent: {},
    received: { count: 0, bytes: 0 },
    commits: {},
  };
  (window as unknown as { __ippTrace: Counters }).__ippTrace = counters;
  const size = (data: unknown): number => {
    if (data instanceof ArrayBuffer) return data.byteLength;
    if (ArrayBuffer.isView(data)) return data.byteLength;
    if (data && typeof data === "object") {
      let total = 0;
      for (const value of Object.values(data)) {
        if (value instanceof ArrayBuffer) total += value.byteLength;
        else if (ArrayBuffer.isView(value)) total += value.byteLength;
      }
      return total;
    }
    return 0;
  };
  for (const prototype of [MessagePort.prototype, Worker.prototype]) {
    const post = prototype.postMessage as (...args: unknown[]) => void;
    prototype.postMessage = function (this: unknown, ...args: unknown[]) {
      const data = args[0] as { type?: unknown } | undefined;
      const type =
        data && typeof data === "object" && typeof data.type === "string"
          ? data.type
          : "other";
      const entry = (counters.sent[type] ??= { count: 0, bytes: 0 });
      entry.count += 1;
      entry.bytes += size(data);
      return post.apply(this, args);
    } as typeof prototype.postMessage;
  }
  const countIncoming = (event: Event) => {
    counters.received.count += 1;
    counters.received.bytes += size((event as MessageEvent).data);
  };
  for (const prototype of [MessagePort.prototype, Worker.prototype]) {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, "onmessage");
    if (descriptor?.set) {
      const set = descriptor.set;
      Object.defineProperty(prototype, "onmessage", {
        ...descriptor,
        set(this: EventTarget, listener: ((event: Event) => void) | null) {
          set.call(
            this,
            listener &&
              ((event: Event) => {
                countIncoming(event);
                listener.call(this, event);
              }),
          );
        },
      });
    }
    const add = prototype.addEventListener as (...args: unknown[]) => void;
    prototype.addEventListener = function (this: unknown, ...args: unknown[]) {
      const [type, listener] = args as [string, unknown];
      if (type === "message" && typeof listener === "function") {
        args[1] = function (this: unknown, event: Event) {
          countIncoming(event);
          return (listener as (event: Event) => unknown).call(this, event);
        };
      }
      return add.apply(this, args);
    } as typeof prototype.addEventListener;
  }
  // React reports every commit of every renderer to this hook.
  const renderers = new Map<number, string>();
  (
    window as unknown as { __REACT_DEVTOOLS_GLOBAL_HOOK__: unknown }
  ).__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
    supportsFiber: true,
    isDisabled: false,
    renderers: new Map(),
    inject(renderer: { rendererPackageName?: string }) {
      const id = renderers.size + 1;
      renderers.set(id, renderer.rendererPackageName ?? `renderer-${id}`);
      return id;
    },
    onCommitFiberRoot(id: number) {
      const name = renderers.get(id) ?? `renderer-${id}`;
      counters.commits[name] = (counters.commits[name] ?? 0) + 1;
    },
    onCommitFiberUnmount() {},
    onPostCommitFiberRoot() {},
    onScheduleFiberRoot() {},
    checkDCE() {},
  };
}

interface FrameWindow {
  readonly label: string;
  readonly elapsedMs: number;
  readonly frames: number;
  readonly ticks: number;
  readonly drawCalls: readonly number[];
  readonly triangles: readonly number[];
  readonly perFrame: Record<string, number[]>;
  readonly totals: Record<string, number>;
  readonly before: unknown;
  readonly after: unknown;
  readonly sent: Record<string, { count: number; bytes: number }>;
  readonly received: { count: number; bytes: number };
  readonly commits: Record<string, number>;
  readonly device: unknown;
  readonly surfaceCaches: unknown;
}

/** Observe completed frames for `durationMs` while `drive` runs. */
async function frameWindow(
  page: Page,
  label: string,
  durationMs: number,
  drive: (until: number) => Promise<void>,
): Promise<FrameWindow> {
  const observing = page.evaluate(
    async ({ label, durationMs }) => {
      const handle = (window as unknown as { ippWorldCanvas: any })
        .ippWorldCanvas;
      const port =
        handle.host[Symbol.for("ipp.presentation")] ??
        handle.client[Symbol.for("ipp.presentation")];
      const counters = (window as unknown as { __ippTrace: any }).__ippTrace;
      const copy = (value: unknown) => JSON.parse(JSON.stringify(value));
      const flat = (statistics: any) => ({
        ...statistics.frame,
        ...statistics.gui,
        ...Object.fromEntries(
          Object.entries(statistics.surfaces).filter(
            ([key]) => key !== "surfaceCaches",
          ),
        ),
        layoutReflows: statistics.guiLayout?.total?.reflows ?? 0,
        layoutVisited: statistics.guiLayout?.total?.visitedEntities ?? 0,
        layoutMeasurements: statistics.guiLayout?.total?.textMeasurements ?? 0,
        shaderProgramsCreated: statistics.device?.shaderProgramsCreated ?? 0,
      });
      const first = await handle.frame();
      const before = await port.statistics();
      const sentBefore = copy(counters.sent);
      const receivedBefore = copy(counters.received);
      const commitsBefore = copy(counters.commits);
      const start = performance.now();
      const wallStart = Date.now();
      let tick: bigint | undefined;
      let startTick: bigint | undefined;
      let frames = 0;
      const perFrame: Record<string, number[]> = {};
      while (performance.now() - start < durationMs) {
        const frame = await handle.client.waitForFrame(tick);
        tick = frame.tick;
        startTick ??= tick;
        frames += 1;
        const statistics = await port.statistics();
        for (const [key, value] of Object.entries(flat(statistics)))
          if (typeof value === "number") (perFrame[key] ??= []).push(value);
      }
      const elapsedMs = performance.now() - start;
      const wallEnd = Date.now();
      const after = await port.statistics();
      const last = await handle.frame();
      const delta = (a: any, b: any) =>
        Object.fromEntries(
          Object.entries(flat(b))
            .filter(([, value]) => typeof value === "number")
            .map(([key, value]) => [
              key,
              (value as number) - (flat(a)[key] ?? 0),
            ]),
        );
      const diff = (a: Record<string, any>, b: Record<string, any>) => {
        const out: Record<string, any> = {};
        for (const [key, value] of Object.entries(b)) {
          if (typeof value === "number") out[key] = value - (a[key] ?? 0);
          else
            out[key] = {
              count: value.count - (a[key]?.count ?? 0),
              bytes: value.bytes - (a[key]?.bytes ?? 0),
            };
        }
        return out;
      };
      return {
        label,
        wallStart,
        wallEnd,
        elapsedMs,
        frames,
        ticks: Number((tick ?? 0n) - (startTick ?? 0n)),
        drawCalls: [first.drawCalls, last.drawCalls],
        triangles: [first.triangles, last.triangles],
        perFrame,
        totals: delta(before, after),
        before: copy({ ...flat(before) }),
        after: copy({ ...flat(after) }),
        sent: diff(sentBefore, counters.sent),
        received: diff(receivedBefore, counters.received),
        commits: diff(commitsBefore, counters.commits),
        device: copy(after.device),
        surfaceCaches: JSON.parse(
          JSON.stringify(after.surfaces.surfaceCaches, (_key, value) =>
            typeof value === "bigint" ? String(value) : value,
          ),
        ),
      };
    },
    { label, durationMs },
  );
  const until = Date.now() + durationMs + 200;
  await drive(until);
  return (await observing) as unknown as FrameWindow;
}

/** Screen bounds of the panel's canvas content. */
async function panelBounds(g: Gallery) {
  const corners = await projectContent(g, [
    [0, 0],
    [CANVAS_SIZE[0], 0],
    [0, CANVAS_SIZE[1]],
    [CANVAS_SIZE[0], CANVAS_SIZE[1]],
  ]);
  const xs = corners.map(({ clientX }) => clientX);
  const ys = corners.map(({ clientY }) => clientY);
  return {
    left: Math.min(...xs),
    right: Math.max(...xs),
    top: Math.min(...ys),
    bottom: Math.max(...ys),
  };
}

const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

async function idle(until: number) {
  await pause(Math.max(0, until - Date.now()));
}

test("Trace the gallery GUI demo's frame cost", {
  timeout: 900_000,
}, async (context) => {
  await mkdir(OUTPUT, { recursive: true });
  await runBrowserEnvironment(
    "GUI demo trace",
    {
      ...galleryEnvironment,
      operationTimeoutMs: 60_000,
      deviceScaleFactor: DEVICE_SCALE,
      evidenceParent: resolve("target/integration-artifacts/gallery-gui-trace"),
    },
    context.signal,
    async (scenario) => {
      await scenario.page.setViewportSize(VIEWPORT);
      await scenario.page.addInitScript(installPageCounters);
      const g = await openGallery(scenario, {
        initialPage: "gui",
        canvasShare: 1,
      });
      const { page } = g;
      await page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await awaitStationIdle(g);
      if (ISOLATE) {
        await page.locator("#gui-vector-only").click();
        await g.waitFor(
          (inspection) =>
            !inspection.entities.some(({ metadata }) =>
              metadata.symbolicId?.startsWith("gui-projector-"),
            ),
        );
      }
      await page.mouse.move(1, 1);
      await pause(1_500);
      const browser = page.context().browser()!;
      const workerUrl = page.workers().at(-1)!.url();
      const bounds = await panelBounds(g);
      const canvasBox = (await page
        .locator("#ipp-world-canvas")
        .boundingBox())!;
      const results: Record<string, unknown> = {
        viewport: VIEWPORT,
        canvas: canvasBox,
        devicePixelRatio: await page.evaluate(() => window.devicePixelRatio),
        isolated: ISOLATE,
        panelBounds: bounds,
        windowMs: WINDOW_MS,
      };
      const save = () =>
        writeFile(
          join(OUTPUT, `counters-${[...STATES].join("")}.json`),
          `${JSON.stringify(results, (_key, value) => (typeof value === "bigint" ? String(value) : value), 2)}\n`,
        );

      // Main-thread profile beside the worker's.
      const mainProfile = async <T>(collect: () => Promise<T>) => {
        const session = await page.context().newCDPSession(page);
        await session.send("Profiler.enable");
        await session.send("Profiler.setSamplingInterval", { interval: 1000 });
        await session.send("Profiler.start");
        try {
          const value = await collect();
          const { profile } = await session.send("Profiler.stop");
          return { value, profile };
        } finally {
          await session.detach().catch(() => {});
        }
      };

      const measure = async (
        name: string,
        drive: (until: number) => Promise<void>,
      ) => {
        if (!selected(name)) return;
        await browser.startTracing(page, {
          path: join(OUTPUT, `${name}.trace.json`),
          categories: [
            "devtools.timeline",
            "disabled-by-default-devtools.timeline",
            "disabled-by-default-devtools.timeline.frame",
            "v8.execute",
            "blink.user_timing",
            "gpu",
            "toplevel",
          ],
        });
        const counters = await frameWindow(page, name, WINDOW_MS, drive);
        await browser.stopTracing();
        const profiled = await sampleWorkerCpu(browser, workerUrl, () =>
          mainProfile(() =>
            frameWindow(page, `${name}-profiled`, WINDOW_MS, drive),
          ),
        );
        await writeFile(
          join(OUTPUT, `${name}.worker.cpuprofile`),
          JSON.stringify(profiled.profile),
        );
        await writeFile(
          join(OUTPUT, `${name}.main.cpuprofile`),
          JSON.stringify(profiled.window.profile),
        );
        results[name] = { counters, profiled: profiled.window.value };
        await save();
      };

      // 1. Idle at load: SCAN on, nothing touched.
      await measure("1-idle", idle);

      // 3. Pointer moving over the panel's controls.
      const hover = async (until: number) => {
        const rows = [0.12, 0.3, 0.5, 0.7, 0.88];
        const width = bounds.right - bounds.left;
        const height = bounds.bottom - bounds.top;
        let row = 0;
        while (Date.now() < until) {
          const y = bounds.top + height * rows[row % rows.length]!;
          const forward = row % 2 === 0;
          for (let step = 0; step <= 60 && Date.now() < until; step += 1) {
            const fraction = 0.05 + (0.9 * (forward ? step : 60 - step)) / 60;
            await page.mouse.move(bounds.left + width * fraction, y);
            await pause(16);
          }
          row += 1;
        }
      };
      await measure("3-hover", hover);
      await page.mouse.move(1, 1);
      await pause(1_000);

      // 4. Dragging the GAIN slider back and forth.
      const drag = async (until: number) => {
        const from = await controlPoint(
          g,
          { role: "slider" },
          gainFraction(0.64),
        );
        const low = await controlPoint(
          g,
          { role: "slider" },
          gainFraction(0.1),
        );
        const high = await controlPoint(
          g,
          { role: "slider" },
          gainFraction(0.95),
        );
        await page.mouse.move(from.clientX, from.clientY);
        await page.mouse.down();
        let phase = 0;
        while (Date.now() < until) {
          const t = (Math.sin(phase) + 1) / 2;
          await page.mouse.move(
            low.clientX + (high.clientX - low.clientX) * t,
            low.clientY + (high.clientY - low.clientY) * t,
          );
          phase += 0.12;
          await pause(16);
        }
        await page.mouse.up();
      };
      await measure("4-drag-gain", drag);
      await page.mouse.move(1, 1);
      await pause(1_000);

      // 5. Layers exploded, otherwise idle.
      if (selected("5-exploded")) {
        await toggleLayers(g, true);
        await page.mouse.move(1, 1);
        await pause(1_500);
        await measure("5-exploded", idle);
        await toggleLayers(g, false);
        await page.mouse.move(1, 1);
        await pause(1_500);
      }

      // 2. Every animation off: SCAN off, reduced motion on.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "SCAN" },
        { kind: "toggle" },
      );
      await g.call(
        "galleryGuiAction",
        { role: "checkbox", name: "REDUCED MOTION" },
        { kind: "toggle" },
      );
      await page.mouse.move(1, 1);
      await pause(2_000);
      await measure("2-idle-static", idle);

      // 6. Orbit the camera from the background, nothing else changing.
      const orbit = async (until: number) => {
        const x = canvasBox.x + 40;
        const y = canvasBox.y + canvasBox.height - 40;
        await page.mouse.move(x, y);
        await page.mouse.down();
        let phase = 0;
        while (Date.now() < until) {
          await page.mouse.move(
            x + 120 * Math.sin(phase),
            y - 10 * Math.cos(phase),
          );
          phase += 0.08;
          await pause(16);
        }
        await page.mouse.up();
      };
      const cameraBefore = transform(await g.inspect());
      await measure("6-orbit-static", orbit);
      results.orbitCamera = {
        before: cameraBefore,
        after: transform(await g.inspect()),
      };
      await save();
    },
  );
});
