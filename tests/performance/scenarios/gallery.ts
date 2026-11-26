/** Gallery workload driven through authored actions and completed presentations. */
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { GallerySceneDefinition } from "../../../examples/demos/shared/scene.js";
import type {
  RenderDeviceInfo,
  RenderDiagnostics,
} from "@ipp/client/diagnostics";
import type { HostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import type { ClientContext } from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import { encodePng } from "../../../tools/shared-host/png.js";
import { image } from "../../../tools/shared-host/presentation.js";

export interface GalleryBenchmarkConfig {
  readonly source: { revision: string; sourceSha256: string };
  readonly backend: "metal" | "gles";
  readonly repeat: number;
  readonly instrumented: boolean;
  readonly frames: number;
  readonly cases?: readonly string[] | null;
  readonly minimumSampleMs: number;
  readonly eglDirectory: string;
  readonly output: string;
}

const viewport = { width: 1600, height: 900, devicePixelRatio: 1 };
export const registry = "examples/demos/scene-registry.ts";
const settings = { skin: "glass", font: "shure-tech-mono", renderScale: 1 };
const json = (value: unknown) =>
  JSON.stringify(
    value,
    (_, item) => (typeof item === "bigint" ? String(item) : item),
    2,
  );

function percentiles(samples: readonly number[]) {
  const sorted = [...samples].sort((a, b) => a - b);
  return {
    median: sorted[Math.ceil(sorted.length * 0.5) - 1]!,
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1]!,
  };
}

export async function measureGallery(
  config: GalleryBenchmarkConfig,
  definitions: readonly GallerySceneDefinition[],
  context: ClientContext,
  diagnostics?: RenderDiagnostics,
  profiler?: HostProfiling,
) {
  const { repeat } = config;
  let device: RenderDeviceInfo | undefined;
  const available = [
    ...definitions.map(({ id }) => ({ name: id, scene: id })),
    { name: "charts-streaming", scene: "charts" },
  ];
  const requested = config.cases?.map((name) => name.trim());
  assert.ok(
    !requested ||
      requested.every((name) => available.some((item) => item.name === name)),
    `Unknown gallery workload in ${requested?.join(", ")}`,
  );
  const cases = available.filter(
    ({ name }) => !requested || requested.includes(name),
  );
  const failures: { name: string; repeat: number; error: string }[] = [];
  const runs: Record<string, unknown>[] = [];
  {
    const order = repeat % 2 === 0 ? cases : [...cases].reverse();
    for (const { name, scene } of order) {
      const key = `${name}-${repeat}`;
      const gallery = new NativeGallerySession(
        context,
        scene,
        registry,
        context.assetRoot,
        viewport,
        settings,
      );
      try {
        await gallery.open();
        const app = gallery.application;
        assert.equal(app.sceneId, scene);
        assert.ok(app.sceneClient && app.sceneOutput);
        if (name === "charts-streaming")
          await app.action("dataSource", "streaming");
        await app.flush();
        await gallery.present(async (view) => {
          let sequence = 0n;
          const frame = async () => {
            const result = await context.host.presentation.frame(view, {
              afterOutputs: app.frameOutputs,
              afterSequence: sequence,
            });
            assert.ok(result.sequence > sequence && result.drawCalls > 0);
            assert.equal(result.failedDrawCalls, 0);
            sequence = result.sequence;
            return result;
          };
          // A probe without publication fences can diagnose missing draws
          // before timing a workload with the exact required publications.
          const probe = async () => context.host.presentation.frame(view, {});
          device = (await diagnostics?.statistics())?.device;
          const deadline = performance.now() + 60_000;
          for (;;) {
            const { sceneWorlds, shell } = await app.inspectApplication();
            const snapshots = [shell];
            for (const world of sceneWorlds) {
              const client = await context.host.openWorld(world);
              try {
                snapshots.push(await client.inspect());
              } finally {
                await client.close();
              }
            }
            const resources = snapshots.flatMap(
              (snapshot) => snapshot.resources,
            );
            assert.ok(
              !resources.some((resource) => resource.status === "failed"),
              `${name} asset failure`,
            );
            if (
              !resources.some(
                (resource) =>
                  resource.status === "start" || resource.status === "progress",
              )
            )
              break;
            assert.ok(
              performance.now() < deadline,
              `${name} asset readiness timed out`,
            );
            for (let i = 0; i < 10; i++) await probe();
          }

          const initial = await probe();
          if (initial.failedDrawCalls !== 0) {
            const incomplete = await context.host.presentation.capture(view);
            const evidence = `${key}-incomplete.png`;
            await writeFile(
              join(config.output, evidence),
              encodePng(image(incomplete)),
            );
            throw new Error(
              `Incomplete rendering with ${initial.failedDrawCalls} failed draws; see ${evidence}`,
            );
          }

          const warm = performance.now();
          for (let i = 0; i < 30 || performance.now() - warm < 2500; i++)
            await frame();
          const before = await diagnostics?.statistics();
          const captureId = await profiler?.start({
            counters: false,
            gpu: "frame",
            trace: { maxEvents: 100_000 },
            maxArtifactBytes: 134_217_728,
          });
          const samples: {
            intervalMs: number;
            sequence: string;
            drawCalls: number;
            triangles: number;
          }[] = [];
          let elapsedMs: number;
          let completedFrames: number;
          let profile: string | undefined;
          try {
            await frame();
            const first = sequence;
            const start = performance.now();
            let previous = start;
            for (
              let i = 0;
              i < config.frames || previous - start < config.minimumSampleMs;
              i++
            ) {
              const completed = await frame();
              const now = performance.now();
              samples.push({
                intervalMs: now - previous,
                sequence: String(completed.sequence),
                drawCalls: completed.drawCalls,
                triangles: completed.triangles,
              });
              previous = now;
            }
            elapsedMs = previous - start;
            completedFrames = Number(sequence - first);
            if (profiler) {
              const data = await profiler.stop();
              profile = `${key}-profile.json`;
              await writeFile(join(config.output, profile), json(data));
            }
          } finally {
            if (captureId) await profiler!.release(captureId);
          }
          const after = await diagnostics?.statistics();
          const captured = await context.host.presentation.capture(view, {
            afterOutputs: app.frameOutputs,
            afterSequence: sequence,
          });
          const pixels = image(captured);
          assert.equal(captured.failedDrawCalls, 0);
          assert.equal(pixels.width, viewport.width);
          assert.equal(pixels.height, viewport.height);
          const capture = `${key}-live.png`;
          await writeFile(join(config.output, capture), encodePng(pixels));
          const timing = config.instrumented
            ? null
            : {
                elapsedMs,
                completedFrames,
                framesPerSecond: (completedFrames * 1000) / elapsedMs,
                requestIntervalMs: percentiles(
                  samples.map(({ intervalMs }) => intervalMs),
                ),
              };
          const raw = `${key}-samples.json`;
          await writeFile(
            join(config.output, raw),
            json({
              name,
              repeat,
              timing,
              samples: config.instrumented
                ? samples.map(({ sequence, drawCalls, triangles }) => ({
                    sequence,
                    drawCalls,
                    triangles,
                  }))
                : samples,
              before,
              after,
              profile,
              options: app.options,
              capture,
              captureSequence: String(captured.sequence),
            }),
          );
          runs.push({ name, repeat, timing, raw, profile, capture });
        });
      } catch (error) {
        failures.push({
          name,
          repeat,
          error:
            error instanceof Error
              ? (error.stack ?? error.message)
              : String(error),
        });
      } finally {
        await gallery.close();
      }
      await writeFile(
        join(config.output, "progress.json"),
        json({ device, viewport, runs, failures }),
      );
    }
  }
  return { device, viewport, runs, failures };
}
