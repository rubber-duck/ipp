import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import type { Client } from "@ipp/client";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../fixtures/commands.js";
import {
  GUI,
  LIFECYCLE,
  selectSystems,
} from "../fixtures/system-selections.js";
import { encodePng } from "../harness/images.js";
import { runNativeEnvironment } from "../harness/native.js";

test("CEF IOSurface paint and Metal device recovery through the native Host", {
  timeout: 120_000,
}, async ({ signal }) => {
  const product = resolve("target/metal-host-instrumentation");
  const contract = (await import(
    pathToFileURL(join(product, "generated.js")).href
  )) as typeof import("@ipp/host-contract");
  await runNativeEnvironment(
    "metal-cef",
    {
      executable: join(product, "ipp-metal-host"),
      schemaArtifact: join(product, "contract.bin"),
      workingDirectory: process.cwd(),
      extraArguments: [
        "--cef",
        resolve("target/cef-engine"),
        resolve("tests/rendering/fixtures/native-cef"),
        "--cef-page",
        "cef:metal-fixture",
        "page.html",
        "--simulate-device-recovery-after-frame",
        "180",
      ],
      environment: { IPP_LOG: "info" },
      readinessTimeoutMs: 40_000,
      operationTimeoutMs: 35_000,
    },
    signal,
    async (environment) => {
      const host = await contract.IppHostClient.connectWebSocket(
        environment.url,
      );
      try {
        const world = await host.createWorld({
          selectedSystems: selectSystems(GUI, LIFECYCLE),
          canvas: { extent: [192, 128], unitsPerMetre: 1 },
        });
        const client = await host.openWorld(world.reference);
        const author = client as unknown as Client;
        const background = { kind: "alias", alias: 1 } as const;
        const html = { kind: "alias", alias: 2 } as const;
        successfulBatch(
          await client.batch([
            createEntity(1, "background"),
            insertComponent(author, "CanvasStyle", background, {
              red: 0,
              green: 0,
              blue: 1,
            }),
            insertComponent(author, "CanvasBox", background, {
              width: 192,
              height: 128,
            }),
            createEntity(2, "html"),
            insertComponent(author, "CanvasHtml", html, {
              source: "cef:metal-fixture",
              width: 192,
              height: 128,
            }),
          ]),
        );
        const output = { kind: "canvas", world: world.reference } as const;
        const binding = await host.setRootOutput(output, {
          width: 192,
          height: 128,
          devicePixelRatio: 1,
        });
        const surface = await host.presentation.surface();
        let view = await host.presentation.select(surface, binding);
        const captures = new Set<string>();
        const deadline = performance.now() + 25_000;
        while (captures.size < 2 && performance.now() < deadline) {
          await new Promise((done) => setTimeout(done, 30));
          const current = await host.presentation.surface();
          if (current.context !== view.surface.context)
            view = await host.presentation.select(current, binding);
          const captured = await host.presentation
            .capture(view)
            .catch((error: unknown) => {
              if (
                error instanceof Error &&
                "reason" in error &&
                error.reason === "staleView"
              )
                return;
              throw error;
            });
          if (!captured) continue;
          const pixels = new Uint8Array(captured.pixels);
          const pixel = (x: number, y: number) =>
            pixels.subarray((y * 192 + x) * 4, (y * 192 + x + 1) * 4);
          const near = (x: number, y: number, expected: readonly number[]) =>
            expected.every(
              (value, lane) => Math.abs(pixel(x, y)[lane]! - value) <= 5,
            );
          if (
            !near(4, 4, [0, 255, 0, 255]) ||
            !near(32, 24, [188, 0, 188, 255]) ||
            !near(100, 110, [255, 128, 0, 255])
          )
            continue;
          assert.equal(captured.failedDrawCalls, 0);
          const key = String(current.context);
          if (!captures.has(key)) {
            captures.add(key);
            await writeFile(
              join(environment.evidence.directory, `cef-context-${key}.png`),
              encodePng({ width: 192, height: 128, pixels }),
            );
          }
        }
        assert.deepEqual(
          [...captures],
          [String(surface.context), String(surface.context + 1n)],
        );
        await host.presentation.clear(view);
        await host.destroyWorld(world.reference);
      } catch (error) {
        await host.close().catch(() => {});
        throw error;
      }
      await host.close();
    },
  );
});
