import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { hostProfiling } from "../../packages/ipp-client/src/profiling.js";
import {
  differenceImage,
  encodePng,
  pixelDifference,
  type RgbaFrame,
} from "../harness/images.js";
import { runNativeEnvironment } from "../harness/native.js";
import { assertMetalCamera, metalCamera } from "./scenarios/metal-camera.js";
import { assertCanvasRetention } from "./scenarios/metal-canvas-retention.js";

import {
  assertCompositionPixels,
  metalComposition,
} from "./scenarios/metal-composition.js";

test("camera and nested canvas presentation through native Metal and GLES", {
  timeout: 120_000,
}, async ({ signal }) => {
  const profiles = [
    { directory: "metal-host", executable: "ipp-metal-host", args: [] },
    {
      directory: "metal-host-instrumentation",
      executable: "ipp-metal-host",
      args: [],
    },
    {
      directory: "gles-host",
      executable: "gles_host",
      args: ["--egl-dir", process.env.IPP_EGL_LIBRARY_DIR!],
    },
  ];
  const results: {
    frames: RgbaFrame[];
    composition: RgbaFrame[];
    evidence: string;
  }[] = [];
  const fontBytes = new Uint8Array(
    await readFile("target/font-assets/shure-tech-mono.ippf"),
  ).buffer;
  for (const { directory, executable, args } of profiles) {
    const product = resolve("target", directory);
    const schemaArtifact = join(product, "contract.bin");
    const contract = (await import(
      pathToFileURL(join(product, "generated.js")).href
    )) as typeof import("@ipp/host-contract");
    const frames = await runNativeEnvironment(
      directory,
      {
        executable: join(product, executable),
        schemaArtifact,
        workingDirectory: process.cwd(),
        extraArguments: args,
        environment: { IPP_LOG: "info" },
        operationTimeoutMs: 20_000,
      },
      signal,
      async (environment) => {
        const host = await contract.IppHostClient.connectWebSocket(
          environment.url,
        );
        let failure: unknown;
        try {
          // Each generated module carries its own copies of private SDK classes.
          const profiler =
            directory === "metal-host-instrumentation"
              ? hostProfiling(host)
              : undefined;
          const { captures, profile } = await metalCamera(
            host as unknown as Parameters<typeof metalCamera>[0],
            profiler,
          );
          const frames = captures.map((capture) => {
            const { width, height } = capture.view.binding.viewport;
            return { width, height, pixels: new Uint8Array(capture.pixels) };
          });
          for (const [i, frame] of frames.entries())
            await writeFile(
              join(environment.evidence.directory, `camera-${i}.png`),
              encodePng(frame),
            );
          for (const capture of captures)
            assertMetalCamera(capture, executable === "gles_host" ? 7 : 5);
          if (profiler) {
            assert.ok(profile);
            await writeFile(
              join(environment.evidence.directory, "gpu-profile.json"),
              JSON.stringify(profile, null, 2),
            );
            assert.ok(
              profile.trace?.events.some(
                (event) => event.name === "MetalRenderService.draw",
              ),
            );
            assert.ok(
              profile.gpu?.records.some(
                (record) =>
                  record.scope === "frame" &&
                  record.availability.status === "available" &&
                  Number(record.availability.duration) > 0,
              ),
            );
          }
          const composition = await metalComposition(
            host as unknown as Parameters<typeof metalComposition>[0],
          );
          await environment.execute("Canvas retention", {}, async () => {
            const retention = await assertCanvasRetention(
              host as unknown as Parameters<typeof assertCanvasRetention>[0],
              fontBytes,
              async (stage, pixels) => {
                await writeFile(
                  join(
                    environment.evidence.directory,
                    `retention-${stage}.png`,
                  ),
                  encodePng({ width: 640, height: 400, pixels }),
                );
                await environment.evidence.record("canvas_retention", {
                  stage,
                });
              },
              profiler,
            );
            if (!retention) return;
            await writeFile(
              join(environment.evidence.directory, "retention-profile.json"),
              JSON.stringify(retention, null, 2),
            );
            for (const stage of [
              "MetalCanvasDamage.repaint",
              "MetalCanvasBatches.replay",
            ]) {
              assert.ok(
                retention.trace?.events.some((event) => event.name === stage),
                `expected ${stage} in the Canvas retention profile`,
              );
            }
          });
          for (const [i, frame] of composition.entries())
            await writeFile(
              join(environment.evidence.directory, `composition-${i}.png`),
              encodePng(frame),
            );
          return {
            frames,
            composition,
            evidence: environment.evidence.directory,
          };
        } catch (error) {
          failure = error;
          throw error;
        } finally {
          await host.close().catch(async (error: unknown) => {
            if (failure !== undefined) return;
            if (
              !(error instanceof Error) ||
              error.message !== "WebSocket close timed out"
            )
              throw error;
            // The harness owns this process and reaps it even if its close
            // handshake times out after all presentation assertions finish.
            await environment.evidence.record("client_close_timeout", {
              backend: directory,
              message: error.message,
            });
          });
        }
      },
    );
    results.push(frames.value);
  }

  const [metal, instrumented, gles] = results;
  assert.ok(metal && instrumented && gles);
  assert.match(
    await readFile(join(metal.evidence, "server-stderr.log"), "utf8"),
    /\[IPP Metal\] device=\S+/,
  );
  for (const [i, reference] of gles.composition.entries()) {
    const actual: RgbaFrame = metal.composition[i]!;
    assert.deepEqual(actual.pixels, instrumented.composition[i]!.pixels);
    await writeFile(
      join(metal.evidence, `composition-difference-${i}.png`),
      encodePng(differenceImage(reference, actual, 2)),
    );
    assertCompositionPixels(actual, i, metal.evidence);
    const probes =
      i === 2
        ? ([
            [58, 48],
            [64, 38],
            [72, 48],
          ] as const)
        : ([
            [12, 30],
            [33, 48],
            [73, 30],
            [78, 40],
            [14, 15],
            [66, 83],
            [106, 78],
          ] as const);
    for (const [x, y] of probes)
      assert.ok(pixelDifference(reference, actual, y * actual.width + x) <= 2);
  }
  for (const [i, reference] of gles.frames.entries()) {
    const actual: RgbaFrame = metal.frames[i]!;
    assert.deepEqual(actual.pixels, instrumented.frames[i]!.pixels);
    await writeFile(
      join(metal.evidence, `difference-${i}.png`),
      encodePng(differenceImage(reference, actual, 2)),
    );
    let peak = 0;
    for (let pixel = 0; pixel < reference.width * reference.height; pixel++)
      peak = Math.max(peak, pixelDifference(reference, actual, pixel));
    assert.ok(
      peak <= 2,
      `Metal/GLES peak channel delta ${peak}, see ${metal.evidence}`,
    );
  }
});
