import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import type { EntitySnapshot, Inspection } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { startBlender } from "./blender-environment.js";
import { invoke, recordCapture } from "./evidence.js";
import type * as Fixture from "./blender-fixture.js";

const workspace = resolve(process.cwd());
const profile = resolve(workspace, "target/browser-build/render-expanded");
const build = {
  name: "render-expanded" as const,
  generatedModule: resolve(profile, "generated.js"),
  runtimeWasm: resolve(profile, "runtime.wasm"),
  exportWasm: resolve(profile, "export.wasm"),
  contractArtifact: resolve(profile, "contract.bin"),
};

test("Blender exports synchronize real resources, animation and overlays through HTTPS/WSS and WebGL", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "blender authored fixture",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 45_000,
      closeTimeoutMs: 10_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/blender",
      ),
    },
    context.signal,
    async (environment) => {
      const cdp = await environment.page
        .context()
        .newCDPSession(environment.page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: environment.urls.origin,
      });
      const blender = await startBlender(environment);
      const fragment = new URLSearchParams({
        endpoint: blender.ready.origin,
        token: blender.ready.token,
      });
      await environment.page.goto(
        `${environment.urls.origin}/target/blender-viewer/index.html#${fragment}`,
      );
      const module = `${environment.urls.origin}/target/blender-test/blender-fixture.js`;
      const captured = new Set<string>();
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(environment.page, module, name, args);
      const capture = async (label: string, revision = 0) => {
        const state = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, revision],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          captured,
          label,
          { canvasSelector: "canvas" },
        );
        await environment.evidence.writeJson(`${label}-inspection.json`, state);
        assert.equal(state.inspection.renderDiagnostics.length, 0);
        return state;
      };
      try {
        const initial = await capture("initial");
        assert.ok(
          initial.summary.foregroundPixels > 1000,
          "real exported geometry must occupy a meaningful image region",
        );
        assert.ok(
          initial.drawCalls >= 3,
          "rigid textured panel, marker and skinned beam must draw",
        );
        const kinds = new Set(
          initial.inspection.resources.map((resource) => resource.kind),
        );
        for (const kind of [1, 2, 3, 4, 5, 10])
          assert.ok(kinds.has(kind), `resource type ${kind} must participate`);
        assert.ok(
          (initial.inspection.controllers?.length ?? 0) >= 2,
          "rigid and skeleton clips must create runtime players",
        );
        const colors = await call<ReturnType<typeof Fixture.colorCounts>>(
          "colorCounts",
          ["initial"],
        );
        for (const color of ["red", "green", "blue"] as const)
          assert.ok(
            colors[color] > 40,
            `${color} texture region must survive export and sampling`,
          );

        const cube = entity(initial.inspection, "fixture-cube");
        assert.equal(cube.metadata.symbolicId, "fixture-cube");
        const cubeId = cube.id;
        const originalSource = field(cube, "source", "MeshInstance");
        const initialTransform = field(cube, "x", "Transform");
        const rejected = await call<
          Awaited<ReturnType<typeof Fixture.rejectInvalidRevision>>
        >("rejectInvalidRevision");
        assert.match(rejected.error, /failed/);
        assert.equal(rejected.revision, initial.revision);
        const afterRejection = await capture("partial-revision");
        assert.equal(
          field(
            entity(afterRejection.inspection, "fixture-cube"),
            "x",
            "Transform",
          ),
          50,
        );
        assert.ok(
          afterRejection.inspection.entities.some(
            (entity) => entity.metadata.symbolicId === "partial-entity",
          ),
        );
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "initial",
              "partial-revision",
            ])
          ).changedPixels > 100,
          "The failed revision keeps the moved cube in the rendered frame",
        );
        const immutable = await environment.page.evaluate(async (url) => {
          const response = await fetch(url);
          const etag = response.headers.get("etag");
          const bytes = await response.arrayBuffer();
          const sha = [
            ...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
          ]
            .map((value) => value.toString(16).padStart(2, "0"))
            .join("");
          const wrong = await fetch(url, {
            headers: { "If-Match": '"wrong"' },
          });
          return { etag, sha, wrong: wrong.status };
        }, String(originalSource));
        assert.ok(immutable.etag && !immutable.etag.startsWith("W/"));
        assert.equal(
          immutable.wrong,
          412,
          "cross-origin precondition failure must be observable",
        );
        assert.match(
          new URL(String(originalSource)).pathname,
          /^\/assets\/[a-f0-9]{32}\/\d+\/\d+$/,
        );
        assert.ok(
          !String(originalSource).includes(immutable.sha),
          "content hashes must remain independent of asset names",
        );

        await call("overrideTransform", ["fixture-cube", -0.25]);
        const override = await capture("overlay");
        assert.equal(
          field(
            entity(override.inspection, "fixture-cube"),
            "x",
            "Transform",
            true,
          ),
          -0.25,
        );
        const renameRevision = await blender.command({
          action: "rename",
          name: "Hero Mesh",
        });
        const renamed = await capture("object-renamed", renameRevision);
        const renamedCube = entity(renamed.inspection, "Hero Mesh");
        assert.equal(field(renamedCube, "x", "Transform"), initialTransform);
        assert.equal(
          renamed.inspection.entities.some(
            (entity) => entity.metadata.symbolicId === "partial-entity",
          ),
          false,
          "The next export cleans up entities created by the failed batch",
        );
        assert.equal(
          renamedCube.id,
          cubeId,
          "renaming must retain runtime identity",
        );
        assert.equal(
          field(renamedCube, "source", "MeshInstance"),
          originalSource,
        );
        assert.equal(
          field(renamedCube, "x", "Transform", true),
          -0.25,
          "an existing overlay survives a name change",
        );
        assert.equal(
          renamed.inspection.entities.some(
            (value) => value.metadata.symbolicId === "fixture-cube",
          ),
          false,
        );
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "overlay",
              "object-renamed",
            ])
          ).changedPixels,
          0,
        );
        const restoreName = await blender.command({
          action: "rename",
          name: "fixture-cube",
        });
        await capture("object-name-restored", restoreName);
        const beamId = entity(initial.inspection, "fixture-beam").id;
        const swapRevision = await blender.command({ action: "swap_names" });
        const swapped = await capture("object-names-swapped", swapRevision);
        assert.equal(entity(swapped.inspection, "fixture-beam").id, cubeId);
        assert.equal(entity(swapped.inspection, "fixture-cube").id, beamId);
        const restoreSwap = await blender.command({ action: "swap_names" });
        await capture("object-names-restored", restoreSwap);
        const movedRevision = await blender.command({ action: "transform" });
        const moved = await capture("base-moved-under-overlay", movedRevision);
        const movedCube = entity(moved.inspection, "fixture-cube");
        assert.equal(
          movedCube.id,
          cubeId,
          "transform edits preserve runtime identity",
        );
        assert.equal(
          field(movedCube, "source", "MeshInstance"),
          originalSource,
        );
        assert.notEqual(field(movedCube, "x", "Transform"), initialTransform);
        assert.equal(
          field(movedCube, "x", "Transform", true),
          -0.25,
          "React override remains effective over updated base",
        );
        await call("releaseComponentStateOverlay");
        const revealed = await capture("latest-base-revealed");
        assert.equal(
          field(
            entity(revealed.inspection, "fixture-cube"),
            "x",
            "Transform",
            true,
          ),
          field(movedCube, "x", "Transform"),
        );
        const revealDifference = await call<ReturnType<typeof Fixture.compare>>(
          "compare",
          ["base-moved-under-overlay", "latest-base-revealed"],
        );
        assert.ok(
          revealDifference.changedPixels > 100,
          "removing overlay must reveal changed rendered position",
        );

        const meshRevision = await blender.command({ action: "mesh" });
        const changed = await capture("mesh-replaced", meshRevision);
        assert.equal(entity(changed.inspection, "fixture-cube").id, cubeId);
        assert.notEqual(
          field(
            entity(changed.inspection, "fixture-cube"),
            "source",
            "MeshInstance",
          ),
          originalSource,
        );
        const old = await environment.page.evaluate(
          async ({ url, etag }) => {
            const response = await fetch(url, {
              headers: { "If-Match": etag! },
            });
            return {
              status: response.status,
              etag: response.headers.get("etag"),
              sha: [
                ...new Uint8Array(
                  await crypto.subtle.digest(
                    "SHA-256",
                    await response.arrayBuffer(),
                  ),
                ),
              ]
                .map((value) => value.toString(16).padStart(2, "0"))
                .join(""),
            };
          },
          { url: String(originalSource), etag: immutable.etag },
        );
        assert.equal(old.status, 200);
        assert.equal(old.etag, immutable.etag);
        assert.equal(
          old.sha,
          immutable.sha,
          "an old name must still return the original bytes",
        );

        const materialRevision = await blender.command({ action: "material" });
        await capture("material-edited", materialRevision);
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "mesh-replaced",
              "material-edited",
            ])
          ).changedPixels > 100,
          "Blender material edits must change rendered pixels",
        );

        await call("seek", ["fixture-cube", 0]);
        await capture("rigid-keyframe-start");
        await call("seek", ["fixture-cube", 0.5]);
        await capture("rigid-keyframe-moved");
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "rigid-keyframe-start",
              "rigid-keyframe-moved",
            ])
          ).changedPixels > 100,
          "semantic property tracks must animate the exported rigid mesh",
        );
        await call("stopPlayers");

        await call("seek", ["fixture-rig", 0]);
        await capture("rig-keyframe-rest");
        await call("seek", ["fixture-rig", 0.5]);
        const animated = await capture("rig-keyframe-bent");
        assert.ok(
          animated.inspection.controllers?.some(
            (player) => player.state === "paused",
          ),
        );
        const animationDifference = await call<
          ReturnType<typeof Fixture.compare>
        >("compare", ["rig-keyframe-rest", "rig-keyframe-bent"]);
        assert.ok(
          animationDifference.changedPixels > 100,
          "exported joint keyframes must change skinned pixels",
        );
        await call("stopPlayers");
        const poseRevision = await blender.command({ action: "pose" });
        await capture("authored-pose", poseRevision);
        const poseDifference = await call<ReturnType<typeof Fixture.compare>>(
          "compare",
          ["material-edited", "authored-pose"],
        );
        assert.ok(
          poseDifference.changedPixels > 100,
          "authored Blender pose must change runtime deformation",
        );

        const removeRevision = await blender.command({ action: "remove" });
        const removed = await capture("object-removed", removeRevision);
        assert.equal(
          removed.inspection.entities.some((value) => value.id === cubeId),
          false,
        );
        assert.ok(removed.drawCalls < changed.drawCalls);
        await call("restoreContext");
        const restored = await capture("context-restored");
        assert.ok(
          restored.contextGeneration > removed.contextGeneration,
          "a new GL context must actually present the recovered scene",
        );
        const recovery = await call<ReturnType<typeof Fixture.compare>>(
          "compare",
          ["object-removed", "context-restored"],
        );
        assert.equal(
          recovery.changedPixels,
          0,
          "immutable recovery must reproduce the scene",
        );
        // Session numbers are Host-local and may repeat in a fresh worker.
        // Verify replacement through the actual client instance and readiness.
        const previousClient = await environment.page.evaluateHandle(
          () => window.ippBlender!.canvas.client,
        );
        try {
          await environment.page
            .getByRole("button", { name: "Reconnect" })
            .click();
          await environment.page.waitForFunction(
            (previous) =>
              window.ippBlender?.canvas.client !== previous &&
              !!window.ippBlender?.latest,
            previousClient,
          );
        } finally {
          await previousClient.dispose();
        }
        const reconnected = await capture("fresh-runtime-session");
        assert.equal(reconnected.exportSession, removed.exportSession);
        assert.equal(
          reconnected.inspection.entities.length,
          removed.inspection.entities.length,
        );
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "object-removed",
              "fresh-runtime-session",
            ])
          ).changedPixels,
          0,
        );
        const litPanel = await capture(
          "textured-principled",
          await blender.command({ action: "textured_pbr" }),
        );
        assert.ok(
          entity(litPanel.inspection, "fixture-panel").effective.some(
            (entry) => "roughness" in entry.fields,
          ),
        );
        await capture(
          "textured-principled-dark",
          await blender.command({ action: "lighting", intensity: 0 }),
        );
        const darkColors = await call<ReturnType<typeof Fixture.colorCounts>>(
          "colorCounts",
          ["textured-principled-dark"],
        );
        assert.ok(
          darkColors.red + darkColors.green + darkColors.blue < 10,
          "The image panel must go dark without illumination, not remain unlit",
        );
        await capture(
          "world-ambient-only",
          await blender.command({
            action: "ambient",
            color: [1.0, 0.25, 0.125],
            strength: 1.0,
          }),
        );
        const ambientColors = await call<
          ReturnType<typeof Fixture.colorCounts>
        >("colorCounts", ["world-ambient-only"]);
        assert.ok(
          ambientColors.red > darkColors.red + 40,
          "Blender World background must light the textured PBR panel without direct lights",
        );
        await capture(
          "world-ambient-disabled",
          await blender.command({ action: "ambient", color: [0, 0, 0] }),
        );
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "textured-principled-dark",
              "world-ambient-disabled",
            ])
          ).changedPixels,
          0,
        );
        await capture(
          "textured-principled-restored",
          await blender.command({ action: "lighting", restore: true }),
        );

        const correctedControllers = await call<
          ReturnType<typeof Fixture.correctPartialControllerRevision>
        >("correctPartialControllerRevision");
        assert.equal(correctedControllers.calls, 3);
        assert.equal(correctedControllers.controllers, 2);
        assert.equal(correctedControllers.sameSession, true);
        assert.equal(
          correctedControllers.correctedRevision,
          correctedControllers.previousRevision + 1,
        );

        await blender.stop();
        await environment.page.waitForFunction(() =>
          window.ippBlender?.error?.includes("disconnected"),
        );
      } catch (error) {
        await environment.page
          .screenshot({
            path: resolve(environment.evidence.directory, "failure.png"),
          })
          .catch(() => {});
        throw error;
      }
    },
  );
});

test("Packed Fox combines vertex and skeletal animation with its original UV texture", {
  timeout: 120_000,
}, async (context) => {
  await runBrowserEnvironment(
    "blender packed fox",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 45_000,
      closeTimeoutMs: 10_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/blender",
      ),
    },
    context.signal,
    async (environment) => {
      const cdp = await environment.page
        .context()
        .newCDPSession(environment.page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: environment.urls.origin,
      });
      const blender = await startBlender(environment, {
        blend: "tests/fixtures/blender/fox.blend",
        fixture: "tests/blender/fox_fixture.py",
      });
      const fragment = new URLSearchParams({
        endpoint: blender.ready.origin,
        token: blender.ready.token,
      });
      await environment.page.goto(
        `${environment.urls.origin}/target/blender-viewer/index.html#${fragment}`,
      );
      const module = `${environment.urls.origin}/target/blender-test/blender-fixture.js`;
      const captured = new Set<string>();
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(environment.page, module, name, args);
      const capture = async (label: string, revision = 0) => {
        const state = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, revision],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          captured,
          label,
          { canvasSelector: "canvas" },
        );
        await environment.evidence.writeJson(`${label}-inspection.json`, state);
        assert.equal(state.inspection.renderDiagnostics.length, 0);
        return state;
      };
      try {
        const fox = await capture("fox-packed-texture");
        assert.ok(fox.summary.foregroundPixels > 1000);
        assert.equal(fox.triangles, 576);
        for (const kind of [1, 2, 3, 4, 5, 10])
          assert.ok(
            fox.inspection.resources.some(
              (resource) =>
                resource.kind === kind && resource.status === "loaded",
            ),
          );
        assert.equal(fox.inspection.controllers?.length, 2);
        await environment.page
          .getByRole("button", { name: "Play", exact: true })
          .click();
        await environment.page.waitForFunction(async () =>
          (
            await window.ippBlender?.canvas.client.inspect()
          )?.controllers?.every((player) => player.state === "playing"),
        );
        await environment.page
          .getByRole("button", { name: "Pause", exact: true })
          .click();
        await environment.page.waitForFunction(async () =>
          (
            await window.ippBlender?.canvas.client.inspect()
          )?.controllers?.every((player) => player.state === "paused"),
        );
        await call("stopPlayers");
        await call("seek", ["fox-root", 0]);
        await capture("fox-survey-start");
        await call("seek", ["fox-root", 1]);
        await capture("fox-survey-one-second");
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "fox-survey-start",
              "fox-survey-one-second",
            ])
          ).changedPixels > 100,
          "Fox's actual 24-joint Survey action must change its textured silhouette",
        );
        await call("seek", ["fox-fox", 1]);
        const combined = await capture("fox-skeleton-and-mesh");
        const poseFields = entity(combined.inspection, "fox").effective.find(
          (value) => "source" in value.fields && "weight" in value.fields,
        )!.fields;
        assert.equal(poseFields.weight, 1);
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "fox-survey-one-second",
              "fox-skeleton-and-mesh",
            ])
          ).changedPixels > 100,
          "Mesh animation must deform the already animated Fox",
        );
        await call("seek", ["fox-root", 0]);
        await capture("fox-mesh-only");
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "fox-mesh-only",
              "fox-skeleton-and-mesh",
            ])
          ).changedPixels > 100,
          "Skinning must still move the morphed Fox",
        );
        await call("seek", ["fox-root", 0.5]);
        await call("seek", ["fox-fox", 0.5]);
        const midpoint = await capture("fox-combined-midpoint");
        await call("restoreContext");
        const recovered = await capture("fox-combined-recovered");
        assert.ok(recovered.contextGeneration > midpoint.contextGeneration);
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "fox-combined-midpoint",
              "fox-combined-recovered",
            ])
          ).changedPixels,
          0,
        );
        await call("stopPlayers");
        await capture(
          "fox-authored-combined",
          await blender.command({ action: "frame", frame: 12 }),
        );
        assert.equal(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "fox-combined-midpoint",
              "fox-authored-combined",
            ])
          ).changedPixels,
          0,
          "Authored and independently controlled animation times must agree",
        );
        // Blender recomputes deformed smooth normals; IPP interpolates supplied
        // endpoint normals before skinning. Isolate geometry/UV parity here.
        await capture(
          "fox-authored-unlit",
          await blender.command({ action: "unlit_comparison" }),
        );
        await capture(
          "fox-blender-baked",
          await blender.command({ action: "bake_current" }),
        );
        const bakedDifference = await call<ReturnType<typeof Fixture.compare>>(
          "compare",
          ["fox-authored-unlit", "fox-blender-baked"],
        );
        await environment.evidence.writeJson(
          "fox-baked-comparison.json",
          bakedDifference,
        );
        assert.ok(
          bakedDifference.changedPixels < 50,
          `Combined runtime deformation differs from Blender's evaluated mesh: ${bakedDifference.changedPixels} pixels`,
        );
      } catch (error) {
        await environment.page
          .screenshot({
            path: resolve(environment.evidence.directory, "failure.png"),
          })
          .catch(() => {});
        throw error;
      }
    },
  );
});

test("Blender hierarchy and shape keyframes preserve poses through live edits and recovery", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "blender hierarchy mesh poses",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 45_000,
      closeTimeoutMs: 10_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/blender",
      ),
    },
    context.signal,
    async (environment) => {
      const cdp = await environment.page
        .context()
        .newCDPSession(environment.page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: environment.urls.origin,
      });
      const blender = await startBlender(environment, {
        fixture: "tests/blender/hierarchy_pose_fixture.py",
      });
      const fragment = new URLSearchParams({
        endpoint: blender.ready.origin,
        token: blender.ready.token,
      });
      await environment.page.goto(
        `${environment.urls.origin}/target/blender-viewer/index.html#${fragment}`,
      );
      const module = `${environment.urls.origin}/target/blender-test/blender-fixture.js`;
      const captured = new Set<string>();
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(environment.page, module, name, args);
      const capture = async (label: string, revision = 0) => {
        const state = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, revision],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          captured,
          label,
          { canvasSelector: "canvas" },
        );
        await environment.evidence.writeJson(`${label}-inspection.json`, state);
        assert.deepEqual(state.inspection.renderDiagnostics, []);
        assert.deepEqual(state.diagnostics, []);
        return state;
      };
      const changed = async (a: string, b: string) =>
        (await call<ReturnType<typeof Fixture.compare>>("compare", [a, b]))
          .changedPixels;
      const parentOf = (state: Inspection, name: string) =>
        entity(state, name).base.find((value) => "parent" in value.fields)
          ?.fields.parent;
      const meshPose = (state: Inspection) =>
        entity(state, "fixture-panel").effective.find(
          (value) => "weight" in value.fields && "source" in value.fields,
        )!.fields;
      try {
        const initial = await capture("hierarchy-initial");
        assert.ok(initial.summary.foregroundPixels > 1000);
        assert.equal(
          initial.triangles,
          82,
          "Both material parts must actually render",
        );
        const parent = entity(initial.inspection, "fixture-parent").id;
        const cube = entity(initial.inspection, "fixture-cube").id;
        const panel = entity(initial.inspection, "fixture-panel").id;
        assert.equal(parentOf(initial.inspection, "fixture-cube"), parent);
        assert.equal(parentOf(initial.inspection, "fixture-panel"), parent);
        assert.equal(
          parentOf(initial.inspection, "fixture-cube [material 0]"),
          cube,
        );
        assert.equal(
          parentOf(initial.inspection, "fixture-cube [material 1]"),
          cube,
        );
        const baseSource = field(
          entity(initial.inspection, "fixture-panel"),
          "source",
          "MeshInstance",
        );
        const targetSource = meshPose(initial.inspection).source;

        await call("seek", ["fixture-parent", 0.5]);
        await capture("parent-animation");
        assert.ok(
          (await changed("hierarchy-initial", "parent-animation")) > 100,
        );
        await call("stopPlayers");
        await call("seek", ["fixture-panel", 0]);
        await capture("shape-start");
        await call("seek", ["fixture-panel", 0.5]);
        const midpoint = await capture("shape-midpoint");
        assert.equal(meshPose(midpoint.inspection).weight, 0.5);
        assert.ok((await changed("shape-start", "shape-midpoint")) > 100);
        await call("seek", ["fixture-panel", 1]);
        const endpoint = await capture("shape-endpoint");
        assert.equal(meshPose(endpoint.inspection).weight, 1);
        assert.ok((await changed("shape-midpoint", "shape-endpoint")) > 100);
        await call("restoreContext");
        const recovered = await capture("shape-context-restored");
        assert.ok(recovered.contextGeneration > endpoint.contextGeneration);
        assert.equal(
          await changed("shape-endpoint", "shape-context-restored"),
          0,
        );
        await call("stopPlayers");
        const edited = await capture(
          "shape-edited",
          await blender.command({ action: "shape_edit" }),
        );
        assert.equal(entity(edited.inspection, "fixture-panel").id, panel);
        assert.equal(
          field(
            entity(edited.inspection, "fixture-panel"),
            "source",
            "MeshInstance",
          ),
          baseSource,
        );
        assert.notEqual(meshPose(edited.inspection).source, targetSource);

        const basisEdited = await capture(
          "shape-basis-edited",
          await blender.command({ action: "shape_basis_edit" }),
        );
        assert.notEqual(
          field(
            entity(basisEdited.inspection, "fixture-panel"),
            "source",
            "MeshInstance",
          ),
          baseSource,
        );
        assert.equal(
          meshPose(basisEdited.inspection).source,
          meshPose(edited.inspection).source,
        );
        await call("seek", ["fixture-panel", 0.5]);
        const rebound = await capture("shape-after-basis-edit");
        assert.equal(
          meshPose(rebound.inspection).weight,
          0.5,
          "Endpoint edits preserve the weight animation binding",
        );
        await call("stopPlayers");

        // Compare a rendered midpoint with an independently evaluated Blender mesh.
        await capture(
          "authored-midpoint",
          await blender.command({ action: "frame", frame: 13 }),
        );
        const baked = await capture(
          "blender-baked-midpoint",
          await blender.command({ action: "bake_shape" }),
        );
        assert.equal(
          await changed("authored-midpoint", "blender-baked-midpoint"),
          0,
        );
        assert.equal(
          entity(baked.inspection, "fixture-panel").base.some(
            (value) => "weight" in value.fields,
          ),
          false,
        );

        const reparented = await capture(
          "reparented",
          await blender.command({ action: "reparent" }),
        );
        assert.equal(parentOf(reparented.inspection, "fixture-cube"), panel);
        assert.equal(entity(reparented.inspection, "fixture-cube").id, cube);
        assert.ok(
          (await changed("blender-baked-midpoint", "reparented")) > 100,
        );
        await capture(
          "parent-restored",
          await blender.command({ action: "reparent", to_parent: true }),
        );
        const reversed = await capture(
          "parent-chain-reversed",
          await blender.command({ action: "reverse_parent" }),
        );
        assert.equal(parentOf(reversed.inspection, "fixture-parent"), panel);
        assert.equal(parentOf(reversed.inspection, "fixture-panel"), undefined);
        const removed = await capture(
          "parent-deleted",
          await blender.command({ action: "remove_parent" }),
        );
        assert.equal(parentOf(removed.inspection, "fixture-cube"), undefined);
        assert.equal(entity(removed.inspection, "fixture-cube").id, cube);
        assert.equal(
          removed.inspection.entities.some((value) => value.id === parent),
          false,
        );
      } catch (error) {
        await environment.page
          .screenshot({
            path: resolve(environment.evidence.directory, "failure.png"),
          })
          .catch(() => {});
        throw error;
      }
    },
  );
});

function entity(inspection: Inspection, id: string): EntitySnapshot {
  const entity = inspection.entities.find(
    (value) => value.metadata.symbolicId === id,
  );
  assert.ok(entity, `exported entity ${id} must exist`);
  return entity;
}

function field(
  entity: EntitySnapshot,
  name: string,
  component: string,
  effective = false,
) {
  // Field shapes distinguish the small relevant components without hard-coded target IDs.
  const fields = (effective ? entity.effective : entity.base).map(
    (value) => value.fields,
  );
  const selected = fields.find((value) =>
    component === "Transform"
      ? "qx" in value
      : "source" in value &&
        !("target" in value) &&
        !("skeleton" in value) &&
        !("pose_source" in value) &&
        !("weight" in value),
  );
  assert.ok(selected && name in selected, `${component}.${name} must exist`);
  return selected[name];
}

test("Blender preserves larger scenes, images and long actions through the real viewer", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "blender capacity fixture",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 60_000,
      closeTimeoutMs: 10_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/blender",
      ),
    },
    context.signal,
    async (environment) => {
      const cdp = await environment.page
        .context()
        .newCDPSession(environment.page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: environment.urls.origin,
      });
      const blender = await startBlender(environment, {
        fixture: "tests/blender/capacity_fixture.py",
      });
      try {
        const fragment = new URLSearchParams({
          endpoint: blender.ready.origin,
          token: blender.ready.token,
        });
        await environment.page.goto(
          `${environment.urls.origin}/target/blender-viewer/index.html#${fragment}`,
        );
        const module = `${environment.urls.origin}/target/blender-test/blender-fixture.js`;
        const call = <T>(name: string, args: unknown[] = []) =>
          invoke<T>(environment.page, module, name, args);
        const initial = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          ["capacity-initial"],
        );
        await recordCapture(
          environment.page,
          module,
          environment.evidence.directory,
          new Set(),
          "capacity-initial",
          { canvasSelector: "canvas" },
        );
        await environment.evidence.writeJson(
          "capacity-inspection.json",
          initial,
        );
        assert.ok(initial.inspection.entities.length > 300);
        assert.ok(initial.drawCalls >= 300);
        assert.ok(initial.summary.foregroundPixels > 1000);
        assert.equal(initial.inspection.renderDiagnostics.length, 0);
        assert.equal(initial.diagnostics.length, 0);
        assert.ok(
          initial.inspection.resources.every(
            (resource) => resource.status === "loaded",
          ),
        );
        await call("seek", ["fixture-rig", 10]);
        await call("capture", ["capacity-late-pose"]);
        assert.ok(
          (
            await call<ReturnType<typeof Fixture.compare>>("compare", [
              "capacity-initial",
              "capacity-late-pose",
            ])
          ).changedPixels > 100,
        );
        const revision = await blender.command({ action: "transform", x: 0.7 });
        const changed = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          ["capacity-edited", revision],
        );
        assert.ok(
          Math.abs(
            Number(
              field(
                entity(changed.inspection, "fixture-cube"),
                "x",
                "Transform",
              ),
            ) - 0.7,
          ) < 1e-6,
        );
      } finally {
        await blender.stop();
      }
    },
  );
});
