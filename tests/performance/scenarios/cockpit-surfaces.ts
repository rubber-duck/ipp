/** The actual cockpit; only the four curved panes' SurfaceCache policy changes. */
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import type {
  AnimationWorldClient,
  Client,
  Command,
  WorldReference,
} from "@ipp/client";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { nativePresentationTransport } from "@ipp/client/testing";
import { hostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import { defineClient } from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import { image } from "../../../tools/shared-host/presentation.js";
import {
  crop,
  enlarge,
  sideBySide,
  type RgbaImage,
} from "../../../tools/shared-host/images.js";
import { insertComponent, successfulBatch } from "../../fixtures/commands.js";
import { nativeUsage } from "../support/native-usage.js";

const viewport = { width: 1600, height: 900, devicePixelRatio: 1 };
const json = (value: unknown) =>
  JSON.stringify(value, (_, v) => (typeof v === "bigint" ? String(v) : v), 2);
const summary = (samples: readonly number[]) => {
  const sorted = [...samples].sort((a, b) => a - b);
  return {
    count: sorted.length,
    meanMs: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    p50Ms: sorted[Math.ceil(sorted.length * 0.5) - 1],
    p95Ms: sorted[Math.ceil(sorted.length * 0.95) - 1],
    minMs: sorted[0],
    maxMs: sorted.at(-1),
  };
};

export default defineClient({
  async open(context, args) {
    const config = JSON.parse(args[0]!);
    const diagnosticsHost = await (
      context.contract
        .IppHostClient as typeof import("@ipp/host-contract").IppHostClient
    ).connectTransport(
      nativePresentationTransport(config.url, config.presentationUrl),
    );
    const gallery = new NativeGallerySession(
      context,
      "cockpit",
      "examples/demos/scene-registry.ts",
      context.assetRoot,
      viewport,
      {},
    );
    try {
      await gallery.open();
      const controller = gallery.application.controller as {
        panels: readonly { index: number; world: WorldReference }[];
      };
      const panels: Client[] = [];
      for (const panel of controller.panels)
        panels.push(await context.host.openWorld(panel.world));
      return { gallery, diagnosticsHost, config, panels };
    } catch (error) {
      await gallery.close();
      await diagnosticsHost.close();
      throw error;
    }
  },
  async capture(state, context) {
    const { gallery, diagnosticsHost, config } = state;
    const app = gallery.application;
    const client = app.sceneClient!;
    const diagnostics = renderDiagnostics(diagnosticsHost)!;
    const images: Record<string, RgbaImage> = {};
    const runs: unknown[] = [];
    let failure: string | null = null;
    let device: unknown;
    let quality: unknown;
    const scene = await client.inspect();
    const component = client.components.SurfaceCache!.id;
    const policies = Array.from({ length: 4 }, (_, i) => {
      const entity = scene.entities.find(
        (entry) => entry.metadata.symbolicId === `cockpit-screen-${i}`,
      )!;
      assert.ok(entity, `Cockpit pane ${i} exists`);
      const fields = entity.components.find(
        (value) => value.component === component,
      )!.fields;
      assert.ok(
        Number(fields.direct_distance) > 12,
        "Texture reference refreshes current content, without optional throttling",
      );
      return {
        entity: { kind: "handle" as const, id: entity.id },
        fields: fields as Record<string, number | boolean>,
      };
    });
    let textured = true;
    async function mode(next: "analytic" | "texture", scale?: number) {
      const commands: Command[] = [];
      for (const policy of policies) {
        if (textured)
          commands.push({
            kind: "removeComponent",
            entity: policy.entity,
            component,
          });
        if (next === "texture")
          commands.push(
            insertComponent(
              client,
              "SurfaceCache",
              policy.entity,
              scale
                ? {
                    ...policy.fields,
                    resolution_scale: scale,
                    max_texture_size: 4096,
                  }
                : policy.fields,
            ),
          );
      }
      if (commands.length) successfulBatch(await client.batch(commands));
      textured = next === "texture";
    }
    try {
      await gallery.present(async (view) => {
        let sequence = 0n;
        const frame = async () => {
          const result = await context.host.presentation.frame(view, {
            afterOutputs: app.frameOutputs,
            afterSequence: sequence,
          });
          assert.ok(result.sequence > sequence);
          assert.equal(
            result.failedDrawCalls,
            0,
            "Cockpit frame has no failed draws",
          );
          assert.ok(result.drawCalls > 0);
          sequence = result.sequence;
          return result;
        };
        const warm = async (count = 60, minimumMs = 1000) => {
          const started = performance.now();
          for (
            let i = 0;
            i < count || performance.now() - started < minimumMs;
            i++
          )
            await frame();
        };
        await warm();
        const worlds = [client, ...state.panels];
        const deadline = performance.now() + 60000;
        for (;;) {
          const snapshots = await Promise.all(
            worlds.map((world) => world.inspect()),
          );
          assert.ok(
            snapshots.every(
              (snapshot) =>
                !snapshot.resources.some((r) => r.status === "failed"),
            ),
            "Cockpit assets load successfully",
          );
          if (
            snapshots.every(
              (snapshot) =>
                !snapshot.resources.some(
                  (r) => r.status === "start" || r.status === "progress",
                ),
            )
          )
            break;
          assert.ok(
            performance.now() < deadline,
            "Cockpit resource readiness timed out",
          );
          await warm(5);
        }
        await warm();
        const initial = await diagnostics.statistics();
        device = initial.device;
        const renderer = String(
          initial.device.renderer ?? initial.device.unmaskedRenderer ?? "",
        );
        assert.ok(renderer, "Renderer identity is available");
        assert.ok(
          config.allowSoftware ||
            !/llvmpipe|softpipe|swiftshader|lavapipe/i.test(renderer),
          `Hardware timing required: ${renderer}`,
        );
        for (let repeat = 0; repeat < config.repetitions; repeat++) {
          const order =
            repeat % 2 === 0
              ? (["analytic", "texture"] as const)
              : (["texture", "analytic"] as const);
          for (const selected of order) {
            await mode(selected);
            await warm();
            const before = await diagnostics.statistics();
            const profiler = config.instrumented
              ? hostProfiling(diagnosticsHost)
              : undefined;
            let captureId: string | undefined;
            const samples = [];
            const usageBefore = await nativeUsage(config.hostPid);
            let firstSequence = sequence;
            let elapsedMs = 0;
            let usageAfter: Awaited<ReturnType<typeof nativeUsage>> | undefined;
            let completedFrames = 0;
            try {
              if (profiler)
                captureId = await profiler.start({
                  counters: false,
                  gpu: "frame",
                  glCalls: false,
                  maxArtifactBytes: 134217728,
                });
              // Anchor throughput to actual completed frames, excluding setup
              // and including any intervening draws between client requests.
              await frame();
              firstSequence = sequence;
              const windowStart = performance.now();
              for (
                let i = 0;
                i < config.frames ||
                performance.now() - windowStart < config.minimumSampleMs;
                i++
              ) {
                const start = performance.now();
                const completed = await frame();
                samples.push({
                  completedMs: performance.now() - start,
                  sequence: String(completed.sequence),
                  drawCalls: completed.drawCalls,
                  triangles: completed.triangles,
                });
              }
              elapsedMs = performance.now() - windowStart;
              completedFrames = Number(sequence - firstSequence);
              usageAfter = await nativeUsage(config.hostPid);
              if (profiler && captureId) {
                // Stop before pixel readback: the Host keeps drawing while a
                // capture transfers, which would contaminate and overflow the
                // query window. Queries still pending at stop remain explicit.
                const capture = await profiler.stop();
                await writeFile(
                  join(config.output, `${selected}-${repeat}-profile.json`),
                  json(capture),
                );
              }
            } finally {
              if (profiler && captureId) await profiler.release(captureId);
            }
            const after = await diagnostics.statistics();
            const capture = await context.host.presentation.capture(view, {
              afterOutputs: app.frameOutputs,
              afterSequence: sequence,
            });
            assert.equal(capture.failedDrawCalls, 0);
            images[`${selected}-${repeat}-live`] = image(capture);
            sequence = capture.sequence;
            runs.push({
              mode: selected,
              repeat,
              usageBefore,
              usageAfter,
              completedFrames,
              elapsedMs,
              completedFramesPerSecond: (completedFrames * 1000) / elapsedMs,
              timing: config.instrumented
                ? null
                : summary(samples.map((s) => s.completedMs)),
              samples: config.instrumented ? [] : samples,
              before,
              after,
            });
            await writeFile(
              join(config.output, "progress.json"),
              json({ device, runs }),
            );
          }
        }
        // Freeze authored animation controllers for the image comparison. Host
        // clocks still run normally; no simulation step or time override is used.
        for (const world of worlds) {
          const snapshot = await world.inspect();
          for (const controller of snapshot.controllers ?? []) {
            await (world as AnimationWorldClient).controlAnimationController(
              controller.id,
              { action: "pause" },
            );
            await (world as AnimationWorldClient).controlAnimationController(
              controller.id,
              { action: "seek", time: 0 },
            );
          }
        }
        for (const selected of [
          "analytic",
          "texture",
          "texture-reference",
        ] as const) {
          await mode(
            selected === "analytic" ? "analytic" : "texture",
            selected === "texture-reference" ? 4 : undefined,
          );
          await warm(10);
          const capture = await context.host.presentation.capture(view, {
            afterOutputs: app.frameOutputs,
            afterSequence: sequence,
          });
          assert.equal(capture.failedDrawCalls, 0);
          sequence = capture.sequence;
          images[`${selected}-still`] = image(capture);
        }
        const names = ["texture", "analytic", "texture-reference"];
        const crops = names.map((name) =>
          crop(images[`${name}-still`]!, [523, 258, 290, 170]),
        );
        images["controls-comparison-texture-analytic-reference"] = sideBySide(
          crops.map((value) => enlarge(value, 2)),
        );
        const reference = images["texture-reference-still"]!;
        quality = {
          reference:
            "4x texture resolution scale; comparison reference, not exact ground truth",
          region: [520, 250, 550, 350],
          comparisons: names.slice(0, 2).map((name) => {
            const actual = images[`${name}-still`]!;
            let absolute = 0,
              squared = 0,
              count = 0;
            for (let y = 250; y < 600; y++)
              for (let x = 520; x < 1070; x++)
                for (let channel = 0; channel < 3; channel++) {
                  const offset = (y * reference.width + x) * 4 + channel;
                  const delta =
                    actual.pixels[offset]! - reference.pixels[offset]!;
                  absolute += Math.abs(delta);
                  squared += delta * delta;
                  count++;
                }
            return {
              mode: name,
              meanAbsoluteRgbError: absolute / count,
              rmsRgbError: Math.sqrt(squared / count),
            };
          }),
        };
      });
    } catch (error) {
      failure =
        error instanceof Error ? (error.stack ?? error.message) : String(error);
    } finally {
      await mode("texture");
    }
    return {
      images,
      report: {
        failure,
        viewport,
        device,
        quality,
        runs,
        options: app.options,
        policies,
        scope:
          "Uncapped ordinary release completed-draw throughput and request latency, including transport and Host scheduling. Frame responses acknowledge draw submission, not GPU retirement; separate GPU queries and engine counters observe device work. Same animated cockpit and unthrottled texture policy. Frozen stills pause authored animation; the live FPS monitor remains dynamic.",
      },
    };
  },
  async close(state) {
    await Promise.all(state.panels.map((panel) => panel.close()));
    await state.gallery.close();
    await state.diagnosticsHost.close();
  },
});
