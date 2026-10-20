import assert from "node:assert/strict";

export interface HtmlCanvasCapture {
  width: number;
  height: number;
  pixels: number[];
  sequence: number;
  statistics: {
    received: number;
    rejected: number;
    closed: number;
    replayed: number;
  };
}

export interface HtmlCanvasDriver {
  call<T>(operation: string, args?: unknown[]): Promise<T>;
  save(label: string, frame: HtmlCanvasCapture): Promise<void>;
}

function srgbOverBlack(channel: number, alpha: number): number {
  const s = channel / 255;
  const linear =
    (s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4) * alpha;
  return Math.round(
    255 *
      (linear <= 0.0031308
        ? linear * 12.92
        : 1.055 * linear ** (1 / 2.4) - 0.055),
  );
}

function assertFrame(
  frame: HtmlCanvasCapture,
  child: number[],
  width = 64,
  height = 64,
): void {
  assert.equal(frame.width, width);
  assert.equal(frame.height, height);
  const background = [128, 64, 32].map((channel) =>
    srgbOverBlack(channel, 128 / 255),
  );
  for (let y = 1; y < height - 1; y++)
    for (let x = 1; x < width - 1; x++) {
      if (x === 15 || x === 16 || y === 15 || y === 16) continue;
      const expected = x < 16 && y < 16 ? child : background;
      for (let channel = 0; channel < 3; channel++)
        assert.ok(
          Math.abs(
            frame.pixels[(y * width + x) * 4 + channel]! - expected[channel]!,
          ) <= 2,
          `pixel ${x},${y} channel ${channel}: ${frame.pixels[(y * width + x) * 4 + channel]} expected ${expected[channel]}`,
        );
      assert.equal(frame.pixels[(y * width + x) * 4 + 3], 255);
    }
}

async function exerciseHtmlCanvasFrames(driver: HtmlCanvasDriver) {
  const initial = await driver.call<HtmlCanvasCapture>("capture");
  await driver.save("initial-green", initial);
  assertFrame(initial, [0, 255, 0]);
  const origins = await driver.call<HtmlCanvasCapture>("originPolicy");
  await driver.save("same-and-cross-origin", origins);
  for (let y = 2; y < 14; y++)
    for (let x = 22; x < 34; x++)
      assert.deepEqual(
        origins.pixels.slice((y * 64 + x) * 4, (y * 64 + x) * 4 + 4),
        [0, 255, 0, 255],
        "Same-origin frame should be painted",
      );
  const background = [128, 64, 32].map((channel) =>
    srgbOverBlack(channel, 128 / 255),
  );
  for (let y = 2; y < 14; y++)
    for (let x = 42; x < 54; x++)
      for (let channel = 0; channel < 3; channel++)
        assert.ok(
          Math.abs(
            origins.pixels[(y * 64 + x) * 4 + channel]! - background[channel]!,
          ) <= 2,
          "Cross-origin frame must be excluded",
        );
  const updated = await driver.call<HtmlCanvasCapture>("update", ["#0000ff"]);
  await driver.save("updated-blue", updated);
  assertFrame(updated, [0, 0, 255]);
  assert.ok(updated.sequence > initial.sequence);
  const resized = await driver.call<HtmlCanvasCapture>("update", [
    "#00ff00",
    96,
    48,
  ]);
  await driver.save("resized-green", resized);
  assertFrame(resized, [0, 255, 0], 96, 48);
  const coalesced = await driver.call<{
    before: { captured: number; coalesced: number };
    after: { captured: number };
    capture: HtmlCanvasCapture;
  }>("coalesce");
  await driver.save("coalesced-blue", coalesced.capture);
  assertFrame(coalesced.capture, [0, 0, 255], 96, 48);
  assert.ok(coalesced.before.coalesced >= 3);
  assert.equal(coalesced.after.captured, coalesced.before.captured + 1);
  return coalesced;
}

export async function exerciseHtmlCanvas(driver: HtmlCanvasDriver) {
  const coalesced = await exerciseHtmlCanvasFrames(driver);
  const replay = await driver.call<HtmlCanvasCapture>("recoverExact");
  await driver.save("replayed-after-loss", replay);
  assert.deepEqual(
    replay.pixels,
    coalesced.capture.pixels,
    "Recovery must reproduce the retained image after the live DOM changed",
  );
  assert.equal(replay.sequence, coalesced.capture.sequence);
  assert.equal(replay.statistics.replayed, 1);
  const rejected = await driver.call<{
    statistics: HtmlCanvasCapture["statistics"];
  }>("rejectStale");
  assert.equal(rejected.statistics.rejected, 1);
  assert.equal(
    rejected.statistics.closed,
    rejected.statistics.received - 1,
    "Only the currently admitted image remains leased",
  );
  const statistics = await driver.call<{
    producer: unknown;
    latencies: number[];
  }>("statistics");
  assert.ok(
    statistics.latencies.every((value) => value >= 0 && value < 10_000),
  );
  const disposed = await driver.call<{
    statistics: HtmlCanvasCapture["statistics"];
  }>("dispose");
  assert.equal(
    disposed.statistics.closed,
    disposed.statistics.received,
    "Disposal releases every transferred image",
  );
  return {
    statistics,
    consumer: disposed.statistics,
    frameCount: 6,
    latencyMeaning:
      "main capture to worker import/draw submission; not physical display latency",
  };
}

export async function exerciseHtmlCanvasDevice(driver: HtmlCanvasDriver) {
  await exerciseHtmlCanvasFrames(driver);
  const rejected = await driver.call<{
    statistics: HtmlCanvasCapture["statistics"];
  }>("rejectStale");
  assert.equal(rejected.statistics.rejected, 1);
  assert.equal(rejected.statistics.closed, rejected.statistics.received);
  const { checks } = await driver.call<{
    checks: Record<string, boolean | number>;
  }>("deviceChecks");
  for (const [name, result] of Object.entries(checks)) {
    if (name !== "fencePolls") assert.equal(result, true, name);
  }
  const disposed = await driver.call<{
    statistics: HtmlCanvasCapture["statistics"];
  }>("dispose");
  assert.equal(disposed.statistics.closed, disposed.statistics.received);
  return { checks, consumer: disposed.statistics };
}
