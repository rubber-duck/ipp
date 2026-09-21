import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { verifyHardwareRenderer } from "#ipp-browser-options";
import { runBrowserEnvironment } from "../browser/environment.js";
import { startBlender } from "./blender-environment.js";
import { invoke, recordCapture } from "./evidence.js";
import type * as Fixture from "./blender-fixture.js";

test("Blender streamed imports overlap extraction and preserve complete images and recovery", {
  timeout: 1_800_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render-expanded");
  const build = {
    name: "render-expanded" as const,
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    exportWasm: resolve(profile, "export.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  await runBrowserEnvironment(
    "blender stream",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 900_000,
      closeTimeoutMs: 10_000,
      evidenceParent: resolve("target/integration-artifacts/blender-stream"),
    },
    context.signal,
    async (environment) => {
      const { page } = environment;
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: environment.urls.origin,
      });
      const blend = process.env.IPP_BLENDER_STREAM_BLEND;
      const blender = await startBlender(
        environment,
        blend
          ? { blend, startupTimeoutMs: 900_000 }
          : { fixture: "tests/blender/stream_fixture.py" },
      );
      const module = `${environment.urls.origin}/target/blender-test/blender-fixture.js`;
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(page, module, name, args);
      const open = async (size: number) => {
        // Navigation releases the preceding World and socket before the next export.
        await page.goto("about:blank");
        const fragment = new URLSearchParams({
          endpoint: blender.ready.origin,
          token: blender.ready.token,
          stream: String(size),
          refresh: "1",
        });
        await page.goto(
          `${environment.urls.origin}/target/blender-viewer/index.html#${fragment}`,
        );
      };
      const runs = [];
      let expectedImage: string | undefined;
      let expectedCount = 0;
      let sawStreamedImport = false;
      const sizes = (process.env.IPP_BLENDER_STREAM_SIZES ?? "0,100,100,0")
        .split(",")
        .map(Number);
      for (const [index, size] of sizes.entries()) {
        await open(size);
        const label = `import-${index}-${size}`;
        const state = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, 0, 900_000, !!blend],
        ).catch(async (error) => {
          await environment.evidence.writeJson(
            "failed-import-profile.json",
            await page.evaluate(() => window.ippBlender?.adapter.importProfile),
          );
          throw error;
        });
        const timing =
          await call<ReturnType<typeof Fixture.importMeasurements>>(
            "importMeasurements",
          );
        timing.ready = state.readyMilliseconds;
        assert.ok(timing.maxBatchCommands <= 256);
        if (!blend) {
          assert.ok(timing.commandBatches > 1);
          if (!size) assert.equal(timing.maxBatchCommands, 256);
        }
        if (process.env.IPP_BROWSER_ANGLE)
          verifyHardwareRenderer(state.backend.unmaskedRenderer);
        assert.equal(state.inspection.renderDiagnostics.length, 0);
        assert.ok(state.summary.foregroundPixels > 1000);
        assert.ok(state.inspection.entities.length > 300);
        assert.ok(
          state.inspection.resources.every(
            (resource) => resource.status === "loaded",
          ),
        );
        const image = await call<string>("captureDataUrl", [label]);
        expectedImage ??= image;
        expectedCount ||= state.inspection.entities.length;
        assert.equal(
          image,
          expectedImage,
          "Full and streamed imports must render identical pixels",
        );
        assert.equal(state.inspection.entities.length, expectedCount);
        if (size) {
          assert.ok(timing.chunks > 1);
          assert.ok(
            timing.firstChunk > 0 && timing.firstChunk < timing.finalSnapshot,
          );
          const stream = timing.stream as {
            firstAckSeconds: number;
            entities: number;
            extractionSeconds: number;
            assetsPublishedAfterFirstAck: number;
            pendingAssetReads: number;
          };
          assert.ok(
            stream.firstAckSeconds < stream.extractionSeconds &&
              stream.assetsPublishedAfterFirstAck > 0,
            "IPP must acknowledge a chunk while Blender is still exporting",
          );
          if (!sawStreamedImport) {
            assert.ok(
              stream.pendingAssetReads > 0,
              "the cold streamed import must observe pending sources",
            );
          }
          sawStreamedImport = true;
          assert.ok(timing.entityBatches >= 1);
          assert.equal(timing.logicalBatches, timing.entityBatches + 1);
          assert.ok(timing.firstEntityBatch > timing.firstChunk);
          assert.equal(stream.entities, expectedCount - 1); // Viewer-owned camera.
        }
        runs.push({
          size,
          ...timing,
          entities: expectedCount,
          renderer: state.backend.unmaskedRenderer,
        });
        await recordCapture(
          page,
          module,
          environment.evidence.directory,
          new Set(),
          label,
          { canvasSelector: "canvas" },
        );
        await environment.evidence.writeJson("timings.json", runs);
        console.log(JSON.stringify(runs.at(-1)));
      }
      if (!blend) {
        // Reject a later chunk after earlier buffers have really applied. The
        // same adapter must retain its acknowledged handles for a full correction.
        let entityChunks = 0;
        await page.routeWebSocket(/\/v1\/updates/, (socket) => {
          const server = socket.connectToServer();
          server.onMessage((data) => {
            const message = JSON.parse(String(data));
            if (
              message.type === "chunk" &&
              message.entities &&
              ++entityChunks === 2
            )
              message.entities[0].parent = "missing-stream-parent";
            socket.send(JSON.stringify(message));
          });
        });
        await open(100);
        await page.waitForFunction(() =>
          window.ippBlender?.error?.includes("aborted"),
        );
        const partial = await page.evaluate(async () => {
          const current = window.ippBlender!;
          assertNeverCompleted(current.latest);
          return (await current.canvas.client.inspect()).entities.map(
            (entity) => [entity.metadata.symbolicId, entity.id] as const,
          );
          function assertNeverCompleted(latest: unknown) {
            if (latest)
              throw new Error("Failed stream advanced the completed revision");
          }
        });
        assert.ok(partial.length >= 100);
        await page.evaluate(async () => {
          const params = new URLSearchParams(location.hash.slice(1));
          const url = new URL("/v1/sync", params.get("endpoint")!);
          url.searchParams.set("token", params.get("token")!);
          const response = await fetch(url, { method: "POST" });
          if (!response.ok) throw new Error(await response.text());
        });
        await page.waitForFunction(() => !!window.ippBlender?.latest);
        const recovered = await call<
          Awaited<ReturnType<typeof Fixture.capture>>
        >("capture", ["recovered"]);
        const handles = new Map(
          recovered.inspection.entities.map((entity) => [
            entity.metadata.symbolicId,
            entity.id,
          ]),
        );
        for (const [name, id] of partial) assert.equal(handles.get(name), id);
        assert.equal(
          await call<string>("captureDataUrl", ["recovered"]),
          expectedImage,
        );
        await recordCapture(
          page,
          module,
          environment.evidence.directory,
          new Set(),
          "recovered",
          { canvasSelector: "canvas" },
        );
        const encoding = await call<
          Awaited<ReturnType<typeof Fixture.correctCommandEncodingFailure>>
        >("correctCommandEncodingFailure");
        assert.match(encoding.error, /Invalid numeric field/);
        assert.ok(encoding.acknowledged >= 256);
        assert.equal(encoding.preserved, true);
        assert.equal(encoding.entities, expectedCount);
        const boundary = await call<
          Awaited<ReturnType<typeof Fixture.captureCommandBatchBoundary>>
        >("captureCommandBatchBoundary");
        assert.equal(boundary.firstTick, boundary.secondTick);
        assert.equal(boundary.firstTick, boundary.heldTick);
        assert.ok(boundary.completeTick > boundary.heldTick);
        assert.equal(boundary.heldDifference.changedPixels, 0);
        assert.ok(boundary.completedDifference.changedPixels > 100);
        assert.equal(boundary.restoredDifference.changedPixels, 0);
        await environment.evidence.writeJson(
          "command-batch-boundary.json",
          boundary,
        );
        for (const label of [
          "batch-before",
          "batch-held",
          "batch-complete",
          "batch-restored",
        ])
          await recordCapture(
            page,
            module,
            environment.evidence.directory,
            new Set(),
            label,
            { canvasSelector: "canvas" },
          );
      }
    },
  );
});
