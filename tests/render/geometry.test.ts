import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type { GeometryPickResultEvent } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, recordCapture } from "./evidence.js";
import type * as Fixture from "./skinning-fixture.js";

test("joint geometry queries, visualization and conservative culling agree through WASM and WebGL", {
  timeout: 60_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const directory = resolve(workspace, "target/browser-build/render-expanded");
  const build = {
    name: "render-expanded" as const,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "shared geometry",
    { workspace, build, mismatchBuild: build },
    context.signal,
    async (environment) => {
      const module = `${environment.urls.origin}/dist/tests/render/skinning-fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        environment.execute(name, args, () =>
          invoke<T>(environment.page, module, name, args),
        );
      const captured = new Set<string>();
      const capture = async (label: string, draws: number) => {
        const result = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, draws],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          captured,
          label,
          { canvasSelector: "#skinning-canvas" },
        );
        return result;
      };
      const hit = async (
        x: number,
        y: number,
        entity: bigint,
        distance: number,
      ) => {
        const result = await call<GeometryPickResultEvent>("geometryPick", [
          x,
          y,
        ]);
        await environment.evidence.record("geometry-query", result);
        assert.ok(result.ok && result.hit);
        assert.equal(result.hit.entity, entity);
        assert.equal(result.hit.part, 0);
        assert.ok(Math.abs(result.hit.distance - distance) < 0.001);
      };
      try {
        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        const rigs = await call<bigint[]>("geometryScene");
        await capture("geometry-rest", 4);
        await hit(-0.8, 0.5, rigs[0]!, 4.88);
        await hit(0.8, 0.5, rigs[1]!, 4.88);
        assert.deepEqual(
          await call("pixel", ["geometry-rest", 120, 200]),
          [0, 255, 0],
        );
        await call("geometryPose");
        await capture("geometry-posed", 4);
        await hit(-1.35, 0, rigs[0]!, 4.62);
        await hit(0.8, 0.5, rigs[1]!, 4.88);
        assert.deepEqual(
          await call("pixel", ["geometry-posed", 65, 250]),
          [0, 255, 0],
        );
        assert.ok(
          (await call<number>("regionDifference", [
            "geometry-rest",
            "geometry-posed",
            0,
            200,
          ])) > 1000,
        );
        assert.equal(
          await call<number>("regionDifference", [
            "geometry-rest",
            "geometry-posed",
            210,
            400,
          ]),
          0,
        );
        await call("geometryCull", [true]);
        await capture("geometry-culled", 2);
        await call("geometryCull", [false]);
        await capture("geometry-unculled", 3);
        const difference = await call<ReturnType<typeof Fixture.difference>>(
          "difference",
          ["geometry-culled", "geometry-unculled"],
        );
        assert.equal(
          difference.changedPixels,
          0,
          "culling removes an offscreen draw without changing the image",
        );
        await call("geometryReplaceSkeleton");
        const invalid = await call<GeometryPickResultEvent>(
          "geometryPick",
          [-0.6, 0],
        );
        assert.ok(
          !invalid.ok && invalid.error === "InvalidGeometry",
          "replaced skeletons do not silently reuse joint ordinals",
        );
      } finally {
        await call("close");
      }
    },
  );
});
