import assert from "node:assert/strict";
import {
  compareFrames,
  count,
  intersectionOverUnion,
  mask,
  pixelDifference,
  type RgbaFrame,
} from "./retained-gui-images.js";

export interface WorkloadFrame {
  width: number;
  height: number;
  devicePixelRatio: number;
  textPixels: number;
  triangles: number;
  drawCalls: number;
  backend: Record<string, unknown>;
}

export interface RetainedGuiDriver {
  call<T>(name: string, args?: readonly unknown[]): Promise<T>;
  /** Capture the frame after the latest acknowledged state, or with `next` the next frame. */
  capture(label: string, next?: boolean): Promise<WorkloadFrame>;
  /** Completed RGBA pixels of the latest capture with this label. */
  pixels(label: string): RgbaFrame;
}

/** Counters that only GUI-capable render builds export; others report them unavailable. */
export const RETAINED_COUNTERS = [
  "guiBatches",
  "guiRebuilds",
  "guiAllocations",
  "guiResidentBytes",
  "glyphMisses",
  "glyphPopulates",
  "glyphPopulationFailures",
  "glyphPageRetirements",
  "glyphPages",
  "glyphResidentBytes",
  "totalGuiRebuilds",
  "totalGuiAllocations",
  "totalGlyphMisses",
  "totalGlyphPopulates",
  "totalGlyphPopulationFailures",
  "totalGlyphPageRetirements",
] as const;

/**
 * Glyph atlas bounds for the atlas phase and the rest of the run. One unseen
 * window fits in two 512x512 pages, so three pages hold it while the sliding
 * windows exceed the budget. Idle expiry never elapses and some World always
 * demands glyphs, so every page retired after the first window is pressure
 * eviction.
 */
const ATLAS_LIMITS = { maxPages: 3, idlePagePublications: 0xffff_ffff };

/** Terminal text pixels, as classified by the fixture's coverage count. */
const isText = (r: number, g: number, b: number) =>
  g > 160 && r > 120 && b > 120;

/** Scenario-owned GUI shape geometry in Surface metres. */
const GUI_SHAPE = {
  width: 1.6,
  height: 1.2,
  borderWidth: 0.06,
  cornerRadius: 0.12,
} as const;

/** Observable assertions are independent of the worker/process arrangement. */
export async function exerciseRetainedGui(
  driver: RetainedGuiDriver,
  retained: boolean,
  iterations: number,
) {
  const { call } = driver;
  const terminal = {
    rows: 12,
    columns: 36,
    sequence: 0,
    mode: "idle",
    cursor: true,
    width: 640,
    height: 480,
  };
  const atlas = {
    rows: 8,
    columns: 24,
    mode: "unseen",
    cursor: true,
    width: 1280,
    height: 960,
  };
  const samples: {
    label: string;
    config: Record<string, unknown>;
    elapsedMs: number;
    frame: WorkloadFrame;
  }[] = [];
  const number = (
    frame: WorkloadFrame,
    key: (typeof RETAINED_COUNTERS)[number],
  ) => frame.backend[key] as number;
  const checkFrame = (label: string, frame: WorkloadFrame) => {
    assert.equal(frame.backend.failedDrawCalls, 0, label);
    // Missing statistics are unavailable, never a report of zero work.
    for (const key of RETAINED_COUNTERS)
      assert.equal(
        typeof frame.backend[key],
        retained ? "number" : "undefined",
        `${label}: ${key}`,
      );
  };
  const capture = async (label: string, next = false) => {
    const frame = await driver.capture(label, next);
    checkFrame(label, frame);
    return frame;
  };
  /**
   * Submit a workload and capture the next completed frame, so cold atlas
   * work is observed before later frames finish populating it.
   */
  const measure = async (
    label: string,
    operation: string,
    config: Record<string, unknown>,
  ) => {
    const started = performance.now();
    const result = await call<Record<string, unknown> | undefined>(operation, [
      config,
    ]);
    const frame = await capture(label, true);
    samples.push({
      label,
      config,
      elapsedMs: performance.now() - started,
      frame,
    });
    // Only the atlas phase may exceed the page budget and back off populations.
    if (retained && !label.startsWith("atlas"))
      assert.equal(number(frame, "glyphPopulationFailures"), 0, label);
    return { frame, result };
  };
  /**
   * Capture until the atlas has populated every demanded glyph and no batch
   * changes; analytic builds have no such state and settle after one frame.
   */
  const settle = async (label: string) => {
    for (let attempt = 1; attempt <= 120; attempt++) {
      const frame = await capture(label);
      if (
        !retained ||
        (number(frame, "glyphMisses") === 0 &&
          number(frame, "glyphPopulates") === 0 &&
          number(frame, "glyphPopulationFailures") === 0 &&
          number(frame, "guiRebuilds") === 0 &&
          frame.backend.uploadedBytes === 0)
      )
        return { frame, attempts: attempt };
    }
    throw new Error(`${label}: retained presentation did not settle`);
  };
  const upload = (after: WorkloadFrame, before: WorkloadFrame) =>
    Number(after.backend.totalUploadedBytes) -
    Number(before.backend.totalUploadedBytes);
  /** Work accumulated by every rendered tick between two captures. */
  const since = (
    after: WorkloadFrame,
    before: WorkloadFrame,
    key: (typeof RETAINED_COUNTERS)[number],
  ) => number(after, key) - number(before, key);
  const glyphSets = await call<{ printable: number; unseen: number }>(
    "glyphSets",
  );

  // Cold: every printable glyph misses once and populates exactly once.
  const { frame: initial } = await settle("initial");
  const cold = (await measure("cold", "workload", terminal)).frame;
  assert.ok(cold.textPixels > 100, "cold: visible glyph coverage");
  const { frame: warm, attempts: warmup } = await settle("warm");
  assert.ok(warm.textPixels > 100, "warm: visible glyph coverage");
  const coldWork = retained
    ? {
        misses: since(warm, initial, "totalGlyphMisses"),
        populates: since(warm, initial, "totalGlyphPopulates"),
        failures: since(warm, initial, "totalGlyphPopulationFailures"),
      }
    : null;
  if (retained) {
    assert.equal(
      coldWork!.populates,
      glyphSets.printable,
      "cold: each printable glyph populates once",
    );
    assert.ok(coldWork!.misses >= coldWork!.populates, "cold: atlas misses");
    assert.equal(coldWork!.failures, 0);
    assert.ok(number(warm, "glyphPages") > 0);
    assert.ok(number(warm, "guiResidentBytes") > 0);
  }

  const rotated = (
    await measure("oblique", "workload", { ...terminal, angle: 0.4 })
  ).frame;
  if (retained)
    assert.equal(
      upload(rotated, warm),
      0,
      "placement-only change reuses local geometry",
    );
  const front = (await measure("front", "workload", terminal)).frame;
  assert.equal(await call("equal", ["warm", "front"]), true);

  // WebGL presents the rear of the same retained content mirrored, as through glass.
  const rear = (
    await measure("rear", "workload", { ...terminal, angle: Math.PI })
  ).frame;
  const terminalMirror = await call<{
    contentPixels: number;
    mismatchedPixels: number;
    meanError: number;
  }>("mirrorComparison", ["front", "rear"]);
  assert.ok(
    terminalMirror.contentPixels > 10_000 &&
      terminalMirror.mismatchedPixels < terminalMirror.contentPixels * 0.01 &&
      terminalMirror.meanError < 1,
    `rear terminal must mirror the front: ${JSON.stringify(terminalMirror)}`,
  );
  if (retained)
    assert.equal(upload(rear, front), 0, "rear view reuses local geometry");

  // Cursor blink and typing uploads.
  const beforeBlink = (await measure("before-blink", "workload", terminal))
    .frame;
  assert.equal(await call("equal", ["warm", "before-blink"]), true);
  const blink = (
    await measure("blink", "workload", { ...terminal, cursor: false })
  ).frame;
  assert.equal(await call("equal", ["warm", "blink"]), false);
  const blinkUpload = upload(blink, beforeBlink);
  const beforeTyping = await capture("before-typing");
  const typed = (
    await measure("typing", "workload", {
      ...terminal,
      mode: "typing",
      sequence: 1,
    })
  ).frame;
  assert.equal(await call("equal", ["warm", "typing"]), false);
  const typingUpload = upload(typed, beforeTyping);
  if (retained) {
    assert.equal(
      number(blink, "guiResidentBytes"),
      number(warm, "guiResidentBytes"),
    );
  }

  const streaming: number[] = [];
  let previous = typed;
  for (let sequence = 1; sequence <= iterations; sequence++) {
    const frame = (
      await measure(`stream-${sequence}`, "workload", {
        ...terminal,
        mode: sequence % 2 ? "scroll" : "full",
        sequence,
      })
    ).frame;
    streaming.push(upload(frame, previous));
    previous = frame;
    if (retained) {
      assert.equal(
        number(frame, "guiResidentBytes"),
        number(warm, "guiResidentBytes"),
      );
      assert.equal(
        number(frame, "glyphResidentBytes"),
        number(warm, "glyphResidentBytes"),
      );
    }
  }
  // Every row changes when the screen scrolls or is replaced: one full upload
  // derives the vertex size and the per-row bound from this run.
  const fullUpload = streaming[streaming.length - 1]!;
  const glyphQuads = terminal.rows * terminal.columns;
  const glyphVertexBytes = retained ? fullUpload / (glyphQuads * 6) : null;
  if (retained) {
    assert.ok(
      streaming.every((bytes) => bytes === fullUpload),
      `each full replacement uploads the same geometry: ${streaming}`,
    );
    assert.ok(
      Number.isInteger(glyphVertexBytes) && glyphVertexBytes! > 0,
      `full replacement uploads whole glyph quads: ${fullUpload}`,
    );
    const rowBytes = fullUpload / terminal.rows;
    assert.ok(
      typingUpload > 0 && typingUpload <= rowBytes,
      `typing replaces at most one row: ${typingUpload} of ${rowBytes}`,
    );
    assert.ok(
      blinkUpload <= 6 * glyphVertexBytes!,
      `cursor blink uploads at most one quad: ${blinkUpload}`,
    );
  }

  const panels = (
    await measure("three-panels", "workload", { ...terminal, panels: 3 })
  ).frame;
  if (retained) {
    assert.equal(
      number(panels, "glyphResidentBytes"),
      number(warm, "glyphResidentBytes"),
      "panels share font coverage",
    );
    assert.equal(
      number(panels, "guiResidentBytes"),
      3 * number(warm, "guiResidentBytes"),
    );
  }
  await measure("before-recovery", "workload", terminal);
  await call("recover");
  await settle("recovered");
  // Repopulated atlas slots may round coverage by one level at glyph edges.
  const recovered = compareFrames(
    driver.pixels("before-recovery"),
    driver.pixels("recovered"),
    2,
  );
  assert.equal(
    recovered.changedPixels,
    0,
    `recovery must restore the presented text: ${JSON.stringify(recovered)}`,
  );
  // Geometry-count churn must retire removed runs and atlas pages.
  const smaller = (
    await measure("smaller", "workload", { ...terminal, rows: 3, columns: 8 })
  ).frame;
  if (retained)
    assert.ok(
      number(smaller, "guiResidentBytes") < number(warm, "guiResidentBytes"),
    );

  // Unseen glyphs at a larger band grow the atlas past one page; sliding the
  // window through more glyphs than the page budget holds forces eviction.
  const window = atlas.rows * atlas.columns;
  const slide = Math.floor(window / 2);
  assert.ok(glyphSets.unseen >= 7 * slide + window, "unseen glyph set size");
  const atlasSteps: {
    label: string;
    populates: number | null;
    misses: number | null;
    failures: number | null;
    retirements: number | null;
    pages: number | null;
    attempts: number;
  }[] = [];
  let maximumPages = 0;
  let before = smaller;
  // Only GUI render builds own a glyph atlas; the budget applies at the next publication.
  if (retained) await call("glyphAtlasLimits", [ATLAS_LIMITS]);
  const atlasStep = async (label: string, sequence: number) => {
    await measure(label, "workload", { ...atlas, sequence });
    const { frame, attempts } = await settle(label);
    assert.ok(frame.textPixels > 100, `${label}: visible glyph coverage`);
    const step = {
      label,
      populates: retained ? since(frame, before, "totalGlyphPopulates") : null,
      misses: retained ? since(frame, before, "totalGlyphMisses") : null,
      failures: retained
        ? since(frame, before, "totalGlyphPopulationFailures")
        : null,
      retirements: retained
        ? since(frame, before, "totalGlyphPageRetirements")
        : null,
      pages: retained ? number(frame, "glyphPages") : null,
      attempts,
    };
    atlasSteps.push(step);
    before = frame;
    if (retained) {
      maximumPages = Math.max(maximumPages, step.pages!);
      assert.ok(
        step.pages! <= ATLAS_LIMITS.maxPages,
        `${label}: ${step.pages} pages exceed the configured budget`,
      );
    }
    return step;
  };
  const first = await atlasStep("atlas-0", 0);
  if (retained) {
    assert.ok(
      first.populates! >= window,
      "atlas-0: every unseen glyph populates",
    );
    assert.ok(
      first.pages! > 1,
      "an unseen glyph window grows the atlas past one page",
    );
  }
  for (let sequence = 1; sequence < 8; sequence++) {
    const step = await atlasStep(`atlas-${sequence}`, sequence);
    if (retained)
      assert.ok(
        step.populates! >= slide,
        `atlas-${sequence}: new glyphs populate`,
      );
  }
  // Eight windows demand more glyphs than the page budget retains. Each glyph
  // populates once while resident, so repopulating the first window proves its
  // entries were evicted during the run.
  const evicted = await atlasStep("atlas-return", 0);
  if (retained) {
    assert.ok(evicted.populates! > 0, "the first unseen window was evicted");
    const pressure = atlasSteps
      .slice(1)
      .reduce((total, step) => total + step.retirements!, 0);
    assert.ok(
      pressure > 0,
      `atlas pressure must retire pages within the budget: ${JSON.stringify(atlasSteps)}`,
    );
  }
  const repopulated = compareFrames(
    driver.pixels("atlas-0"),
    driver.pixels("atlas-return"),
    8,
  );
  assert.ok(
    repopulated.changedFraction < 0.001,
    `repopulated glyphs must present the same text: ${JSON.stringify(repopulated)}`,
  );

  const gui = retained ? await exerciseRetainedControls(driver, settle) : null;
  const worldSwitch = retained
    ? await exercisePresentedWorldSwitch(driver, settle)
    : null;

  await call("clearWorkload", [
    { width: terminal.width, height: terminal.height },
  ]);
  const empty = await capture("empty");
  assert.equal(empty.textPixels, 0);
  if (retained) {
    assert.equal(number(empty, "guiResidentBytes"), 0);
    assert.equal(number(empty, "glyphResidentBytes"), 0);
  }
  return {
    retained,
    counters: retained ? "reported" : "unavailable",
    viewports: [
      ...new Set(samples.map(({ frame }) => `${frame.width}x${frame.height}`)),
    ],
    devicePixelRatio: warm.devicePixelRatio,
    terminal: { rows: terminal.rows, columns: terminal.columns },
    atlas: {
      rows: atlas.rows,
      columns: atlas.columns,
      pageBudget: ATLAS_LIMITS.maxPages,
      maximumPages: retained ? maximumPages : null,
      steps: atlasSteps,
    },
    cold: coldWork,
    uploads: {
      glyphVertexBytes,
      fullReplacement: retained ? fullUpload : null,
      typing: retained ? typingUpload : null,
      cursorBlink: retained ? blinkUpload : null,
    },
    warmupCaptures: warmup,
    mirror: { terminal: terminalMirror, gui: gui?.mirror ?? null },
    gui,
    worldSwitch,
    warm,
    samples,
    timing:
      "Wall time from submitting a React workload update to completed GPU readback; includes client, transport, scheduling and capture overhead. This is not frame rate or isolated GPU time.",
  };
}

/** GUI shapes, gradients, glow, glyphs and curves exist only in GUI builds. */
async function exerciseRetainedControls(
  driver: RetainedGuiDriver,
  settle: (
    label: string,
  ) => Promise<{ frame: WorkloadFrame; attempts: number }>,
) {
  const { call } = driver;
  const panel = async (variant: string, label: string, angle = 0) => {
    const { pixelsPerMetre } = await call<{ pixelsPerMetre: number }>(
      "guiPanel",
      [{ variant, shape: GUI_SHAPE, angle }],
    );
    const { frame } = await settle(label);
    assert.ok(
      Number(frame.backend.guiBatches) > 0,
      `${label}: retained GUI batches`,
    );
    return { frame, pixelsPerMetre, pixels: driver.pixels(label) };
  };

  // Mixed content: gradient shape with glow, atlas glyphs and a curve drawing.
  const mixed = await panel("mixed", "gui-mixed");
  const classify = (select: (r: number, g: number, b: number) => boolean) =>
    count(mask(mixed.pixels, select));
  const content = {
    gradient: classify((r, _g, b) => r > 200 && b < 90),
    glyphs: classify((r, g, b) => g > 200 && g - r > 60 && g - b > 40),
    // The icon's #2468a0 evenodd frame.
    curve: classify((r, g, b) => b > 120 && b - r > 80 && g < 140),
  };
  for (const [name, pixels] of Object.entries(content))
    assert.ok(
      pixels > 200,
      `mixed GUI content lacks ${name}: ${pixels} pixels`,
    );
  const withoutGlow = await panel(
    "mixed-without-glow",
    "gui-mixed-without-glow",
  );
  const glow = compareFrames(withoutGlow.pixels, mixed.pixels, 12);
  assert.ok(
    glow.changedPixels > 500,
    `glow must extend paint beyond the shape: ${JSON.stringify(glow)}`,
  );
  await panel("mixed", "gui-mixed-rear", Math.PI);
  const mirror = await call<{
    contentPixels: number;
    mismatchedPixels: number;
    meanError: number;
  }>("mirrorComparison", ["gui-mixed", "gui-mixed-rear"]);
  assert.ok(
    mirror.contentPixels > 10_000 &&
      mirror.mismatchedPixels < mirror.contentPixels * 0.01 &&
      mirror.meanError < 1,
    `rear GUI panel must mirror the front: ${JSON.stringify(mirror)}`,
  );

  // Sparse and filled controls under identical camera, viewport and DPR.
  const empty = await panel("empty", "gui-empty");
  const filled = await panel("filled", "gui-filled");
  const sparse = await panel("sparse", "gui-sparse");
  for (const frame of [filled.frame, sparse.frame])
    assert.deepEqual(
      [frame.width, frame.height, frame.devicePixelRatio],
      [empty.frame.width, empty.frame.height, empty.frame.devicePixelRatio],
    );
  const pixelCount = empty.frame.width * empty.frame.height;
  let footprint = 0;
  let outline = 0;
  let outlineOutside = 0;
  let outlineAgrees = 0;
  let interior = 0;
  let interiorFilled = 0;
  // Linear [0.85, 0.25, 0.08] encoded as sRGB.
  const fill = [239, 137, 79];
  for (let index = 0; index < pixelCount; index++) {
    const inFootprint =
      pixelDifference(filled.pixels, empty.pixels, index) > 24;
    const inOutline = pixelDifference(sparse.pixels, empty.pixels, index) > 24;
    footprint += Number(inFootprint);
    if (inOutline) {
      outline++;
      outlineOutside += Number(!inFootprint);
      outlineAgrees += Number(
        pixelDifference(sparse.pixels, filled.pixels, index) <= 16,
      );
    } else if (inFootprint) {
      interior++;
      const offset = index * 4;
      interiorFilled += Number(
        fill.every(
          (value, channel) =>
            Math.abs(filled.pixels.pixels[offset + channel]! - value) <= 12,
        ),
      );
    }
  }
  const ppm = filled.pixelsPerMetre;
  const innerRadius = Math.max(
    GUI_SHAPE.cornerRadius - GUI_SHAPE.borderWidth,
    0,
  );
  const expectedInterior =
    ((GUI_SHAPE.width - 2 * GUI_SHAPE.borderWidth) *
      (GUI_SHAPE.height - 2 * GUI_SHAPE.borderWidth) -
      (4 - Math.PI) * innerRadius ** 2) *
    ppm ** 2;
  const sparseFilled = {
    footprint,
    outline,
    outlineOutside,
    outlineAgreement: outline ? outlineAgrees / outline : 0,
    interior,
    expectedInterior,
    interiorFillAgreement: interior ? interiorFilled / interior : 0,
    triangles: {
      sparse: sparse.frame.triangles,
      filled: filled.frame.triangles,
    },
  };
  assert.ok(
    outline > 0 &&
      outlineOutside <= outline * 0.01 &&
      sparseFilled.outlineAgreement >= 0.85,
    `sparse outline must match the filled border: ${JSON.stringify(sparseFilled)}`,
  );
  assert.ok(
    Math.abs(interior - expectedInterior) <= expectedInterior * 0.08 &&
      sparseFilled.interiorFillAgreement >= 0.95,
    `only the filled control covers its interior: ${JSON.stringify(sparseFilled)}`,
  );
  assert.ok(
    sparse.frame.triangles > filled.frame.triangles,
    `a sparse outline draws edge strips instead of an interior quad: ${JSON.stringify(sparseFilled.triangles)}`,
  );
  return { content, glow, mirror, sparseFilled };
}

/**
 * Presenting another World on the same graphics context releases the previous
 * World's retained batches and glyph atlas demand while the next World renders.
 * Later scenario steps address the second World.
 */
async function exercisePresentedWorldSwitch(
  driver: RetainedGuiDriver,
  settle: (
    label: string,
  ) => Promise<{ frame: WorkloadFrame; attempts: number }>,
) {
  const { call } = driver;
  const stats = (frame: WorkloadFrame) => ({
    failedDrawCalls: Number(frame.backend.failedDrawCalls),
    guiBatches: Number(frame.backend.guiBatches),
    guiResidentBytes: Number(frame.backend.guiResidentBytes),
    glyphPages: Number(frame.backend.glyphPages),
    glyphResidentBytes: Number(frame.backend.glyphResidentBytes),
  });
  const panel = { shape: GUI_SHAPE };

  // The text-free panel's own batches, measured while World A presents it.
  await call("guiPanel", [{ ...panel, variant: "empty" }]);
  const alone = stats((await settle("world-a-empty")).frame);
  await call("guiPanel", [{ ...panel, variant: "mixed" }]);
  const first = stats((await settle("world-a-mixed")).frame);
  assert.ok(
    first.guiResidentBytes > alone.guiResidentBytes && first.glyphPages > 0,
    `World A must hold shape and glyph batches: ${JSON.stringify({ alone, first })}`,
  );

  // Pages without demand now retire at the next publication; World A's text
  // keeps its pages resident while it is presented.
  await call("glyphAtlasLimits", [{ maxPages: 3, idlePagePublications: 1 }]);
  for (let frame = 0; frame < 3; frame++)
    await driver.capture("world-a-held", true);
  const held = stats(await driver.capture("world-a-held", true));
  assert.ok(
    held.glyphPages > 0,
    `demanded pages retired: ${JSON.stringify(held)}`,
  );

  await call("presentSecondWorld", [{ ...panel, variant: "empty" }]);
  let second = stats(await driver.capture("world-b-empty", true));
  let attempts = 1;
  while (
    (second.guiResidentBytes !== alone.guiResidentBytes ||
      second.glyphPages !== 0 ||
      second.glyphResidentBytes !== 0) &&
    attempts < 120
  ) {
    second = stats(await driver.capture("world-b-empty", true));
    attempts++;
  }
  // Only World B's identical panel stays resident, and no World demands glyphs.
  assert.deepEqual(
    [second.guiResidentBytes, second.glyphPages, second.glyphResidentBytes],
    [alone.guiResidentBytes, 0, 0],
    `World A's render caches outlived its presentation: ${JSON.stringify({ alone, first, second, attempts })}`,
  );
  assert.ok(
    second.guiBatches > 0 && second.failedDrawCalls === 0,
    `World B did not render its panel: ${JSON.stringify(second)}`,
  );
  const presented = compareFrames(
    driver.pixels("world-a-empty"),
    driver.pixels("world-b-empty"),
    8,
  );
  assert.ok(
    presented.changedFraction < 0.001,
    `World B must present the same panel through the same camera: ${JSON.stringify(presented)}`,
  );
  return { alone, first, held, second, attempts, presented };
}

export type RetainedGuiReport = Awaited<ReturnType<typeof exerciseRetainedGui>>;

/**
 * Per-label tolerances between analytic and atlas text. Atlas coverage is
 * resampled from band-sized rasters, so glyph edges differ slightly; the
 * reviewed software-GL baseline differs on at most 2.7% of pixels, with text
 * masks agreeing at 0.87 or better and atlas coverage 8% heavier at band 24.
 */
export const COMPARISON_TOLERANCE = {
  channelThreshold: 64,
  maxChangedFraction: 0.035,
  minTextAgreement: 0.85,
  textCoverageRatio: [0.85, 1.15],
} as const;

export interface LabelComparison {
  readonly label: string;
  readonly width: number;
  readonly height: number;
  readonly changedPixels: number;
  readonly changedFraction: number;
  readonly meanChannelDifference: number;
  readonly maxChannelDifference: number;
  readonly analyticTextPixels: number;
  readonly retainedTextPixels: number;
  readonly textAgreement: number;
  readonly textCoverageRatio: number;
  readonly withinTolerance: boolean;
}

/** Compare every label both builds captured; analytic frames are the expectation. */
export function compareBuildFrames(
  analytic: ReadonlyMap<string, RgbaFrame>,
  retained: ReadonlyMap<string, RgbaFrame>,
): LabelComparison[] {
  const labels = [...analytic.keys()].filter((label) => retained.has(label));
  assert.ok(labels.length > 10, `builds share only ${labels.length} labels`);
  return labels.map((label) => {
    const expected = analytic.get(label)!;
    const actual = retained.get(label)!;
    const difference = compareFrames(
      expected,
      actual,
      COMPARISON_TOLERANCE.channelThreshold,
    );
    const expectedText = mask(expected, isText);
    const actualText = mask(actual, isText);
    const textAgreement = intersectionOverUnion(expectedText, actualText);
    const analyticTextPixels = count(expectedText);
    const retainedTextPixels = count(actualText);
    const textCoverageRatio =
      analyticTextPixels === 0
        ? retainedTextPixels === 0
          ? 1
          : Infinity
        : retainedTextPixels / analyticTextPixels;
    const [low, high] = COMPARISON_TOLERANCE.textCoverageRatio;
    return {
      label,
      width: expected.width,
      height: expected.height,
      ...difference,
      analyticTextPixels,
      retainedTextPixels,
      textAgreement,
      textCoverageRatio,
      withinTolerance:
        difference.changedFraction <= COMPARISON_TOLERANCE.maxChangedFraction &&
        textAgreement >= COMPARISON_TOLERANCE.minTextAgreement &&
        textCoverageRatio >= low &&
        textCoverageRatio <= high,
    };
  });
}

export function assertBuildComparisons(
  comparisons: readonly LabelComparison[],
) {
  const failures = comparisons.filter(
    (comparison) => !comparison.withinTolerance,
  );
  assert.deepEqual(
    failures.map(({ label }) => label),
    [],
    `retained frames differ from analytic frames beyond tolerance: ${JSON.stringify(failures)}`,
  );
}
