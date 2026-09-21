import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import { writeFile } from "node:fs/promises";
import test from "node:test";
import type {
  GuiInspectResponse,
  GuiSemanticNode,
  GuiSemanticRole,
  GuiSemanticTree,
  Inspection,
} from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { responseGate } from "../browser/response-gate.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./gallery-driver.js";
import {
  PROJECTOR_MESH_SOURCES,
  PROJECTOR_TEXTURE_SOURCES,
} from "../../examples/world-gallery/worlds/gui/projector.js";

interface GalleryGuiState {
  readonly semantic: GuiSemanticTree;
  readonly detailed: GuiInspectResponse;
  readonly surfaceItems: number;
  readonly entityComponents: readonly (string | undefined)[];
}

interface ProjectedGuiNode {
  readonly clientX: number;
  readonly clientY: number;
  readonly node: GuiSemanticNode;
}

const guiEnvironment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/reviews/gui-demo"),
};

function guiEntity(inspection: Inspection) {
  return inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === "gui-demo",
  );
}

function sceneEntity(inspection: Inspection, symbolicId: string) {
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === symbolicId,
  );
  assert.ok(entity, `missing scene entity ${symbolicId}`);
  return entity;
}

function fieldsWith(inspection: Inspection, symbolicId: string, field: string) {
  const fields = sceneEntity(inspection, symbolicId).effective.find(
    (entry) => field in entry.fields,
  )?.fields;
  assert.ok(fields, `${symbolicId} has no effective ${field} field`);
  return fields;
}

function dynamicProperty(
  inspection: Inspection,
  symbolicId: string,
  property: string,
) {
  const value = sceneEntity(inspection, symbolicId).effective.find(
    (entry) => entry.properties && property in entry.properties,
  )?.properties?.[property];
  assert.ok(value, `${symbolicId} has no effective ${property} property`);
  return value;
}

function animationFor(inspection: Inspection, symbolicId: string) {
  const target = sceneEntity(inspection, symbolicId).id;
  const controller = inspection.controllers?.find((candidate) =>
    candidate.description.drivers.some((driver) => driver.target === target),
  );
  assert.ok(controller, `missing animation for ${symbolicId}`);
  return controller;
}

function waveformAnimation(inspection: Inspection, pulse = false) {
  const target = sceneEntity(inspection, "gui-demo").id;
  const controller = inspection.controllers?.find(
    ({ description }) =>
      description.drivers.length === 1 &&
      description.looping === !pulse &&
      description.drivers.every((driver) => driver.target === target) &&
      description.drivers.some((driver) =>
        driver.property.name?.endsWith("_position"),
      ),
  );
  assert.ok(
    controller,
    `missing GUI waveform ${pulse ? "pulse" : "scan"} animation`,
  );
  return controller;
}

function waveformNode(state: GalleryGuiState, asset = "waveform") {
  const node = state.detailed.nodes.find(({ style }) =>
    style.asset?.source.endsWith(`/${asset}.ippd`),
  );
  assert.ok(node, `missing ${asset} drawing`);
  return node;
}

function waveformX(inspection: Inspection, pulse = false): number {
  const position = dynamicProperty(
    inspection,
    "gui-demo",
    waveformAnimation(inspection, pulse).description.drivers[0]!.property.name!,
  );
  assert.equal(position.kind, "vec2");
  return (position.value as readonly number[])[0]!;
}

function assertNoScanner(inspection: Inspection): void {
  assert.ok(
    !inspection.entities.some(
      ({ metadata }) => metadata.symbolicId === "gui-projector-scanner",
    ),
    "obsolete sweeping scanline is still mounted",
  );
}

function semanticNode(
  tree: GuiSemanticTree,
  role: GuiSemanticRole,
  name?: string,
) {
  const matches = tree.nodes.filter(
    (node) => node.role === role && (name === undefined || node.name === name),
  );
  assert.equal(
    matches.length,
    1,
    `expected one ${role} ${name ?? "node"}, found ${matches.length}`,
  );
  return matches[0]!;
}

function controlValue(
  tree: GuiSemanticTree,
  role: GuiSemanticRole,
  name?: string,
) {
  return semanticNode(tree, role, name).value;
}

function assertScalarValue(tree: GuiSemanticTree, expected: number) {
  const value = controlValue(tree, "slider");
  assert.equal(value.kind, "scalar");
  assert.ok(Math.abs(value.value - expected) < 1e-6);
}

function assertCameraFov(inspection: Inspection, expected: number): void {
  const actual = Number(
    fieldsWith(inspection, "gallery-camera", "fov_y").fov_y,
  );
  assert.ok(
    Math.abs(actual - expected) < 1e-6,
    `camera FOV was ${actual}, expected ${expected}`,
  );
}

function assertRetainedGuiNodes(
  before: GuiSemanticTree,
  after: GuiSemanticTree,
  controlsOnly = false,
): void {
  for (const node of before.nodes.filter(
    ({ actions }) => !controlsOnly || actions.length > 0,
  )) {
    const retained = after.nodes.find(({ id }) => id === node.id);
    assert.ok(retained, `removed GUI node ${node.id}`);
    assert.equal(retained.lifetime, node.lifetime);
    assert.equal(retained.role, node.role);
    assert.deepEqual(retained.value, node.value);
    assert.equal(retained.revision, node.revision);
  }
}

function documentStatus(value: string | null): string {
  return value?.trim() ?? "";
}

test("Gallery runs a real GUI demo and cleans it up", {
  timeout: 240_000,
}, async (context) => {
  const cancelFont = responseGate();
  const cancelDrawing = responseGate();
  const readyFont = responseGate();
  const readyDrawing = responseGate();
  let responseMode: "failure" | "cancel" | "ready" | "pass" = "pass";
  let failedRequest!: () => void;
  const failureRequested = new Promise<void>((resolve) => {
    failedRequest = resolve;
  });
  await runBrowserEnvironment(
    "GUI demo",
    guiEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, {
        initialPage: "gui",
        beforeResponse: async (url, signal) => {
          const font = url.pathname.endsWith("/shure-tech-mono.ippf");
          const drawing = url.pathname.endsWith("/gui-mark.ippd");
          if (!font && !drawing) return;
          if (responseMode === "failure") {
            failedRequest();
            return { status: 503, body: "GUI demo resource unavailable\n" };
          }
          if (responseMode === "cancel") {
            await (font ? cancelFont : cancelDrawing).hold(signal);
          } else if (responseMode === "ready") {
            await (font ? readyFont : readyDrawing).hold(signal);
          }
          return undefined;
        },
      });
      const waveformEvidence: Record<string, unknown> = {};
      const recordWaveform = async (name: string, value: unknown) => {
        waveformEvidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "waveform-evidence.json"),
          JSON.stringify(
            waveformEvidence,
            (_key, value) =>
              typeof value === "bigint" ? String(value) : value,
            2,
          ) + "\n",
        );
      };
      const waitForGui = async (
        predicate: (state: GalleryGuiState) => boolean = () => true,
      ) => {
        const deadline = performance.now() + 15_000;
        let lastError: unknown;
        while (performance.now() < deadline) {
          try {
            const state = await g.call<GalleryGuiState>("galleryGuiState");
            if (predicate(state)) return state;
          } catch (failure) {
            lastError = failure;
          }
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
        throw new Error(
          `GUI demo did not settle${lastError instanceof Error ? `: ${lastError.message}` : ""}`,
        );
      };
      const point = async (role: GuiSemanticRole, name?: string, x = 0.5) => {
        await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
        return g.call<ProjectedGuiNode>(
          "galleryGuiPoint",
          { role, name },
          x,
          0.5,
        );
      };
      const waveformRegion = async () => {
        const state = await waitForGui();
        const grid = waveformNode(state, "waveform-grid");
        const [x, y, width, height] = state.semantic.nodes.find(
          ({ id }) => id === grid.id,
        )!.bounds;
        const corners = await g.call<readonly { x: number; y: number }[]>(
          "projectGalleryPoints",
          "gui-demo",
          [
            [x - 3.7, 2.4 - y, 0],
            [x + width - 3.7, 2.4 - y, 0],
            [x - 3.7, 2.4 - y - height, 0],
            [x + width - 3.7, 2.4 - y - height, 0],
          ],
        );
        return [
          Math.min(...corners.map((p) => p.x)),
          Math.min(...corners.map((p) => p.y)),
          Math.max(...corners.map((p) => p.x)),
          Math.max(...corners.map((p) => p.y)),
        ] as const;
      };
      const waveformDifference = async (before: string, after: string) =>
        g.call<{
          changedPixels: number;
          changedFraction: number;
          meanAbsoluteChannelDifference: number;
        }>("compareViewerCaptureRegion", before, after, await waveformRegion());
      const outerWaveformPixels = async (label: string) => {
        const state = await waitForGui();
        const grid = waveformNode(state, "waveform-grid");
        const [x, y, width, height] = state.semantic.nodes.find(
          ({ id }) => id === grid.id,
        )!.bounds;
        const points: [number, number][] = [];
        for (let row = 1; row < 40; row++) {
          if (row >= 12 && row <= 28) continue;
          for (let column = 1; column < 100; column++)
            points.push([x + (width * column) / 100, y + (height * row) / 40]);
        }
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          points,
        );
        return samples.filter(
          ([r, g, b]) => g! > 110 && b! > 125 && g! - r! > 12,
        ).length;
      };
      const sampleWaveformBaseline = async (label: string) => {
        const state = await waitForGui();
        const grid = waveformNode(state, "waveform-grid");
        const [x, y, width, height] = state.semantic.nodes.find(
          ({ id }) => id === grid.id,
        )!.bounds;
        const groups = [0.06, 0.15, 0.85, 0.94].map((fraction) =>
          [-0.02, 0, 0.02].map(
            (offset) =>
              [x + width * fraction, y + height / 2 + offset] as const,
          ),
        );
        const pixels = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          groups.flat(),
        );
        return groups.map((_, group) =>
          Math.max(
            ...pixels.slice(group * 3, group * 3 + 3).map((pixel) => pixel[1]!),
          ),
        );
      };
      const sampleSineExtrema = async (label: string) => {
        const state = await waitForGui();
        const grid = waveformNode(state, "waveform-grid");
        const [x, y, width, height] = state.semantic.nodes.find(
          ({ id }) => id === grid.id,
        )!.bounds;
        const gain = controlValue(state.semantic, "slider");
        assert.equal(gain.kind, "scalar");
        const amplitude = ((20 * width) / 330) * (0.12 + 0.88 * gain.value);
        // Two complete sine cycles at phase zero: alternating crests and troughs.
        const fractions = [0.125, 0.375, 0.625, 0.875];
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          fractions.flatMap((fraction) =>
            [-0.015, 0, 0.015].map((offset) => [
              x + width * fraction,
              y +
                height / 2 -
                amplitude * Math.sin(4 * Math.PI * fraction) +
                offset,
            ]),
          ),
        );
        return fractions.map((_, index) =>
          Math.max(
            ...samples
              .slice(index * 3, index * 3 + 3)
              .map((pixel) => pixel[1]!),
          ),
        );
      };
      const assertTransparentCorners = async (label: string) => {
        const previousX = await g.call<number>("setGalleryGuiX", 1000);
        try {
          await g.capture(`${label}-backdrop`);
        } finally {
          await g.call("setGalleryGuiX", previousX);
        }
        await g.capture(label);
        const corners = [
          [0.02, 0.02],
          [7.38, 0.02],
          [7.38, 4.78],
          [0.02, 4.78],
        ];
        const painted = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          label,
          corners,
        );
        const backdrop = await g.call<number[][]>(
          "sampleGalleryGuiCapture",
          `${label}-backdrop`,
          corners,
        );
        assert.ok(
          painted.every((pixel, index) =>
            pixel
              .slice(0, 3)
              .every(
                (value, channel) =>
                  Math.abs(value - backdrop[index]![channel]!) <= 2,
              ),
          ),
          `outside rounded panel corners must reveal the unchanged scene: ${JSON.stringify({ painted, backdrop })}`,
        );
      };
      const startGui = async () => {
        await g.selectScene("gui");
        await g.page.waitForFunction(
          () =>
            document.querySelector<HTMLElement>(".viewer-shell")?.dataset
              .page === "gui",
        );
      };
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLElement>(".viewer-shell")?.dataset.page ===
            "gui" &&
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      const coldDirect = await g.capture("gui-demo-cold-direct");
      assert.equal(coldDirect.frame.backend.failedDrawCalls, 0);
      assertCameraFov(coldDirect.inspection, (21 * Math.PI) / 180);
      const backdrop = await g.call<number[][]>(
        "sampleViewerCapture",
        "gui-demo-cold-direct",
        [
          [0.04, 0.04],
          [0.04, 0.45],
        ],
      );
      assert.ok(
        backdrop.every(
          ([r, g, b]) =>
            r! > 40 &&
            b! < 110 &&
            Math.max(r!, g!, b!) - Math.min(r!, g!, b!) < 16,
        ),
        `studio backdrop must paint soft gray: ${JSON.stringify(backdrop)}`,
      );
      assert.ok(
        backdrop[0]![0]! > backdrop[1]![0]! + 3,
        "studio backdrop lost its vertical gradient",
      );
      await assertTransparentCorners("gui-demo-rounded-corners");
      for (const [kind, sources] of [
        [1, PROJECTOR_MESH_SOURCES],
        [2, PROJECTOR_TEXTURE_SOURCES],
      ] as const) {
        for (const source of sources) {
          assert.ok(
            coldDirect.inspection.resources.some(
              (resource) =>
                resource.kind === kind &&
                resource.source.endsWith(source) &&
                resource.status === "loaded",
            ),
            `cold direct entry did not load ${source}`,
          );
        }
      }
      await g.page.screenshot({
        path: join(
          scenario.evidence.directory,
          "gui-demo-cold-direct-page.png",
        ),
        fullPage: true,
      });
      const coldSources = new Set(
        coldDirect.inspection.resources
          .filter(({ source }) => !source.startsWith("ipp://"))
          .map(({ source }) => source),
      );
      await g.navigate("shapes");
      await g.waitFor(
        (inspection) =>
          guiEntity(inspection) === undefined &&
          inspection.resources.every(({ source }) => !coldSources.has(source)),
      );
      const baseline = await g.call<{ session: bigint }>("observeViewer");
      const baselineInspection = await g.inspect();
      assertCameraFov(baselineInspection, Math.PI / 4);
      const baselineSources = new Set(
        baselineInspection.resources.map(({ source }) => source),
      );
      responseMode = "failure";
      const guiSourcesGone = (inspection: Inspection) =>
        guiEntity(inspection) === undefined &&
        inspection.resources.every(({ source }) => baselineSources.has(source));

      await startGui();
      await failureRequested;
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "error" &&
          document.querySelector("#gui-loading[role='alert']"),
      );
      assert.equal(
        documentStatus(await g.page.locator("#gui-status").textContent()),
        "error",
      );
      await g.waitFor((inspection) => guiEntity(inspection) === undefined);
      await g.navigate("shapes");
      await g.waitFor(guiSourcesGone);

      responseMode = "cancel";
      await startGui();
      await Promise.all([cancelFont.requested, cancelDrawing.requested]);
      const cancelledStage = await waitForGui();
      assert.ok(
        cancelledStage.semantic.nodes
          .filter(({ actions }) => actions.length > 0)
          .every(({ visible }) => !visible),
        "staged controls became interactive while resources were pending",
      );
      const cancelledInspection = await g.inspect();
      assert.ok(
        Number(fieldsWith(cancelledInspection, "gui-demo", "qx").x) > 900,
      );
      assert.ok(
        Number(fieldsWith(cancelledInspection, "gui-projector-core", "qx").x) >
          900,
        "the 3D projector was not staged with its pending panel",
      );
      assert.equal(
        documentStatus(await g.page.locator("#gui-status").textContent()),
        "loading",
      );
      await g.navigate("shapes");
      await Promise.all([cancelFont.aborted, cancelDrawing.aborted]);
      cancelFont.release();
      cancelDrawing.release();
      await g.waitFor(guiSourcesGone);

      responseMode = "ready";
      const startupStarted = performance.now();
      await startGui();
      await Promise.all([readyFont.requested, readyDrawing.requested]);
      assert.equal(
        await g.page.getByRole("button", { name: "Reset camera" }).count(),
        1,
      );
      const loading = await waitForGui();
      assert.ok(
        loading.semantic.nodes
          .filter(({ actions }) => actions.length > 0)
          .every(({ visible }) => !visible),
      );
      const loadingFrame = await g.capturePending("gui-demo-loading");
      assertNoScanner(loadingFrame.inspection);
      assert.notEqual(
        waveformAnimation(loadingFrame.inspection).state,
        "playing",
      );
      assert.notEqual(
        waveformAnimation(loadingFrame.inspection, true).state,
        "playing",
      );
      assert.notEqual(
        animationFor(loadingFrame.inspection, "gui-projector-beam").state,
        "playing",
      );

      assert.ok(
        loadingFrame.summary.coverage < 0.02,
        "the off-screen staging GUI demo leaked into the loading frame",
      );
      await g.page.screenshot({
        path: join(scenario.evidence.directory, "gui-demo-loading-page.png"),
      });
      assert.equal(
        await g.page.locator("#gui-loading").getAttribute("role"),
        "status",
      );
      assert.equal(
        documentStatus(
          await g.page.locator("#status").getAttribute("data-state"),
        ),
        "starting",
      );

      readyDrawing.release();
      await g.waitFor(
        (inspection) =>
          inspection.resources.some(
            ({ source, status }) =>
              source.endsWith("/gui-mark.ippd") && status === "loaded",
          ) &&
          inspection.resources.some(
            ({ source, status }) =>
              source.endsWith("/shure-tech-mono.ippf") && status !== "loaded",
          ),
      );
      await g.call("delayNextGuiBatch");
      readyFont.release();
      await g.held();
      const resourcesReady = await g.inspect();
      const essentialResources = resourcesReady.resources.filter(
        ({ source }) =>
          source.endsWith("/shure-tech-mono.ippf") ||
          source.endsWith("/gui-mark.ippd"),
      );
      assert.equal(essentialResources.length, 2);
      assert.ok(essentialResources.every(({ status }) => status === "loaded"));

      assertNoScanner(resourcesReady);
      assert.notEqual(waveformAnimation(resourcesReady).state, "playing");
      const prepared = await g.call<GalleryGuiState>("galleryGuiState", false);
      assert.ok(
        prepared.semantic.nodes
          .filter(({ actions }) => actions.length > 0)
          .every(({ visible }) => visible),
        "the held GUI reveal batch did not prepare the complete GUI demo",
      );
      assert.ok(
        Number(fieldsWith(resourcesReady, "gui-demo", "qx").x) > 900,
        "the GUI demo moved on-screen before GUI reveal acknowledgement",
      );
      const preparedFrame = await g.call<{ summary: { coverage: number } }>(
        "captureUnflushedViewer",
        "gui-demo-prepared-offscreen",
      );
      assert.ok(
        preparedFrame.summary.coverage < 0.02,
        "prepared GUI content appeared before GUI demo placement",
      );
      await scenario.evidence.record(
        "gui-demo-prepared-offscreen",
        preparedFrame,
      );
      const canvasBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(canvasBounds);
      await g.page.mouse.click(
        canvasBounds.x + canvasBounds.width / 2,
        canvasBounds.y + canvasBounds.height / 2,
      );
      await g.settle();
      assert.equal(
        documentStatus(await g.page.locator("#gui-autoscan").textContent()),
        "enabled",
        "staged GUI demo accepted pointer input before placement",
      );
      const resourcesLoadedMs = performance.now() - startupStarted;
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
        "resource readiness bypassed the reveal acknowledgement",
      );
      await g.call("delayNextPresentationCapture");
      await g.call("releaseQuery");
      await g.page.waitForFunction(async (helper) => {
        const fixture = await import(helper);
        return fixture.presentationCaptureHeld();
      }, g.helper);
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
        "the GUI demo reported ready before its first completed visible frame",
      );
      await g.call("releasePresentationCapture");
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      await scenario.evidence.record("gui-demo-startup-timing", {
        resourcesLoadedMs,
        firstReadyMs: performance.now() - startupStarted,
        resources: essentialResources.map(({ kind, source, status }) => ({
          kind,
          source,
          status,
        })),
      });
      responseMode = "pass";

      await g.page.waitForFunction(() => {
        const fps = document.querySelector<HTMLOutputElement>("#canvas-fps");
        return (
          fps?.dataset.state === "live" &&
          /^Canvas FPS [1-9]\d*$/.test(fps.textContent?.trim() ?? "")
        );
      });
      await g.waitFor((inspection) => guiEntity(inspection) !== undefined);
      const initial = await waitForGui(({ semantic }) => {
        const controls = semantic.nodes.filter(
          ({ actions }) => actions.length > 0,
        );
        return (
          semantic.nodes.length >= 30 &&
          controls.length > 0 &&
          controls.every(({ available }) => available)
        );
      });
      const firstReadyFrame = await g.capture("gui-demo-first-ready");
      assert.ok(firstReadyFrame.summary.coverage > 0.08);
      assert.equal(
        initial.surfaceItems,
        0,
        "GuiRoot must be the sole producer",
      );
      assert.ok(initial.entityComponents.includes("Surface"));
      assert.ok(initial.entityComponents.includes("GuiRoot"));
      const paintedBackgrounds = initial.detailed.nodes
        .map(({ style }) => style)
        .filter(({ backgroundColor }) => backgroundColor !== undefined);
      assert.ok(paintedBackgrounds.length >= 3);
      assert.ok(
        paintedBackgrounds.every(
          ({ backgroundColor, opacity }) =>
            Math.abs(backgroundColor![3] * (opacity ?? 1) - 0.9) < 1e-6,
        ),
        "each panel fill must paint at effective alpha 0.9",
      );
      assert.equal(
        initial.detailed.nodes.filter(
          ({ content, style }) =>
            content.kind === "container" &&
            content.containerKind === "scrollView" &&
            style.enabled !== false,
        ).length,
        1,
      );

      const buttons = initial.semantic.nodes.filter(
        ({ role }) => role === "button",
      );
      assert.deepEqual(buttons.map(({ name }) => name).sort(), [
        "AURORA",
        "EMBER",
        "PULSE",
      ]);
      assert.deepEqual(controlValue(initial.semantic, "checkbox"), {
        kind: "bool",
        value: true,
      });
      assertScalarValue(initial.semantic, 0.64);
      assert.deepEqual(
        controlValue(initial.semantic, "textInput", "CALLSIGN"),
        {
          kind: "text",
          value: "VESPER-7",
        },
      );
      for (const node of initial.semantic.nodes.filter(
        ({ actions }) => actions.length > 0,
      )) {
        assert.equal(node.enabled, true);
        assert.equal(node.visible, true);
        assert.equal(node.available, true);
        assert.ok(node.bounds[2] > 0 && node.bounds[3] > 0);
      }

      const overview = await g.capture("gui-demo-overview");
      const panelPaint = await g.call<number[][]>(
        "sampleGalleryGuiCapture",
        "gui-demo-overview",
        [
          [3.7, 0.12],
          [3.7, 4.66],
          [0.15, 2.4],
          [7.22, 2.4],
        ],
      );
      assert.ok(
        panelPaint.every(
          ([r, g, b]) => r! < 55 && g! < 90 && b! < 100 && b! > r! + 10,
        ),
        `panel paint lost its dark translucent navy fill: ${JSON.stringify(panelPaint)}`,
      );
      const overviewInspection = overview.inspection;
      for (const symbol of [
        "gui-projector-core",
        "gui-projector-trim",
        "gui-projector-emitter",
        "gui-projector-beam",
        "gui-projector-base",
        "gui-projector-floor",
        "gui-projector-background",
        "gui-projector-light",
      ]) {
        sceneEntity(overviewInspection, symbol);
      }
      assert.ok(
        Number(fieldsWith(overviewInspection, "gui-projector-core", "qx").x) <
          1,
        "the projector did not leave its offscreen staging position",
      );
      assert.equal(waveformAnimation(overviewInspection).state, "playing");
      assertNoScanner(overviewInspection);
      const initialWaveform = waveformAnimation(overviewInspection);
      await g.waitFor((inspection) => {
        const time = waveformAnimation(inspection).time;
        return (time - initialWaveform.time + 2.4) % 2.4 > 0.45;
      });
      await g.capture("gui-waveform-advancing");
      assert.ok(
        (
          await waveformDifference(
            "gui-demo-overview",
            "gui-waveform-advancing",
          )
        ).changedPixels > 40,
        "playing SCAN did not move the curve inside its grid",
      );
      const initialProjectorIntensity = Number(
        fieldsWith(overviewInspection, "gui-projector-light", "intensity")
          .intensity,
      );
      const surfaceSources = new Set(
        overview.inspection.resources
          .map(({ source }) => source)
          .filter((source) => !baselineSources.has(source)),
      );
      assert.ok(
        overview.inspection.resources.some(
          ({ kind, source, status }) =>
            kind === 17 &&
            source.endsWith("/shure-tech-mono.ippf") &&
            status === "loaded",
        ),
      );
      assert.ok(
        overview.inspection.resources.some(
          ({ kind, source, status }) =>
            kind === 18 &&
            source.endsWith("/gui-mark.ippd") &&
            status === "loaded",
        ),
      );
      assert.equal(
        overview.inspection.resources.filter(
          ({ kind, source, status }) =>
            kind === 10 && source.includes("generated-") && status === "loaded",
        ).length,
        5,
        "skin, waveform and projector motion clips must all be resident",
      );
      assert.ok(overview.frame.drawCalls > 0 && overview.frame.triangles > 0);
      assert.equal(overview.frame.backend.failedDrawCalls, 0);
      assert.ok(overview.summary.coverage > 0.08);
      assert.deepEqual(overview.inspection.renderDiagnostics, []);

      const cameraBeforeControls = transform(await g.inspect());
      const checkboxRegion = await g.call<
        readonly [number, number, number, number]
      >("galleryGuiRegion", { role: "checkbox" });
      const sliderRegion = await g.call<
        readonly [number, number, number, number]
      >("galleryGuiRegion", { role: "slider" }, 0.02, 0.08);
      const checkbox = await point("checkbox");
      await g.page.mouse.click(checkbox.clientX, checkbox.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      let current = await waitForGui(({ semantic }) => {
        const value = controlValue(semantic, "checkbox");
        return value.kind === "bool" && value.value === false;
      });
      await g.waitFor(
        (inspection) => waveformAnimation(inspection).state === "paused",
      );

      const dustStart = await g.capture("gui-projector-dust-start");
      const dustController = animationFor(
        dustStart.inspection,
        "gui-projector-beam",
      );
      assert.equal(
        dustController.state,
        "playing",
        "SCAN must not pause the ambient dust",
      );
      const initialPhase = dynamicProperty(
        dustStart.inspection,
        "gui-projector-beam",
        "phase",
      );
      assert.equal(initialPhase.kind, "f32");
      await g.waitFor(
        (inspection) =>
          Number(
            dynamicProperty(inspection, "gui-projector-beam", "phase").value,
          ) >
          Number(initialPhase.value) + 0.3,
      );
      await g.capture("gui-projector-dust-drifted");
      const dustDifference = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-projector-dust-start",
        "gui-projector-dust-drifted",
        [0.44, 0.24, 0.535, 0.79],
      );
      assert.ok(
        dustDifference.changedPixels > 3,
        "Host-owned dust drift did not change the visible projection volume",
      );

      const pausedWaveform = waveformAnimation(await g.inspect());
      assert.equal(pausedWaveform.id, initialWaveform.id);
      await g.capture("gui-waveform-paused");
      assert.ok(
        (
          await waveformDifference(
            "gui-projector-dust-start",
            "gui-waveform-paused",
          )
        ).changedPixels < 12,
        "the SCAN curve moved while paused",
      );
      assert.equal(
        waveformAnimation(await g.inspect()).time,
        pausedWaveform.time,
      );
      await g.call("controlGalleryAnimation", pausedWaveform.id, {
        action: "seek",
        time: 0,
      });
      await g.capture("gui-waveform-loop-start");
      await g.call("controlGalleryAnimation", pausedWaveform.id, {
        action: "seek",
        time: 2.399999,
      });
      const loopEnd = await g.capture("gui-waveform-loop-end");
      const loopPosition = dynamicProperty(
        loopEnd.inspection,
        "gui-demo",
        waveformAnimation(loopEnd.inspection).description.drivers[0]!.property
          .name!,
      );
      assert.equal(loopPosition.kind, "vec2");
      assert.ok(
        (loopPosition.value as readonly number[])[0]! < -3.5,
        "SCAN must travel left through the second authored tile",
      );
      const seamDifference = await waveformDifference(
        "gui-waveform-loop-start",
        "gui-waveform-loop-end",
      );
      await recordWaveform("seam", {
        difference: seamDifference,
        position: loopPosition,
        controller: waveformAnimation(loopEnd.inspection),
      });
      // Clipped curve endpoints and subpixel coverage can differ between tiles.
      // Bound both the affected area and total contrast, not raw pixel density.
      assert.ok(
        seamDifference.changedFraction < 0.005 &&
          seamDifference.meanAbsoluteChannelDifference < 0.25,
        `the periodic curve visibly jumps at its loop seam: ${JSON.stringify(seamDifference)}`,
      );
      await g.call("controlGalleryAnimation", pausedWaveform.id, {
        action: "seek",
        time: 0,
      });
      await g.call(
        "galleryGuiAction",
        { role: "slider" },
        { kind: "setScalar", value: 0.1 },
      );
      await waitForGui(({ semantic }) => {
        const value = controlValue(semantic, "slider");
        return value.kind === "scalar" && Math.abs(value.value - 0.1) < 1e-6;
      });
      await g.page.waitForFunction(
        () => document.querySelector("#gui-gain")?.textContent === "10%",
      );
      await g.capture("gui-waveform-low-gain");
      const sliderStart = await point("slider", undefined, 0.18);
      const sliderEnd = await point("slider", undefined, 0.84);
      await g.drag(
        [sliderStart.clientX, sliderStart.clientY],
        [sliderEnd.clientX, sliderEnd.clientY],
      );
      await g.page.waitForFunction(
        () =>
          Number.parseInt(
            document.querySelector("#gui-gain")?.textContent ?? "0",
          ) >= 75,
      );
      current = await waitForGui(({ semantic }) => {
        const value = controlValue(semantic, "slider");
        return value.kind === "scalar" && value.value >= 0.75;
      });
      await g.waitFor(
        (inspection) =>
          Number(
            fieldsWith(inspection, "gui-projector-light", "intensity")
              .intensity,
          ) >
          initialProjectorIntensity * 1.1,
      );

      await g.capture("gui-waveform-high-gain");
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-low-gain",
            "gui-waveform-high-gain",
          )
        ).changedPixels > 35,
        "gain did not visibly change the paused waveform amplitude",
      );
      const lowAmplitudePixels = await outerWaveformPixels(
        "gui-waveform-low-gain",
      );
      const highAmplitudePixels = await outerWaveformPixels(
        "gui-waveform-high-gain",
      );
      await recordWaveform("gain", { lowAmplitudePixels, highAmplitudePixels });
      assert.ok(
        highAmplitudePixels > lowAmplitudePixels + 12,
        `higher gain must extend the visible curve above and below its center: ${JSON.stringify({ lowAmplitudePixels, highAmplitudePixels })}`,
      );
      const sineExtrema = await sampleSineExtrema("gui-waveform-high-gain");
      await recordWaveform("sine", { extrema: sineExtrema });
      assert.ok(
        sineExtrema.every((green) => green > 100),
        `SCAN must paint two smooth sine cycles across the graph: ${JSON.stringify(sineExtrema)}`,
      );
      assert.equal(
        waveformAnimation(await g.inspect()).id,
        initialWaveform.id,
        "gain replaced the waveform scan controller",
      );
      const identityBeforeCamera = current.semantic;
      await g.capture("gui-demo-before-camera");
      assert.equal(
        await g.page
          .getByRole("button", { name: /^(Rotate|Tilt|Zoom) / })
          .count(),
        0,
        "GUI camera navigation should use background gestures",
      );
      const cameraCanvasBounds = await g.page
        .locator("#ipp-world-canvas")
        .boundingBox();
      assert.ok(cameraCanvasBounds);
      const cameraDrag = [
        cameraCanvasBounds.x + 20,
        cameraCanvasBounds.y + 20,
      ] as const;
      await g.drag(cameraDrag, [cameraDrag[0] + 36, cameraDrag[1] + 24]);
      await g.waitFor(
        (inspection) =>
          Math.abs(
            Number(transform(inspection).qy) - Number(cameraBeforeControls.qy),
          ) > 0.01,
      );
      await g.page.mouse.move(...cameraDrag);
      await g.page.mouse.wheel(0, -80);
      await g.settle();
      const cameraOblique = transform(await g.inspect());
      assert.notDeepEqual(
        cameraOblique,
        cameraBeforeControls,
        "background gestures did not move the GUI demo camera",
      );
      const obliqueState = await waitForGui();
      assertRetainedGuiNodes(identityBeforeCamera, obliqueState.semantic);
      const obliqueFrame = await g.capture("gui-demo-camera-oblique");
      await assertTransparentCorners("gui-demo-rounded-corners-oblique");
      await g.page.screenshot({
        path: join(
          scenario.evidence.directory,
          "gui-demo-camera-oblique-page.png",
        ),
        fullPage: true,
      });
      assert.equal(obliqueFrame.frame.backend.failedDrawCalls, 0);
      assert.ok(
        (
          await g.difference(
            "gui-demo-before-camera",
            "gui-demo-camera-oblique",
          )
        ).changedPixels > 1_000,
        "camera gestures did not visibly change the completed GUI demo frame",
      );

      await g.capture("gui-waveform-before-pulse");
      const pulse = await point("button", "PULSE");
      await g.page.mouse.click(pulse.clientX, pulse.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-command")?.textContent === "Pulse sent",
      );
      const pulsing = await g.waitFor((inspection) => {
        const controller = waveformAnimation(inspection, true);
        return controller.state === "playing" && controller.time > 0.32;
      });
      const activeWavePulse = waveformAnimation(pulsing, true);
      assert.equal(activeWavePulse.state, "playing");
      assert.equal(
        waveformAnimation(pulsing).state,
        "paused",
        "PULSE resumed disabled SCAN",
      );
      // Freeze an observed playing pose before software frame capture can outlast the burst.
      await g.call("controlGalleryAnimation", activeWavePulse.id, {
        action: "pause",
      });
      // Center the pulse so both baseline spans can be checked in the completed frame.
      await g.call("controlGalleryAnimation", activeWavePulse.id, {
        action: "seek",
        time: 0.6,
      });
      const pulseFrame = await g.capture("gui-waveform-pulse-active");
      const capturedWavePulse = waveformAnimation(pulseFrame.inspection, true);
      assert.ok(
        !pulseFrame.inspection.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-projector-pulse",
        ),
        "PULSE must not create a 3D pulse sphere",
      );
      assert.equal(capturedWavePulse.state, "paused");
      assert.ok(
        capturedWavePulse.time > 0.32 && capturedWavePulse.time < 1.05,
        `pulse left its visible interval before capture: ${capturedWavePulse.time}`,
      );
      const pulseOpacityName =
        capturedWavePulse.description.drivers[0]!.property.name!.replace(
          /_position$/,
          "_opacity",
        );
      await recordWaveform("pulse", {
        started: activeWavePulse,
        controller: capturedWavePulse,
        scan: waveformAnimation(pulseFrame.inspection),
        opacity: dynamicProperty(
          pulseFrame.inspection,
          "gui-demo",
          pulseOpacityName,
        ),
      });
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-before-pulse",
            "gui-waveform-pulse-active",
          )
        ).changedPixels > 25,
        "PULSE did not paint a visible curve burst while SCAN was off",
      );
      const baselineSignal = await sampleWaveformBaseline(
        "gui-waveform-pulse-active",
      );
      assert.ok(
        baselineSignal.every((green) => green > 150),
        `manual pulse must join a visible baseline on both sides: ${JSON.stringify(baselineSignal)}`,
      );
      const scanOpacityName = waveformAnimation(
        pulseFrame.inspection,
      ).description.drivers[0]!.property.name!.replace(
        /_position$/,
        "_opacity",
      );
      assert.ok(
        Math.abs(
          Number(
            dynamicProperty(pulseFrame.inspection, "gui-demo", scanOpacityName)
              .value,
          ) - 0.35,
        ) < 1e-6,
        "PULSE must preserve the visible paused sine trace",
      );
      const pulseToggle = await point("checkbox");
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await g.waitFor(
        (inspection) =>
          waveformAnimation(inspection).state === "playing" &&
          waveformAnimation(inspection).time > 0.2,
      );
      const toggledPulse = await g.capture("gui-waveform-pulse-scan-toggled");
      assert.ok(
        Number(
          dynamicProperty(toggledPulse.inspection, "gui-demo", scanOpacityName)
            .value,
        ) > 0.85,
        "the moving sine must remain visible alongside PULSE",
      );
      assert.equal(waveformAnimation(toggledPulse.inspection).state, "playing");
      assert.equal(
        waveformAnimation(toggledPulse.inspection, true).time,
        capturedWavePulse.time,
      );
      assert.equal(
        Number(
          dynamicProperty(toggledPulse.inspection, "gui-demo", pulseOpacityName)
            .value,
        ),
        1,
        "SCAN must preserve the independent pulse trace",
      );
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-pulse-active",
            "gui-waveform-pulse-scan-toggled",
          )
        ).changedPixels > 25,
        "the sine did not visibly move while the independent pulse remained in place",
      );
      await recordWaveform("baseline", {
        baselineSignal,
        scanVisible: true,
        scanToggledWhilePulseVisible: true,
      });
      await g.call("controlGalleryAnimation", activeWavePulse.id, {
        action: "play",
      });
      const advancedPulse = await g.waitFor(
        (inspection) =>
          waveformAnimation(inspection, true).time >
          capturedWavePulse.time + 0.12,
      );
      assert.equal(waveformAnimation(advancedPulse).state, "playing");
      assert.equal(waveformAnimation(advancedPulse, true).state, "playing");
      assert.ok(
        waveformX(advancedPulse, true) <
          waveformX(pulseFrame.inspection, true) - 0.3,
        "PULSE must travel right to left alongside SCAN",
      );
      await recordWaveform("direction", {
        pulseStart: waveformX(pulseFrame.inspection, true),
        pulseAdvanced: waveformX(advancedPulse, true),
        scan: waveformAnimation(advancedPulse),
        pulse: waveformAnimation(advancedPulse, true),
      });
      const advancedPulseTime = waveformAnimation(advancedPulse, true).time;
      await g.page.mouse.click(pulse.clientX, pulse.clientY);
      await g.waitFor((inspection) => {
        const controller = waveformAnimation(inspection, true);
        return (
          controller.id === activeWavePulse.id &&
          controller.state === "playing" &&
          controller.time > 0.02 &&
          controller.time < advancedPulseTime - 0.1
        );
      });
      await g.waitFor(
        (inspection) =>
          waveformAnimation(inspection, true).state === "completed",
      );
      await g.waitFor(
        (inspection) =>
          Number(
            dynamicProperty(inspection, "gui-demo", pulseOpacityName).value,
          ) === 0,
      );
      const finishedPulse = await g.capture("gui-waveform-pulse-finished");
      assert.equal(
        waveformAnimation(finishedPulse.inspection).state,
        "playing",
        "pulse completion interrupted SCAN",
      );
      assert.equal(
        dynamicProperty(finishedPulse.inspection, "gui-demo", pulseOpacityName)
          .value,
        0,
      );
      assert.ok(
        (
          await waveformDifference(
            "gui-waveform-pulse-active",
            "gui-waveform-pulse-finished",
          )
        ).changedPixels > 25,
        "the independent pulse did not disappear after completion",
      );
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await g.waitFor(
        (inspection) => waveformAnimation(inspection).state === "paused",
      );
      const beforeReset = await waitForGui();

      await g.page.locator("#reset-camera").click();
      const resetInspection = await g.waitFor(
        (inspection) =>
          JSON.stringify(transform(inspection)) ===
          JSON.stringify(cameraBeforeControls),
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      assertCameraFov(resetInspection, (21 * Math.PI) / 180);
      current = await waitForGui();
      assertRetainedGuiNodes(beforeReset.semantic, current.semantic);
      await g.capture("gui-demo-camera-reset");

      const callsign = await point("textInput", "CALLSIGN");
      await g.page.mouse.click(callsign.clientX, callsign.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelectorAll("textarea").length === 1 &&
          document.activeElement === document.querySelector("textarea"),
      );
      await g.page.keyboard.press("ControlOrMeta+A");
      await g.page.keyboard.insertText("NOVA-12");
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-callsign")?.textContent === "NOVA-12",
      );
      current = await waitForGui(({ semantic }) => {
        const value = controlValue(semantic, "textInput", "CALLSIGN");
        return value.kind === "text" && value.value === "NOVA-12";
      });
      assert.deepEqual(transform(await g.inspect()), cameraBeforeControls);
      const controlsFrame = await g.capture("gui-demo-controls-active");
      assert.equal(controlsFrame.frame.backend.failedDrawCalls, 0);
      const checkboxPaint = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-demo-overview",
        "gui-demo-controls-active",
        checkboxRegion,
      );
      assert.ok(
        checkboxPaint.changedPixels > 20,
        `checked indicator region did not visibly change: ${JSON.stringify(checkboxPaint)}`,
      );
      const sliderPaint = await g.call<{ changedPixels: number }>(
        "compareViewerCaptureRegion",
        "gui-demo-overview",
        "gui-demo-controls-active",
        sliderRegion,
      );
      assert.ok(
        sliderPaint.changedPixels > 80,
        `slider thumb did not visibly move: ${JSON.stringify(sliderPaint)}`,
      );
      assert.ok(
        (await g.difference("gui-demo-overview", "gui-demo-controls-active"))
          .changedPixels > 500,
        "trusted control input did not produce a visible GUI demo change",
      );

      const identityBeforeSkin = current.semantic;
      const sceneBeforeSkin = await g.inspect();
      const projectorBeforeSkin = {
        core: dynamicProperty(sceneBeforeSkin, "gui-projector-core", "accent"),
        light: fieldsWith(sceneBeforeSkin, "gui-projector-light", "intensity"),
        scanController: waveformAnimation(sceneBeforeSkin).id,
        wavePulseController: waveformAnimation(sceneBeforeSkin, true).id,
      };
      const callsignBeforeSkin = semanticNode(
        identityBeforeSkin,
        "textInput",
        "CALLSIGN",
      );
      assert.deepEqual(identityBeforeSkin.focused, {
        id: callsignBeforeSkin.id,
        lifetime: callsignBeforeSkin.lifetime,
      });
      // Exercise the production machine-client action while the editor remains
      // focused, which isolates reskinning from normal pointer focus transfer.
      const ember = await g.call<GuiSemanticTree>(
        "galleryGuiAction",
        { role: "button", name: "EMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      const emberState = await waitForGui();
      assertRetainedGuiNodes(identityBeforeSkin, emberState.semantic, true);
      assert.deepEqual(ember.focused, identityBeforeSkin.focused);
      assert.deepEqual(emberState.semantic.focused, identityBeforeSkin.focused);
      const emberFrame = await g.capture("gui-demo-ember-active");
      const emberCore = dynamicProperty(
        emberFrame.inspection,
        "gui-projector-core",
        "accent",
      );
      const emberLight = fieldsWith(
        emberFrame.inspection,
        "gui-projector-light",
        "intensity",
      );
      assert.notDeepEqual(
        emberCore,
        projectorBeforeSkin.core,
        "Ember did not recolor the projector cube",
      );
      assert.notDeepEqual(
        [emberLight.r, emberLight.g, emberLight.b],
        [
          projectorBeforeSkin.light.r,
          projectorBeforeSkin.light.g,
          projectorBeforeSkin.light.b,
        ],
        "Ember did not recolor the projector light",
      );
      assert.equal(
        waveformAnimation(emberFrame.inspection).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        waveformAnimation(emberFrame.inspection, true).id,
        projectorBeforeSkin.wavePulseController,
        "reskin replaced the waveform pulse controller",
      );
      assert.equal(
        animationFor(emberFrame.inspection, "gui-projector-beam").id,
        dustController.id,
        "reskin replaced the ambient dust controller",
      );
      const skinDifference = await g.difference(
        "gui-demo-controls-active",
        "gui-demo-ember-active",
      );
      assert.ok(skinDifference.changedPixels > 1_000);
      assert.ok(skinDifference.meanAbsoluteChannelDifference > 1);
      assert.equal(emberFrame.frame.backend.failedDrawCalls, 0);
      assert.deepEqual(emberFrame.inspection.renderDiagnostics, []);

      await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
      const scroll = await g.call<ProjectedGuiNode>("galleryGuiScrollPoint");
      const cameraBeforeScroll = transform(await g.inspect());
      await g.capture("gui-demo-before-scroll");
      await g.page.mouse.move(scroll.clientX, scroll.clientY);
      await g.page.mouse.wheel(0, 480);
      await g.settle();
      assert.deepEqual(
        transform(await g.inspect()),
        cameraBeforeScroll,
        "GUI scroll was also processed as a camera gesture",
      );
      await g.capture("gui-demo-after-scroll");
      assert.ok(
        (await g.difference("gui-demo-before-scroll", "gui-demo-after-scroll"))
          .changedPixels > 100,
        "event stream did not visibly scroll",
      );

      const auroraPoint = await point("button", "AURORA");
      await g.page.mouse.click(auroraPoint.clientX, auroraPoint.clientY);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "aurora",
      );
      const emberPoint = await point("button", "EMBER");
      await g.page.mouse.click(emberPoint.clientX, emberPoint.clientY);
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      const pointerReskin = await waitForGui();
      assertRetainedGuiNodes(identityBeforeSkin, pointerReskin.semantic, true);
      await g.capture("gui-demo-final-ember-controls");

      const mountedEntity = guiEntity(await g.inspect())!;
      const mountedRoot = (await waitForGui()).semantic;
      const mountedScan = waveformAnimation(await g.inspect());
      const mountedWavePulse = waveformAnimation(await g.inspect(), true);
      const fullSceneFrame = await g.capture("gui-demo-full-before-isolation");
      const vectorButton = g.page.locator("#gui-vector-only");
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "false");
      await vectorButton.click();
      const isolated = await g.waitFor(
        (inspection) =>
          !inspection.entities.some(({ metadata }) =>
            metadata.symbolicId?.startsWith("gui-projector-"),
          ),
      );
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "true");
      assert.deepEqual(
        isolated.entities.map(({ metadata }) => metadata.symbolicId).sort(),
        ["gallery-camera", "gui-demo"],
      );
      assert.ok(
        !isolated.controllers?.some(({ id }) => id === dustController.id),
        "vector isolation retained the projector dust controller",
      );
      const isolatedGui = await waitForGui();
      assertRetainedGuiNodes(mountedRoot, isolatedGui.semantic);
      const vectorFrame = await g.capture("gui-demo-vector-only");
      assert.ok(
        vectorFrame.frame.drawCalls > 0,
        "vector panel stopped drawing",
      );
      assert.ok(
        vectorFrame.frame.drawCalls < fullSceneFrame.frame.drawCalls,
        "removing the projector did not reduce draw calls",
      );
      assert.ok(
        (
          await g.difference(
            "gui-demo-full-before-isolation",
            "gui-demo-vector-only",
          )
        ).changedPixels > 100,
        "vector isolation did not visibly remove the projector",
      );
      await vectorButton.click();
      await g.waitFor((inspection) =>
        inspection.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-projector-beam",
        ),
      );
      assert.equal(await vectorButton.getAttribute("aria-pressed"), "false");
      const restoredFrame = await g.capture("gui-demo-projector-restored");
      assert.ok(
        restoredFrame.frame.drawCalls > vectorFrame.frame.drawCalls,
        "restoring the projector did not restore its draws",
      );
      await g.navigate("shapes");
      const cleaned = await g.waitFor(
        (inspection) =>
          guiEntity(inspection) === undefined &&
          [...surfaceSources].every(
            (source) =>
              !inspection.resources.some(
                (resource) => resource.source === source,
              ),
          ),
      );
      assertCameraFov(cleaned, Math.PI / 4);
      assert.ok(
        !cleaned.controllers?.some(
          ({ id }) => id === mountedScan.id || id === mountedWavePulse.id,
        ),
        "navigation retained a waveform controller",
      );
      assert.ok(
        !cleaned.controllers?.some(({ id }) => id === dustController.id),
        "navigation retained the ambient dust controller",
      );
      assert.equal(await g.page.locator("textarea").count(), 0);
      assert.deepEqual(
        (await g.call<{ session: bigint }>("observeViewer")).session,
        baseline.session,
        "authored gallery navigation replaced the shared session",
      );

      await g.navigate("gui");
      const returned = await waitForGui();
      assert.equal(
        await g.page.locator("#gui-vector-only").getAttribute("aria-pressed"),
        "false",
      );
      assert.notEqual(returned.semantic.entity, mountedEntity.id);
      assert.notEqual(
        returned.semantic.rootIncarnation,
        mountedRoot.rootIncarnation,
      );
      const returnedInspection = await g.inspect();
      assertCameraFov(returnedInspection, (21 * Math.PI) / 180);
      const returnedScanner = waveformAnimation(returnedInspection);
      assertNoScanner(returnedInspection);
      assert.notEqual(
        waveformAnimation(returnedInspection, true).id,
        mountedWavePulse.id,
      );
      assert.notEqual(
        returnedScanner.id,
        mountedScan.id,
        "reentry reused the removed waveform scan controller",
      );
      assert.equal(returnedScanner.state, "playing");
      const returnedDust = animationFor(
        returnedInspection,
        "gui-projector-beam",
      );
      assert.notEqual(returnedDust.id, dustController.id);
      assert.equal(returnedDust.state, "playing");
      assert.deepEqual(controlValue(returned.semantic, "checkbox"), {
        kind: "bool",
        value: true,
      });
      assertScalarValue(returned.semantic, 0.64);
      assert.deepEqual(
        controlValue(returned.semantic, "textInput", "CALLSIGN"),
        {
          kind: "text",
          value: "VESPER-7",
        },
      );
      await recordWaveform("lifecycle", {
        removedScan: mountedScan.id,
        removedPulse: mountedWavePulse.id,
        returnedScan: returnedScanner.id,
        returnedPulse: waveformAnimation(returnedInspection, true).id,
      });
      await g.capture("gui-demo-returned");
      assert.deepEqual(g.errors, []);
    },
  );
});
