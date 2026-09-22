import assert from "node:assert/strict";

export interface WorkloadFrame {
  textPixels: number;
  triangles: number;
  drawCalls: number;
  backend: Record<string, number | string | boolean>;
}

export interface RetainedGuiDriver {
  call<T>(name: string, args?: readonly unknown[]): Promise<T>;
  capture(label: string): Promise<WorkloadFrame>;
}

/** Observable assertions are independent of the worker/process arrangement. */
export async function exerciseRetainedGui(
  driver: RetainedGuiDriver,
  retained: boolean,
  iterations: number,
) {
  const { call, capture } = driver;
  const config = {
    rows: 12,
    columns: 36,
    sequence: 0,
    mode: "idle",
    cursor: true,
  };
  const samples: { label: string; elapsedMs: number; frame: WorkloadFrame }[] =
    [];
  const measure = async (label: string, patch: Record<string, unknown>) => {
    const started = performance.now();
    await call("workload", [{ ...config, ...patch }]);
    const frame = await capture(label);
    samples.push({ label, elapsedMs: performance.now() - started, frame });
    assert.equal(frame.backend.failedDrawCalls, 0, label);
    assert.equal(
      Number(frame.backend.glyphPopulationFailures),
      0,
      `${label}: glyph population failures`,
    );
    assert.ok(frame.textPixels > 100, `${label}: visible glyph coverage`);
    return frame;
  };
  await measure("cold", {});
  // A completed presentation barrier, then a second frame proves steady state.
  const warm = await capture("warm");
  const number = (frame: WorkloadFrame, key: string) =>
    Number(frame.backend[key]);
  if (retained) {
    assert.equal(number(warm, "uploadedBytes"), 0);
    assert.equal(number(warm, "guiRebuilds"), 0);
    assert.equal(number(warm, "glyphPopulates"), 0);
    assert.equal(number(warm, "glyphPopulationFailures"), 0);
    assert.ok(number(warm, "glyphPages") > 0);
    assert.ok(number(warm, "guiResidentBytes") > 0);
  }
  const rotated = await measure("oblique", { angle: 0.4 });
  if (retained) {
    assert.equal(
      number(rotated, "totalUploadedBytes"),
      number(warm, "totalUploadedBytes"),
      "placement-only change reuses local geometry",
    );
  }
  await measure("front", {});
  assert.equal(await call("equal", ["warm", "front"]), true);
  const blink = await measure("blink", { cursor: false });
  assert.equal(await call("equal", ["warm", "blink"]), false);
  if (retained)
    assert.equal(
      number(blink, "guiResidentBytes"),
      number(warm, "guiResidentBytes"),
    );
  const beforeTyping = await capture("before-typing");
  const typed = await measure("typing", { mode: "typing", sequence: 1 });
  assert.equal(await call("equal", ["warm", "typing"]), false);
  if (retained) {
    const uploaded =
      number(typed, "totalUploadedBytes") -
      number(beforeTyping, "totalUploadedBytes");
    // One row of 36 glyphs, six vertices each, eight f32 values per vertex.
    assert.ok(
      uploaded > 0 && uploaded <= 36 * 6 * 32 + 1024,
      `bounded row replacement: ${uploaded}`,
    );
  }
  for (let sequence = 1; sequence <= iterations; sequence++) {
    const frame = await measure(`stream-${sequence}`, {
      mode: sequence % 2 ? "scroll" : "full",
      sequence,
    });
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
  const panels = await measure("three-panels", { panels: 3 });
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
  await measure("before-recovery", {});
  await call("recover");
  await capture("recovered");
  assert.equal(await call("equal", ["before-recovery", "recovered"]), true);
  // Geometry-count churn must retire removed runs and atlas pages.
  const smaller = await measure("smaller", { rows: 3, columns: 8 });
  if (retained)
    assert.ok(
      number(smaller, "guiResidentBytes") < number(warm, "guiResidentBytes"),
    );
  await call("clearWorkload");
  const empty = await capture("empty");
  assert.equal(empty.textPixels, 0);
  if (retained) {
    assert.equal(number(empty, "guiResidentBytes"), 0);
    assert.equal(number(empty, "glyphResidentBytes"), 0);
  }
  return {
    retained,
    viewport: [640, 480],
    dpr: 1,
    rows: 12,
    columns: 36,
    glyphVertexBytes: 32,
    warm,
    samples,
    timing:
      "Wall time from submitting a React workload update to completed GPU readback and PNG artifact write; includes client, transport, scheduling and capture overhead. This is not frame rate or isolated GPU time.",
  };
}

export type RetainedGuiReport = Awaited<ReturnType<typeof exerciseRetainedGui>>;

/** Atlas glyphs must cover the same text pixels as analytic glyphs. */
export function assertEquivalentTextCoverage(
  analytic: RetainedGuiReport,
  retained: RetainedGuiReport,
) {
  assert.equal(analytic.retained, false, "analytic report");
  assert.equal(retained.retained, true, "retained report");
  const coverageRatio = retained.warm.textPixels / analytic.warm.textPixels;
  assert.ok(
    coverageRatio > 0.8 && coverageRatio < 1.2,
    `analytic/atlas text coverage: ${coverageRatio}`,
  );
}
