/** Runs the fixed React GUI stress workload once and writes its report. */
import type { ProfileCapture, HostProfileStatus } from "@ipp/client/testing";
import type { HostGuiLayoutStatistics } from "@ipp/client/diagnostics";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  lstat,
  mkdir,
  readFile,
  readdir,
  readlink,
  writeFile,
} from "node:fs/promises";
import { arch, cpus, loadavg, platform, release } from "node:os";
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
import {
  accountedMemory,
  sampleChromeMemory,
  sampleDrmMemory,
} from "./gpu-memory.js";
import { sampleWorkerAllocations } from "./worker-profiling.js";
import {
  browserLaunchOptions,
  verifyHardwareRenderer,
} from "#ipp-browser-options";

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
    execFileSync("git", args, { encoding: "utf8" });
  const names = [
    ...new Set(
      git("ls-files", "--cached", "--others", "--exclude-standard", "-z").split(
        "\0",
      ),
    ),
  ]
    .filter(Boolean)
    .sort();
  const hash = createHash("sha256");
  for (const path of names) {
    hash.update(path).update("\0");
    const metadata = await lstat(path).catch(() => null);
    hash
      .update(
        metadata?.isSymbolicLink()
          ? `link:${await readlink(path)}`
          : metadata?.isFile()
            ? await digest(path)
            : "missing",
      )
      .update("\0");
  }
  return {
    revision: git("rev-parse", "HEAD").trim(),
    sourceSha256: hash.digest("hex"),
  };
}

async function clientArtifacts(directory: string) {
  return Promise.all(
    (await readdir(directory))
      .filter((name) => name.endsWith(".js"))
      .sort()
      .map(async (name) => ({
        name,
        sha256: await digest(resolve(directory, name)),
      })),
  );
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
  nativeProcessId: number | null = null,
  instrumentedDiagnostics = false,
) {
  const moduleUrl = `${env.urls.origin}/target/gui-stress/gui-stress-fixture.js`;
  const call = <T>(name: string, args: readonly unknown[] = []) =>
    name.startsWith("hostProfile")
      ? invoke<T>(env.page, moduleUrl, name, args)
      : env.execute(name, args, () =>
          invoke<T>(env.page, moduleUrl, name, args),
        );
  const output = [];
  const mountedEntities = new Set<string>();
  let lastRangeChanges = 0;
  const init = await call<Record<string, unknown>>("initialize", [
    { generatedModuleUrl: env.urls.generated, measure, ...connection },
  ]);
  let scenarioFailure: unknown;
  try {
    const profilingStatus = await call<HostProfileStatus>("hostProfileStatus");
    assert.equal(
      profilingStatus.available,
      instrumentedDiagnostics,
      "Selected product instrumentation availability mismatch",
    );
    for (const sweep of GUI_STRESS_WORKLOAD.sweeps) {
      for (let repeat = 0; repeat < repetitions; repeat += 1) {
        const mounted = await call<{
          entity: string;
          rootIncarnation: string;
          initialAnchor?: number;
          cameraX?: number;
          rangeChanges: number;
          range?: [number, number];
          buildToFrameMs: number | null;
          observedPanelEntities: number;
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
          mount: {
            buildToFrameMs: mounted.buildToFrameMs,
            observedPanelEntities: mounted.observedPanelEntities,
          },
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
    let nativeProfile: {
      status: HostProfileStatus;
      capture: ProfileCapture | null;
      actions: readonly string[];
      ownershipArtifact: string | null;
    } | null = null;
    if ("nativeHost" in connection) {
      const status = profilingStatus;
      assert.equal(
        status.available,
        instrumentedDiagnostics,
        "Host instrumentation availability must match selected product",
      );
      nativeProfile = {
        status,
        capture: null,
        actions: [],
        ownershipArtifact: null,
      };
      if (instrumentedDiagnostics) {
        await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
        await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
        const ownership = await call<Record<string, unknown>>(
          "hostProfileOwnershipScenario",
        );
        await env.evidence.writeJson(
          "native-profile-ownership.json",
          ownership,
        );
        const captureId = await call<string>("hostProfileStart");
        for (const action of GUI_STRESS_WORKLOAD.allocationActions)
          await call("step", [action]);
        const captured = await call<ProfileCapture>("hostProfileStop");
        assert.equal(captured.captureId, captureId);
        validateSemanticProfile(captured, "native");
        await env.evidence.writeJson("native-cpu-allocations.json", captured);
        nativeProfile = {
          status,
          capture: captured,
          ownershipArtifact: "native-profile-ownership.json",
          actions: GUI_STRESS_WORKLOAD.allocationActions,
        };
      }
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
    const panelDiagnostics = [];
    for (const count of GUI_STRESS_WORKLOAD.diagnosticPanelEntities) {
      const panel = await call<{
        requestedEntities: number;
        observedEntities: number;
        buildMs: number | null;
        inspectMs: number | null;
        inspectionJsonUtf8Bytes: number;
        inspectionScope: string;
        edits: {
          action: string;
          updateMs: number | null;
          updateToFrameMs: number | null;
        }[];
        before: FixtureCapture;
        after: FixtureCapture;
      }>("panelDiagnostic", [count, samplesPerSweep]);
      assert.equal(panel.observedEntities, count);
      assert.ok(paintedPixels(panel.before) > 1000);
      const changed = changedPixels(panel.before, panel.after);
      assert.ok(changed > 0, `Panel ${count} local edit changed no pixels`);
      await retainCapture(env, `diagnostic-${count}-before`, panel.before);
      await retainCapture(env, `diagnostic-${count}-after`, panel.after);
      panelDiagnostics.push({
        ...panel,
        before: undefined,
        after: undefined,
        correctness: {
          paintedPixels: paintedPixels(panel.before),
          changedPixels: changed,
        },
        timings: Object.fromEntries(
          ["local-colour", "local-position"].map((action) => [
            action,
            {
              updateMs: measure
                ? distribution(
                    panel.edits
                      .filter((edit) => edit.action === action)
                      .map((edit) => edit.updateMs!),
                  )
                : null,
              updateToFrameMs: measure
                ? distribution(
                    panel.edits
                      .filter((edit) => edit.action === action)
                      .map((edit) => edit.updateToFrameMs!),
                  )
                : null,
            },
          ]),
        ),
      });
    }
    // Restore the rich probe before final external memory observations.
    await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
    await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
    // External diagnostics run only after all timing/allocation windows have ended.
    const memoryFrame = await call<FixtureCapture>("capture");
    const memoryIdentity = {
      capture: `${env.evidence.directory}/memory-frame.json`,
      pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
      device: memoryFrame.statistics?.device ?? null,
    };
    const browser = env.page.context().browser();
    const chromeMemory = browser
      ? await sampleChromeMemory(memoryIdentity, browser)
      : null;
    const drmMemory = await sampleDrmMemory(
      memoryIdentity,
      nativeProcessId === null
        ? (chromeMemory?.processIds ?? [])
        : [nativeProcessId],
    );
    const memory = {
      samplingWindow: "after timing, capture, work and allocation windows",
      semantics:
        "IPP-accounted resources, DRM client counters and Chrome process backing allocations are distinct overlapping observations, not additive ownership totals. Chrome fields do not establish physical VRAM residency. Shared DRM buffers may overlap across clients.",
      accounted: accountedMemory(memoryIdentity, memoryFrame.statistics),
      external: [
        ...(chromeMemory?.observations ?? []),
        ...drmMemory.observations,
      ],
      chromeRole:
        nativeProcessId === null
          ? "runtime browser"
          : "native presentation client browser; does not include native Host GPU allocations",
      rawArtifact: "gpu-memory-raw.json",
    };
    await env.evidence.writeJson("gpu-memory-raw.json", {
      chrome: chromeMemory?.raw ?? null,
      drm: drmMemory.raw,
    });
    await retainCapture(env, "memory", memoryFrame);
    const afterMemory = await call<FixtureCapture>("capture");
    assert.equal(afterMemory.failedDrawCalls, 0);
    assert.deepEqual(
      afterMemory.worlds,
      memoryFrame.worlds,
      "Memory sampling changed World identities",
    );
    return {
      init,
      memory,
      panelDiagnostics,
      browser: env.page.context().browser()?.version() ?? null,
      device: output[0]?.after.statistics?.device ?? null,
      softwareRenderer: Object.values(
        output[0]?.after.statistics?.device ?? {},
      ).some((value) => SOFTWARE_RENDERER.test(String(value))),
      evidence: env.evidence.directory,
      sweeps: output,
      workProbe,
      allocationProfile,
      nativeProfile,
      profilingStatus,
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
    const status = await call<HostProfileStatus>("hostProfileStatus");
    assert.equal(
      status.available,
      true,
      "Instrumented worker profiling unavailable",
    );
    const collect = async (
      sweep: {
        readonly name: string;
        readonly panels: number;
        readonly treeRows: number;
      },
      idle: boolean,
    ) => {
      await call("mount", [sweep]);
      await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
      const captureId = await call<string>("hostProfileStart");
      if (idle) {
        for (
          let frame = 0;
          frame < GUI_STRESS_WORKLOAD.idleProfileFrames;
          frame += 1
        )
          await call("step", ["idle", frame]);
      } else {
        for (const action of GUI_STRESS_WORKLOAD.allocationActions)
          await call("step", [action]);
      }
      const capture = await call<ProfileCapture>("hostProfileStop");
      assert.equal(capture.captureId, captureId);
      validateSemanticProfile(capture, "wasm");
      return capture;
    };
    await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
    await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
    const ownership = await call<Record<string, unknown>>(
      "hostProfileOwnershipScenario",
    );
    await env.evidence.writeJson("worker-profile-ownership.json", ownership);
    const mixed = await collect(GUI_STRESS_WORKLOAD.probeSweep, false);
    const idleSweeps = [];
    for (const sweep of GUI_STRESS_WORKLOAD.sweeps.filter((item) =>
      item.name.startsWith("panels-"),
    ))
      idleSweeps.push({
        sweep,
        requestedFrames: GUI_STRESS_WORKLOAD.idleProfileFrames,
        capture: await collect(sweep, true),
      });
    await env.evidence.writeJson("core-profile.json", { mixed, idleSweeps });
    return {
      evidence: env.evidence.directory,
      artifact: "core-profile.json",
      build: "render-instrumentation",
      mixed,
      ownership,
      idleSweeps,
      stageInterpretation:
        "Semantic System/phase and fixed stages carry exact Host/World/incarnation/composition identities. Inclusive stage CPU durations are not additive wall time; exclusive allocation categories sum to evaluation-thread totals, excluding background allocations.",
    };
  } finally {
    await call("close");
  }
}

function validateSemanticProfile(
  captured: ProfileCapture,
  target: "native" | "wasm",
) {
  assert.equal(captured.source.target, target);
  assert.equal(captured.source.scope, "evaluation-thread");
  assert.equal(captured.source.backgroundAllocations, "excluded");
  assert.equal(captured.availability.cpu, "available");
  assert.equal(captured.availability.allocations, "available");
  assert.ok(
    captured.stages.some(
      (stage) =>
        stage.kind === "system" &&
        stage.identity.scope === "world" &&
        stage.identity.system.length > 0,
    ),
  );
  assert.ok(captured.stages.some((stage) => BigInt(stage.duration) > 0n));
  assert.equal(
    captured.categories.reduce(
      (total, item) => total + BigInt(item.allocationCalls),
      0n,
    ),
    BigInt(captured.allocations.calls),
  );
  assert.equal(
    captured.categories.reduce(
      (total, item) => total + BigInt(item.requestedBytes),
      0n,
    ),
    BigInt(captured.allocations.requestedBytes),
  );
}

export async function runGuiProfileCapture(
  signal: AbortSignal,
  arrangement: Arrangement,
  output: string,
  eglDirectory?: string,
) {
  const directory = resolve(
    arrangement === "browser"
      ? "target/browser-build/render-instrumentation"
      : "target/gles-host-instrumentation",
  );
  const build: BrowserBuildConfiguration = {
    name: arrangement === "browser" ? "render-instrumentation" : "gles",
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(
      directory,
      arrangement === "browser"
        ? "runtime.wasm"
        : process.platform === "win32"
          ? "gles_host.exe"
          : "gles_host",
    ),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  const config = {
    workspace: process.cwd(),
    build,
    rendering: arrangement === "browser",
    deviceScaleFactor: GUI_STRESS_WORKLOAD.viewport.dpr,
    operationTimeoutMs: 60_000,
    evidenceParent: output,
  };
  const collect = async (
    env: BrowserEnvironmentContext,
    connection: Record<string, unknown>,
  ) => {
    const moduleUrl = `${env.urls.origin}/target/gui-stress/gui-stress-fixture.js`;
    const call = <T>(name: string, args: readonly unknown[] = []) =>
      invoke<T>(env.page, moduleUrl, name, args);
    try {
      await call("initialize", [
        {
          generatedModuleUrl: env.urls.generated,
          measure: false,
          ...connection,
        },
      ]);
      assert.equal(
        (await call<HostProfileStatus>("hostProfileStatus")).available,
        true,
      );
      await call("mount", [GUI_STRESS_WORKLOAD.probeSweep]);
      await call("warmup", [GUI_STRESS_WORKLOAD.warmupFrames]);
      const before = await call<FixtureCapture>("capture");
      assert.ok(paintedPixels(before) > 1000);
      await retainCapture(env, "profile-before", before);
      const ownership = await call<Record<string, unknown>>(
        "hostProfileOwnershipScenario",
      );
      await env.evidence.writeJson("profile-ownership.json", ownership);
      const captureId = await call<string>("hostProfileStart");
      for (const action of GUI_STRESS_WORKLOAD.allocationActions)
        await call("step", [action]);
      const artifact = await call<ProfileCapture>("hostProfileStop");
      assert.equal(artifact.captureId, captureId);
      validateSemanticProfile(
        artifact,
        arrangement === "browser" ? "wasm" : "native",
      );
      await env.evidence.writeJson("profile-capture.json", artifact);
      const after = await call<FixtureCapture>("capture");
      assert.ok(changedPixels(before, after) > 0);
      await retainCapture(env, "profile-after", after);
      await env.evidence.record("profile-summary", {
        captureId: artifact.captureId,
        hostId: artifact.hostId,
        stages: artifact.stages.length,
        categories: artifact.categories.length,
        worlds: new Set(
          artifact.stages.map(
            (stage) =>
              `${stage.identity.worldId}/${stage.identity.incarnation}`,
          ),
        ).size,
      });
      return {
        captureId: artifact.captureId,
        evidence: env.evidence.directory,
      };
    } finally {
      await call("close");
    }
  };
  if (arrangement === "browser")
    return runBrowserEnvironment(
      "gui-profile-capture-worker",
      config,
      signal,
      (env) =>
        collect(env, {
          workerScriptUrl: env.urls.workerScript,
          wasmUrl: env.urls.wasm,
        }),
    );
  if (!eglDirectory)
    throw new Error("Native profiling correctness requires EGL directory");
  return runNativeEnvironment(
    "gui-profile-capture-native",
    {
      executable: build.runtimeWasm,
      schemaArtifact: build.contractArtifact,
      workingDirectory: process.cwd(),
      extraArguments: ["--egl-dir", eglDirectory],
      readinessTimeoutMs: 30_000,
      operationTimeoutMs: 60_000,
      evidenceParent: output,
    },
    signal,
    async (host) => {
      if (!host.presentationUrl)
        throw new Error("Native presentation unavailable");
      return runBrowserEnvironment(
        "gui-profile-capture-native-client",
        config,
        host.signal,
        (env) =>
          collect(env, {
            nativeHost: {
              url: host.url,
              presentationUrl: host.presentationUrl,
            },
          }),
      );
    },
  );
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
  hostBuildDirectory?: string,
  instrumentedDiagnostics = false,
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
  const contentionBefore = {
    loadAverage: loadavg(),
    logicalCpus: cpus().length,
    timestamp: startedAt,
  };
  // Timing runs measure the normal build each backend ships.
  const buildName =
    arrangement === "browser"
      ? "render"
      : instrumentedDiagnostics
        ? "gles-release-instrumented"
        : correctnessOnly
          ? "gles"
          : "gles-release";
  const directory = resolve(
    arrangement === "browser"
      ? "target/browser-build/render"
      : (hostBuildDirectory ??
          (correctnessOnly
            ? "target/gles-host"
            : "target/performance-build/gui-native")),
  );
  const currentSource = await sourceIdentity();
  const fixtureBuild = JSON.parse(
    await readFile("target/gui-stress/build-report.json", "utf8"),
  );
  for (const item of fixtureBuild.artifacts)
    assert.equal(
      item.sha256,
      await digest(
        resolve("target/gui-stress", item.path.split(/[\\/]/).at(-1)),
      ),
      "Fixture artifact changed since build",
    );
  for (const item of fixtureBuild.fixtureSources)
    assert.equal(
      item.sha256,
      await digest(item.path),
      "Fixture source changed since build",
    );
  let buildProvenance = null;
  if (arrangement === "native-gles" && !correctnessOnly) {
    buildProvenance = JSON.parse(
      await readFile(resolve(directory, "build-identity.json"), "utf8"),
    );
    assert.equal(
      buildProvenance.profile,
      "release",
      "Native timing requires a release Host",
    );
    assert.equal(buildProvenance.instrumented, instrumentedDiagnostics);
    assert.deepEqual(
      buildProvenance.source,
      currentSource,
      "Native artifact source differs from current checkout; rebuild before timing",
    );
    assert.equal(
      buildProvenance.executable,
      await digest(
        resolve(
          directory,
          process.platform === "win32" ? "gles_host.exe" : "gles_host",
        ),
      ),
    );
    assert.equal(
      buildProvenance.contract,
      await digest(resolve(directory, "contract.bin")),
    );
  }
  if (arrangement === "browser" && !correctnessOnly) {
    buildProvenance = JSON.parse(
      await readFile(resolve(directory, "build-identity.json"), "utf8"),
    );
    assert.equal(buildProvenance.profile, "release-small");
    assert.equal(buildProvenance.instrumented, false);
    assert.deepEqual(
      buildProvenance.source,
      currentSource,
      "Browser artifact source differs from current checkout; rebuild before timing",
    );
    assert.equal(
      buildProvenance.runtime,
      await digest(resolve(directory, "runtime.wasm")),
    );
    assert.equal(
      buildProvenance.contract,
      await digest(resolve(directory, "contract.bin")),
    );
  }
  const build: BrowserBuildConfiguration = {
    name: arrangement === "browser" ? "render" : "gles",
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(
      directory,
      arrangement === "browser"
        ? "runtime.wasm"
        : process.platform === "win32"
          ? "gles_host.exe"
          : "gles_host",
    ),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  const browserConfig = {
    workspace,
    build,
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
          !correctnessOnly && !instrumentedDiagnostics,
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
              !correctnessOnly && !instrumentedDiagnostics,
              host.processId ?? null,
              instrumentedDiagnostics,
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
    const profileDirectory = resolve(
      "target/browser-build/render-instrumentation",
    );
    const profileProvenance = JSON.parse(
      await readFile(resolve(profileDirectory, "build-identity.json"), "utf8"),
    );
    assert.deepEqual(
      profileProvenance.source,
      currentSource,
      "Instrumented worker artifact source differs from checkout; rebuild",
    );
    assert.equal(profileProvenance.instrumented, true);
    assert.equal(
      profileProvenance.runtime,
      await digest(resolve(profileDirectory, "runtime.wasm")),
    );
    assert.equal(
      profileProvenance.contract,
      await digest(resolve(profileDirectory, "contract.bin")),
    );
    const profileBuild: BrowserBuildConfiguration = {
      name: "render-instrumentation",
      generatedModule: resolve(profileDirectory, "generated.js"),
      runtimeWasm: resolve(profileDirectory, "runtime.wasm"),
      contractArtifact: resolve(profileDirectory, "contract.bin"),
    };
    const profiled = await runBrowserEnvironment(
      "gui-stress-core-profile",
      { ...browserConfig, build: profileBuild },
      signal,
      exerciseCoreProfile,
    );
    coreProfile = {
      ...profiled.value,
      provenance: profileProvenance,
      contractSha256: await digest(profileBuild.contractArtifact),
      runtimeSha256: await digest(profileBuild.runtimeWasm),
    };
  }
  const report = {
    measurementMode: correctnessOnly
      ? "correctness-only"
      : instrumentedDiagnostics
        ? "instrumented-diagnostics"
        : "post-admission-draw",
    workload: GUI_STRESS_WORKLOAD,
    workloadSha256: createHash("sha256")
      .update(JSON.stringify(GUI_STRESS_WORKLOAD))
      .digest("hex"),
    repetitions,
    samplesPerSweep,
    source: currentSource,
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
      provenance: buildProvenance,
      currentSource,
      sourceMatches: buildProvenance
        ? JSON.stringify(buildProvenance.source) ===
          JSON.stringify(currentSource)
        : null,
      clientArtifacts: await clientArtifacts(directory),
      contractSha256: await digest(build.contractArtifact),
      runtimeSha256: await digest(build.runtimeWasm),
    },
    fixture: {
      path: "target/gui-stress/gui-stress-fixture.js",
      sha256: await digest("target/gui-stress/gui-stress-fixture.js"),
      sources: fixtureBuild.fixtureSources,
    },
    assets: await Promise.all(
      ASSETS.map(async (path) => ({ path, sha256: await digest(path) })),
    ),
    pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
    startedAt,
    contention: {
      before: contentionBefore,
      after: {
        loadAverage: loadavg(),
        logicalCpus: cpus().length,
        timestamp: new Date().toISOString(),
      },
      thermal: process.env.IPP_GUI_THERMAL_NOTE
        ? {
            status: "manual-observation",
            note: process.env.IPP_GUI_THERMAL_NOTE,
          }
        : {
            status: "unavailable",
            reason: "No thermal sensor adapter or manual observation recorded",
          },
      scope:
        "machine load averages include unrelated work; never attribute them to IPP alone",
    },
    compositor: {
      settings: {
        gui: "runtime-default",
        chromium: browserLaunchOptions(arrangement === "browser"),
        requestedAngle: process.env.IPP_BROWSER_ANGLE ?? null,
        requestedGpuCompositing:
          process.env.IPP_BROWSER_GPU_COMPOSITING ?? null,
        rawSurfaceCache: GUI_STRESS_WORKLOAD.rawSurfaceCache,
      },
      observed: result.sweeps.map((item) => ({
        sweep: item.sweep.name,
        surfaces: item.after.statistics?.surfaces ?? null,
      })),
    },
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
            ...(instrumentedDiagnostics
              ? {}
              : {
                  coreCpuAndAllocator:
                    "Ordinary Host explicitly reports instrumentation unavailable; use separate --instrumented diagnostics",
                }),
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
    false,
    process.argv.includes("--host-build")
      ? process.argv[process.argv.indexOf("--host-build") + 1]
      : undefined,
    process.argv.includes("--instrumented-diagnostics"),
  );
}
