import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { encodePng } from "../harness/images.js";
import { runNativeEnvironment } from "../harness/native.js";
import {
  metalComposition,
  assertCompositionPixels,
} from "./scenarios/metal-composition.js";
import { assertCanvasRetention } from "./scenarios/metal-canvas-retention.js";
import { assertMetalCamera, metalCamera } from "./scenarios/metal-camera.js";

test("native WebGPU presents and resizes a Camera on the hardware Metal adapter", {
  timeout: 120_000,
}, async ({ signal }) => {
  const product = resolve("target/webgpu-host");
  const contract = (await import(
    pathToFileURL(join(product, "generated.js")).href
  )) as typeof import("@ipp/host-contract");
  await runNativeEnvironment(
    "webgpu-camera",
    {
      executable: join(product, "ipp-webgpu-host"),
      schemaArtifact: join(product, "contract.bin"),
      workingDirectory: process.cwd(),
      environment: { IPP_LOG: "info" },
    },
    signal,
    async (env) => {
      const client = await contract.IppHostClient.connectWebSocket(env.url);
      try {
        const { captures } = await metalCamera(
          client as unknown as Parameters<typeof metalCamera>[0],
        );
        for (const [i, capture] of captures.entries()) {
          const { width, height } = capture.view.binding.viewport;
          await writeFile(
            join(env.evidence.directory, `camera-${i}.png`),
            encodePng({
              width,
              height,
              pixels: new Uint8Array(capture.pixels),
            }),
          );
          assertMetalCamera(capture, 5);
        }
        const frames = await metalComposition(
          client as unknown as Parameters<typeof metalComposition>[0],
        );
        for (const [i, frame] of frames.entries()) {
          await writeFile(
            join(env.evidence.directory, `composition-${i}.png`),
            encodePng(frame),
          );
          assertCompositionPixels(frame, i, env.evidence.directory);
        }
        const font = new Uint8Array(
          await readFile("target/font-assets/shure-tech-mono.ippf"),
        ).buffer;
        await assertCanvasRetention(
          client as unknown as Parameters<typeof assertCanvasRetention>[0],
          font,
          async (stage, pixels) => {
            await writeFile(
              join(env.evidence.directory, `retention-${stage}.png`),
              encodePng({ width: 640, height: 400, pixels }),
            );
          },
        );
        assert.match(
          await readFile(
            join(env.evidence.directory, "server-stderr.log"),
            "utf8",
          ),
          /\[IPP WebGPU\] device=Metal .+/,
        );
      } finally {
        await client.close().catch(async (error: unknown) => {
          if (
            !(error instanceof Error) ||
            error.message !== "WebSocket close timed out"
          )
            throw error;
          await env.evidence.record("client_close_timeout", {
            backend: "webgpu",
          });
        });
      }
    },
  );
});
