import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { inspect } from "node:util";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { nativePresentationTransport } from "@ipp/client/testing";
import { hostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import { defineClient } from "../../../tools/shared-host/client.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { encodePng } from "../../../tools/shared-host/png.js";
import { image } from "../../../tools/shared-host/presentation.js";
import type { GalleryBenchmarkConfig } from "../scenarios/gallery.js";
import {
  bodyTop,
  fiducials,
  selectSlots,
  slots,
} from "../scenarios/gui-board.js";
import { openGuiCase } from "../scenarios/gui-rendering.js";

import {
  modals,
  spatialCases,
  isSpatialCase,
  type Projection,
  type BoardCase,
} from "../scenarios/gui-cases.js";
import { projectPixel } from "../scenarios/gui-projection.js";

const json = (value: unknown) =>
  JSON.stringify(value, (_, v) => (typeof v === "bigint" ? String(v) : v), 2);
function distribution(values: number[]) {
  const sorted = values.toSorted((a, b) => a - b);
  return {
    p50: sorted[Math.ceil(sorted.length * 0.5) - 1],
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1],
    count: values.length,
  };
}
/** Count visible ink below each caption, independently of alpha or backend colour conversion. */
function visiblePixels(
  image: RgbaImage,
  x: number,
  y: number,
  width: number,
  height: number,
  projection: Projection,
) {
  const data = new Uint32Array(
    image.pixels.buffer,
    image.pixels.byteOffset,
    image.width * image.height,
  );
  function pixel(cx: number, cy: number) {
    const [px, py] = projectPixel(cx, cy, image, projection);
    return data[Math.floor(py) * image.width + Math.floor(px)];
  }
  const bg = pixel(4, image.height / 2);
  let ink = 0;
  for (let row = y; row < y + height; row++)
    for (let col = x; col < x + width; col++)
      if (pixel(col + 0.5, row + 0.5) !== bg) ink++;
  return ink;
}

function verifyProjection(image: RgbaImage, projection: Projection) {
  for (const [x, y] of fiducials) {
    const [px, py] = projectPixel(x, y, image, projection);
    let found = false;
    for (let dy = -3; dy <= 3; dy++)
      for (let dx = -3; dx <= 3; dx++) {
        const i =
          4 * ((Math.round(py) + dy) * image.width + Math.round(px) + dx);
        if (
          image.pixels[i]! > 180 &&
          image.pixels[i + 1]! < 90 &&
          image.pixels[i + 2]! > 180
        )
          found = true;
      }
    assert.ok(
      found,
      `Missing ${projection} fiducial at ${px.toFixed(1)}, ${py.toFixed(1)}`,
    );
  }
}

export default defineClient({
  async open(context, args) {
    const config: GalleryBenchmarkConfig & {
      url: string;
      presentationUrl?: string;
      boardCase: BoardCase;
    } = JSON.parse(args[0]!);

    const Host = context.contract
      .IppHostClient as typeof import("@ipp/host-contract").IppHostClient;
    const diagnosticsHost = config.presentationUrl
      ? await Host.connectTransport(
          nativePresentationTransport(config.url, config.presentationUrl),
        )
      : await Host.connectWebSocket(config.url);
    return { config, diagnosticsHost };
  },
  async capture({ config, diagnosticsHost }, context) {
    const diagnostics = renderDiagnostics(diagnosticsHost);
    const profiler = config.instrumented
      ? hostProfiling(diagnosticsHost)
      : undefined;
    let device;
    const font = await context.font();
    const runs: unknown[] = [];
    const failures: { name: string; error: string }[] = [];
    const catalog = slots.map(({ id, family, x, y, width, height }) => ({
      id,
      family,
      x,
      y: y + bodyTop,
      width,
      height,
    }));
    await writeFile(
      join(config.output, "catalog.json"),
      json({ slots: catalog, modalCases: modals, spatialCases }),
    );
    const item = config.boardCase;
    {
      const { name, theme, projection } = item;
      const label = [
        theme,
        isSpatialCase(name) ? "spatial" : projection,
        name,
      ].join("-");
      const key = label + "-" + config.repeat;
      try {
        const workload = await openGuiCase(context, item, font);
        try {
          await context.present(workload.binding, async (view) => {
            const { host } = context;
            await workload.ready(view);
            const frame = async (afterSequence = 0n) => {
              const sample = await host.presentation.frame(view, {
                afterSequence,
                afterOutputs: [workload.binding.output],
              });
              assert.ok(
                sample.sequence > afterSequence && sample.drawCalls > 0,
                "Missing completed draws",
              );
              assert.equal(sample.failedDrawCalls, 0);
              return sample;
            };
            await frame();
            await delay(500);
            const first = await frame();
            device ??= (await diagnostics?.statistics())?.device;
            const samples = [];
            let last = first;
            let gpuFrameMs;
            let profile;
            const captureId = await profiler?.start({
              counters: false,
              gpu: "frame",
              maxArtifactBytes: 67108864,
            });
            try {
              const start = performance.now();
              let previous = start;
              // Only boundary observations cross the socket. Sequence deltas include every Host frame.
              const windowMs = Math.max(100, config.minimumSampleMs / 8);
              do {
                await delay(windowMs);
                const next = await frame(last.sequence);
                const now = performance.now();
                const frames = Number(next.sequence - last.sequence);
                samples.push({
                  sequence: String(next.sequence),
                  frames,
                  elapsedMs: now - previous,
                  drawCalls: next.drawCalls,
                  triangles: next.triangles,
                });
                previous = now;
                last = next;
              } while (
                samples.length < 8 ||
                Number(last.sequence - first.sequence) < config.frames
              );
              if (profiler) {
                const data = await profiler.stop();
                profile = key + "-profile.json";
                await writeFile(join(config.output, profile), json(data));
                const records =
                  data.gpu?.records.filter(
                    (r) => !r.preparation && r.scope === "frame",
                  ) ?? [];
                const durations = records
                  .filter(
                    (r) =>
                      r.availability.status === "available" &&
                      r.availability.duration !== undefined,
                  )
                  .map((r) => Number(r.availability.duration) / 1e6);
                gpuFrameMs = {
                  ...distribution(durations),
                  recorded: records.length,
                };
              }
            } finally {
              if (captureId) await profiler!.release(captureId);
            }
            const interaction =
              "scrollGrid" in workload && workload.scrollGrid
                ? await workload.scrollGrid(view)
                : undefined;
            const scroll = interaction && {
              ...distribution(interaction.latencies),
              furthestFirstRow: interaction.furthestFirstRow,
              latencies: interaction.latencies,
            };
            let captureAfter = last.sequence;
            const updates = workload.updates && (await workload.updates());
            let dynamic;
            let beforeEdits: RgbaImage | undefined;
            if (updates) {
              const initialFrame = await host.presentation.capture(view, {
                afterSequence: last.sequence,
                afterOutputs: [workload.binding.output],
              });
              assert.equal(initialFrame.failedDrawCalls, 0);
              const initial = image(initialFrame);
              beforeEdits = initial;
              const before = key + "-before-edits.png";
              await writeFile(join(config.output, before), encodePng(initial));
              const latency: number[] = [],
                commit: number[] = [];
              let sequence = initialFrame.sequence;
              let updateProfile;
              const updateCapture = await profiler?.start({
                counters: false,
                gpu: "frame",
                trace: { maxEvents: 80000 },
                maxArtifactBytes: 67108864,
              });
              try {
                for (let cycle = 1; cycle <= 18; cycle++) {
                  const apply = updates.changes(cycle);
                  const start = performance.now();
                  const accepted = await apply();
                  const next = await frame(sequence);
                  sequence = next.sequence;
                  if (cycle > 2) {
                    latency.push(performance.now() - start);
                    commit.push(accepted - start);
                  }
                }
                if (profiler) {
                  updateProfile = key + "-updates-profile.json";
                  await writeFile(
                    join(config.output, updateProfile),
                    json(await profiler.stop()),
                  );
                }
              } finally {
                if (updateCapture) await profiler!.release(updateCapture);
              }
              dynamic = {
                before,
                updateProfile,
                rowsPerUpdate: updates.rowCount,
                updateToFrameMs: distribution(latency),
                commitMs: distribution(commit),
                latency,
                commit,
              };
              // Allow camera-facing annotation transitions to settle before comparing fixed data states.
              await delay(500);
              captureAfter = sequence;
            }
            const captured = await host.presentation.capture(view, {
              afterSequence: captureAfter,
              afterOutputs: [workload.binding.output],
            });
            assert.equal(captured.failedDrawCalls, 0);
            const pixels = image(captured);
            const changes: Record<string, number> = {};
            if (beforeEdits) {
              const regions = isSpatialCase(name)
                ? [
                    {
                      id: name,
                      x: 0,
                      y: 0,
                      width: pixels.width,
                      height: pixels.height,
                    },
                  ]
                : selectSlots(name)
                    .filter((s) => s.family === "charts")
                    .map((s) => ({ ...s, y: s.y + bodyTop }));
              for (const region of regions) {
                let changed = 0;
                for (let y = region.y; y < region.y + region.height; y++) {
                  for (let x = region.x; x < region.x + region.width; x++) {
                    const [px, py] = projectPixel(
                      x + 0.5,
                      y + 0.5,
                      pixels,
                      projection,
                    );
                    const i =
                      4 * (Math.floor(py) * pixels.width + Math.floor(px));
                    if (
                      [0, 1, 2].some(
                        (c) =>
                          Math.abs(
                            beforeEdits!.pixels[i + c]! - pixels.pixels[i + c]!,
                          ) > 8,
                      )
                    )
                      changed++;
                  }
                }
                assert.ok(
                  changed >= 8,
                  region.id + " did not display changed data",
                );
                changes[region.id] = changed;
              }
            }
            const capture = key + ".png";
            await writeFile(join(config.output, capture), encodePng(pixels));
            if (!isSpatialCase(name)) verifyProjection(pixels, projection);
            const wholeScreen = isSpatialCase(name) || modals.includes(name);
            const selected = wholeScreen ? [] : selectSlots(name);
            const ink = Object.fromEntries(
              selected.map((s) => [
                s.id,
                visiblePixels(
                  pixels,
                  s.x,
                  s.y + bodyTop,
                  s.width,
                  s.height,
                  projection,
                ),
              ]),
            );
            if (wholeScreen)
              assert.ok(
                visiblePixels(
                  pixels,
                  0,
                  0,
                  pixels.width,
                  pixels.height - 18,
                  projection,
                ) > 110,
                "Empty overlay/chart",
              );
            for (const [id, n] of Object.entries(ink))
              assert.ok(n >= 8, key + " empty specimen body: " + id);
            const elapsedMs = samples.reduce(
              (total, s) => total + s.elapsedMs,
              0,
            );
            const frames = Number(last.sequence - first.sequence);
            const raw = key + "-samples.json";
            const timing = config.instrumented
              ? null
              : {
                  frames,
                  elapsedMs,
                  framesPerSecond: (frames * 1000) / elapsedMs,
                  meanFrameMs: elapsedMs / frames,
                  windowMeanFrameMs: distribution(
                    samples.map((s) => s.elapsedMs / s.frames),
                  ),
                };
            await writeFile(
              join(config.output, raw),
              json({
                samples,
                ink,
                timing,
                gpuFrameMs,
                profile,
                capture,
                scroll,
                dynamic: dynamic && { ...dynamic, changes },
              }),
            );
            runs.push({
              name,
              theme,
              projection: isSpatialCase(name) ? "spatial" : projection,
              timing,
              gpuFrameMs,
              scroll,
              dynamic: dynamic && { ...dynamic, changes },
              profile,
              capture,
              raw,
              drawCalls: last.drawCalls,
              triangles: last.triangles,
            });
          });
        } finally {
          await workload.close();
        }
      } catch (error) {
        failures.push({
          name: label,
          error: inspect(error, { depth: 6 }),
        });
      }
    }
    return { report: { device, runs, failures } };
  },
  async close({ diagnosticsHost }) {
    await diagnosticsHost.close();
  },
});
