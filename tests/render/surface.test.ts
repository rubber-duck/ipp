import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";

test("Surface terminal renders crisp small text, drawings and RGBA through a generated worker client", {
  timeout: 120000,
}, async (context) => {
  const workspace = process.cwd(),
    directory = resolve(workspace, "target/browser-build/render-surfaces");
  const build = {
    name: "render-surfaces" as const,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "surfaces",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 30000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/surfaces/browser",
      ),
    },
    context.signal,
    async (env) => {
      const module = `${env.urls.origin}/target/surface-build/fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        env.execute(name, args, () => invoke<T>(env.page, module, name, args));
      const capture = async (label: string) => {
        const result = await call<{
          textPixels: number;
          drawCalls: number;
          triangles: number;
          background: number[];
          bitmap: number[];
        }>("capture", [label]);
        await writeDataUrl(
          resolve(env.evidence.directory, `${label}.png`),
          await invoke<string>(env.page, module, "captureDataUrl", [label]),
        );
        return result;
      };
      try {
        const initial = await call<{ nextId: number; ids: number[] }>(
          "initialize",
          [
            {
              generatedModuleUrl: env.urls.generated,
              workerScriptUrl: env.urls.workerScript,
              wasmUrl: env.urls.wasm,
            },
          ],
        );
        assert.equal(initial.nextId, 6);
        const front = await capture("front-small");
        assert.ok(front.textPixels > 150, `text coverage ${front.textPixels}`);
        assert.ok(
          front.drawCalls > 0 && front.drawCalls <= 10,
          `glyph run must be batched: ${front.drawCalls} draws`,
        );
        assert.ok(
          front.triangles > front.drawCalls * 2,
          `instanced Surface quads must contribute to frame triangles: ${front.triangles}`,
        );
        assert.ok(
          [33, 44, 63].every(
            (expected, index) =>
              Math.abs(front.background[index]! - expected) < 4,
          ),
          `linear panel colour: ${front.background}`,
        );
        assert.ok(
          [189, 121, 51].every(
            (expected, index) => Math.abs(front.bitmap[index]! - expected) < 5,
          ),
          `RGBA bitmap linear alpha blend: ${front.bitmap}`,
        );
        const oracle = await call<{ similarity: number; union: number }>(
          "referenceText",
          ["front-small"],
        );
        await writeDataUrl(
          resolve(env.evidence.directory, "reference-font.png"),
          await invoke<string>(env.page, module, "referenceDataUrl", [
            "front-small",
          ]),
        );
        assert.ok(
          oracle.union > 150 && oracle.similarity > 0.55,
          `independent browser font rasterizer overlap ${JSON.stringify(oracle)}`,
        );
        assert.deepEqual(await call("changeView", [Math.PI]), initial.ids);
        const rear = await capture("rear-small");
        assert.ok(
          rear.textPixels > 150,
          `rear text coverage ${rear.textPixels}`,
        );
        const mirror = await call<{
          contentPixels: number;
          mismatchedPixels: number;
          meanError: number;
        }>("mirrorComparison", ["front-small", "rear-small"]);
        assert.ok(
          mirror.contentPixels > 10_000,
          `rear mirror has insufficient content: ${JSON.stringify(mirror)}`,
        );
        assert.ok(
          mirror.mismatchedPixels < 300 && mirror.meanError < 0.5,
          `rear Surface must mirror the live front image: ${JSON.stringify(mirror)}`,
        );
        assert.deepEqual(await call("changeView", [0]), initial.ids);
        await capture("rear-view-restored");
        assert.equal(
          await call("equal", ["front-small", "rear-view-restored"]),
          true,
        );
        assert.deepEqual(await call("perspective", [0.65]), initial.ids);
        assert.ok((await capture("perspective")).textPixels > 70);
        await call("orthographic");
        await call("changeView", [0, 640, 480]);
        assert.ok(
          (await capture("high-resolution")).textPixels > front.textPixels * 2,
        );
        await call("changeView", [0]);
        await capture("restored-view");
        assert.equal(
          await call("equal", ["front-small", "restored-view"]),
          true,
        );
        await call("recover");
        await capture("context-restored");
        assert.equal(
          await call("equal", ["front-small", "context-restored"]),
          true,
        );
        const contextAssets = await call<{
          beforeGeneration: number;
          afterGeneration: number;
          whileLost: Array<{
            status: string;
            representation: { decoded: boolean; graphicsReady: boolean | null };
          }>;
          restored: Array<{
            status: string;
            representation: { decoded: boolean; graphicsReady: boolean | null };
          }>;
        }>("loadPendingSurfaceAssetsAcrossContextLoss");
        assert.equal(contextAssets.whileLost.length, 2);
        assert.ok(
          contextAssets.whileLost.every(
            (resource) =>
              resource.status !== "loaded" &&
              !resource.representation.decoded &&
              resource.representation.graphicsReady === null,
          ),
          JSON.stringify(contextAssets.whileLost, (_, value) =>
            typeof value === "bigint" ? String(value) : value,
          ),
        );
        assert.ok(
          contextAssets.afterGeneration > contextAssets.beforeGeneration,
        );
        assert.equal(contextAssets.restored.length, 2);
        assert.ok(
          contextAssets.restored.every(
            (resource) =>
              resource.status === "loaded" &&
              resource.representation.decoded &&
              resource.representation.graphicsReady === true,
          ),
          JSON.stringify(contextAssets.restored, (_, value) =>
            typeof value === "bigint" ? String(value) : value,
          ),
        );
        await capture("pending-assets-context-restored");
        assert.equal(
          await call("equal", [
            "front-small",
            "pending-assets-context-restored",
          ]),
          true,
        );
        const keyed = await call<{
          reordered: number[];
          removedIds: number[];
          removedProperty: unknown;
          restoredIds: number[];
          nextId: number;
        }>("keyedLifecycle");
        assert.deepEqual(keyed.reordered, [1, 2, 3, 4, 6, 5]);
        assert.deepEqual(keyed.removedIds, [1, 2, 3, 6, 5]);
        assert.equal(keyed.removedProperty, null);
        assert.deepEqual(keyed.restoredIds, [1, 2, 3, 7, 6, 5]);
        assert.equal(keyed.nextId, 8);
        await call("paintOrder", [false]);
        await capture("green-over-red");
        assert.deepEqual(
          await call("sample", ["green-over-red", 160, 172]),
          [0, 255, 0, 255],
        );
        assert.deepEqual(
          await call("sample", ["green-over-red", 300, 120]),
          [255, 0, 0, 255],
        );
        assert.deepEqual(
          await call("sample", ["green-over-red", 317, 120]),
          await call("sample", ["green-over-red", 0, 0]),
        );
        await call("paintOrder", [true]);
        await capture("red-over-green");
        assert.deepEqual(
          await call("sample", ["red-over-green", 160, 172]),
          [255, 0, 0, 255],
        );
        await call("paintOrder", [false, Math.PI]);
        await capture("rear-green-over-red");
        assert.deepEqual(
          await call("sample", ["rear-green-over-red", 159, 172]),
          [0, 255, 0, 255],
        );
        assert.deepEqual(
          await call("sample", ["rear-green-over-red", 19, 120]),
          [255, 0, 0, 255],
        );
        assert.deepEqual(
          await call("sample", ["rear-green-over-red", 2, 120]),
          await call("sample", ["rear-green-over-red", 319, 0]),
        );
        // Content is top-left/Y-down: at 80 px per metre around the canvas centre,
        // content (x, y) on the centred 3.8 x 2.4 panel renders at
        // (160 + 80 (x - 1.9), 120 + 80 (y - 1.2)), independent of production code.
        type Region = {
          count: number;
          centroid: [number, number];
          size: [number, number];
        };
        const pixel = (x: number, y: number) => [
          160 + 80 * (x - 1.9) - 0.5,
          120 + 80 * (y - 1.2) - 0.5,
        ];
        const near = (
          actual: readonly number[],
          expected: number[],
          name: string,
          tolerance = 1.5,
        ) =>
          assert.ok(
            actual.every(
              (value, axis) => Math.abs(value - expected[axis]!) < tolerance,
            ),
            `${name}: ${actual} vs ${expected}`,
          );
        const orientation: Array<{
          red: Region;
          green: Region;
          hit: number[];
        }> = [];
        for (const [redY, label] of [
          [0.5, "orientation"],
          [1.0, "orientation-moved"],
        ] as const) {
          const probe = await call<{
            red: Region;
            green: Region;
            hit: number[];
          }>("orientationProbe", [redY, label]);
          await writeDataUrl(
            resolve(env.evidence.directory, `${label}.png`),
            await invoke<string>(env.page, module, "captureDataUrl", [label]),
          );
          assert.ok(probe.red.count > 200 && probe.green.count > 200);
          near(probe.red.centroid, pixel(0.6, redY), `${label} red centroid`);
          near(
            probe.green.centroid,
            pixel(3.2, 1.9),
            `${label} green centroid`,
          );
          // Nonuniform scale keeps its axes: red is wide, green is tall.
          near(probe.red.size, [32, 16], `${label} red extent`);
          near(probe.green.size, [16, 32], `${label} green extent`);
          // The rendered pixel maps back through the camera and Surface inverse.
          near(probe.hit, [0.6, redY], `${label} plane hit`, 0.02);
          orientation.push(probe);
        }
        assert.ok(
          Math.abs(
            orientation[1]!.red.centroid[1] -
              orientation[0]!.red.centroid[1] -
              40,
          ) < 1,
          "Increasing content Y must move content 40 px down",
        );
        await call("pendingFont");
        const pending = await capture("font-pending");
        assert.ok(
          pending.textPixels < front.textPixels * 0.1,
          "pending font should suppress only text",
        );
        assert.deepEqual(pending.background, front.background);
        assert.deepEqual(pending.bitmap, front.bitmap);
        await call("provideFont");
        await capture("font-ready");
        assert.equal(await call("equal", ["front-small", "font-ready"]), true);
        for (const [name, glyph] of [
          ["slash", "/"],
          ["backslash", "\\"],
          ["y", "y"],
        ]) {
          for (const fontSize of [0.14, 0.35, 0.7]) {
            for (const projected of [false, true]) {
              const label = `glyph-${name}-${fontSize}-${projected ? "perspective" : "front"}`;
              const result = await call<{
                interior: number;
                missingInterior: number;
                expectedMass: number;
                relativeError: number;
                expectedVerticalBounds: [number, number] | null;
                actualVerticalBounds: [number, number] | null;
              }>("glyphProbe", [
                glyph,
                fontSize,
                projected ? 0.65 : 0,
                projected,
                label,
              ]);
              await writeDataUrl(
                resolve(env.evidence.directory, `${label}.png`),
                await invoke<string>(env.page, module, "captureDataUrl", [
                  label,
                ]),
              );
              await writeDataUrl(
                resolve(env.evidence.directory, `${label}-reference.png`),
                await invoke<string>(env.page, module, "referenceDataUrl", [
                  label,
                ]),
              );
              assert.ok(
                result.expectedMass > 5,
                `${label}: oracle has visible coverage`,
              );
              if (fontSize >= 0.35)
                assert.ok(
                  result.interior > 5,
                  `${label}: oracle has interior pixels`,
                );
              assert.equal(
                result.missingInterior,
                0,
                `${label}: dropped solid stroke pixels: ${JSON.stringify(result)}`,
              );
              assert.ok(
                result.relativeError < 0.2,
                `${label}: glyph coverage mismatch: ${JSON.stringify(result)}`,
              );
              assert.ok(
                result.expectedVerticalBounds !== null &&
                  result.actualVerticalBounds !== null &&
                  result.actualVerticalBounds.every(
                    (value, index) =>
                      Math.abs(
                        value - result.expectedVerticalBounds![index]!,
                      ) <= 2,
                  ),
                `${label}: baseline-relative vertical bounds mismatch: ${JSON.stringify(result)}`,
              );
            }
          }
        }
      } finally {
        await call("close");
      }
    },
  );
});
