import assert from "node:assert/strict";
import {
  compareFrames,
  type FrameDifference,
  type RgbaFrame,
} from "./retained-gui-images.js";
import { SURFACE_CACHE_COUNTERS } from "./retained-gui-scenario.js";

export interface CacheFrame {
  width: number;
  height: number;
  devicePixelRatio: number;
  drawCalls: number;
  triangles: number;
  backend: Record<string, unknown>;
}

/** Decimal-text entity identities: the fixture reports bigint values as text. */
export interface CacheRecord {
  entity: string;
  mode:
    | "near"
    | "interaction"
    | "fallback"
    | "unavailable"
    | "culled"
    | "reused"
    | "repainted";
  band: number;
  width: number;
  height: number;
  repaints: number;
  reuses: number;
  paintedAtMs: number;
  residentBytes: number;
}

export interface SurfaceCacheDriver {
  /** Browser distribution under test, whose `webgl.js` the bridge probe loads. */
  readonly build: string;
  /** Whether the distribution renders GUI roots. */
  readonly gui: boolean;
  call<T>(name: string, args?: readonly unknown[]): Promise<T>;
  /** Capture the frame after the latest acknowledged state, or with `next` the next frame. */
  capture(label: string, next?: boolean): Promise<CacheFrame>;
  /** Completed RGBA pixels of the latest capture with this label. */
  pixels(label: string): RgbaFrame;
}

/**
 * Terminal policy: direct inside 4 m, band 1 in [4, 8), band 2 in [8, 16).
 * The 320 x 240 view spans 3 m vertically, so band 1 matches the 80 px/m
 * screen density; the orthographic camera keeps that density at any distance.
 */
export const TERMINAL_POLICY = {
  direct_distance: 4,
  texels_per_metre: 80,
  max_refresh_hz: 10,
} as const;

/** A two-second refresh interval separates coalesced content from resource changes. */
export const SLOW_POLICY = { ...TERMINAL_POLICY, max_refresh_hz: 0.5 } as const;

/** GUI panels render into a 640 x 480 view: 160 px/m. */
export const GUI_POLICY = {
  ...TERMINAL_POLICY,
  texels_per_metre: 160,
} as const;

const DISTANCE = { near: 2, band1: 6, moved: 7, band2: 12 } as const;

/** The 3.8 x 2.4 m terminal at band 1 and at the halved band-2 density. */
const BAND1_SIZE = [304, 192] as const;
const BAND2_SIZE = [152, 96] as const;

/**
 * Cached versus direct captures at matched density. The orthographic camera
 * centres the terminal on whole pixels, so bilinear sampling lands on texel
 * centres and both paths share the same shapes and glyph atlas bands; what
 * remains is SRGB8 rounding of premultiplied low-alpha edges. The bounds follow
 * the native plan (mean <= 2, maximum <= 24 per channel), well inside the
 * retained-gui analytic-versus-atlas tolerance (64 per channel on at most 3.5%
 * of pixels) that covers genuinely different glyph rasters.
 */
export const CACHE_TOLERANCE = {
  maxChannelDifference: 24,
  meanChannelDifference: 2,
} as const;

/** Band 2 halves the density; only resampled edges may differ, loosely. */
export const REDUCED_TOLERANCE = { meanChannelDifference: 16 } as const;

/** Recovered glyph atlas slots may round coverage by one level at glyph edges. */
const RECOVERY_THRESHOLD = 2;

const DEFAULT_BUDGET = 32 << 20;

export type SurfaceCacheReport = Awaited<
  ReturnType<typeof exerciseSurfaceCache>
>;

/** Observable assertions are independent of the worker/process arrangement. */
export async function exerciseSurfaceCache(driver: SurfaceCacheDriver) {
  const { call } = driver;
  const records = (frame: CacheFrame) =>
    frame.backend.surfaceCaches as CacheRecord[];
  const counter = (
    frame: CacheFrame,
    key: (typeof SURFACE_CACHE_COUNTERS)[number] | `total${string}`,
  ) => frame.backend[key] as number;
  const capture = async (label: string, next = false) => {
    const frame = await driver.capture(label, next);
    assert.equal(frame.backend.failedDrawCalls, 0, label);
    for (const key of SURFACE_CACHE_COUNTERS)
      assert.equal(typeof frame.backend[key], "number", `${label}: ${key}`);
    assert.ok(Array.isArray(frame.backend.surfaceCaches), label);
    return frame;
  };
  const settle = async (label: string) => {
    for (let attempt = 1; attempt <= 120; attempt++) {
      const frame = await capture(label, attempt > 1);
      if (frame.backend.uploadedBytes === 0) return frame;
    }
    throw new Error(`${label}: presentation did not settle`);
  };
  const compare = (expected: string, actual: string): FrameDifference =>
    compareFrames(
      driver.pixels(expected),
      driver.pixels(actual),
      CACHE_TOLERANCE.maxChannelDifference,
    );
  const identical = (expected: string, actual: string) => {
    const difference = compareFrames(
      driver.pixels(expected),
      driver.pixels(actual),
      0,
    );
    assert.equal(
      difference.changedPixels,
      0,
      `${actual} must equal ${expected}: ${JSON.stringify(difference)}`,
    );
  };
  const matched = (expected: string, actual: string) => {
    const difference = compare(expected, actual);
    assert.ok(
      difference.maxChannelDifference <= CACHE_TOLERANCE.maxChannelDifference &&
        difference.meanChannelDifference <=
          CACHE_TOLERANCE.meanChannelDifference,
      `${actual} differs from direct ${expected}: ${JSON.stringify(difference)}`,
    );
    return difference;
  };
  const comparisons: Record<string, FrameDifference> = {};

  // Device-level bridge oracle against the shipped WebGL bridge (ipp-s1ge.2.2).
  const bridge = await call<Record<string, unknown>>("bridgeProbe", [
    driver.build,
    driver.gui,
  ]);

  // Absent policy: zero cache work and no records at every distance, and the
  // orthographic view keeps direct pixels identical across distances.
  await call("cacheTerminal", [{}]);
  await settle("direct-settled");
  for (const [label, distance] of Object.entries(DISTANCE)) {
    await call("cameraDistance", [distance]);
    const frame = await capture(`direct-${label}`);
    for (const key of SURFACE_CACHE_COUNTERS)
      assert.equal(counter(frame, key), 0, `default policy ${label}: ${key}`);
    assert.deepEqual(records(frame), [], `default policy ${label} records`);
    if (label !== "near") identical("direct-near", `direct-${label}`);
  }

  // Opt in while near. Until RenderService caches (ipp-s1ge.2.3) opted-in
  // Surfaces report no records; the caller reports that as a visible skip.
  await call("cameraDistance", [DISTANCE.near]);
  const terminal = await call<string>("setSurfaceCache", [
    "surface-terminal",
    TERMINAL_POLICY,
  ]);
  const record = (frame: CacheFrame, entity = terminal) =>
    records(frame).find((candidate) => candidate.entity === entity);
  let near = await capture("cache-near");
  for (let attempt = 0; attempt < 4 && !record(near); attempt++)
    near = await capture("cache-near", true);
  if (!record(near) && counter(near, "surfaceCacheDirect") === 0) {
    identical("direct-near", "cache-near");
    return { status: "inactive" as const, bridge, comparisons };
  }
  assert.equal(record(near)?.mode, "near");
  assert.equal(counter(near, "surfaceCacheDirect"), 1);
  assert.equal(counter(near, "surfaceCacheEntries"), 0);
  identical("direct-near", "cache-near");

  /** Capture until the terminal's record satisfies `done`, returning every frame seen. */
  const until = async (
    label: string,
    done: (record: CacheRecord, frame: CacheFrame) => boolean,
    limit = 60,
    entity = terminal,
  ) => {
    const seen: CacheFrame[] = [];
    for (let attempt = 0; attempt < limit; attempt++) {
      const frame = await capture(label, attempt > 0);
      seen.push(frame);
      const current = record(frame, entity);
      if (current && done(current, frame))
        return { frame, record: current, seen };
    }
    throw new Error(
      `${label}: cache state not reached: ${JSON.stringify(records(seen.at(-1)!))}`,
    );
  };
  /** Current content presented directly: move near, capture, restore. */
  const direct = async (label: string, restore: number) => {
    await call("cameraDistance", [DISTANCE.near]);
    await until(label, (current) => current.mode === "near", 10);
    await call("cameraDistance", [restore]);
  };

  // Band 1: one image at matched density, then warm reuse without work.
  await call("cameraDistance", [DISTANCE.band1]);
  const cold = await until(
    "cache-band1",
    (current) => current.mode === "reused",
  );
  assert.deepEqual(
    [cold.record.band, cold.record.width, cold.record.height],
    [1, ...BAND1_SIZE],
  );
  assert.equal(cold.record.residentBytes, BAND1_SIZE[0] * BAND1_SIZE[1] * 4);
  assert.equal(counter(cold.frame, "surfaceCacheEntries"), 1);
  assert.equal(
    counter(cold.frame, "surfaceCacheResidentBytes"),
    cold.record.residentBytes,
  );
  assert.ok(cold.record.repaints >= 1);
  comparisons.band1 = matched("direct-near", "cache-band1");
  const warm = await capture("cache-band1-warm", true);
  assert.equal(record(warm)?.mode, "reused");
  assert.equal(counter(warm, "surfaceCacheRepaints"), 0);
  assert.equal(counter(warm, "surfaceCacheReuses"), 1);
  assert.equal(warm.backend.uploadedBytes, 0, "warm cached frame uploads");
  identical("cache-band1", "cache-band1-warm");

  // Camera motion inside the band composites without repainting or allocating.
  await call("cameraDistance", [DISTANCE.moved]);
  const moved = await capture("cache-band1-moved");
  await capture("cache-band1-moved", true);
  const movedLast = await capture("cache-band1-moved", true);
  assert.equal(
    counter(movedLast, "totalSurfaceCacheRepaints") -
      counter(warm, "totalSurfaceCacheRepaints"),
    0,
    "camera motion inside a band repainted",
  );
  assert.equal(
    counter(movedLast, "totalSurfaceCacheAllocations") -
      counter(warm, "totalSurfaceCacheAllocations"),
    0,
  );
  assert.equal(record(moved)?.band, 1);
  identical("cache-band1", "cache-band1-moved");

  // Content edits wait for the refresh interval and coalesce into the latest
  // state; the stale image stays on screen until then.
  await call("setSurfaceCache", ["surface-terminal", SLOW_POLICY]);
  const slow = await until(
    "cache-slow",
    (current) => current.mode === "reused" && current.repaints >= 1,
  );
  const interval = 1000 / SLOW_POLICY.max_refresh_hz;
  const painted = slow.record;
  await call("cacheTerminal", [{ cursor: [1, 0.2, 0.2, 1] }]);
  await call("cacheTerminal", [{ cursor: [0.2, 0.4, 1, 1] }]);
  const stale = await capture("cache-stale");
  const staleRecord = record(stale)!;
  if (staleRecord.repaints === painted.repaints) {
    assert.equal(staleRecord.mode, "reused");
    identical("cache-slow", "cache-stale");
  }
  const coalesced = await until(
    "cache-coalesced",
    (current) => current.repaints > painted.repaints,
    Math.ceil((interval * 2) / 16) + 60,
  );
  assert.equal(
    coalesced.record.repaints,
    painted.repaints + 1,
    "coalesced edits repaint once",
  );
  assert.ok(
    coalesced.record.paintedAtMs - painted.paintedAtMs >= interval - 1,
    `repainted before the refresh interval: ${JSON.stringify({ painted, coalesced: coalesced.record })}`,
  );
  await direct("direct-coalesced", DISTANCE.band1);
  comparisons.coalesced = matched("direct-coalesced", "cache-coalesced");

  // Resource identity changes bypass the cadence: a pending font and its
  // arrival each repaint well inside the two-second interval.
  const beforePending = (
    await until("cache-before-pending", (current) => current.mode === "reused")
  ).record;
  await call("cacheTerminal", [{ cursor: [0.2, 0.4, 1, 1], font: "pending" }]);
  const pending = await until(
    "cache-pending",
    (current) => current.repaints > beforePending.repaints,
    30,
  );
  assert.ok(
    pending.record.paintedAtMs - beforePending.paintedAtMs < interval,
    `resource change waited for the refresh interval: ${JSON.stringify(pending.record)}`,
  );
  await call("provideFont");
  const arrival = await until(
    "cache-arrival",
    (current) =>
      current.repaints > pending.record.repaints && current.mode === "reused",
    60,
  );
  assert.ok(
    arrival.record.paintedAtMs - pending.record.paintedAtMs < interval,
    `resource arrival waited for the refresh interval: ${JSON.stringify(arrival.record)}`,
  );
  await direct("direct-arrival", DISTANCE.band1);
  comparisons.arrival = matched("direct-arrival", "cache-arrival");

  // Continuous edits at the ordinary cap: no starvation and no excess.
  await call("setSurfaceCache", ["surface-terminal", TERMINAL_POLICY]);
  await call("cacheTerminal", [{}]);
  const first = await until(
    "cache-continuous-start",
    (current) => current.mode === "reused",
  );
  const started = performance.now();
  for (let step = 0; step < 24; step++) {
    await call("cacheTerminal", [
      { cursor: step % 2 ? [0.3, 1, 0.5, 1] : [1, 1, 0.3, 1] },
    ]);
    await capture("cache-continuous", true);
  }
  const last = await capture("cache-continuous", true);
  const elapsed = performance.now() - started;
  const continuous = record(last)!.repaints - first.record.repaints;
  const cap = (elapsed / 1000) * TERMINAL_POLICY.max_refresh_hz;
  assert.ok(
    continuous >= 1 && continuous <= Math.ceil(cap) + 1,
    `continuous edits repainted ${continuous} times in ${elapsed} ms (cap ${cap})`,
  );

  // Band crossing halves the density; hysteresis keeps the band near its boundary.
  await call("cacheTerminal", [{}]);
  await call("cameraDistance", [DISTANCE.band2]);
  const band2 = await until(
    "cache-band2",
    (current) => current.band === 2 && current.mode === "reused",
  );
  assert.deepEqual([band2.record.width, band2.record.height], [...BAND2_SIZE]);
  await direct("direct-band2", DISTANCE.band2);
  const reduced = compare("direct-band2", "cache-band2");
  assert.ok(
    reduced.meanChannelDifference <= REDUCED_TOLERANCE.meanChannelDifference,
    `band 2 differs from direct beyond its reduced density: ${JSON.stringify(reduced)}`,
  );
  comparisons.band2 = reduced;
  const settledBand2 = await until(
    "cache-band2-settled",
    (current) => current.band === 2 && current.mode === "reused",
  );
  for (const distance of [7.5, 8.5, 7.5, 8.5]) {
    await call("cameraDistance", [distance]);
    const frame = await capture(`cache-hysteresis-${distance}`);
    assert.equal(record(frame)?.band, 2, `band changed at ${distance} m`);
  }
  const afterHysteresis = await capture("cache-hysteresis", true);
  assert.equal(
    counter(afterHysteresis, "totalSurfaceCacheAllocations") -
      counter(settledBand2.frame, "totalSurfaceCacheAllocations"),
    0,
    "hysteresis churned allocations",
  );
  assert.equal(
    counter(afterHysteresis, "totalSurfaceCacheRepaints") -
      counter(settledBand2.frame, "totalSurfaceCacheRepaints"),
    0,
    "hysteresis churned repaints",
  );
  await call("cameraDistance", [DISTANCE.band1]);
  const returned = await until(
    "cache-band1-return",
    (current) => current.band === 1 && current.mode === "reused",
  );
  assert.deepEqual(
    [returned.record.width, returned.record.height],
    [...BAND1_SIZE],
  );

  // Translucent overlap and the mirrored rear view through the cache.
  await call("cacheTerminal", [{ translucent: true }]);
  await until("cache-translucent", (current) => current.mode === "reused");
  await direct("direct-translucent", DISTANCE.band1);
  comparisons.translucent = matched("direct-translucent", "cache-translucent");
  await call("cacheTerminal", [{ translucent: true, angle: Math.PI }]);
  await until("cache-rear", (current) => current.mode === "reused");
  await direct("direct-rear", DISTANCE.band1);
  comparisons.rear = matched("direct-rear", "cache-rear");
  const mirror = await call<{
    contentPixels: number;
    mismatchedPixels: number;
    meanError: number;
  }>("mirrorComparison", ["cache-translucent", "cache-rear"]);
  assert.ok(
    mirror.contentPixels > 10_000 &&
      mirror.mismatchedPixels < mirror.contentPixels * 0.01 &&
      mirror.meanError < 1,
    `cached rear view must mirror the front: ${JSON.stringify(mirror)}`,
  );
  await call("cacheTerminal", [{}]);
  await until("cache-front", (current) => current.mode === "reused");

  // Budget exhaustion evicts and presents directly; restoring it repaints.
  await call("surfaceCacheBudget", [1024]);
  const fallback = await until(
    "cache-fallback",
    (current) => current.mode === "fallback",
    10,
  );
  assert.equal(counter(fallback.frame, "surfaceCacheEntries"), 0);
  assert.equal(counter(fallback.frame, "surfaceCacheResidentBytes"), 0);
  assert.equal(fallback.record.residentBytes, 0);
  assert.ok(counter(fallback.frame, "totalSurfaceCacheFallbacks") > 0);
  await direct("direct-fallback", DISTANCE.band1);
  identical("direct-fallback", "cache-fallback");
  await call("surfaceCacheBudget", [DEFAULT_BUDGET]);
  await until("cache-rebudgeted", (current) => current.mode === "reused");

  // Context loss drops every image; recovery repaints identical pixels.
  await capture("cache-before-recovery", true);
  await call("recover");
  const recovered = await until(
    "cache-recovered",
    (current, frame) =>
      current.mode === "reused" && frame.backend.uploadedBytes === 0,
  );
  assert.equal(counter(recovered.frame, "surfaceCacheEntries"), 1);
  const recovery = compareFrames(
    driver.pixels("cache-before-recovery"),
    driver.pixels("cache-recovered"),
    RECOVERY_THRESHOLD,
  );
  assert.equal(
    recovery.changedPixels,
    0,
    `recovery must restore the cached pixels: ${JSON.stringify(recovery)}`,
  );

  // Removing the policy releases the image.
  await call("setSurfaceCache", ["surface-terminal", null]);
  const removed = await capture("cache-removed");
  assert.deepEqual(records(removed), []);
  assert.equal(counter(removed, "surfaceCacheEntries"), 0);
  assert.equal(counter(removed, "surfaceCacheResidentBytes"), 0);

  const gui = driver.gui
    ? await exerciseCachedGui(driver, capture, records)
    : null;

  // Replacing the presented World (GUI builds) or deleting the content
  // releases every image.
  await call("cacheTerminal", [{}]);
  await call("cameraDistance", [DISTANCE.band1]);
  const reopened = await call<string>("setSurfaceCache", [
    "surface-terminal",
    TERMINAL_POLICY,
  ]);
  const open = await until(
    "cache-before-close",
    (current) => current.mode === "reused",
    60,
    reopened,
  );
  assert.ok(counter(open.frame, "surfaceCacheResidentBytes") > 0);
  if (driver.gui)
    await call("presentSecondWorld", [
      {
        variant: "empty",
        shape: {
          width: 1.6,
          height: 1.2,
          borderWidth: 0.06,
          cornerRadius: 0.12,
        },
      },
    ]);
  else await call("clearWorkload", [{ width: 320, height: 240 }]);
  const closed = await capture("cache-closed", true);
  assert.deepEqual(records(closed), []);
  assert.equal(counter(closed, "surfaceCacheEntries"), 0);
  assert.equal(counter(closed, "surfaceCacheResidentBytes"), 0);

  return {
    status: "active" as const,
    bridge,
    comparisons,
    continuous: { repaints: continuous, elapsedMs: elapsed, cap },
    mirror,
    gui,
  };
}

/** Live GUI interaction promotes a cached panel to current direct presentation. */
async function exerciseCachedGui(
  driver: SurfaceCacheDriver,
  capture: (label: string, next?: boolean) => Promise<CacheFrame>,
  records: (frame: CacheFrame) => CacheRecord[],
) {
  const { call } = driver;
  await call("guiPanel", [
    {
      variant: "mixed",
      shape: { width: 1.6, height: 1.2, borderWidth: 0.06, cornerRadius: 0.12 },
    },
  ]);
  await call("cameraDistance", [DISTANCE.band1]);
  const panel = await call<string>("setSurfaceCache", [
    "retained-gui-panel",
    GUI_POLICY,
  ]);
  const until = async (label: string, mode: CacheRecord["mode"]) => {
    for (let attempt = 0; attempt < 60; attempt++) {
      const frame = await capture(label, attempt > 0);
      const current = records(frame).find(
        (candidate) => candidate.entity === panel,
      );
      if (current?.mode === mode) return { frame, record: current };
    }
    throw new Error(`${label}: GUI panel never reached ${mode}`);
  };
  const cached = await until("gui-cached", "reused");
  const hovered = await (async () => {
    await call("hoverPanel", [true]);
    return until("gui-hovered", "interaction");
  })();
  assert.equal(hovered.frame.backend.surfaceCacheRepaints, 0);
  await call("cameraDistance", [DISTANCE.near]);
  const nearHovered = await until("gui-direct-hovered", "interaction");
  const promoted = compareFrames(
    driver.pixels("gui-direct-hovered"),
    driver.pixels("gui-hovered"),
    0,
  );
  assert.equal(
    promoted.changedPixels,
    0,
    `interaction promotion must present current direct content: ${JSON.stringify(promoted)}`,
  );
  await call("cameraDistance", [DISTANCE.band1]);
  await until("gui-hovered-far", "interaction");
  await call("hoverPanel", [false]);
  const released = await until("gui-released", "reused");
  await call("cameraDistance", [DISTANCE.near]);
  await until("gui-direct-released", "near");
  const release = compareFrames(
    driver.pixels("gui-direct-released"),
    driver.pixels("gui-released"),
    CACHE_TOLERANCE.maxChannelDifference,
  );
  assert.ok(
    release.maxChannelDifference <= CACHE_TOLERANCE.maxChannelDifference &&
      release.meanChannelDifference <= CACHE_TOLERANCE.meanChannelDifference,
    `released panel shows a stale image: ${JSON.stringify(release)}`,
  );
  await call("setSurfaceCache", ["retained-gui-panel", null]);
  return {
    cached: cached.record,
    hovered: hovered.record,
    nearHovered: nearHovered.record,
    released: released.record,
    release,
  };
}
