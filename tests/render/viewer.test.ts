import { invoke, writeDataUrl } from "./evidence.js";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import test from "node:test";
import type { ConsoleMessage, Page } from "playwright";
import {
  VIEWER_MESH_SOURCES,
  VIEWER_TEXTURE_SOURCE,
  type ViewerObservation,
} from "./viewer-observation.js";
import type { EntitySnapshot } from "@ipp/client";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import {
  type ImageDifference,
  requireBlank,
  requireShiftedAndScaled,
  requireVisible,
} from "./image-assertions.js";
import type { ViewerBrowserCapture } from "./viewer-browser-helper.js";
import type {
  PlaneUvProbe,
  PlaneUvProbeEvidence,
} from "./viewer-browser-helper.js";

const workspace = resolve(process.cwd());
const helperUrlPath = "/target/gallery-fixtures/viewer-browser-helper.js";

interface PlanePixelEvidence {
  readonly surfacePixels: number;
  readonly arrowPixels: number;
  readonly surfaceBottom: number;
  readonly arrowBottom: number;
}

const UV_GRID_SIZE = 512;
const UV_GRID_CELLS = 8;

test("ReactDOM gallery controls drive the custom scene root and rendered pixels", {
  timeout: 60_000,
}, async (context) => {
  const errors: string[] = [];
  const result = await runBrowserEnvironment(
    "ReactDOM shape gallery controls",
    {
      workspace,
      build: browserBuild("render-expanded"),
      mismatchBuild: browserBuild("headless"),
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-viewer",
      ),
    },
    context.signal,
    async (scenario) => {
      scenario.page.on("pageerror", (error) => errors.push(error.message));
      scenario.page.on("console", (message) =>
        recordConsoleError(message, errors),
      );
      await scenario.page.goto(
        `${scenario.url}/examples/world-gallery/index.html`,
        {
          waitUntil: "load",
        },
      );
      const helperUrl = `${scenario.url}${helperUrlPath}`;
      const galleryObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "waitForViewer",
      );

      assert.equal(
        await scenario.page.locator("#mesh-select").inputValue(),
        "gallery",
      );
      assert.equal(
        await scenario.page.locator("#finish-checker").isChecked(),
        true,
      );
      assert.equal(
        await scenario.page.locator("#finish-checker").isDisabled(),
        true,
      );
      assertGallery(galleryObservation);

      const gallery = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "gallery",
      );
      requireVisible(gallery.summary, "default shape gallery");
      assert.ok(gallery.summary.bounds);
      const galleryMargin = 8;
      assert.ok(gallery.summary.bounds.left > galleryMargin);
      assert.ok(gallery.summary.bounds.top > galleryMargin);
      assert.ok(
        gallery.summary.bounds.right < gallery.frame.width - galleryMargin,
      );
      assert.ok(
        gallery.summary.bounds.bottom < gallery.frame.height - galleryMargin,
      );
      assert.equal(gallery.frame.drawCalls, 12);
      assert.equal(gallery.frame.triangles, 11_246);
      assertViewerProgramCounts(gallery, 3, 3);

      await scenario.page.locator("#mesh-select").selectOption("plane");
      await scenario.page.locator("#finish-solid").check();
      await scenario.page.locator("#override").uncheck();
      assert.equal(
        await scenario.page.locator("#mesh-select").inputValue(),
        "plane",
      );
      assert.equal(
        await scenario.page.locator("#finish-solid").isChecked(),
        true,
      );
      const planeObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const planeEntity = requireCompleteReactEntity(planeObservation, false);
      assertEffectiveMesh(
        planeObservation,
        planeEntity,
        VIEWER_MESH_SOURCES.plane,
      );
      assertComponentAbsent(planeObservation, planeEntity, "UnlitTexture");
      const planeTransform = numericFields(
        effectiveFields(planeObservation, planeEntity, "Transform"),
      );
      const planeQx = Number(planeTransform.qx);
      const planeQw = Number(planeTransform.qw);
      assert.ok(planeQx > 0.38 && planeQx < 0.39);
      assert.ok(planeQw > 0.92 && planeQw < 0.93);
      const plane = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "plane-solid",
      );
      requireVisible(plane.summary, "filled plane and normal arrow");
      assert.equal(plane.frame.drawCalls, 1);
      assert.equal(plane.frame.triangles, 50);
      const planePixels = await analyzePlanePixels(
        scenario.page,
        helperUrl,
        "plane-solid",
      );
      assert.ok(
        planePixels.surfacePixels > 1_000,
        "plane should expose its linear-grey square",
      );
      assert.ok(
        planePixels.arrowPixels > 100,
        "plane should expose its contrasting white normal arrow",
      );
      assert.ok(
        planePixels.arrowBottom > planePixels.surfaceBottom + 5,
        "the rotated +Z arrow should project below the square",
      );

      await scenario.page.locator("#finish-checker").check();
      const checkerPlaneObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const checkerPlaneEntity = requireCompleteReactEntity(
        checkerPlaneObservation,
        true,
      );
      assertEffectiveMesh(
        checkerPlaneObservation,
        checkerPlaneEntity,
        VIEWER_MESH_SOURCES.plane,
      );
      const checkerPlane = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "plane-checker-solid-arrow",
      );
      const checkerPlanePixels = await analyzePlanePixels(
        scenario.page,
        helperUrl,
        "plane-checker-solid-arrow",
      );
      const uvGridProbes = [
        uvGridProbe(1, 1, 16, 16, [73, 73, 96], "cell background"),
        uvGridProbe(1, 1, 2, 16, [16, 16, 16], "cell border", true),
        uvGridProbe(1, 1, 39, 87, [240, 240, 240], "triangle light half"),
        uvGridProbe(1, 1, 75, 87, [16, 16, 16], "triangle dark half"),
        uvGridProbe(1, 1, 39, 40, [73, 73, 96], "above triangle apex"),
        uvGridProbe(6, 5, 16, 16, [198, 173, 192], "second-axis background"),
      ];
      const uvGridEvidence = await samplePlaneUvPixels(
        scenario.page,
        helperUrl,
        "plane-checker-solid-arrow",
        uvGridProbes,
      );
      assertUvGridProbes(uvGridEvidence, uvGridProbes);
      assert.ok(
        checkerPlanePixels.arrowPixels > 100,
        "checker plane should retain a solid white normal arrow",
      );
      assert.ok(
        checkerPlanePixels.arrowBottom >= planePixels.arrowBottom - 3,
        "checker plane should retain the projected normal tip",
      );
      const planeTextureDifference = await compare(
        scenario.page,
        helperUrl,
        "plane-solid",
        "plane-checker-solid-arrow",
      );
      assert.ok(planeTextureDifference.changedFraction > 0.01);
      // Each finish releases the previous recipe; only the active plane program
      // remains resident after switching from solid to textured.
      assertViewerProgramCounts(checkerPlane, 5, 1);

      await scenario.page.locator("#mesh-select").selectOption("planeOutline");
      const planeOutlineObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const planeOutlineEntity = requireCompleteReactEntity(
        planeOutlineObservation,
        false,
      );
      assertEffectiveMesh(
        planeOutlineObservation,
        planeOutlineEntity,
        VIEWER_MESH_SOURCES.planeOutline,
      );
      const planeOutline = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "plane-outline",
      );
      assert.ok(
        planeOutline.summary.foregroundPixels > 150,
        "plane perimeter and normal arrow should produce visible pixels",
      );
      assert.equal(planeOutline.frame.drawCalls, 1);
      assert.equal(planeOutline.frame.triangles, 176);
      assert.ok(planeOutline.summary.bounds);
      assert.ok(
        planeOutline.summary.bounds.bottom >= planePixels.arrowBottom - 3,
        "outlined plane should retain the normal arrow tip",
      );
      const planeFinishDifference = await compare(
        scenario.page,
        helperUrl,
        "plane-solid",
        "plane-outline",
      );
      assert.ok(planeFinishDifference.changedFraction > 0.01);

      await scenario.page.locator("#mesh-select").selectOption("sphereOutline");
      assert.equal(
        await scenario.page.locator("#mesh-select").inputValue(),
        "sphereOutline",
      );
      assert.equal(
        await scenario.page.locator("#finish-solid").isChecked(),
        true,
      );
      const outlineObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const outlineEntity = requireCompleteReactEntity(
        outlineObservation,
        false,
      );
      assertEffectiveMesh(
        outlineObservation,
        outlineEntity,
        VIEWER_MESH_SOURCES.sphereOutline,
      );
      const outline = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "sphere-outline",
      );
      assert.ok(
        outline.summary.foregroundPixels > 150,
        "sphere outline should produce visible pixels",
      );
      assert.equal(outline.frame.drawCalls, 1);

      await scenario.page.locator("#mesh-select").selectOption("sphere");
      await scenario.page.locator("#override").check();
      await scenario.page.locator("#finish-checker").check();
      await scenario.page.locator("#color").fill("#4c7eff");
      assert.equal(
        await scenario.page.locator("#finish-checker").isChecked(),
        true,
      );
      assert.equal(
        await scenario.page.locator("#color").inputValue(),
        "#4c7eff",
      );
      const checkerObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const checkerEntity = requireCompleteReactEntity(
        checkerObservation,
        true,
      );
      assertEffectiveMesh(
        checkerObservation,
        checkerEntity,
        VIEWER_MESH_SOURCES.sphere,
      );
      assert.deepEqual(
        effectiveFields(checkerObservation, checkerEntity, "UnlitTexture"),
        { source: VIEWER_TEXTURE_SOURCE, variant: 0 },
      );
      const checker = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "sphere-checker",
      );
      requireVisible(checker.summary, "checker-textured sphere");
      assert.equal(checker.frame.drawCalls, 1);
      const finishDifference = await compare(
        scenario.page,
        helperUrl,
        "sphere-outline",
        "sphere-checker",
      );
      assert.ok(finishDifference.changedFraction > 0.003);

      await scenario.page.locator("#mounted").uncheck();
      const unmounted = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      assert.equal(galleryWorldEntities(unmounted).length, 0);
      const blank = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "gallery-unmounted",
      );
      requireBlank(blank.summary, "ReactDOM-unmounted gallery frame");
      assert.equal(blank.frame.drawCalls, 0);

      await scenario.page.locator("#mounted").check();
      const remountedObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const remountedEntity = requireCompleteReactEntity(
        remountedObservation,
        true,
      );
      assert.notEqual(remountedEntity.id, checkerEntity.id);
      const remounted = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "gallery-remounted",
      );
      requireVisible(remounted.summary, "ReactDOM-remounted checker sphere");
      assert.deepEqual(errors, []);

      return {
        galleryTick: gallery.frame.tick,
        remountedTick: remounted.frame.tick,
        firstEntity: checkerEntity.id,
        remountedEntity: remountedEntity.id,
      };
    },
  );

  assert.ok(result.value.remountedTick > result.value.galleryTick);
  assert.notEqual(result.value.firstEntity, result.value.remountedEntity);
  await assertLoopbackClosed(result.origin);
});

test("production declarative scene gallery owns controls and entity lifecycle", {
  timeout: 60_000,
}, async (context) => {
  const errors: string[] = [];
  let releaseWasm!: () => void;
  const wasmGate = new Promise<void>((resolvePromise) => {
    releaseWasm = resolvePromise;
  });
  const result = await runBrowserEnvironment(
    "production declarative scene gallery",
    {
      workspace,
      build: browserBuild("render-expanded"),
      mismatchBuild: browserBuild("headless"),
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      beforeArtifactResponse: async (url) => {
        if (
          url.pathname === "/target/browser-build/render-expanded/runtime.wasm"
        ) {
          await wasmGate;
        }
      },
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-viewer",
      ),
    },
    context.signal,
    async (scenario) => {
      scenario.page.on("pageerror", (error) => errors.push(error.message));
      scenario.page.on("console", (message) =>
        recordConsoleError(message, errors),
      );
      const wasmRequest = scenario.page.waitForRequest(scenario.urls.wasm, {
        timeout: 5_000,
      });
      try {
        await scenario.page.goto(
          `${scenario.url}/examples/world-gallery/index.html`,
          {
            waitUntil: "load",
          },
        );
        await wasmRequest;
        assert.equal(
          await scenario.page.evaluate(
            () => window.ippWorldCanvas === undefined,
          ),
          true,
        );
        await scenario.page.locator("#mesh-select").selectOption("cube");
      } finally {
        releaseWasm();
      }
      const helperUrl = `${scenario.url}${helperUrlPath}`;
      await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "waitForViewer",
      );
      assert.equal(
        await scenario.page.locator("#mesh-select").inputValue(),
        "cube",
      );
      assert.equal(
        await scenario.page.locator("#finish-checker").isChecked(),
        true,
      );
      const initialObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const initialEntity = requireCompleteReactEntity(
        initialObservation,
        true,
      );
      assert.deepEqual(
        effectiveFields(initialObservation, initialEntity, "UnlitTexture"),
        { source: VIEWER_TEXTURE_SOURCE, variant: 0 },
      );

      const initial = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "initial",
      );
      requireVisible(initial.summary, "initial viewer cube");
      assert.equal(initial.frame.drawCalls, 1);
      assert.equal(initial.frame.triangles, 12);

      await scenario.page.locator("#finish-solid").check();
      const solidObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const solidEntity = requireCompleteReactEntity(solidObservation, false);
      assertComponentAbsent(solidObservation, solidEntity, "UnlitTexture");
      const solid = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "checker-off",
      );
      requireVisible(solid.summary, "untextured viewer cube");
      assert.equal(solid.frame.drawCalls, 1);
      assert.equal(solid.frame.triangles, 12);
      const checkerRemovedDifference = await compare(
        scenario.page,
        helperUrl,
        "initial",
        "checker-off",
      );
      assert.ok(checkerRemovedDifference.changedFraction > 0.005);

      await scenario.page.locator("#color").fill("#ed4932");
      const coloredObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const coloredEntity = requireCompleteReactEntity(
        coloredObservation,
        false,
      );
      const material = effectiveFields(
        coloredObservation,
        coloredEntity,
        "UnlitMaterial",
      );
      assert.ok(Number(material.r) > 0.8);
      assert.ok(Number(material.g) < 0.1);
      assert.ok(Number(material.b) < 0.05);
      const colored = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "material",
      );
      const materialDifference = await compare(
        scenario.page,
        helperUrl,
        "checker-off",
        "material",
      );
      assert.ok(materialDifference.changedFraction > 0.01);

      await scenario.page.locator("#position").fill("1.1");
      await scenario.page.locator("#scale").fill("0.5");
      const movedObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const movedEntity = requireCompleteReactEntity(movedObservation, false);
      const transform = effectiveFields(
        movedObservation,
        movedEntity,
        "Transform",
      );
      assert.equal(rounded(Number(transform.x)), 1.1);
      assert.equal(rounded(Number(transform.sx)), 0.5);
      assert.equal(rounded(Number(transform.sy)), 0.5);
      assert.equal(rounded(Number(transform.sz)), 0.5);
      const moved = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "moved-scaled",
      );
      requireShiftedAndScaled(colored.summary, moved.summary);

      await scenario.page.locator("#override").uncheck();
      const defaultObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const defaultEntity = requireCompleteReactEntity(
        defaultObservation,
        false,
      );
      assert.deepEqual(
        numericFields(
          effectiveFields(defaultObservation, defaultEntity, "UnlitMaterial"),
        ),
        { r: 1, g: 1, b: 1 },
      );
      await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "default-material",
      );
      const defaultDifference = await compare(
        scenario.page,
        helperUrl,
        "moved-scaled",
        "default-material",
      );
      assert.ok(defaultDifference.changedFraction > 0.005);

      await scenario.page.locator("#finish-checker").check();
      const texturedObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const texturedEntity = requireCompleteReactEntity(
        texturedObservation,
        true,
      );
      assert.deepEqual(
        effectiveFields(texturedObservation, texturedEntity, "UnlitTexture"),
        { source: VIEWER_TEXTURE_SOURCE, variant: 0 },
      );
      await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "checker-restored",
      );
      const checkerRestoredDifference = await compare(
        scenario.page,
        helperUrl,
        "default-material",
        "checker-restored",
      );
      assert.ok(checkerRestoredDifference.changedFraction > 0.005);

      await scenario.page.locator("#mounted").uncheck();
      const unmounted = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      assert.equal(galleryWorldEntities(unmounted).length, 0);
      const blank = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "unmounted",
      );
      requireBlank(blank.summary, "unmounted viewer frame");
      assert.equal(blank.frame.drawCalls, 0);
      assert.equal(blank.frame.triangles, 0);

      await scenario.page.locator("#override").check();
      await scenario.page.locator("#color").fill("#4c7eff");
      await scenario.page.locator("#finish-solid").check();
      await scenario.page.locator("#position").fill("-0.8");
      await scenario.page.locator("#scale").fill("0.7");
      const editedWhileUnmounted = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      assert.equal(galleryWorldEntities(editedWhileUnmounted).length, 0);
      assert.equal(editedWhileUnmounted.session, initialObservation.session);
      const editedBlank = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "unmounted-edited",
      );
      requireBlank(editedBlank.summary, "viewer frame after unmounted edits");
      assert.equal(editedBlank.frame.drawCalls, 0);
      assert.equal(editedBlank.frame.triangles, 0);

      await scenario.page.locator("#mounted").check();
      const remountedObservation = await invoke<ViewerObservation>(
        scenario.page,
        helperUrl,
        "observeViewer",
      );
      const remountedEntity = requireCompleteReactEntity(
        remountedObservation,
        false,
      );
      assert.notEqual(remountedEntity.id, initialEntity.id);
      assert.equal(remountedObservation.session, initialObservation.session);
      assertEffectiveMesh(
        remountedObservation,
        remountedEntity,
        VIEWER_MESH_SOURCES.cube,
      );
      assertComponentAbsent(
        remountedObservation,
        remountedEntity,
        "UnlitTexture",
      );
      const remountedMaterial = effectiveFields(
        remountedObservation,
        remountedEntity,
        "UnlitMaterial",
      );
      assert.ok(Number(remountedMaterial.r) > 0.05);
      assert.ok(Number(remountedMaterial.r) < 0.1);
      assert.ok(Number(remountedMaterial.g) > 0.2);
      assert.ok(Number(remountedMaterial.g) < 0.25);
      assert.ok(Number(remountedMaterial.b) > 0.95);
      const remountedTransform = numericFields(
        effectiveFields(remountedObservation, remountedEntity, "Transform"),
      );
      assert.equal(rounded(remountedTransform.x ?? 0), -0.8);
      assert.equal(rounded(remountedTransform.sx ?? 0), 0.7);
      assert.equal(rounded(remountedTransform.sy ?? 0), 0.7);
      assert.equal(rounded(remountedTransform.sz ?? 0), 0.7);
      const remounted = await capture(
        scenario.page,
        helperUrl,
        scenario.evidence.directory,
        "remounted",
      );
      requireVisible(remounted.summary, "remounted viewer cube");
      assert.equal(remounted.frame.drawCalls, 1);
      assert.equal(remounted.frame.triangles, 12);

      assert.deepEqual(errors, []);
      return {
        firstEntity: initialEntity.id,
        remountedEntity: remountedEntity.id,
        firstTick: initial.frame.tick,
        finalTick: remounted.frame.tick,
      };
    },
  );

  assert.notEqual(result.value.firstEntity, result.value.remountedEntity);
  assert.ok(result.value.finalTick > result.value.firstTick);
  await assertLoopbackClosed(result.origin);
});

test("geometry dropdown edits every recipe and preserves independent mesh settings", {
  timeout: 90_000,
}, async (context) => {
  const errors: string[] = [];
  const result = await runBrowserEnvironment(
    "editable geometry catalog",
    {
      workspace,
      build: browserBuild("render-expanded"),
      mismatchBuild: browserBuild("headless"),
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-viewer",
      ),
    },
    context.signal,
    async (scenario) => {
      const page = scenario.page;
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => recordConsoleError(message, errors));
      await page.goto(`${scenario.url}/examples/world-gallery/index.html`, {
        waitUntil: "load",
      });
      const helper = `${scenario.url}${helperUrlPath}`;
      await invoke(page, helper, "waitForViewer");
      const cases = [
        {
          mesh: "cube",
          triangles: 12,
          params: { width: 1.25, height: 1.5, length: 2.5 },
        },
        { mesh: "sphere", triangles: 960, params: { radius: 0.7 } },
        { mesh: "pill", triangles: 1024, params: { radius: 1.5, height: 3.5 } },
        {
          mesh: "plane",
          triangles: 50,
          params: {
            size: 1.5,
            normalLength: 0.8,
            stroke: 0.08,
            normalOffset: 0.4,
          },
        },
        {
          mesh: "cubeOutline",
          triangles: 384,
          params: { width: 0.6, height: 1.25, length: 2.5, stroke: 0.08 },
        },
        {
          mesh: "sphereOutline",
          triangles: 3072,
          params: { radius: 0.6, stroke: 0.08 },
        },
        {
          mesh: "pillOutline",
          triangles: 4160,
          params: { radius: 0.8, height: 2.4, stroke: 0.1 },
        },
        {
          mesh: "planeOutline",
          triangles: 176,
          params: {
            size: 1.5,
            normalLength: 0.8,
            stroke: 0.08,
            normalOffset: 0.4,
          },
        },
        { mesh: "cone", triangles: 64, params: { radius: 0.7, height: 1.5 } },
        {
          mesh: "coneOutline",
          triangles: 1152,
          params: { radius: 0.4, height: 0.5, rings: 16, stroke: 0.01 },
        },
        { mesh: "arrow", triangles: 48, params: { length: 0.7, stroke: 0.04 } },
        { mesh: "axis", triangles: 144, params: { length: 1.6, stroke: 0.08 } },
      ] as const;
      assert.deepEqual(
        await page
          .locator("#mesh-select option")
          .evaluateAll((options) =>
            options.map((option) => (option as HTMLOptionElement).value),
          ),
        ["gallery", ...cases.map(({ mesh }) => mesh)],
      );
      assert.equal(
        await page.locator('[aria-label="Mesh catalog"] li').count(),
        12,
      );
      assert.equal(
        new URL(VIEWER_TEXTURE_SOURCE).searchParams.get("width"),
        "512",
      );
      assert.equal(
        new URL(VIEWER_TEXTURE_SOURCE).searchParams.get("height"),
        "512",
      );
      const overview = await capture(
        page,
        helper,
        scenario.evidence.directory,
        "catalog-before",
      );
      assert.equal(overview.frame.drawCalls, 12);
      const editedSources = new Map<string, string>();
      for (const { mesh, triangles, params } of cases) {
        await page.locator("#mesh-select").selectOption(mesh);
        const original = await capture(
          page,
          helper,
          scenario.evidence.directory,
          `${mesh}-before-edit`,
        );
        assert.equal(original.frame.drawCalls, 1);
        assert.equal(original.frame.triangles, triangles);
        assert.ok(
          original.summary.foregroundPixels > 100,
          `${mesh} renders visible geometry`,
        );
        assert.deepEqual(
          (
            await page
              .locator('.geometry-parameters input[type="range"]')
              .evaluateAll((inputs) => inputs.map((input) => input.id.slice(6)))
          ).sort(),
          Object.keys(params).sort(),
        );
        for (const [name, value] of Object.entries(params)) {
          await page.locator(`#param-${name}`).fill(String(value));
          if (mesh === "pill" && name === "radius") {
            assert.equal(
              Number(await page.locator("#param-height").inputValue()),
              3,
            );
          }
          if (mesh === "coneOutline" && name === "rings") {
            assert.ok(
              Number(await page.locator("#param-stroke").inputValue()) <=
                0.5 / 34,
            );
            const coupled = await capture(
              page,
              helper,
              scenario.evidence.directory,
              "cone-coupled-limits",
            );
            assert.equal(coupled.frame.drawCalls, 1);
            assert.equal(coupled.frame.triangles, 8832);
          }
        }
        const edited = await capture(
          page,
          helper,
          scenario.evidence.directory,
          `${mesh}-after-edit`,
        );
        const entity = galleryWorldEntities(edited)[0]!;
        const source = String(
          effectiveFields(edited, entity, "MeshInstance").source,
        );
        const query = new URL(source).searchParams;
        for (const [name, value] of Object.entries(params))
          assert.equal(
            Number(query.get(name)),
            value,
            `${mesh}.${name} reaches the runtime`,
          );
        assert.equal(edited.frame.drawCalls, 1);
        const difference = await compare(
          page,
          helper,
          `${mesh}-before-edit`,
          `${mesh}-after-edit`,
        );
        assert.ok(
          difference.changedPixels > 20,
          `${mesh} parameter edits change its rendered geometry`,
        );
        editedSources.set(mesh, source);
        await scenario.evidence.writeJson(`${mesh}-edit.json`, {
          source,
          frame: edited.frame,
          difference,
        });
      }
      for (const [axis, color] of ["#ff8000", "#8000ff", "#00ff80"].entries())
        await page.locator(`#axis-color-${axis}`).fill(color);
      const colored = await capture(
        page,
        helper,
        scenario.evidence.directory,
        "axis-colors",
      );
      const axisSource = String(
        effectiveFields(
          colored,
          galleryWorldEntities(colored)[0]!,
          "MeshInstance",
        ).source,
      );
      const axisQuery = new URL(axisSource).searchParams;
      assert.equal(axisQuery.get("xColor"), "1,0.215861,0");
      assert.equal(axisQuery.get("yColor"), "0.215861,0,1");
      assert.equal(axisQuery.get("zColor"), "0,1,0.215861");
      const colors = await invoke<number[]>(page, helper, "countViewerColors", [
        "axis-colors",
        [
          [255, 128, 0],
          [128, 0, 255],
          [0, 255, 128],
        ],
      ]);
      assert.ok(
        colors.every((count) => count > 50),
        "all three edited axis colors reach rendered vertices",
      );
      editedSources.set("axis", axisSource);

      await page.locator("#mesh-select").selectOption("cube");
      assert.equal(
        Number(await page.locator("#param-width").inputValue()),
        1.25,
      );
      await page.locator("#position").fill("0.35");
      await page.locator("#scale").fill("0.85");
      await page.locator("#mesh-select").selectOption("sphere");
      assert.equal(Number(await page.locator("#position").inputValue()), 0);
      assert.equal(Number(await page.locator("#scale").inputValue()), 1);
      await page.locator("#mesh-select").selectOption("cube");
      assert.equal(Number(await page.locator("#position").inputValue()), 0.35);
      assert.equal(Number(await page.locator("#scale").inputValue()), 0.85);
      await page.locator("#mesh-select").selectOption("gallery");
      const after = await capture(
        page,
        helper,
        scenario.evidence.directory,
        "catalog-after",
      );
      assert.equal(after.frame.drawCalls, 12);
      for (const entity of galleryWorldEntities(after)) {
        const mesh = entity.metadata.symbolicId!.slice("react-gallery-".length);
        assertEffectiveMesh(after, entity, editedSources.get(mesh)!);
      }
      assert.ok(
        (await compare(page, helper, "catalog-before", "catalog-after"))
          .changedPixels > 1000,
      );
      await page.locator("#mesh-select").selectOption("axis");
      assert.equal(await page.locator("#axis-color-0").inputValue(), "#ff8000");
      await page.locator("#mounted").uncheck();
      await page.locator("#param-length").fill("1.8");
      const blank = await capture(
        page,
        helper,
        scenario.evidence.directory,
        "geometry-unmounted-edit",
      );
      requireBlank(blank.summary, "geometry edit while scene is unmounted");
      await page.locator("#mounted").check();
      const remounted = await capture(
        page,
        helper,
        scenario.evidence.directory,
        "geometry-remounted",
      );
      const source = String(
        effectiveFields(
          remounted,
          galleryWorldEntities(remounted)[0]!,
          "MeshInstance",
        ).source,
      );
      assert.equal(new URL(source).searchParams.get("length"), "1.8");
      assert.equal(remounted.frame.drawCalls, 1);
      assert.equal(remounted.session, overview.session);
      assert.deepEqual(errors, []);
      return { tick: remounted.frame.tick };
    },
  );
  assert.ok(result.value.tick > 0n);
  await assertLoopbackClosed(result.origin);
});

function browserBuild(
  name: "render-expanded" | "headless",
): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

function assertGallery(observation: ViewerObservation): void {
  const expected = new Map(
    [
      ["cube", true],
      ["sphere", true],
      ["pill", true],
      ["plane", true],
      ["cubeOutline", false],
      ["sphereOutline", false],
      ["pillOutline", false],
      ["planeOutline", false],
      ["cone", true],
      ["coneOutline", false],
      ["arrow", false],
      ["axis", false],
    ].map(([mesh, checker]) => [
      `react-gallery-${mesh}`,
      {
        source: VIEWER_MESH_SOURCES[mesh as keyof typeof VIEWER_MESH_SOURCES],
        checker: Boolean(checker),
      },
    ]),
  );
  assert.equal(galleryWorldEntities(observation).length, expected.size);
  const colors = new Set<string>();
  for (const entity of galleryWorldEntities(observation)) {
    const symbolicId = entity.metadata.symbolicId;
    if (symbolicId === null) throw new Error("Gallery entity is not symbolic");
    const configuration = expected.get(symbolicId);
    assert.ok(
      configuration,
      `Unexpected gallery entity ${entity.metadata.symbolicId}`,
    );
    assertReactEntityComponents(observation, entity, configuration.checker);
    assertEffectiveMesh(observation, entity, configuration.source);
    const material = numericFields(
      effectiveFields(observation, entity, "UnlitMaterial"),
    );
    colors.add(`${material.r}:${material.g}:${material.b}`);
  }
  assert.equal(
    colors.size,
    expected.size - 1,
    "only arrow and axis share a white material",
  );
}

function requireCompleteReactEntity(
  observation: ViewerObservation,
  checker: boolean,
): EntitySnapshot {
  assert.equal(galleryWorldEntities(observation).length, 1);
  const entity = galleryWorldEntities(observation)[0];
  assert.ok(entity);
  assert.equal(entity.metadata.symbolicId, "react-gallery-selection");
  assertReactEntityComponents(observation, entity, checker);
  return entity;
}

function assertReactEntityComponents(
  observation: ViewerObservation,
  entity: EntitySnapshot,
  checker: boolean,
): void {
  assert.deepEqual(entity.metadata.classes, []);
  const expected = [
    observation.componentIds.Transform,
    observation.componentIds.UnlitMaterial,
    observation.componentIds.MeshInstance,
    observation.componentIds.BoundingGeometry,
    ...(checker ? [observation.componentIds.UnlitTexture] : []),
  ].sort((a, b) => a - b);
  // Auto declarations and required bounds do not create producer base state.
  assert.deepEqual(entity.base, []);
  assert.deepEqual(
    entity.effective.map(({ component }) => component).sort((a, b) => a - b),
    expected,
  );
}

function assertEffectiveMesh(
  observation: ViewerObservation,
  entity: EntitySnapshot,
  source: string,
): void {
  assert.deepEqual(effectiveFields(observation, entity, "MeshInstance"), {
    source,
    variant: 0,
  });
}

function assertComponentAbsent(
  observation: ViewerObservation,
  entity: EntitySnapshot,
  name: keyof ViewerObservation["componentIds"],
): void {
  const id = observation.componentIds[name];
  assert.equal(
    entity.base.some(({ component }) => component === id),
    false,
  );
  assert.equal(
    entity.effective.some(({ component }) => component === id),
    false,
  );
}

function effectiveFields(
  observation: ViewerObservation,
  entity: EntitySnapshot,
  name: keyof ViewerObservation["componentIds"],
): EntitySnapshot["effective"][number]["fields"] {
  return componentFields(
    entity.effective,
    observation.componentIds[name],
    name,
  );
}

function componentFields(
  components: EntitySnapshot["base"],
  id: number,
  name: string,
): EntitySnapshot["base"][number]["fields"] {
  const component = components.find((entry) => entry.component === id);
  assert.ok(component, `Missing ${name} component ${id}`);
  return component.fields;
}

function numericFields(
  fields: EntitySnapshot["base"][number]["fields"],
): Readonly<Record<string, number>> {
  return Object.fromEntries(
    Object.entries(fields).map(([name, value]) => {
      assert.ok(!(value instanceof Uint8Array), `${name} must be numeric`);
      return [name, Number(value)];
    }),
  );
}

async function analyzePlanePixels(
  page: Page,
  helperUrl: string,
  label: string,
): Promise<PlanePixelEvidence> {
  return await invoke(page, helperUrl, "analyzePlaneCapture", [label]);
}

interface NamedUvGridProbe extends PlaneUvProbe {
  readonly expectedRgb: readonly [number, number, number];
  readonly name: string;
  readonly allowNeighborhood: boolean;
}

function uvGridProbe(
  cellX: number,
  cellY: number,
  withinX: number,
  withinY: number,
  expectedRgb: readonly [number, number, number],
  name: string,
  allowNeighborhood = false,
): NamedUvGridProbe {
  const cellWidth = UV_GRID_SIZE / UV_GRID_CELLS;
  const sourceX = cellX * cellWidth + (withinX * UV_GRID_SIZE) / 1024;
  const sourceY = cellY * cellWidth + (withinY * UV_GRID_SIZE) / 1024;
  return {
    u: (sourceX + 0.5) / UV_GRID_SIZE,
    v: (sourceY + 0.5) / UV_GRID_SIZE,
    expectedRgb,
    name,
    allowNeighborhood,
  };
}

async function samplePlaneUvPixels(
  page: Page,
  helperUrl: string,
  label: string,
  probes: readonly PlaneUvProbe[],
): Promise<readonly PlaneUvProbeEvidence[]> {
  return await invoke(page, helperUrl, "samplePlaneUvCapture", [label, probes]);
}

function assertUvGridProbes(
  evidence: readonly PlaneUvProbeEvidence[],
  probes: readonly NamedUvGridProbe[],
): void {
  assert.equal(evidence.length, probes.length);
  for (const [index, actual] of evidence.entries()) {
    const probe = probes[index]!;
    const expected = shadePlaneTexture(probe.expectedRgb);
    const samples = probe.allowNeighborhood
      ? actual.neighborhood
      : [actual.rgba];
    assert.ok(
      samples.some((rgba) => maximumDifference(rgba, expected) <= 4),
      `${probe.name} at UV (${probe.u}, ${probe.v}) expected ${expected.join(
        ",",
      )} at capture pixel ${actual.coordinate.join(",")}, received ${samples
        .map((rgba) => rgba.join(","))
        .join("; ")}`,
    );
  }
  const light = shadePlaneTexture(probes[2]!.expectedRgb);
  assert.ok(
    maximumDifference(evidence[3]!.rgba, light) > 4,
    "horizontally reflected light-half UV should expose the dark half",
  );
  assert.ok(
    maximumDifference(evidence[4]!.rgba, light) > 4,
    "vertically reflected light-half UV should expose the triangle background",
  );
}

function shadePlaneTexture(
  rgb: readonly [number, number, number],
): readonly [number, number, number, number] {
  return [
    ...rgb.map((value) => linearToSrgb8(srgb8ToLinear(value) * 0.25)),
    255,
  ] as [number, number, number, number];
}

function srgb8ToLinear(value: number): number {
  const encoded = value / 255;
  return encoded <= 0.04045
    ? encoded / 12.92
    : ((encoded + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb8(value: number): number {
  const encoded =
    value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, encoded)) * 255);
}

function maximumDifference(
  actual: readonly number[],
  expected: readonly number[],
): number {
  return Math.max(
    ...actual.map((value, index) =>
      Math.abs(value - (expected[index] ?? Number.NaN)),
    ),
  );
}

function assertViewerProgramCounts(
  capture: ViewerBrowserCapture,
  created: number,
  live: number,
): void {
  assert.equal(
    viewerBackendNumber(capture.frame.backend, "shaderProgramsCreated"),
    created,
  );
  assert.equal(
    viewerBackendNumber(capture.frame.backend, "shaderProgramsLive"),
    live,
  );
}

function viewerBackendNumber(
  backend: Readonly<Record<string, unknown>>,
  field: string,
): number {
  const value = backend[field];
  if (typeof value !== "number") {
    throw new Error(`viewer capture backend.${field} is not numeric`);
  }
  return value;
}

async function capture(
  page: Page,
  helperUrl: string,
  evidenceDirectory: string,
  label: string,
): Promise<ViewerBrowserCapture> {
  const captured = await invoke<ViewerBrowserCapture>(
    page,
    helperUrl,
    "captureViewer",
    [label],
  );
  await Promise.all([
    writeDataUrl(
      join(evidenceDirectory, `${label}-capture.png`),
      captured.dataUrl,
    ),
    page
      .locator("#ipp-world-canvas")
      .screenshot({ path: join(evidenceDirectory, `${label}-canvas.png`) }),
  ]);
  return captured;
}

async function compare(
  page: Page,
  helperUrl: string,
  first: string,
  second: string,
): Promise<ImageDifference> {
  return await invoke(page, helperUrl, "compareViewerCaptures", [
    first,
    second,
  ]);
}

function recordConsoleError(message: ConsoleMessage, errors: string[]): void {
  if (message.type() === "error") errors.push(message.text());
}

function rounded(value: number): number {
  return Math.round(value * 100) / 100;
}

function galleryWorldEntities(
  observation: ViewerObservation,
): EntitySnapshot[] {
  const cameras = observation.inspection.entities.filter(
    ({ metadata }) => metadata.symbolicId === "gallery-camera",
  );
  assert.equal(
    cameras.length,
    1,
    "Gallery retains exactly one authored camera",
  );
  assert.deepEqual(cameras[0]?.metadata.classes, ["camera"]);
  return observation.inspection.entities.filter(
    ({ metadata }) => metadata.symbolicId !== "gallery-camera",
  );
}
