/** Runs the fixed React GUI stress workload once and writes its report. */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { arch, cpus, platform, release } from "node:os";
import { resolve } from "node:path";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
  type BrowserEnvironmentContext,
} from "../browser/environment.js";
import { runNativeEnvironment } from "../integration/environment.js";
import { invoke } from "../render/evidence.js";
import { encodePng } from "../render/retained-gui-images.js";
import { GUI_STRESS_WORKLOAD } from "../../examples/gui-stress/workload.js";
import { sampleWorkerAllocations } from "./worker-profiling.js";
import { verifyHardwareRenderer } from "#ipp-browser-options";
import type { HostGuiLayoutStatistics } from "@ipp/client";

type Arrangement = "browser" | "native-gles";
type FixtureCapture = {
  width: number;
  height: number;
  tick: string | null;
  output: { kind: string; world: string; entity: string };
  publication: { host: string; revision: string };
  sequence: string;
  worlds: readonly { id: string; incarnation: string }[];
  drawCalls: number;
  triangles: number;
  failedDrawCalls: number;
  pixels: string;
  statistics: {
    gui?: Record<string, number>;
    guiLayout?: HostGuiLayoutStatistics | null;
    surfaces?: Record<string, number>;
    ingress?: Record<string, number>;
    device?: Record<string, unknown>;
  } | null;
};

type StepSample = {
  action: string;
  updateMs: number | null;
  updateToFrameMs: number | null;
  traffic: { batches: number; pages: number; guiEdits: number };
  effects: number;
  pressCallbacks: number;
  firstRange?: [number, number];
  expectedRange?: number;
  callbackRevision?: number;
  controlValueChanged?: boolean;
  toggleApplied?: boolean;
  sampledAnimationTime?: number;
  frame: {
    sequence: string;
    publication: { host: string; revision: string };
    sources: readonly {
      output: { world: { id: string; incarnation: string } };
      minimumTick: string;
      tick: string;
      publication: { host: string; revision: string };
    }[];
  };
};

type CoreProfile = {
  memoryBytes: number;
  frames: number[][];
  names: string[];
  stages: number[];
  categories: { name: string; calls: number; bytes: number }[];
  allocations: number[];
};

const SOFTWARE_RENDERER = /swiftshader|llvmpipe|softpipe|lavapipe/i;
const ASSETS = [
  "target/font-assets/shure-tech-mono.ippf",
  "target/surface-assets/icon.ippd",
  "target/surface-assets/panel.ippd",
  "target/surface-assets/badge.ippt",
] as const;

async function digest(path: string): Promise<string> {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
}

async function sourceIdentity() {
  const git = (...args: string[]) =>
    execFileSync("git", args, { encoding: "utf8" }).trim();
  const status = git("status", "--porcelain=v1");
  const diff = git("diff", "HEAD", "--binary");
  const hash = createHash("sha256").update(diff).update(status);
  const changed = git(
    "ls-files",
    "--modified",
    "--others",
    "--exclude-standard",
  )
    .split("\n")
    .filter(Boolean)
    .sort();
  for (const path of changed) hash.update(path).update(await readFile(path));
  return {
    revision: git("rev-parse", "HEAD"),
    uncommittedChanges: status.length > 0,
    changesSha256: status ? hash.digest("hex") : null,
  };
}

function image(capture: FixtureCapture) {
  const pixels = new Uint8Array(Buffer.from(capture.pixels, "base64"));
  assert.equal(pixels.length, capture.width * capture.height * 4);
  return { width: capture.width, height: capture.height, pixels };
}

function assertLayoutMembership(capture: FixtureCapture, panels: number): void {
  const layout = capture.statistics?.guiLayout;
  assert.ok(
    layout?.complete && layout.total,
    "Whole-Host ordinary layout diagnostics unavailable",
  );
  assert.equal(layout.scope, "host");
  const identity = (world: {
    readonly id: string;
    readonly incarnation: string;
  }) => `${world.id}/${world.incarnation}`;
  const actual = layout.worlds.map((sample) => identity(sample.world)).sort();
  assert.equal(new Set(actual).size, panels * 2 + 1);
  assert.deepEqual(actual, capture.worlds.map(identity).sort());
  // The presenting camera World selects no GUI layout and reports none; every
  // panel World selects it and reports its layout.
  const root = identity(capture.worlds[0]!);
  for (const sample of layout.worlds) {
    if (identity(sample.world) === root) {
      assert.ok(
        !sample.layout,
        "The presenting World reported GUI layout it does not select",
      );
      continue;
    }
    assert.notEqual(sample.status, "unavailable");
    assert.ok(
      sample.layout,
      "A stress World omitted selected GUI layout diagnostics",
    );
  }
  for (const key of [
    "reflows",
    "visitedEntities",
    "textMeasurements",
    "reusedTexts",
  ] as const) {
    const total: number = layout.worlds.reduce(
      (sum, sample) => sum + (sample.layout?.total[key] ?? 0),
      layout.retired[key],
    );
    assert.equal(layout.total[key], Math.min(0xffff_ffff, total));
  }
  assert.equal(
    capture.statistics!.gui!.totalGuiLayoutReflows,
    layout.total.reflows,
  );
  assert.equal(
    capture.statistics!.gui!.totalGuiTextMeasurements,
    layout.total.textMeasurements,
  );
}

async function retainCapture(
  env: BrowserEnvironmentContext,
  label: string,
  capture: FixtureCapture,
): Promise<void> {
  const { pixels: _pixels, ...metadata } = capture;
  await Promise.all([
    writeFile(
      resolve(env.evidence.directory, `${label}.png`),
      encodePng(image(capture)),
    ),
    env.evidence.writeJson(`${label}-frame.json`, metadata),
  ]);
}

function paintedPixels(capture: FixtureCapture): number {
  const { pixels } = image(capture);
  let painted = 0;
  for (let offset = 0; offset < pixels.length; offset += 4)
    if (pixels[offset]! + pixels[offset + 1]! + pixels[offset + 2]! > 80)
      painted += 1;
  return painted;
}

function changedPixels(first: FixtureCapture, second: FixtureCapture): number {
  const before = image(first);
  const after = image(second);
  assert.equal(before.width, after.width);
  assert.equal(before.height, after.height);
  let changed = 0;
  for (let offset = 0; offset < before.pixels.length; offset += 4)
    if (
      Math.abs(before.pixels[offset]! - after.pixels[offset]!) > 12 ||
      Math.abs(before.pixels[offset + 1]! - after.pixels[offset + 1]!) > 12 ||
      Math.abs(before.pixels[offset + 2]! - after.pixels[offset + 2]!) > 12
    )
      changed += 1;
  return changed;
}

function assertRawBars(capture: FixtureCapture, panels: number): number {
  const frame = image(capture);
  const pixelsPerMetre = frame.height / GUI_STRESS_WORKLOAD.camera.height;
  const columns = Math.ceil(Math.sqrt(panels));
  const rows = Math.ceil(panels / columns);
  const sample = (x: number, y: number) => {
    const offset = (Math.round(y) * frame.width + Math.round(x)) * 4;
    return (
      frame.pixels[offset]! +
      frame.pixels[offset + 1]! +
      frame.pixels[offset + 2]!
    );
  };
  for (let panel = 0; panel < panels; panel += 1) {
    const worldX =
      ((panel % columns) - (columns - 1) / 2) *
      GUI_STRESS_WORKLOAD.layout.stepX;
    const worldY =
      ((rows - 1) / 2 - Math.floor(panel / columns)) *
        GUI_STRESS_WORKLOAD.layout.stepY +
      GUI_STRESS_WORKLOAD.layout.rawYOffset;
    const x = frame.width / 2 + worldX * pixelsPerMetre;
    const y = frame.height / 2 - worldY * pixelsPerMetre;
    const bar = sample(x, y);
    const outside = sample(
      x,
      y + GUI_STRESS_WORKLOAD.layout.rawHeight * pixelsPerMetre * 1.35,
    );
    assert.ok(
      bar - outside > 120,
      `Raw panel ${panel} drawing bar is not visible (${bar} vs ${outside})`,
    );
  }
  return panels;
}

function distribution(values: readonly number[]) {
  const ordered = [...values].sort((left, right) => left - right);
  const at = (fraction: number) =>
    ordered[
      Math.min(ordered.length - 1, Math.ceil(fraction * ordered.length) - 1)
    ]!;
  return {
    count: ordered.length,
    medianMs: at(0.5),
    p90Ms: at(0.9),
    minMs: ordered[0],
    maxMs: ordered.at(-1),
  };
}

async function exercise(
  env: BrowserEnvironmentContext,
  connection: Record<string, unknown>,
  repetitions: number,
  samplesPerSweep: number,
  measure: boolean,
) {
  const moduleUrl = `${env.urls.origin}/target/gui-stress/gui-stress-fixture.js`;
  const call = <T>(name: string, args: readonly unknown[] = []) =>
    env.execute(name, args, () => invoke<T>(env.page, moduleUrl, name, args));
  const output = [];
  const mountedEntities = new Set<string>();
  let lastRangeChanges = 0;
  const init = await call<Record<string, unknown>>("initialize", [
    { generatedModuleUrl: env.urls.generated, measure, ...connection },
  ]);
  let scenarioFailure: unknown;
  try {
    for (const sweep of GUI_STRESS_WORKLOAD.sweeps) {
      for (let repeat = 0; repeat < repetitions; repeat += 1) {
        const mounted = await call<{
          entity: string;
          rootIncarnation: string;
          initialAnchor?: number;
          cameraX?: number;
          rangeChanges: number;
          range?: [number, number];
        }>("mount", [sweep]);
        assert.ok(
          !mountedEntities.has(mounted.entity),
          "A repeat reused an occupied panel identity",
        );
        mountedEntities.add(mounted.entity);
        assert.ok(
          mounted.rangeChanges > lastRangeChanges,
          "A fresh mount published no wanted range",
        );
        assert.ok(
          (mounted.range?.[0] ?? -1) === 0,
          "A repeat inherited a stale VirtualList range",
        );
        assert.equal(
          mounted.initialAnchor,
          0,
          "A repeat inherited a stale VirtualList anchor",
        );
        assert.equal(mounted.cameraX, 0, "A repeat inherited a moved camera");
        lastRangeChanges = mounted.rangeChanges;
        await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
        const before = await invoke<FixtureCapture>(
          env.page,
          moduleUrl,
          "capture",
        );
        const prefix = `${sweep.name}-r${repeat}`;
        await retainCapture(env, `${prefix}-before`, before);
        assertLayoutMembership(before, sweep.panels);
        assert.equal(before.output.kind, "camera");
        assert.equal(before.failedDrawCalls, 0);
        assert.ok(before.statistics?.gui, "GUI diagnostics are unavailable");
        const samples: StepSample[] = [];
        let lastSequence = BigInt(before.sequence);
        for (let cycle = 0; cycle < samplesPerSweep; cycle += 1) {
          for (const action of GUI_STRESS_WORKLOAD.actions) {
            const sample = await call<StepSample>("step", [action, cycle]);
            assert.ok(BigInt(sample.frame.sequence) > lastSequence);
            lastSequence = BigInt(sample.frame.sequence);
            const identity = (world: { id: string; incarnation: string }) =>
              `${world.id}/${world.incarnation}`;
            assert.deepEqual(
              sample.frame.sources
                .map((source) => identity(source.output.world))
                .sort(),
              before.worlds.map(identity).sort(),
              "Completed action draw includes every exact participating World",
            );
            for (const source of sample.frame.sources) {
              assert.ok(BigInt(source.tick) >= BigInt(source.minimumTick));
              assert.equal(
                source.publication.host,
                sample.frame.publication.host,
              );
            }
            if (["callback-only", "idle", "cache-idle"].includes(action))
              assert.deepEqual(
                sample.traffic,
                { batches: 0, pages: 0, guiEdits: 0 },
                `${sweep.name}: ${action} emitted transport writes`,
              );
            if (action === "control-press") {
              assert.equal(
                sample.pressCallbacks,
                1,
                "The semantic press callback was skipped",
              );
              assert.ok(
                sample.effects >= 1,
                "The control press published no effect",
              );
              assert.equal(
                sample.callbackRevision,
                cycle + 1,
                "The replaced callback was stale",
              );
            }
            if (action === "control-toggle") {
              // A toggle settles with the change it applied and is observed
              // as the checked field; it publishes no effect.
              assert.equal(sample.toggleApplied, true);
              assert.equal(sample.controlValueChanged, true);
            }
            if (action === "virtual-scroll")
              assert.ok(
                Math.abs(
                  (sample.firstRange?.[0] ?? 0) - sample.expectedRange!,
                ) < 8,
                "VirtualList did not move to a fresh range",
              );
            if (action === "animation")
              assert.ok(
                sample.sampledAnimationTime !== undefined,
                "Animation time was not observed",
              );
            if (
              [
                "local-text",
                "layout",
                "theme",
                "churn",
                "camera-only",
              ].includes(action)
            )
              assert.ok(
                sample.traffic.batches + sample.traffic.guiEdits > 0,
                `${action} made no authored change`,
              );
            if (action === "local-text") {
              // One batch to the first panel's World, one SetField per
              // changed field: the title text and the red, green and blue
              // of the previously and newly highlighted row indicators.
              assert.equal(
                sample.traffic.batches,
                1,
                "Local text update touched more than one panel subtree",
              );
              assert.ok(
                sample.traffic.guiEdits <= 7,
                "Local text update wrote more than its changed fields",
              );
            }
            samples.push(sample);
          }
        }
        const after = await invoke<FixtureCapture>(
          env.page,
          moduleUrl,
          "capture",
        );
        await retainCapture(env, `${prefix}-after`, after);
        assertLayoutMembership(after, sweep.panels);
        const state = await call<{
          panelCount: number;
          rawSurfaceCount: number;
          semanticRoles: Record<string, number>;
          virtualList?: {
            itemCount: number;
            anchorIndex: number;
            anchorOffset: number;
            loadedFirst: number;
            loadedLast: number;
          };
          declaredRowChildren: number;
          pressCallbacks: number;
          observedEffects: number;
          failures: string[];
        }>("observeState");
        assert.deepEqual(state.failures, []);
        assert.equal(state.panelCount, sweep.panels);
        assert.equal(state.rawSurfaceCount, sweep.panels);
        assert.ok((state.semanticRoles.button ?? 0) >= 1);
        assert.ok((state.semanticRoles.slider ?? 0) >= 1);
        assert.ok((state.semanticRoles.textInput ?? 0) >= 1);
        assert.equal(
          state.virtualList?.itemCount,
          GUI_STRESS_WORKLOAD.virtualItemsPerPanel,
        );
        assert.equal(
          state.declaredRowChildren,
          sweep.treeRows -
            (samplesPerSweep % 2 === 0 ? 0 : Math.ceil(sweep.treeRows / 4)),
        );
        assert.equal(
          state.virtualList?.anchorIndex,
          GUI_STRESS_WORKLOAD.virtualScrollStart +
            (samplesPerSweep - 1) * GUI_STRESS_WORKLOAD.virtualScrollStride,
        );
        assert.equal(state.virtualList?.anchorOffset, 0);
        assert.ok(
          state.virtualList &&
            Number.isInteger(state.virtualList.loadedFirst) &&
            Number.isInteger(state.virtualList.loadedLast) &&
            state.virtualList.loadedFirst <= state.virtualList.anchorIndex &&
            state.virtualList.loadedLast > state.virtualList.anchorIndex,
          "Actual evaluated virtual item bounds do not include the anchor",
        );
        assert.ok(
          state.pressCallbacks >= samplesPerSweep &&
            state.observedEffects >= samplesPerSweep,
        );
        const painted = paintedPixels(before);
        const changed = changedPixels(before, after);
        const rawBarsVerified = assertRawBars(before, sweep.panels);
        assert.ok(
          painted > 1000,
          `GUI frame is nearly empty (${painted} painted pixels)`,
        );
        assert.ok(changed > 100, `GUI workload changed only ${changed} pixels`);
        assert.ok(
          (before.statistics?.surfaces?.surfaceCacheEntries ?? 0) >=
            sweep.panels,
          "Raw Surface cache entries are missing",
        );
        output.push({
          sweep,
          repeat,
          state,
          samples,
          timings: Object.fromEntries(
            GUI_STRESS_WORKLOAD.actions.map((action) => {
              const matching = samples.filter(
                (sample) => sample.action === action,
              );
              return [
                action,
                {
                  updateMs: matching.every((sample) => sample.updateMs !== null)
                    ? distribution(
                        matching.flatMap((sample) =>
                          sample.updateMs === null ? [] : [sample.updateMs],
                        ),
                      )
                    : null,
                  updateToFrameMs: matching.every(
                    (sample) => sample.updateToFrameMs !== null,
                  )
                    ? distribution(
                        matching.flatMap((sample) =>
                          sample.updateToFrameMs === null
                            ? []
                            : [sample.updateToFrameMs],
                        ),
                      )
                    : null,
                },
              ];
            }),
          ),
          correctness: {
            paintedPixels: painted,
            changedPixels: changed,
            rawBarsVerified,
          },
          before: {
            tick: before.tick,
            drawCalls: before.drawCalls,
            triangles: before.triangles,
            statistics: before.statistics,
          },
          after: {
            tick: after.tick,
            drawCalls: after.drawCalls,
            triangles: after.triangles,
            statistics: after.statistics,
          },
          artifacts: [`${prefix}-before.png`, `${prefix}-after.png`],
        });
      }
    }
    await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
    await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
    const workBefore = await invoke<FixtureCapture>(
      env.page,
      moduleUrl,
      "capture",
    );
    const local = await call<{
      traffic: { batches: number; pages: number; guiEdits: number };
    }>("step", ["local-text"]);
    const workAfterLocal = await invoke<FixtureCapture>(
      env.page,
      moduleUrl,
      "capture",
    );
    const callbackOnly = await call<{
      traffic: { batches: number; pages: number; guiEdits: number };
    }>("step", ["callback-only"]);
    const workAfterCallback = await invoke<FixtureCapture>(
      env.page,
      moduleUrl,
      "capture",
    );
    await Promise.all([
      retainCapture(env, "work-before", workBefore),
      retainCapture(env, "work-after-local", workAfterLocal),
      retainCapture(env, "work-after-callback", workAfterCallback),
    ]);
    for (const capture of [workBefore, workAfterLocal, workAfterCallback])
      assertLayoutMembership(capture, GUI_STRESS_WORKLOAD.probeSweep.panels);
    assert.ok(
      local.traffic.guiEdits > 0,
      "Warm local edit emitted no GUI changes",
    );
    assert.deepEqual(callbackOnly.traffic, {
      batches: 0,
      pages: 0,
      guiEdits: 0,
    });
    const guiBefore = workBefore.statistics?.gui;
    const guiLocal = workAfterLocal.statistics?.gui;
    const guiCallback = workAfterCallback.statistics?.gui;
    assert.ok(
      guiBefore && guiLocal && guiCallback,
      "GUI work diagnostics are unavailable",
    );
    assert.ok(
      (guiBefore.totalGuiTextMeasurements ?? 0) > 0,
      "GUI diagnostics omit the child Worlds' completed text-measurement work",
    );
    const workDelta = (
      key: string,
      from: Record<string, number>,
      to: Record<string, number>,
    ) => {
      const before = from[key];
      const after = to[key];
      return before === undefined || after === undefined
        ? null
        : after - before;
    };
    const workProbe = {
      sweep: GUI_STRESS_WORKLOAD.probeSweep,
      local: {
        traffic: local.traffic,
        layoutReflows: workDelta("totalGuiLayoutReflows", guiBefore, guiLocal),
        textMeasurements: workDelta(
          "totalGuiTextMeasurements",
          guiBefore,
          guiLocal,
        ),
        rebuilds: workDelta("totalGuiRebuilds", guiBefore, guiLocal),
      },
      callbackOnly: {
        traffic: callbackOnly.traffic,
        layoutReflows: workDelta(
          "totalGuiLayoutReflows",
          guiLocal,
          guiCallback,
        ),
        textMeasurements: workDelta(
          "totalGuiTextMeasurements",
          guiLocal,
          guiCallback,
        ),
        rebuilds: workDelta("totalGuiRebuilds", guiLocal, guiCallback),
      },
    };
    assert.equal(workProbe.callbackOnly.layoutReflows, 0);
    assert.equal(workProbe.callbackOnly.textMeasurements, 0);
    assert.equal(workProbe.callbackOnly.rebuilds, 0);

    let allocationProfile: Record<string, unknown> | null = null;
    if ("workerScriptUrl" in connection) {
      const browser = env.page.context().browser();
      if (!browser)
        throw new Error("Browser worker allocation profiler is unavailable");
      await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
      await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
      const sampled = await sampleWorkerAllocations(
        browser,
        String(connection.workerScriptUrl),
        async () => {
          for (const action of GUI_STRESS_WORKLOAD.allocationActions)
            await call("step", [action]);
          return { actions: GUI_STRESS_WORKLOAD.allocationActions };
        },
      );
      const heap = sampled.heap as {
        profile?: { samples?: { size: number }[] };
      };
      const samples = heap.profile?.samples ?? [];
      await writeFile(
        resolve(env.evidence.directory, "worker-allocations.json"),
        `${JSON.stringify(sampled.heap)}\n`,
      );
      allocationProfile = {
        kind: "Chromium worker sampled heap allocations",
        sampleCount: samples.length,
        sampledBytes: samples.reduce((total, sample) => total + sample.size, 0),
        artifact: "worker-allocations.json",
        actions: GUI_STRESS_WORKLOAD.allocationActions,
      };
    }
    let layoutDiagnosticsProbe = null;
    if (!measure) {
      const probe = await call<{
        identity: { id: string; incarnation: string };
        unsupported: { id: string; incarnation: string };
        evaluated: HostGuiLayoutStatistics;
        resumed: HostGuiLayoutStatistics;
        retired: HostGuiLayoutStatistics;
        missing: HostGuiLayoutStatistics;
      }>("layoutDiagnosticsProbe");
      await env.evidence.writeJson("layout-diagnostics-probe.json", probe);
      const find = (
        sample: HostGuiLayoutStatistics,
        identity = probe.identity,
      ) => {
        assert.ok(
          sample.complete && sample.total,
          "Complete whole-Host sample",
        );
        return sample.worlds.find(
          (entry) =>
            entry.world.id === identity.id &&
            entry.world.incarnation === identity.incarnation,
        );
      };
      const evaluated = find(probe.evaluated);
      assert.ok(evaluated?.layout);
      assert.equal(evaluated.status, "evaluated");
      // The World canvas exists from creation: an empty initial pass, then
      // the pass over the declared entity.
      assert.deepEqual(
        evaluated.layout.total,
        { reflows: 2, visitedEntities: 1, textMeasurements: 0, reusedTexts: 0 },
        "Unpresented Canvas performs actual ordinary layout",
      );
      assert.deepEqual(
        find(probe.resumed)?.layout?.total,
        evaluated.layout.total,
      );
      assert.equal(find(probe.retired), undefined);
      assert.equal(
        probe.retired.retiredWorlds,
        probe.resumed.retiredWorlds + 1,
      );
      assert.equal(
        probe.retired.retired.reflows,
        probe.resumed.retired.reflows + evaluated.layout.total.reflows,
      );
      assert.deepEqual(
        probe.retired.total,
        probe.resumed.total,
        "Retirement preserves Host work",
      );
      assert.equal(find(probe.missing, probe.unsupported)?.layout, null);
      assert.deepEqual(probe.missing.total, probe.retired.total);
      layoutDiagnosticsProbe = probe;
    }
    return {
      init,
      browser: env.page.context().browser()?.version() ?? null,
      device: output[0]?.after.statistics?.device ?? null,
      softwareRenderer: Object.values(
        output[0]?.after.statistics?.device ?? {},
      ).some((value) => SOFTWARE_RENDERER.test(String(value))),
      evidence: env.evidence.directory,
      sweeps: output,
      workProbe,
      allocationProfile,
      layoutDiagnosticsProbe,
    };
  } catch (error) {
    scenarioFailure = error;
    throw error;
  } finally {
    try {
      await call("close");
    } catch (error) {
      if (scenarioFailure !== undefined)
        throw new AggregateError(
          [scenarioFailure, error],
          "Stress scenario and cleanup failed",
          { cause: scenarioFailure },
        );
      throw error;
    }
  }
}

async function exerciseCoreProfile(env: BrowserEnvironmentContext) {
  const moduleUrl = `${env.urls.origin}/target/gui-stress/gui-stress-fixture.js`;
  const call = <T>(name: string, args: readonly unknown[] = []) =>
    invoke<T>(env.page, moduleUrl, name, args);
  await call("initialize", [
    {
      generatedModuleUrl: env.urls.generated,
      workerScriptUrl: env.urls.workerScript,
      wasmUrl: env.urls.wasm,
    },
  ]);
  try {
    const worker = env.page.workers().at(-1);
    if (!worker || !(await worker.evaluate(() => "ippProfile" in globalThis)))
      throw new Error("The GUI profiling worker has no host profiling hooks");
    const collect = async (
      sweep: {
        readonly name: string;
        readonly panels: number;
        readonly treeRows: number;
      },
      allocations: boolean,
      idle: boolean,
    ): Promise<CoreProfile> => {
      await call("mount", [sweep]);
      await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
      await worker.evaluate((enabled) => {
        (
          globalThis as unknown as {
            ippProfile: { start(profile: boolean): void };
          }
        ).ippProfile.start(enabled);
      }, allocations);
      if (idle) {
        for (
          let frame = 0;
          frame < GUI_STRESS_WORKLOAD.idleProfileFrames;
          frame += 1
        )
          await call("step", ["idle", frame]);
      } else {
        for (const action of GUI_STRESS_WORKLOAD.allocationActions)
          await call("step", [action, 0]);
      }
      return worker.evaluate(() =>
        (
          globalThis as unknown as { ippProfile: { stop(): CoreProfile } }
        ).ippProfile.stop(),
      );
    };
    const cpuSummary = (profile: CoreProfile) => ({
      frameCount: profile.frames.length,
      evaluationMs: distribution(profile.frames.map((frame) => frame[0]!)),
      hostFrameMs: distribution(profile.frames.map((frame) => frame[1]!)),
      wasmMemoryBytes: profile.memoryBytes,
    });
    const allocationSummary = (profile: CoreProfile) => {
      const categoryTotals = new Map<
        string,
        { calls: number; bytes: number }
      >();
      for (const category of profile.categories) {
        const name = category.name || "unclassified";
        const current = categoryTotals.get(name) ?? { calls: 0, bytes: 0 };
        categoryTotals.set(name, {
          calls: current.calls + category.calls,
          bytes: current.bytes + category.bytes,
        });
      }
      return {
        calls: profile.allocations[0],
        requestedBytes: profile.allocations[1],
        wasmMemoryBytes: profile.memoryBytes,
        bySemanticName: [...categoryTotals]
          .filter(([, totals]) => totals.calls > 0)
          .map(([name, totals]) => ({ name, ...totals })),
      };
    };
    const validate = (cpu: CoreProfile, allocations: CoreProfile) => {
      assert.ok(cpu.frames.length > 0 && allocations.frames.length > 0);
      assert.ok(cpu.memoryBytes > 0 && allocations.memoryBytes > 0);
      assert.equal(
        allocations.categories.reduce(
          (total, category) => total + category.calls,
          0,
        ),
        allocations.allocations[0],
        "Core allocation categories do not sum to the allocator total",
      );
    };
    const cpu = await collect(GUI_STRESS_WORKLOAD.probeSweep, false, false);
    const allocations = await collect(
      GUI_STRESS_WORKLOAD.probeSweep,
      true,
      false,
    );
    validate(cpu, allocations);
    const idleSweeps = [];
    for (const sweep of GUI_STRESS_WORKLOAD.sweeps.filter((candidate) =>
      candidate.name.startsWith("panels-"),
    )) {
      const idleCpu = await collect(sweep, false, true);
      const idleAllocations = await collect(sweep, true, true);
      validate(idleCpu, idleAllocations);
      idleSweeps.push({ sweep, cpu: idleCpu, allocations: idleAllocations });
    }
    await writeFile(
      resolve(env.evidence.directory, "core-profile.json"),
      `${JSON.stringify({ mixed: { cpu, allocations }, idleSweeps }, null, 2)}\n`,
    );
    return {
      evidence: env.evidence.directory,
      artifact: "core-profile.json",
      build: "gui-stress-profile",
      cpu: cpuSummary(cpu),
      allocations: allocationSummary(allocations),
      idleSweeps: idleSweeps.map((profile) => ({
        sweep: profile.sweep,
        requestedFrames: GUI_STRESS_WORKLOAD.idleProfileFrames,
        cpu: cpuSummary(profile.cpu),
        allocations: allocationSummary(profile.allocations),
      })),
      stageInterpretation:
        "Raw slot-indexed stage tables are preserved in the artifact; compare semantic names, not slot positions, across changed selected-system schedules.",
    };
  } finally {
    await call("close");
  }
}

export async function runGuiStress(
  signal: AbortSignal,
  arrangement: Arrangement,
  repetitions: number,
  output: string,
  eglDirectory?: string,
  samplesPerSweep: number = GUI_STRESS_WORKLOAD.samplesPerSweep,
  withCoreProfile = false,
  correctnessOnly = false,
) {
  if (!Number.isSafeInteger(repetitions) || repetitions < 1)
    throw new Error("GUI stress repetitions must be positive");
  if (
    !Number.isSafeInteger(samplesPerSweep) ||
    samplesPerSweep < 1 ||
    samplesPerSweep > 32
  )
    throw new Error("GUI stress samples per sweep must be in 1..32");
  await mkdir(output, { recursive: true });
  const workspace = process.cwd();
  const startedAt = new Date().toISOString();
  const buildName = arrangement === "browser" ? "headless-gui" : "gles-gui";
  const directory = resolve(
    arrangement === "browser" ? "target/browser-build" : "target/gles-host",
    buildName,
  );
  const build: BrowserBuildConfiguration = {
    name: buildName,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(
      directory,
      arrangement === "browser" ? "runtime.wasm" : "gles_host",
    ),
    exportWasm: resolve(
      directory,
      arrangement === "browser" ? "export.wasm" : "contract.bin",
    ),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  const browserConfig = {
    workspace,
    build,
    mismatchBuild: build,
    rendering: arrangement === "browser",
    deviceScaleFactor: GUI_STRESS_WORKLOAD.viewport.dpr,
    operationTimeoutMs: 60_000,
    evidenceParent: output,
  };
  let result;
  let hostEvidence: string | null = null;
  if (arrangement === "browser") {
    const run = await runBrowserEnvironment(
      `gui-stress-${buildName}`,
      browserConfig,
      signal,
      (env) =>
        exercise(
          env,
          { workerScriptUrl: env.urls.workerScript, wasmUrl: env.urls.wasm },
          repetitions,
          samplesPerSweep,
          !correctnessOnly,
        ),
    );
    result = run.value;
    if (process.env.IPP_BROWSER_ANGLE)
      verifyHardwareRenderer(result.device?.unmaskedRenderer);
  } else {
    if (!eglDirectory) throw new Error("Native GUI stress requires --egl-dir");
    const native = await runNativeEnvironment(
      "gui-stress-gles",
      {
        executable: build.runtimeWasm,
        schemaArtifact: build.contractArtifact,
        workingDirectory: workspace,
        extraArguments: ["--egl-dir", eglDirectory],
        readinessTimeoutMs: 30_000,
        operationTimeoutMs: 60_000,
        evidenceParent: output,
      },
      signal,
      async (host) => {
        if (!host.presentationUrl)
          throw new Error("Native host has no presentation channel");
        const browser = await runBrowserEnvironment(
          "gui-stress-gles-client",
          browserConfig,
          host.signal,
          (env) =>
            exercise(
              env,
              {
                nativeHost: {
                  url: host.url,
                  presentationUrl: host.presentationUrl,
                },
              },
              repetitions,
              samplesPerSweep,
              !correctnessOnly,
            ),
        );
        return browser.value;
      },
    );
    result = native.value;
    hostEvidence = native.evidenceDirectory;
  }
  let coreProfile = null;
  if (withCoreProfile) {
    if (arrangement !== "browser")
      throw new Error("Core GUI profile currently requires a browser worker");
    const profileDirectory = resolve("target/browser-build/gui-stress-profile");
    const profileBuild: BrowserBuildConfiguration = {
      name: "gui-stress-profile",
      generatedModule: resolve(profileDirectory, "generated.js"),
      runtimeWasm: resolve(profileDirectory, "runtime.wasm"),
      exportWasm: resolve(profileDirectory, "export.wasm"),
      contractArtifact: resolve(profileDirectory, "contract.bin"),
    };
    const profiled = await runBrowserEnvironment(
      "gui-stress-core-profile",
      { ...browserConfig, build: profileBuild, mismatchBuild: profileBuild },
      signal,
      exerciseCoreProfile,
    );
    coreProfile = {
      ...profiled.value,
      contractSha256: await digest(profileBuild.contractArtifact),
      runtimeSha256: await digest(profileBuild.runtimeWasm),
    };
  }
  const report = {
    measurementMode: correctnessOnly
      ? "correctness-only"
      : "post-admission-draw",
    workload: GUI_STRESS_WORKLOAD,
    workloadSha256: createHash("sha256")
      .update(JSON.stringify(GUI_STRESS_WORKLOAD))
      .digest("hex"),
    repetitions,
    samplesPerSweep,
    source: await sourceIdentity(),
    machine: {
      platform: platform(),
      release: release(),
      arch: arch(),
      cpu: cpus()[0]?.model ?? null,
      node: process.version,
    },
    build: {
      arrangement,
      name: buildName,
      contractSha256: await digest(build.contractArtifact),
      runtimeSha256: await digest(build.runtimeWasm),
    },
    assets: await Promise.all(
      ASSETS.map(async (path) => ({ path, sha256: await digest(path) })),
    ),
    pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
    startedAt,
    timingScope:
      "Update-to-frame measures post-ACK Host-admission evaluation cuts through an actual completed root draw including all exact scene outputs. It includes barrier latency, not the first visible instant. Correctness-only runs record null latency. Captures, diagnostics and allocation windows remain separate.",
    hardwareClaim: result.softwareRenderer
      ? "software graphics correctness only"
      : "reported graphics device; verify device identity before hardware claims",
    hostEvidence,
    coreProfile,
    unavailable:
      arrangement === "native-gles"
        ? {
            workerAllocationSampling:
              "Native GLES host has no Chromium worker; sampled JS heap is unavailable",
            coreCpuAndAllocator:
              "This harness has no native host ippProfile hook; native core CPU/allocator sampling is unavailable, while GPU-resident bytes remain in diagnostics captures",
          }
        : {},
    ...result,
  };
  await writeFile(
    resolve(output, "gui-stress-report.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
  return report;
}

const direct = process.argv[1]?.endsWith("gui-stress.js");
if (direct) {
  const [
    ,
    ,
    backend = "browser",
    repetitions = "1",
    output = "target/performance/gui-stress",
    samples = String(GUI_STRESS_WORKLOAD.samplesPerSweep),
    eglDirectory,
  ] = process.argv;
  await runGuiStress(
    AbortSignal.timeout(3_600_000),
    backend as Arrangement,
    Number(repetitions),
    resolve(output),
    eglDirectory,
    Number(samples),
    process.argv.includes("--core-profile"),
  );
}
