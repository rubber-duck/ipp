import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, recordCapture, writeDataUrl } from "./evidence.js";
import type * as Fixture from "./custom-materials-fixture.js";

test("custom materials use named instance values, sparse overlays, fallback, linear alpha and context recovery", {
  timeout: 120000,
}, async (context) => {
  const workspace = process.cwd(),
    directory = resolve(workspace, "target/browser-build/render-expanded");
  const build = {
    name: "render-expanded" as const,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "custom-materials",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      closeTimeoutMs: 5000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/custom-materials",
      ),
    },
    context.signal,
    async (env) => {
      const module = `${env.urls.origin}/dist/tests/render/custom-materials-fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        env.execute(name, args, () => invoke<T>(env.page, module, name, args));
      const captured = new Set<string>();
      const capture = async (name: string, draws = 2) => {
        const value = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [name, draws],
        );
        await recordCapture(
          env.page,
          module,
          env.evidence.directory,
          captured,
          name,
          { canvasSelector: "#custom-materials-canvas" },
        );
        return value;
      };
      let sampleIndex = 0;
      const rgb = async (actual: number[], expected: number[]) => {
        const evidence = await call<ReturnType<typeof Fixture.sampleEvidence>>(
          "sampleEvidence",
          [actual, expected],
        );
        for (const [kind, url] of Object.entries(evidence))
          await writeDataUrl(
            resolve(
              env.evidence.directory,
              `sample-${sampleIndex}-${kind}.png`,
            ),
            url,
          );
        sampleIndex++;
        assert.ok(
          expected.every((value, i) => Math.abs(actual[i]! - value) <= 3),
          `${actual} != ${expected}`,
        );
      };
      const sameFrame = async (actual: string, expected: string) => {
        await writeDataUrl(
          resolve(env.evidence.directory, `${actual}-expected.png`),
          await call<string>("captureDataUrl", [expected]),
        );
        await writeDataUrl(
          resolve(env.evidence.directory, `${actual}-difference.png`),
          await call<string>("differenceDataUrl", [actual, expected]),
        );
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.difference>>("difference", [
              actual,
              expected,
            ])
          ).changedPixels,
          0,
        );
      };
      const tintShader = {
        parameters: { tint: "vec4" },
        backends: {
          "glsl-es-300": {
            fragment: "vec4 materialFragment() { return p_tint; }",
          },
        },
      };
      try {
        await call("initialize", [
          {
            generatedModuleUrl: env.urls.generated,
            workerScriptUrl: env.urls.workerScript,
            wasmUrl: env.urls.wasm,
          },
        ]);
        const initial = await capture("instances");
        await rgb(initial.left, [0, 255, 0]);
        await rgb(initial.right, [255, 0, 0]);
        await call("select", [
          {
            parameters: {},
            backends: {
              "glsl-es-300": {
                fragment:
                  "vec4 materialFragment() { float v = 0.001 + clamp((gl_FragCoord.x - 80.0) / 54.0, 0.0, 1.0) * 0.07; return vec4(vec3(v),1); }",
              },
            },
          },
        ]);
        await capture("dark-gradient");
        const ramp = await call<number[]>("darkRamp", ["dark-gradient"]);
        await env.evidence.record("dark_gradient_levels", ramp);
        assert.ok(
          new Set(ramp).size >= 22,
          `Dark gradient collapses into ${new Set(ramp).size} bands`,
        );
        assert.ok(
          ramp.slice(1).every((v, i) => v >= ramp[i]! && v - ramp[i]! <= 2),
          "Dark gradient should advance smoothly through display levels",
        );
        await call("select", [tintShader]);
        await call("shaderEditor");
        const editorFirst = await capture("react-editor-blue");
        await rgb(editorFirst.left, [0, 0, 255]);
        await call("shaderEditor", [
          "vec4 materialFragment() { return p_tint; }",
          [0, 1, 0, 1],
        ]);
        const editorValue = await capture("react-editor-value");
        await rgb(editorValue.left, [0, 255, 0]);
        assert.equal(
          editorValue.backend.shaderProgramsCreated,
          editorFirst.backend.shaderProgramsCreated,
        );
        await call("shaderEditor", [
          "vec4 materialFragment() { return p_tint * vec4(0.25,0.25,0.25,1); }",
          [0, 1, 0, 1],
        ]);
        await rgb((await capture("react-editor-code")).left, [0, 137, 0]);
        await call("beginShaderReplacement", [
          "vec4 materialFragment() { return p_tint * vec4(0.5,0.5,0.5,1); }",
          [0, 1, 0, 1],
        ]);
        await rgb(
          (await capture("react-editor-replacement-pending")).left,
          [0, 137, 0],
        );
        await call("finishShaderReplacement");
        await rgb(
          (await capture("react-editor-replacement-ready")).left,
          [0, 188, 0],
        );
        await call("shaderEditor", ["invalid GLSL", [0, 1, 0, 1]]);
        await rgb((await capture("react-editor-invalid")).left, [0, 188, 0]);
        await call("shaderEditor", [
          "vec4 materialFragment() { return p_tint; }",
          [1, 1, 0, 1],
        ]);
        await rgb(
          (await capture("react-editor-corrected")).left,
          [255, 255, 0],
        );
        await call("closeShaderEditor");
        await rgb((await capture("react-editor-restored")).left, [0, 255, 0]);
        await call("poseAndSkin", [true]);
        await capture("pose-skinned");
        await call("poseAndSkin", [false]);
        await capture("pose-baked");
        await sameFrame("pose-skinned", "pose-baked");
        await call("flatPose", [true]);
        await capture("pose-derived-normals");
        await call("flatPose", [false]);
        await capture("baked-derived-normals");
        await sameFrame("pose-derived-normals", "baked-derived-normals");
        await call("update", [
          "left",
          "CustomMaterial",
          { receives_light: false },
        ]);
        await call("vertexFixture");
        await rgb((await capture("vertex-colors")).left, [137, 188, 225]);
        await call("deformation");
        await capture("deform-zero");
        await call("parameter", [
          "left",
          "displacement",
          { kind: "vec2", value: [0.15, 0] },
        ]);
        await capture("deform-moved");
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.difference>>("difference", [
              "deform-zero",
              "deform-moved",
            ])
          ).changedPixels > 300,
        );
        await call("select", [
          {
            parameters: {},
            requiredAttributes: 4,
            backends: {
              "glsl-es-300": {
                fragment: "vec4 materialFragment() { return vec4(1); }",
              },
            },
          },
        ]);
        const missingStream = await capture("missing-normal-fallback");
        assert.ok(missingStream.left[2]! > 40 && missingStream.left[0] === 0);
        await call("bounds");
        await rgb((await capture("bounds-unproven")).left, [0, 255, 0]);
        await call("update", [
          "left",
          "CustomMaterial",
          { conservative_bounds: true },
        ]);
        await rgb((await capture("bounds-asserted", 1)).left, [13, 13, 22]);
        await call("update", [
          "left",
          "CustomMaterial",
          { conservative_bounds: false },
        ]);
        await call("update", ["left", "Transform", { x: -0.9 }]);
        await call("packing");
        await rgb((await capture("packing-all-types")).left, [0, 255, 0]);
        await call("textures");
        await rgb((await capture("multiple-textures")).left, [188, 188, 0]);
        const independentAsset = await call<
          Awaited<ReturnType<typeof Fixture.assetBinding>>
        >("assetBinding", ["unreferenced"]);
        assert.deepEqual(independentAsset.observed, independentAsset.reference);
        await capture("unreferenced-mesh-property");
        await sameFrame("unreferenced-mesh-property", "multiple-textures");
        const requiredAsset = await call<
          Awaited<ReturnType<typeof Fixture.assetBinding>>
        >("assetBinding", ["required"]);
        assert.deepEqual(requiredAsset.observed, independentAsset.observed);
        const incompatibleAsset = await capture("mesh-rejected-by-sampler");
        assert.ok(
          incompatibleAsset.left[2]! > 40 && incompatibleAsset.left[0] === 0,
        );
        await call("assetBinding", ["texture"]);
        await rgb((await capture("asset-binding-corrected")).left, [255, 0, 0]);
        await call("textures");
        const textureAnimation = await call<string>("textureAnimation");
        await rgb((await capture("texture-step-animation")).left, [0, 188, 0]);
        await capture("texture-step-held");
        await sameFrame("texture-step-held", "texture-step-animation");
        assert.ok(
          Math.abs(
            (await call<number>("seekAnimation", [textureAnimation, 0])) - 0.65,
          ) < 1e-6,
        );
        await rgb((await capture("texture-step-backward")).left, [188, 188, 0]);
        const mixedNumeric = await call<number>("seekAnimation", [
          textureAnimation,
          1,
        ]);
        assert.ok(Math.abs(mixedNumeric - 0.55) < 1e-6);
        await rgb((await capture("texture-step-forward")).left, [0, 188, 0]);
        await call("stopAnimation", [textureAnimation]);
        await rgb(
          (await capture("texture-animation-restored")).left,
          [188, 188, 0],
        );
        await call("deviceLimit");
        const limited = await capture("device-limit-fallback");
        assert.ok(limited.left[2]! > 40 && limited.left[0] === 0);
        await call("select", [
          {
            parameters: {},
            backends: { vulkan: { fragment: "preserved foreign source" } },
          },
        ]);
        const unsupported = await capture("unsupported-backend-fallback");
        assert.ok(unsupported.left[2]! > 40 && unsupported.left[0] === 0);
        await call("select", [tintShader]);
        const animation = await call<string>("animate");
        await rgb((await capture("animated")).left, [0, 188, 0]);
        await call("stopAnimation", [animation]);
        await rgb((await capture("animation-restored")).left, [0, 255, 0]);
        await call("select", [
          {
            parameters: {},
            backends: {
              "glsl-es-300": {
                fragment:
                  "vec4 materialFragment() {\n#if IPP_RECEIVES_LIGHT\nreturn vec4(u_ambient,1);\n#else\nreturn vec4(1,0,0,1);\n#endif\n}",
              },
            },
          },
        ]);
        await call("ambient", [0.5]);
        await rgb((await capture("light-disabled")).left, [255, 0, 0]);
        await call("update", [
          "left",
          "CustomMaterial",
          { receives_light: true },
        ]);
        await call("select", [
          {
            recipe: { lighting: true },
            parameters: {},
            backends: {
              "glsl-es-300": {
                fragment:
                  "vec4 materialFragment() {\n#if IPP_RECEIVES_LIGHT\nreturn vec4(u_ambient,1);\n#else\nreturn vec4(1,0,0,1);\n#endif\n}",
              },
            },
          },
        ]);
        await rgb((await capture("light-enabled")).left, [188, 188, 188]);
        await call("update", [
          "left",
          "CustomMaterial",
          { receives_light: false },
        ]);
        await call("select", [tintShader]);
        const beforeWrites = await capture("before-writes");
        const owner = await call<string>("overlays");
        await rgb((await capture("overlay")).left, [0, 0, 255]);
        await call("parameter", [
          "left",
          "tint",
          { kind: "vec4", value: [1, 1, 0, 1] },
        ]);
        await rgb((await capture("hidden-write")).left, [0, 0, 255]);
        await call("releaseOverlay", [owner]);
        const restored = await capture("restored");
        await rgb(restored.left, [255, 255, 0]);
        assert.equal(
          restored.backend.shaderProgramsCreated,
          beforeWrites.backend.shaderProgramsCreated,
        );
        await call("select", [
          {
            parameters: {},
            backends: { "glsl-es-300": { fragment: "this is invalid GLSL" } },
          },
        ]);
        await call("insert", [
          "left",
          "PbrMaterial",
          { r: 1, g: 1, b: 0, metallic: 0, roughness: 1 },
        ]);
        const pbr = await capture("pbr-fallback");
        assert.ok(pbr.left[0]! > 30 && pbr.left[1]! > 30 && pbr.left[2] === 0);
        await call("remove", ["left", "PbrMaterial"]);
        const failed = await capture("compile-fallback");
        assert.ok(failed.left[2]! > 40 && failed.left[0] === 0);
        const cachedFailure = await capture("cached-compile-failure");
        assert.equal(
          cachedFailure.backend.shaderProgramAttempts,
          failed.backend.shaderProgramAttempts,
        );
        await call("remove", ["left", "UnlitMaterial"]);
        await rgb((await capture("red-fallback")).left, [255, 0, 0]);
        await call("select", [tintShader]);
        await call("update", ["left", "Transform", { x: 0, z: 1.5 }]);
        await call("update", ["right", "Transform", { x: 0, z: 0 }]);
        await call("parameter", [
          "left",
          "tint",
          { kind: "vec4", value: [0, 1, 0, 1] },
        ]);
        await rgb((await capture("opaque-green-front")).center, [0, 255, 0]);
        await call("update", ["left", "Transform", { z: -1.5 }]);
        await rgb((await capture("opaque-red-front")).center, [255, 0, 0]);
        await call("parameter", [
          "right",
          "tint",
          { kind: "vec4", value: [0, 0, 1, 1] },
        ]);
        await rgb(
          (await capture("opaque-material-change")).center,
          [0, 0, 255],
        );
        await call("parameter", [
          "right",
          "tint",
          { kind: "vec4", value: [1, 0, 0, 1] },
        ]);
        await call("update", ["left", "Transform", { z: 1.5 }]);
        await call("parameter", [
          "left",
          "tint",
          { kind: "vec4", value: [0, 1, 0, 0.5] },
        ]);
        await call("update", ["left", "CustomMaterial", { alpha_mode: 2 }]);
        const blended = await capture("linear-blending");
        await rgb(blended.center, [187, 188, 0]);
        await call("recoverContext");
        await capture("recovered");
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.difference>>("difference", [
              "linear-blending",
              "recovered",
            ])
          ).changedPixels,
          0,
        );
        await call("parameter", [
          "right",
          "tint",
          { kind: "vec4", value: [1, 0, 0, 0.5] },
        ]);
        await call("update", ["right", "CustomMaterial", { alpha_mode: 2 }]);
        const greenFront = await capture("two-blended-green-front");
        assert.ok(greenFront.center[1]! > greenFront.center[0]! + 40);
        await call("update", ["left", "Transform", { z: -1.5 }]);
        const redFront = await capture("two-blended-red-front");
        assert.ok(redFront.center[0]! > redFront.center[1]! + 40);
        await call("shadowScene");
        await capture("custom-shadow-full");
        await call("parameter", [
          "left",
          "coverage",
          { kind: "f32", value: 0 },
        ]);
        await capture("custom-shadow-cutout");
        await call("update", [
          "left",
          "CustomMaterial",
          { casts_shadows: false },
        ]);
        await capture("custom-shadow-disabled");
        await sameFrame("custom-shadow-cutout", "custom-shadow-disabled");
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.difference>>("difference", [
              "custom-shadow-full",
              "custom-shadow-cutout",
            ])
          ).changedPixels > 100,
        );
        assert.ok(
          (await call<number>("shadowPixels", [
            "custom-shadow-cutout",
            "custom-shadow-full",
          ])) > 20,
        );
        await call("shadowReceiver");
        await capture("custom-receives-shadow");
        await call("update", [
          "right",
          "CustomMaterial",
          { receives_shadows: false },
        ]);
        await capture("custom-reception-disabled");
        assert.ok(
          (await call<number>("shadowPixels", [
            "custom-reception-disabled",
            "custom-receives-shadow",
          ])) > 20,
        );
        const saved =
          await call<Awaited<ReturnType<typeof Fixture.saveWithoutShader>>>(
            "saveWithoutShader",
          );
        assert.ok(saved.bytes > 0);
        assert.ok(
          saved.assetSessionIsolated,
          "Pending asset notifications stay fenced across World/session replacement",
        );
        assert.deepEqual(saved.restored, saved.expected);
      } finally {
        await call("close");
      }
    },
  );
});
