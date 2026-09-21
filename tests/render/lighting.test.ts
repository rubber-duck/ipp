import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { invoke, recordCapture } from "./evidence.js";
import type * as Fixture from "./lighting-fixture.js";

test("direct lights and spotlight depth maps follow authored state and recover in real WebGL", {
  timeout: 90000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const directory = resolve(workspace, "target/browser-build/render-shadows");
  const build: BrowserBuildConfiguration = {
    name: "render-shadows",
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "direct lighting and shadows",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      closeTimeoutMs: 5000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/lighting",
      ),
    },
    context.signal,
    async (environment) => {
      const module = `${environment.urls.origin}/dist/tests/render/lighting-fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        environment.execute(name, args, () =>
          invoke<T>(environment.page, module, name, args),
        );
      const captured = new Set<string>();
      const capture = async (label: string, expectedDraws = 3) => {
        const result = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          captured,
          label,
          { canvasSelector: "#lighting-canvas" },
        );
        assert.equal(result.drawCalls, expectedDraws);
        return result;
      };
      const update = (
        name: string,
        component: string,
        fields: Record<string, number | boolean>,
      ) => call("update", [name, component, fields]);
      const difference = (a: string, b: string) =>
        call<ReturnType<typeof Fixture.difference>>("difference", [a, b]);
      const shadowDifference = (a: string, b: string) =>
        call<ReturnType<typeof Fixture.shadowDifference>>("shadowDifference", [
          a,
          b,
        ]);
      try {
        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        const initial = await capture("shadowed");
        const bounds =
          await call<Awaited<ReturnType<typeof Fixture.automaticBounds>>>(
            "automaticBounds",
          );
        assert.equal(bounds.before.length, 3);
        assert.ok(
          bounds.before.every((entity) => entity.effective && !entity.authored),
        );
        assert.ok(
          bounds.authored.find((entity) => entity.name === "cube")?.authored,
        );
        assert.deepEqual(bounds.restored, bounds.before);
        await capture("bounds-restored");
        assert.ok(
          (await difference("shadowed", "bounds-restored")).changedPixels < 5,
        );
        await update("spot", "Light", { cast_shadows: false });
        await capture("unshadowed");
        const shadow = await shadowDifference("unshadowed", "shadowed");
        environment.evidence.record("shadow_measurement", shadow);
        assert.ok(
          shadow.darkened > 180,
          `The caster must darken a measurable receiver region: ${JSON.stringify(shadow)}`,
        );
        assert.equal(shadow.brightened, 0, "Occlusion cannot add illumination");
        assert.ok(shadow.sentinel > 100, "Unlit sentinel remains visible");
        assert.equal(
          shadow.changedSentinel,
          0,
          "Lights and shadows preserve unlit color",
        );

        await update("spot", "Light", {
          cast_shadows: true,
          shadow_radius: 0.12,
        });
        await capture("small-emitter");
        const small = await call<
          ReturnType<typeof Fixture.softShadowDifference>
        >("softShadowDifference", ["unshadowed", "shadowed", "small-emitter"]);
        await update("spot", "Light", { shadow_radius: 0.35 });
        await capture("large-emitter");
        const soft = await call<
          ReturnType<typeof Fixture.softShadowDifference>
        >("softShadowDifference", ["unshadowed", "shadowed", "large-emitter"]);
        environment.evidence.record("soft_shadow_edges", { small, soft });
        assert.ok(
          soft.softened > 100,
          "A finite emitter lightens the inner shadow edge",
        );
        assert.ok(
          soft.spread > small.spread + 30,
          "A larger emitter widens the penumbra",
        );
        assert.equal(soft.brightened, 0, "Soft shadows never add light");
        await call("recoverContext");
        await capture("soft-shadow-restored");
        assert.ok(
          (await difference("large-emitter", "soft-shadow-restored"))
            .changedPixels < 5,
        );
        await update("spot", "Light", {
          shadow_radius: 0,
          cast_shadows: false,
        });

        await update("cube", "PbrMaterial", { cast_shadows: false });
        await update("spot", "Light", { cast_shadows: true });
        await capture("caster-disabled");
        assert.ok(
          (await difference("unshadowed", "caster-disabled")).changedFraction <
            0.0001,
        );
        await update("cube", "PbrMaterial", { cast_shadows: true });
        await update("floor", "PbrMaterial", { receive_shadows: false });
        await capture("receiver-disabled");
        assert.ok(
          (await difference("unshadowed", "receiver-disabled"))
            .changedFraction < 0.002,
        );
        await update("floor", "PbrMaterial", { receive_shadows: true });

        await call("moveLight", [2]);
        await capture("light-moved-shadowed");
        await update("spot", "Light", { cast_shadows: false });
        await capture("light-moved-lit");
        const moved = await shadowDifference(
          "light-moved-lit",
          "light-moved-shadowed",
        );
        assert.ok(moved.darkened > 180);
        assert.ok(
          Math.hypot(
            moved.centroid[0]! - shadow.centroid[0]!,
            moved.centroid[1]! - shadow.centroid[1]!,
          ) > 20,
          "The shadow must move with the light",
        );
        await update("cube", "Transform", { x: 0.9 });
        await capture("caster-moved-lit");
        await update("spot", "Light", { cast_shadows: true });
        await capture("caster-moved-shadowed");
        const caster = await shadowDifference(
          "caster-moved-lit",
          "caster-moved-shadowed",
        );
        assert.ok(caster.darkened > 100);
        assert.ok(
          Math.hypot(
            caster.centroid[0]! - moved.centroid[0]!,
            caster.centroid[1]! - moved.centroid[1]!,
          ) > 15,
        );

        const partial =
          await call<Awaited<ReturnType<typeof Fixture.failedLightUpdate>>>(
            "failedLightUpdate",
          );
        assert.equal(partial.outcome.ok, false);
        assert.equal(partial.after.length, partial.before.length + 1);
        assert.ok(
          partial.after.some(
            (entity) => entity.metadata.symbolicId === "partial-light-update",
          ),
        );
        await capture("partially-updated-light");
        assert.ok(
          (await difference("caster-moved-shadowed", "partially-updated-light"))
            .changedFraction > 0.003,
          "The light update before the failed operation remains visible",
        );
        await call("correctLightUpdate");
        await capture("corrected-light");
        assert.ok(
          (await difference("caster-moved-shadowed", "corrected-light"))
            .changedFraction < 0.0001,
        );
        await call("recoverContext");
        const recovered = await capture("context-restored");
        assert.ok(recovered.contextGeneration > initial.contextGeneration);
        assert.equal(recovered.session, initial.session);
        assert.ok(
          (await difference("caster-moved-shadowed", "context-restored"))
            .changedFraction < 0.0001,
        );

        await update("spot", "Light", { cast_shadows: false, kind: 1 });
        await capture("point-light");
        assert.ok(
          (await difference("caster-moved-lit", "point-light"))
            .changedFraction > 0.003,
          "Point light illuminates outside the spot cone",
        );
        await update("spot", "Light", { kind: 0, intensity: 2 });
        await capture("directional-light");
        assert.ok(
          (await difference("point-light", "directional-light"))
            .changedFraction > 0.03,
        );
        await update("spot", "Light", { intensity: 0 });
        await update("fill", "Light", { intensity: 0 });
        await capture("lights-off");
        assert.ok(
          (await difference("directional-light", "lights-off"))
            .changedFraction > 0.08,
        );
        const unlit = await shadowDifference("unshadowed", "lights-off");
        assert.equal(unlit.changedSentinel, 0);

        const corpus = `${environment.urls.origin}/target/shapes-build`;
        await call("normalMesh", [`${corpus}/sphere.mesh`]);
        const smooth = await capture("normals-smooth");
        await call("normalMesh", [`${corpus}/sphere-flat.mesh`]);
        const flat = await capture("normals-flat");
        const facets = await difference("normals-smooth", "normals-flat");
        environment.evidence.record("normal_shading_difference", facets);
        assert.ok(
          facets.changedPixels > 300,
          "Supplied normals must smooth the same sphere triangles",
        );
        assert.equal(
          Number(flat.backend.totalUploadedBytes) -
            Number(smooth.backend.totalUploadedBytes),
          559 * 32 + 2880 * 2,
        );

        await call("normalMesh", [`${corpus}/sphere.mesh`, true]);
        await capture("normals-scaled");
        await call("normalMesh", [`${corpus}/sphere-baked.mesh`]);
        const baked = await capture("normals-baked");
        const transformed = await difference("normals-scaled", "normals-baked");
        environment.evidence.record("normal_transform_oracle", transformed);
        assert.ok(
          transformed.changedFraction < 0.0001,
          "Inverse-transpose normals must match independently baked scale and rotation",
        );
        assert.ok(transformed.meanAbsoluteChannelDifference < 0.02);
        await call("recoverContext");
        const restoredNormals = await capture("normals-restored");
        assert.ok(restoredNormals.contextGeneration > baked.contextGeneration);
        assert.equal(restoredNormals.session, baked.session);
        assert.equal(
          (await difference("normals-baked", "normals-restored")).changedPixels,
          0,
        );

        await call("baseColorTexture", [true]);
        const textured = await capture("textured-pbr");
        assert.ok(
          (await difference("normals-restored", "textured-pbr")).changedPixels >
            300,
          "Base-color sampling must visibly modulate lit geometry",
        );
        await update("spot", "Light", { intensity: 0 });
        await update("fill", "Light", { intensity: 0 });
        await capture("textured-pbr-dark");
        assert.ok(
          (await difference("textured-pbr", "textured-pbr-dark"))
            .changedPixels > 300,
          "Textured PBR must respond to authored lights",
        );
        await call("ambientLight", [[0.25, 0.5, 0.75]]);
        await capture("textured-pbr-ambient");
        assert.ok(
          (await difference("textured-pbr-dark", "textured-pbr-ambient"))
            .changedPixels > 300,
          "Ambient fill must illuminate textured PBR with punctual lights off",
        );
        assert.equal(
          (await shadowDifference("textured-pbr-dark", "textured-pbr-ambient"))
            .changedSentinel,
          0,
          "Ambient state must preserve unlit material color",
        );
        assert.deepEqual(await call("ambientChanges"), [[0.25, 0.5, 0.75]]);
        await call("ambientLight", [[0.25, 0.5, 0.75]]);
        await capture("ambient-noop");
        assert.deepEqual(await call("ambientChanges"), [[0.25, 0.5, 0.75]]);
        await call("recoverContext");
        await capture("ambient-restored");
        assert.equal(
          (await difference("textured-pbr-ambient", "ambient-restored"))
            .changedPixels,
          0,
        );
        await call("ambientLight", [[0, 0, 0]]);
        await capture("ambient-disabled");
        assert.equal(
          (await difference("textured-pbr-dark", "ambient-disabled"))
            .changedPixels,
          0,
        );
        await update("spot", "Light", { intensity: 2 });
        await update("fill", "Light", { intensity: 0.25 });
        await call("recoverContext");
        const restoredTexture = await capture("textured-pbr-restored");
        assert.ok(
          restoredTexture.contextGeneration > textured.contextGeneration,
        );
        assert.equal(
          (await difference("textured-pbr", "textured-pbr-restored"))
            .changedPixels,
          0,
        );
        await update("spot", "Light", {
          kind: 2,
          intensity: 65,
          cast_shadows: true,
        });
        await capture("textured-pbr-shadowed");
        await update("spot", "Light", { cast_shadows: false });
        await capture("textured-pbr-unshadowed");
        const texturedShadow = await shadowDifference(
          "textured-pbr-unshadowed",
          "textured-pbr-shadowed",
        );
        assert.ok(
          texturedShadow.darkened > 100,
          "Textured PBR and the spotlight shadow sampler must work together",
        );
        assert.equal(texturedShadow.brightened, 0);
        await update("spot", "Light", { kind: 0, intensity: 2 });
        await call("baseColorTexture", [false]);
        await capture("textured-pbr-removed");
        assert.equal(
          (await difference("normals-restored", "textured-pbr-removed"))
            .changedPixels,
          0,
        );
        // Recreate the small fixture so earlier material/pose edits cannot hide
        // individual atlas contributions. Keep all four lights on during each A/B.
        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        const sources = await call<string[]>("multipleShadowSources");
        await capture("four-shadows");
        for (const [index, source] of sources.entries()) {
          await update(source, "Light", { cast_shadows: false });
          await capture(`without-shadow-${index}`);
          const contribution = await shadowDifference(
            `without-shadow-${index}`,
            "four-shadows",
          );
          await environment.evidence.record(
            `shadow-source-${index}`,
            contribution,
          );
          assert.ok(
            contribution.darkened > 40,
            `Shadow source ${index} must independently occlude the receiver`,
          );
          assert.equal(
            contribution.brightened,
            0,
            "Removing one shadow cannot darken other atlas tiles",
          );
          await update(source, "Light", { cast_shadows: true });
        }
        await call("recoverContext");
        await capture("four-shadows-restored");
        assert.ok(
          (await difference("four-shadows", "four-shadows-restored"))
            .changedPixels < 5,
        );
        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        const eight = await call<string[]>("multipleShadowSources", [8]);
        await capture("eight-shadows");
        await update(eight[7]!, "Light", { cast_shadows: false });
        await capture("without-eighth-shadow");
        assert.ok(
          (await shadowDifference("without-eighth-shadow", "eight-shadows"))
            .darkened > 20,
          "The eighth source renders and samples the third atlas row",
        );
        const worker = environment.page.workers()[0]!;
        // A constrained allocation fixture still uses the real WebGL programs,
        // atlas rendering, samplers and completed-frame capture.
        await worker.evaluate(() => {
          const original = WebGL2RenderingContext.prototype.texImage2D;
          const state = globalThis as typeof globalThis & {
            restoreShadowFixture?: () => void;
          };
          state.restoreShadowFixture = () => {
            WebGL2RenderingContext.prototype.texImage2D = original;
          };
          WebGL2RenderingContext.prototype.texImage2D = new Proxy(original, {
            apply(target, receiver, args) {
              if (args[2] === receiver.DEPTH_COMPONENT24 && args[3] > 1024)
                throw new Error("Fixture shadow allocation exceeds one tile");
              return Reflect.apply(target, receiver, args);
            },
          });
        });
        try {
          await call("recoverContext");
          const limited = await capture("shadow-allocation-fallback");
          assert.equal(limited.backend.unshadowedLights, 6);
          assert.ok(
            (
              await shadowDifference(
                "shadow-allocation-fallback",
                "without-eighth-shadow",
              )
            ).darkened > 20,
            "exhausted shadow slots leave lights illuminating",
          );
          const again = await capture("shadow-allocation-fallback-stable");
          assert.equal(again.backend.unshadowedLights, 6);
          assert.ok(
            (
              await difference(
                "shadow-allocation-fallback",
                "shadow-allocation-fallback-stable",
              )
            ).changedPixels < 5,
          );
        } finally {
          await worker.evaluate(() => {
            const state = globalThis as typeof globalThis & {
              restoreShadowFixture?: () => void;
            };
            state.restoreShadowFixture?.();
            delete state.restoreShadowFixture;
          });
        }
        await call("recoverContext");
        const recoveredShadows = await capture("shadow-capacity-recovered");
        assert.equal(recoveredShadows.backend.unshadowedLights, 0);
        assert.ok(
          (
            await difference(
              "without-eighth-shadow",
              "shadow-capacity-recovered",
            )
          ).changedPixels < 5,
        );

        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        await call("separatedLightGroups");
        await capture("sixteen-lights-separated");
        const colors = await call<number[][]>("selectedLightColors", [
          "sixteen-lights-separated",
        ]);
        assert.ok(
          colors[0]![0]! > colors[0]![2]! + 50,
          "left object selects nearby red lights",
        );
        assert.ok(
          colors[1]![2]! > colors[1]![0]! + 50,
          "right object selects nearby blue lights",
        );
        await call("recoverContext");
        await capture("sixteen-lights-recovered");
        assert.ok(
          (
            await difference(
              "sixteen-lights-separated",
              "sixteen-lights-recovered",
            )
          ).changedPixels < 5,
        );
        await call("initialize", [
          {
            generatedModuleUrl: environment.urls.generated,
            workerScriptUrl: environment.urls.workerScript,
            wasmUrl: environment.urls.wasm,
          },
        ]);
        await call("contactShadowFixture");
        await capture("distant-contact-shadow", 2);
        await update("cube", "PbrMaterial", { cast_shadows: false });
        await capture("distant-receiver-only", 2);
        const contact = await call<number[]>("contactShadowSamples", [
          "distant-receiver-only",
          "distant-contact-shadow",
        ]);
        environment.evidence.record("analytic_contact_shadow", { contact });
        assert.ok(
          contact.every((value) => value > 25),
          `Small contact shadows must cover the analytically occluded ground points: ${contact}`,
        );
        await update("spot", "Light", { cast_shadows: false });
        await capture("distant-no-shadow", 2);
        const acne = await shadowDifference(
          "distant-no-shadow",
          "distant-receiver-only",
        );
        assert.equal(
          acne.darkened,
          0,
          "A planar receiver must not shadow itself with a wide filter and small bias",
        );
      } catch (error) {
        await capture("failure").catch(() => {});
        throw error;
      } finally {
        await call("close");
      }
    },
  );
});

test("example lighting inspector edits shadows, materials and synchronized light markers", {
  timeout: 90000,
}, async (context) => {
  const { openGallery, galleryEnvironment, entity, selected } = await import(
    "./gallery-driver.js"
  );
  await runBrowserEnvironment(
    "combined lighting inspector",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("lighting");
      await g.seek(0);
      await g.page.locator("#object-select").selectOption("lighting-spot");
      const shadowed = await g.capture("shadowed");
      await g.page.locator("#object-cast-shadows").uncheck();
      const unshadowed = await g.capture("unshadowed");
      assert.ok(
        (await g.difference("shadowed", "unshadowed")).changedPixels > 100,
      );
      assert.equal(shadowed.frame.drawCalls, unshadowed.frame.drawCalls);
      assert.equal(
        entity(unshadowed.inspection, "lighting-spot").effective.find(
          (entry) => "intensity" in entry.fields,
        )!.fields.cast_shadows,
        false,
      );
      await g.page.locator("#object-cast-shadows").check();
      await g.page.locator("#object-intensity").fill("0");
      const fillOnly = await g.capture("fill-only");
      assert.ok(fillOnly.summary.foregroundPixels > 1000);
      for (const id of ["lighting-spot", "lighting-fill", "lighting-point"]) {
        const value = entity(fillOnly.inspection, id);
        assert.ok(value.effective.some((entry) => "intensity" in entry.fields));
        assert.ok(
          value.effective.some((entry) =>
            String(entry.fields.source).includes("mesh"),
          ),
        );
      }
      await g.page.locator("#object-intensity").fill("90");
      await g.page.locator("#object-select").selectOption("lighting-sphere");
      await g.capture("glossy");
      await g.page.locator("#object-roughness").fill("0.95");
      await g.capture("rough");
      assert.ok((await g.difference("glossy", "rough")).changedPixels > 100);
      assert.equal(
        entity(await g.inspect(), "lighting-cube").effective.find(
          (entry) => "roughness" in entry.fields,
        )!.fields.roughness! === Math.fround(0.38),
        true,
      );
      await g.navigate("shapes");
      assert.equal((await g.capture("geometry-return")).frame.drawCalls, 12);
      await g.navigate("lighting");
      await g.seek(0);
      assert.equal(
        await g.page.locator("#object-roughness").inputValue(),
        "0.95",
      );
      assert.deepEqual(
        selected((await g.capture("lighting-return")).inspection),
        ["lighting-sphere"],
      );
      assert.deepEqual(g.errors, []);
    },
  );
});
