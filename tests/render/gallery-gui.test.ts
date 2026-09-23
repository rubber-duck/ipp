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
  SurfaceCacheRecord,
} from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { responseGate } from "../browser/response-gate.js";
import {
  galleryEnvironment,
  openGallery,
  transform,
} from "./gallery-driver.js";
import {
  compareFrames,
  count,
  differenceImage,
  encodePng,
  intersectionOverUnion,
  mask,
  type RgbaFrame,
} from "./retained-gui-images.js";
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

type LogicalRect = readonly [number, number, number, number];

interface RegionStats {
  readonly pixels: number;
  readonly mean: readonly [number, number, number];
  readonly min: readonly [number, number, number];
  readonly max: readonly [number, number, number];
}

/** Verified Nerd Font code points, restated independently of the fixture. */
const ICON_CODE_POINTS = {
  cube: "\uf1b2",
  signal: "\uf012",
  pulse: "\ueb31",
  aurora: "\uf2dc",
  ember: "\uf06d",
  neon: "\uf0e7",
} as const;

/** Aurora idle `background` lane authored by the gallery control theme. */
const AURORA_IDLE = [0.38, 0.85, 1, 0.9] as const;

/** The gallery's 0.16 s skin transition plus host-frame and inspection
 * latency, matching the hover probe in gallery-gui-camera. */
const SKIN_SETTLE_MS = 550;

/** Largest per-channel difference between two region means. */
function meanDifference(first: RegionStats, second: RegionStats): number {
  return Math.max(
    ...first.mean.map((value, channel) =>
      Math.abs(value - second.mean[channel]!),
    ),
  );
}

function srgbToLinear(value: number): number {
  const unit = value / 255;
  return unit <= 0.04045 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(value: number): number {
  const encoded =
    value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return encoded * 255;
}

function near(actual: number, expected: number, what: string): void {
  assert.ok(
    Math.abs(actual - expected) < 0.01,
    `${what}: ${actual} is not ${expected}`,
  );
}

/** The resizable SPAN frame, matched by its authored sizes. */
function spanFrame(state: GalleryGuiState): GuiSemanticNode {
  const node = state.detailed.nodes.find(
    ({ content, style }) =>
      content.kind === "container" &&
      content.containerKind === "stack" &&
      style.enabled === false &&
      Math.abs((style.height ?? 0) - 0.38) < 1e-5 &&
      [1.2, 2.0].some((width) => Math.abs((style.width ?? 0) - width) < 1e-5),
  );
  assert.ok(node, "missing resizable SPAN frame");
  const semantic = state.semantic.nodes.find(({ id }) => id === node.id);
  assert.ok(semantic, "SPAN frame is absent from semantics");
  return semantic;
}

/** Evaluated bounds of the one text leaf, such as an icon glyph, with this text. */
function textBounds(state: GalleryGuiState, text: string) {
  const matches = state.detailed.nodes.filter(
    ({ content }) => content.kind === "text" && content.text === text,
  );
  assert.equal(matches.length, 1, `expected one text leaf ${text}`);
  const semantic = state.semantic.nodes.find(({ id }) => id === matches[0]!.id);
  assert.ok(semantic, `text leaf ${text} is absent from semantics`);
  return semantic.bounds;
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

function waveformViewport(state: GalleryGuiState) {
  const node = state.detailed.nodes.find(
    ({ content, style }) =>
      content.kind === "container" &&
      content.containerKind === "scrollView" &&
      style.enabled === false &&
      Math.abs((style.width ?? 0) - 3.54) < 1e-5 &&
      Math.abs((style.height ?? 0) - 0.46) < 1e-5,
  );
  assert.ok(node, "missing retained waveform viewport");
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
  const cancelMesh = responseGate();
  const readyFont = responseGate();
  const readyMesh = responseGate();
  const readyWaveform = responseGate();
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
          const mesh = url.pathname.endsWith(PROJECTOR_MESH_SOURCES[0]);
          const waveform = url.pathname.endsWith("/waveform.ippd");
          if (waveform) {
            // The waveform drawing gates readiness like the font and mesh.
            if (responseMode === "ready") await readyWaveform.hold(signal);
            return undefined;
          }
          if (!font && !mesh) return;
          if (responseMode === "failure") {
            failedRequest();
            return { status: 503, body: "GUI demo resource unavailable\n" };
          }
          if (responseMode === "cancel") {
            await (font ? cancelFont : cancelMesh).hold(signal);
          } else if (responseMode === "ready") {
            await (font ? readyFont : readyMesh).hold(signal);
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
        const grid = waveformViewport(state);
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
        const grid = waveformViewport(state);
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
        const grid = waveformViewport(state);
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
        const grid = waveformViewport(state);
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
        await g.call("overrideGalleryGuiTransform", { x: 1000 });
        try {
          await g.capture(`${label}-backdrop`);
        } finally {
          await g.call("releaseGalleryGuiTransform");
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
      // Region measurements and their expected values collect in one review
      // file beside waveform-evidence.json.
      const regionEvidence: Record<string, unknown> = {};
      const recordRegions = async (name: string, value: unknown) => {
        regionEvidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "region-evidence.json"),
          JSON.stringify(regionEvidence, null, 2) + "\n",
        );
      };
      const regionStats = <K extends string>(
        label: string,
        rects: Record<K, LogicalRect>,
      ) =>
        g.call<Record<K, RegionStats>>("galleryGuiRegionStats", label, rects);
      // Face the panel to the camera so thin skin features span several
      // pixels, then restore the authored placement.
      const withDetailView = async <T>(body: () => Promise<T>) => {
        await g.page.mouse.move(1, 1);
        await g.call("faceGalleryGuiToCamera");
        try {
          return await body();
        } finally {
          await g.call("releaseGalleryGuiTransform");
        }
      };
      // Named background lanes: `background` carries the animated appearance,
      // `background_disabled` the authored disabled state lane.
      const partValue = (
        tree: GuiSemanticTree,
        node: GuiSemanticNode,
        part: "background" | "background_disabled",
        property: "color" | "opacity",
      ) =>
        g.call<{ kind: string; value: number | readonly number[] }>(
          "galleryGuiPartValue",
          tree.entity,
          node.id,
          part,
          property,
        );
      const waitForPartOpacity = async (
        tree: GuiSemanticTree,
        node: GuiSemanticNode,
        expected: number,
      ) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const value = await partValue(tree, node, "background", "opacity");
          if (Math.abs(Number(value.value) - expected) < 1e-4) return;
          if (performance.now() > deadline)
            throw new Error(
              `${node.name} opacity stayed ${JSON.stringify(value)}, expected ${expected}`,
            );
          await new Promise((resolve) => setTimeout(resolve, 25));
        }
      };
      // A re-enabled control returns to the idle lane within the skin
      // transition: a refused request or a lane restored to the held
      // disabled sample would stay dim instead.
      const waitForIdleLane = async (
        tree: GuiSemanticTree,
        node: GuiSemanticNode,
      ) => {
        const started = performance.now();
        for (;;) {
          const [color, opacity] = await Promise.all([
            partValue(tree, node, "background", "color"),
            partValue(tree, node, "background", "opacity"),
          ]);
          const elapsedMs = performance.now() - started;
          if (
            Math.abs(Number(opacity.value) - 1) < 1e-4 &&
            (color.value as readonly number[]).every(
              (value, channel) =>
                Math.abs(value - AURORA_IDLE[channel]!) < 1e-4,
            )
          )
            return { elapsedMs, color: color.value, opacity: opacity.value };
          assert.ok(
            elapsedMs < SKIN_SETTLE_MS,
            `${node.name} kept ${JSON.stringify({ color, opacity })} ${Math.round(elapsedMs)} ms after enabling`,
          );
          await new Promise((resolve) => setTimeout(resolve, 16));
        }
      };
      // UPLINK fill beside its label and the panel gap to its right.
      const uplinkRegions = (label: string, node: GuiSemanticNode) => {
        const [x, y, width, height] = node.bounds;
        return regionStats(label, {
          fill: [x + 0.72, y + 0.12, x + 0.9, y + height - 0.12],
          gap: [
            x + width + 0.04,
            y + 0.12,
            x + width + 0.09,
            y + height - 0.12,
          ],
        });
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
      await Promise.all([cancelFont.requested, cancelMesh.requested]);
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
      await Promise.all([cancelFont.aborted, cancelMesh.aborted]);
      cancelFont.release();
      cancelMesh.release();
      await g.waitFor(guiSourcesGone);

      responseMode = "ready";
      const startupStarted = performance.now();
      await startGui();
      await Promise.all([readyFont.requested, readyMesh.requested]);
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

      readyMesh.release();
      await g.waitFor(
        (inspection) =>
          inspection.resources.some(
            ({ source, status }) =>
              source.endsWith(PROJECTOR_MESH_SOURCES[0]) && status === "loaded",
          ) &&
          inspection.resources.some(
            ({ source, status }) =>
              source.endsWith("/shure-tech-mono.ippf") && status !== "loaded",
          ),
      );
      readyFont.release();
      await readyWaveform.requested;
      await g.waitFor((inspection) =>
        inspection.resources.some(
          ({ source, status }) =>
            source.endsWith("/shure-tech-mono.ippf") && status === "loaded",
        ),
      );
      const waveformPending = await waitForGui();
      assert.ok(
        waveformPending.semantic.nodes
          .filter(({ actions }) => actions.length > 0)
          .every(({ visible }) => !visible),
        "the GUI demo prepared before its essential waveform drawing loaded",
      );
      assert.equal(
        await g.page.locator("#status").getAttribute("data-state"),
        "starting",
      );
      await g.call("delayNextGuiBatch");
      readyWaveform.release();
      await g.held();
      const resourcesReady = await g.inspect();
      const essentialResources = resourcesReady.resources.filter(
        ({ source }) =>
          source.endsWith("/shure-tech-mono.ippf") ||
          source.endsWith("/waveform.ippd") ||
          source.endsWith(PROJECTOR_MESH_SOURCES[0]),
      );
      assert.equal(essentialResources.length, 3);
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
        .filter(
          ({ backgroundColor, enabled }) =>
            backgroundColor !== undefined && enabled !== false,
        );
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
        "NEON",
        "PULSE",
        "SPAN",
        "UPLINK",
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
      assert.deepEqual(
        overview.inspection.resources
          .filter(({ kind }) => kind === 18)
          .map(({ source }) => source.split("/").pop())
          .sort(),
        ["waveform-grid.ippd", "waveform-pulse.ippd", "waveform.ippd"],
        "complex waveform curves share Surface drawing resources",
      );
      assert.equal(
        initial.detailed.nodes.filter(
          ({ content }) => content.kind === "drawing",
        ).length,
        3,
        "waveform geometry must not expand into hundreds of GUI nodes",
      );
      assert.ok(Number(overview.frame.backend.guiBatches) > 0);
      assert.ok(Number(overview.frame.backend.glyphPages) > 0);
      const glyphTexts = initial.detailed.nodes.flatMap(({ content }) =>
        content.kind === "text" ? [content.text] : [],
      );
      for (const icon of Object.values(ICON_CODE_POINTS))
        assert.ok(glyphTexts.includes(icon), `missing Nerd Font icon ${icon}`);

      assert.equal(
        overview.inspection.resources.filter(
          ({ kind, source, status }) =>
            kind === 10 && source.includes("generated-") && status === "loaded",
        ).length,
        6,
        "three skins, waveform and projector motion clips must all be resident",
      );
      assert.ok(overview.frame.drawCalls > 0 && overview.frame.triangles > 0);
      assert.equal(overview.frame.backend.failedDrawCalls, 0);
      assert.ok(overview.summary.coverage > 0.08);
      assert.deepEqual(overview.inspection.renderDiagnostics, []);

      // UPLINK is mounted disabled while SCAN runs. Its disabled lane is an
      // ordinary named part property and paints a solid dim fill.
      const disabledUplink = semanticNode(initial.semantic, "button", "UPLINK");
      assert.equal(disabledUplink.enabled, false);
      assert.deepEqual(disabledUplink.actions, []);
      const auroraDisabled = [0.2, 0.35, 0.42, 0.45] as const;
      const disabledLane = {
        color: await partValue(
          initial.semantic,
          disabledUplink,
          "background_disabled",
          "color",
        ),
        opacity: await partValue(
          initial.semantic,
          disabledUplink,
          "background_disabled",
          "opacity",
        ),
      };
      (disabledLane.color.value as readonly number[]).forEach(
        (value, channel) =>
          near(value, auroraDisabled[channel]!, "disabled UPLINK colour"),
      );
      near(Number(disabledLane.opacity.value), 0.45, "disabled UPLINK opacity");
      const disabledRegions = await withDetailView(async () => {
        await g.capture("gui-detail-uplink-disabled");
        return uplinkRegions("gui-detail-uplink-disabled", disabledUplink);
      });
      // Straight linear colour at alpha 0.45 x opacity 0.45 over the
      // measured panel background, encoded once for display.
      const disabledAlpha = auroraDisabled[3] * 0.45;
      const expectedDisabled = auroraDisabled
        .slice(0, 3)
        .map((value, channel) =>
          linearToSrgb(
            value * disabledAlpha +
              srgbToLinear(disabledRegions.gap.mean[channel]!) *
                (1 - disabledAlpha),
          ),
        );
      await recordRegions("uplink-disabled", {
        ...disabledRegions,
        expectedDisabled,
      });
      disabledRegions.fill.mean.forEach((value, channel) =>
        assert.ok(
          Math.abs(value - expectedDisabled[channel]!) < 12,
          `disabled UPLINK fill ${JSON.stringify(disabledRegions.fill.mean)} is not ${JSON.stringify(expectedDisabled)}`,
        ),
      );

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

      // SCAN standby enables UPLINK; the skin transition returns its part
      // properties to the idle lane and the captured fill changes with them.
      current = await waitForGui(
        ({ semantic }) => semanticNode(semantic, "button", "UPLINK").enabled,
      );
      const enabledUplink = semanticNode(current.semantic, "button", "UPLINK");
      assert.equal(enabledUplink.id, disabledUplink.id);
      const enabledLane = await waitForIdleLane(
        current.semantic,
        enabledUplink,
      );
      const enabledRegions = await withDetailView(async () => {
        await g.capture("gui-detail-uplink-enabled");
        return uplinkRegions("gui-detail-uplink-enabled", enabledUplink);
      });
      await recordRegions("uplink-enabled", {
        ...enabledRegions,
        lane: enabledLane,
      });
      assert.ok(
        meanDifference(enabledRegions.fill, disabledRegions.fill) > 8,
        "enabling UPLINK did not change its captured fill",
      );
      assert.ok(
        meanDifference(enabledRegions.gap, disabledRegions.gap) < 4,
        "UPLINK comparison background moved between captures",
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
      // Running SCAN disables UPLINK again; its skin transition animates the
      // background part's ordinary properties into the disabled sample.
      current = await waitForGui(
        ({ semantic }) => !semanticNode(semantic, "button", "UPLINK").enabled,
      );
      await waitForPartOpacity(current.semantic, enabledUplink, 0.45);
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
      // inspect() collects independently timed entity and controller pages.
      // Await the rendered property as well as the later controller page so
      // the assertion cannot compare samples from opposite sides of a tick.
      const advancedPulse = await g.waitFor(
        (inspection) =>
          waveformAnimation(inspection, true).time >
            capturedWavePulse.time + 0.12 &&
          waveformX(inspection, true) <
            waveformX(pulseFrame.inspection, true) - 0.3,
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
      current = await waitForGui(
        ({ semantic }) => semanticNode(semantic, "button", "UPLINK").enabled,
      );
      // The second disabled -> idle transition reuses UPLINK's skin
      // controller; it must blend back to the idle lane rather than keep the
      // disabled sample, and paint the enabled fill again.
      const reenabledLane = await waitForIdleLane(
        current.semantic,
        enabledUplink,
      );
      const reenabledRegions = await withDetailView(async () => {
        await g.capture("gui-detail-uplink-reenabled");
        return uplinkRegions("gui-detail-uplink-reenabled", enabledUplink);
      });
      await recordRegions("uplink-reenabled", {
        ...reenabledRegions,
        lane: reenabledLane,
      });
      assert.ok(
        meanDifference(reenabledRegions.fill, enabledRegions.fill) < 6,
        `re-enabled UPLINK fill ${JSON.stringify(reenabledRegions.fill.mean)} is not the enabled fill ${JSON.stringify(enabledRegions.fill.mean)}`,
      );
      assert.ok(
        meanDifference(reenabledRegions.fill, disabledRegions.fill) > 8,
        "re-enabled UPLINK still paints its disabled fill",
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

      // Neon reskins the same controls through shape-material lanes instead
      // of drawing assets, preserving node identity and focus throughout.
      const neon = await g.call<GuiSemanticTree>(
        "galleryGuiAction",
        { role: "button", name: "NEON" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "neon",
      );
      const neonState = await waitForGui();
      assertRetainedGuiNodes(identityBeforeSkin, neonState.semantic, true);
      assert.deepEqual(neon.focused, identityBeforeSkin.focused);
      assert.deepEqual(neonState.semantic.focused, identityBeforeSkin.focused);
      const neonFrame = await g.capture("gui-demo-neon-active");
      const neonCore = dynamicProperty(
        neonFrame.inspection,
        "gui-projector-core",
        "accent",
      );
      assert.notDeepEqual(
        neonCore,
        emberCore,
        "Neon did not recolor the projector cube",
      );
      assert.equal(
        waveformAnimation(neonFrame.inspection).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        waveformAnimation(neonFrame.inspection, true).id,
        projectorBeforeSkin.wavePulseController,
        "reskin replaced the waveform pulse controller",
      );
      const neonDifference = await g.difference(
        "gui-demo-ember-active",
        "gui-demo-neon-active",
      );
      assert.ok(neonDifference.changedPixels > 1_000);
      assert.ok(neonDifference.meanAbsoluteChannelDifference > 1);
      assert.equal(neonFrame.frame.backend.failedDrawCalls, 0);
      assert.deepEqual(neonFrame.inspection.renderDiagnostics, []);

      // Detailed Neon regions. Every rectangle derives from evaluated bounds
      // plus the layout expectations restated here.
      await withDetailView(async () => {
        const detail = await waitForGui();
        await g.capture("gui-detail-neon");
        const evidence: Record<string, unknown> = {};

        // Six icon cells: each glyph lands in its documented cell and paints.
        const pulseButton = semanticNode(detail.semantic, "button", "PULSE");
        const [px, py, pw, ph] = pulseButton.bounds;
        const title = textBounds(detail, "GUI DEMO");
        const spanButton = semanticNode(detail.semantic, "button", "SPAN");
        const pulseIcon = textBounds(detail, ICON_CODE_POINTS.pulse);
        near(pulseIcon[0] + pulseIcon[2] / 2, px + 0.45, "PULSE icon x");
        near(pulseIcon[1] + pulseIcon[3] / 2, py + ph / 2, "PULSE icon y");
        assert.ok(pulseIcon[0] + pulseIcon[2] <= px + 1.08);
        for (const skin of ["aurora", "ember", "neon"] as const) {
          const [bx, by, , bh] = semanticNode(
            detail.semantic,
            "button",
            skin.toUpperCase(),
          ).bounds;
          const icon = textBounds(detail, ICON_CODE_POINTS[skin]);
          near(icon[0] + icon[2] / 2, bx + 0.125, `${skin} icon x`);
          near(icon[1] + icon[3] / 2, by + bh / 2, `${skin} icon y`);
          assert.ok(icon[0] + icon[2] <= bx + 0.27);
        }
        const cube = textBounds(detail, ICON_CODE_POINTS.cube);
        near(cube[0], px, "cube icon x");
        near(cube[1] + cube[3] / 2, title[1] + title[3] / 2, "cube icon y");
        assert.ok(cube[0] + cube[2] <= title[0]);
        const signal = textBounds(detail, ICON_CODE_POINTS.signal);
        near(
          signal[0] + signal[2],
          spanButton.bounds[0] + spanButton.bounds[2] + 0.67,
          "signal icon x",
        );
        near(signal[1] + signal[3] / 2, title[1] + title[3] / 2, "signal y");
        const iconRects = Object.fromEntries(
          Object.entries(ICON_CODE_POINTS).map(([name, glyph]) => {
            const [x, y, width, height] = textBounds(detail, glyph);
            return [name, [x, y, x + width, y + height] as LogicalRect];
          }),
        );
        const [ix, iy, iw, ih] = semanticNode(
          detail.semantic,
          "textInput",
          "CALLSIGN",
        ).bounds;
        // PULSE bands above/below its centre row, then farther out.
        const band = (x0: number, top: boolean): LogicalRect => [
          px + x0,
          top ? py + 0.04 : py + ph - 0.12,
          px + x0 + 0.3,
          top ? py + 0.12 : py + ph - 0.04,
        ];
        const neon = await regionStats<string>("gui-detail-neon", {
          ...iconRects,
          nearTop: band(0.3, true),
          nearBottom: band(0.3, false),
          middle: band(1.2, false),
          farTop: band(2.6, true),
          farBottom: band(2.6, false),
          halo: [px - 0.06, py + 0.25, px - 0.02, py + ph - 0.25],
          haloBaseline: [px - 0.2, py + 0.25, px - 0.16, py + ph - 0.25],
          ringTop: [ix + 0.35 * iw, iy + 0.004, ix + 0.65 * iw, iy + 0.02],
          ringBottom: [
            ix + 0.35 * iw,
            iy + ih - 0.02,
            ix + 0.65 * iw,
            iy + ih - 0.004,
          ],
          ringRight: [
            ix + iw - 0.02,
            iy + 0.3 * ih,
            ix + iw - 0.004,
            iy + 0.7 * ih,
          ],
          hollow: [ix + 0.62 * iw, iy + 0.3 * ih, ix + 0.9 * iw, iy + 0.7 * ih],
        });
        evidence.regions = neon;
        for (const name of Object.keys(ICON_CODE_POINTS)) {
          const stats = neon[name]!;
          assert.ok(
            stats.max[1] > 170 && stats.max[2] > 170,
            `${name} icon cell did not paint its glyph: ${JSON.stringify(stats)}`,
          );
        }

        // PULSE fill falls off radially from its icon cell: symmetric above
        // and below the centre, decreasing outward, flat beyond the radius.
        const { nearTop, nearBottom, middle, farTop, farBottom } = neon;
        assert.ok(
          Math.abs(nearTop!.mean[1] - nearBottom!.mean[1]) < 10 &&
            nearBottom!.mean[1] > middle!.mean[1] + 12 &&
            middle!.mean[1] > farBottom!.mean[1] + 12 &&
            Math.abs(farTop!.mean[1] - farBottom!.mean[1]) < 8,
          "PULSE lost its radial gradient",
        );

        // Neon's resting PULSE halo brightens only the band beside the
        // button; the semantic bounds stay on the button itself.
        assert.ok(
          neon.halo!.mean[1] > neon.haloBaseline!.mean[1] + 20 &&
            neon.halo!.mean[2] > neon.haloBaseline!.mean[2] + 20,
          "PULSE halo is missing",
        );
        near(pw, 3.24, "PULSE semantic width excludes its halo");

        // The focused callsign ring strokes the edge and leaves the centre clear.
        const callsign = semanticNode(detail.semantic, "textInput", "CALLSIGN");
        assert.deepEqual(detail.semantic.focused, {
          id: callsign.id,
          lifetime: callsign.lifetime,
        });
        assert.ok(
          [neon.ringTop!, neon.ringBottom!, neon.ringRight!].every(
            ({ mean: [r, , b] }) => r > 150 && r > b + 30,
          ),
          "focused callsign ring lost its Neon focus colour",
        );
        assert.ok(
          neon.hollow!.mean[0] < 60 &&
            neon.hollow!.mean[2] > neon.hollow!.mean[0],
          "focus ring filled the callsign centre",
        );

        // SPAN widens its frame; corner radii and border widths keep their
        // authored metres, so every corner and border region is unchanged.
        const narrow = spanFrame(detail);
        near(narrow.bounds[2], 1.2, "narrow SPAN width");
        const frameRects = ([x, y, width, height]: readonly number[]) => ({
          topLeft: [x! - 0.03, y! - 0.03, x! + 0.15, y! + 0.15] as LogicalRect,
          bottomLeft: [
            x! - 0.03,
            y! + height! - 0.15,
            x! + 0.15,
            y! + height! + 0.03,
          ] as LogicalRect,
          left: [
            x! - 0.03,
            y! + 0.12,
            x! + 0.06,
            y! + height! - 0.12,
          ] as LogicalRect,
          topRight: [
            x! + width! - 0.15,
            y! - 0.03,
            x! + width! + 0.03,
            y! + 0.15,
          ] as LogicalRect,
          bottomRight: [
            x! + width! - 0.15,
            y! + height! - 0.15,
            x! + width! + 0.03,
            y! + height! + 0.03,
          ] as LogicalRect,
          top: [
            x! + width! / 2 - 0.1,
            y! - 0.03,
            x! + width! / 2 + 0.1,
            y! + 0.06,
          ] as LogicalRect,
        });
        // Beyond the narrow frame but inside the wide one, clear of its text.
        const widened: LogicalRect = [
          narrow.bounds[0] + 1.4,
          narrow.bounds[1] + 0.12,
          narrow.bounds[0] + 1.7,
          narrow.bounds[1] + 0.26,
        ];
        const narrowRegions = await regionStats("gui-detail-neon", {
          ...frameRects(narrow.bounds),
          widened,
        });
        await g.call(
          "galleryGuiAction",
          { role: "button", name: "SPAN" },
          { kind: "press" },
        );
        await g.page.waitForFunction(
          () => document.querySelector("#gui-span")?.textContent === "wide",
        );
        const wideState = await waitForGui(
          (state) => Math.abs(spanFrame(state).bounds[2] - 2.0) < 1e-4,
        );
        const wide = spanFrame(wideState);
        assert.equal(wide.id, narrow.id);
        near(wide.bounds[0], narrow.bounds[0], "SPAN keeps its left edge");
        // The flex spacer after the frame absorbs the resize, so the SPAN
        // button and signal icon that follow it in tree order stay in place.
        near(
          semanticNode(wideState.semantic, "button", "SPAN").bounds[0],
          semanticNode(detail.semantic, "button", "SPAN").bounds[0],
          "SPAN button x",
        );
        await g.capture("gui-detail-span-wide");
        const wideRegions = await regionStats("gui-detail-span-wide", {
          ...frameRects(wide.bounds),
          widened,
        });
        evidence.span = {
          narrow: narrow.bounds,
          wide: wide.bounds,
          narrowRegions,
          wideRegions,
        };
        assert.ok(
          meanDifference(wideRegions.widened, narrowRegions.widened) > 20,
          "SPAN did not paint its widened frame",
        );
        for (const key of Object.keys(
          frameRects(narrow.bounds),
        ) as (keyof ReturnType<typeof frameRects>)[]) {
          const difference = meanDifference(
            narrowRegions[key],
            wideRegions[key],
          );
          assert.ok(
            difference < 4,
            `SPAN ${key} region changed by ${difference} when resized`,
          );
        }
        await g.call(
          "galleryGuiAction",
          { role: "button", name: "SPAN" },
          { kind: "press" },
        );
        await g.page.waitForFunction(
          () => document.querySelector("#gui-span")?.textContent === "narrow",
        );
        await waitForGui(
          (state) => Math.abs(spanFrame(state).bounds[2] - 1.2) < 1e-4,
        );
        await recordRegions("neon", evidence);
      });

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

/** The gallery panel's cache policy, restated independently of scene.tsx. */
const CACHE_DIRECT_DISTANCE = 20;
const CACHE_TEXELS_PER_METRE = 80;
const CACHE_REFRESH_HZ = 15;
const CACHE_HYSTERESIS = 0.1;

/**
 * Cache image size in a band: content metres times the band's halved density,
 * rounded up. Surface sizes are f32 fields, so 7.4 m reads as 7.40000010 m;
 * like the renderer, a 1e-4 texel tolerance keeps that error from adding a texel.
 */
function expectedCacheSize(band: number): readonly [number, number] {
  const density = CACHE_TEXELS_PER_METRE / 2 ** (band - 1);
  return [
    Math.ceil(Math.fround(7.4) * density - 1e-4),
    Math.ceil(Math.fround(4.8) * density - 1e-4),
  ];
}

/**
 * Cached versus direct panel tolerance, taken from the retained-gui text
 * comparison: at most 3.5% of panel pixels may differ by more than 64 levels
 * and bright text/glow masks must overlap by at least 0.85. The band-one
 * image resamples the panel once more than direct drawing, which moves glyph
 * and border edges by about one texel.
 */
const CACHE_COMPARISON = {
  channelThreshold: 64,
  maxChangedFraction: 0.035,
  minTextAgreement: 0.85,
} as const;

interface CacheObservation {
  readonly label: string;
  readonly record: SurfaceCacheRecord | undefined;
  readonly backend: Record<string, unknown>;
  readonly repaints: number;
  readonly allocations: number;
  readonly reuses: number;
  readonly direct: number;
}

function cacheDelta(before: CacheObservation, after: CacheObservation) {
  return {
    repaints: after.repaints - before.repaints,
    allocations: after.allocations - before.allocations,
    reuses: after.reuses - before.reuses,
  };
}

function decodeRegion(region: {
  width: number;
  height: number;
  pixels: string;
}): RgbaFrame {
  return {
    width: region.width,
    height: region.height,
    pixels: new Uint8Array(Buffer.from(region.pixels, "base64")),
  };
}

/** Bright text, icon and glow pixels of the panel. */
function isBright(r: number, g: number, b: number): boolean {
  return Math.max(r, g, b) >= 170;
}

test("Gallery GUI panel caches distant presentation within direct-rendering tolerance", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI demo Surface cache",
    {
      ...galleryEnvironment,
      evidenceParent: resolve("target/reviews/gui-demo-surface-cache"),
    },
    context.signal,
    async (scenario) => {
      const started = performance.now();
      const g = await openGallery(scenario, { initialPage: "gui" });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      // Keep the pointer off the canvas: hover would promote the panel.
      await g.page.mouse.move(1, 1);
      // Compare the vector panel alone, as the gallery's isolation control
      // intends, so projector geometry never occludes a placed panel.
      await g.page.locator("#gui-vector-only").click();
      await g.waitFor(
        (inspection) =>
          !inspection.entities.some(({ metadata }) =>
            metadata.symbolicId?.startsWith("gui-projector-"),
          ),
      );
      await g.page.mouse.move(1, 1);
      const evidence: Record<string, unknown> = {};
      const record = async (name: string, value: unknown) => {
        evidence[name] = value;
        await writeFile(
          join(scenario.evidence.directory, "surface-cache-evidence.json"),
          JSON.stringify(
            evidence,
            (_key, value) =>
              typeof value === "bigint" ? String(value) : value,
            2,
          ) + "\n",
        );
      };
      const observe = async (label: string): Promise<CacheObservation> => {
        const entity = await g.call<bigint | undefined>(
          "galleryEntityId",
          "gui-demo",
        );
        const { frame } = await g.capture(label);
        const backend = frame.backend;
        const records = backend.surfaceCaches as
          | readonly SurfaceCacheRecord[]
          | undefined;
        assert.ok(records, "Surface cache diagnostics are unavailable");
        const observation = {
          label,
          record:
            entity === undefined
              ? undefined
              : records.find((entry) => entry.entity === entity),
          backend,
          repaints: Number(backend.totalSurfaceCacheRepaints),
          allocations: Number(backend.totalSurfaceCacheAllocations),
          reuses: Number(backend.totalSurfaceCacheReuses),
          direct: Number(backend.totalSurfaceCacheDirect),
        };
        const { ingress: _ingress, ...stats } = backend;
        await record(label, { ...stats, records });
        return observation;
      };
      // Poll completed frames until the panel's record satisfies `ready`.
      const observeUntil = async (
        label: string,
        ready: (record: SurfaceCacheRecord | undefined) => boolean,
      ) => {
        const deadline = performance.now() + 10_000;
        for (;;) {
          const observation = await observe(label);
          if (ready(observation.record)) return observation;
          assert.ok(
            performance.now() < deadline,
            `${label}: cache record stayed ${JSON.stringify(observation.record, (_key, value) => (typeof value === "bigint" ? String(value) : value))}`,
          );
        }
      };
      const reused = (band: number) => (entry?: SurfaceCacheRecord) =>
        entry?.mode === "reused" && entry.band === band;
      const assertWarm = (observation: CacheObservation, band: number) => {
        const [width, height] = expectedCacheSize(band);
        assert.equal(observation.record?.mode, "reused");
        assert.equal(observation.record?.band, band);
        assert.equal(observation.record?.width, width);
        assert.equal(observation.record?.height, height);
        assert.equal(observation.record?.residentBytes, 4 * width * height);
        // A warm frame composites the unchanged image: no raster or upload work.
        assert.equal(observation.backend.surfaceCacheReuses, 1);
        assert.equal(observation.backend.surfaceCacheRepaints, 0);
        assert.equal(observation.backend.surfaceCacheDirect, 0);
        assert.equal(observation.backend.surfaceCacheAllocations, 0);
        assert.equal(observation.backend.uploadedBytes, 0);
        assert.equal(
          observation.backend.surfaceCacheResidentBytes,
          4 * width * height,
        );
      };
      const setMode = async (mode: "automatic" | "cached" | "direct") => {
        await g.page.locator("#gui-surface-cache").selectOption(mode);
        await g.settle();
      };
      const place = (distance: number, replace = true) =>
        g.call("faceGalleryGuiToCamera", 0.92, distance, replace);
      const panelBounds = async () => {
        const corners = await g.call<readonly { x: number; y: number }[]>(
          "projectGalleryPoints",
          "gui-demo",
          [
            [-3.7, 2.4, 0],
            [3.7, 2.4, 0],
            [-3.7, -2.4, 0],
            [3.7, -2.4, 0],
          ],
        );
        return [
          Math.min(...corners.map((p) => p.x)),
          Math.min(...corners.map((p) => p.y)),
          Math.max(...corners.map((p) => p.x)),
          Math.max(...corners.map((p) => p.y)),
        ] as const;
      };
      // Cached (actual) against direct (expected) at one placement and camera.
      const compareWithDirect = async (name: string, band: number) => {
        const cached = await observeUntil(`${name}-cached`, reused(band));
        assertWarm(cached, band);
        await setMode("direct");
        const direct = await observeUntil(
          `${name}-direct`,
          (entry) => entry === undefined,
        );
        assert.equal(direct.backend.surfaceCacheEntries, 0);
        assert.equal(direct.backend.surfaceCacheResidentBytes, 0);
        assert.equal(direct.backend.surfaceCacheDirect, 0);
        const bounds = await panelBounds();
        const [expected, actual] = await Promise.all(
          [direct.label, cached.label].map((label) =>
            g
              .call<{
                width: number;
                height: number;
                pixels: string;
              }>("viewerCaptureRegionPixels", label, bounds)
              .then(decodeRegion),
          ),
        );
        const difference = compareFrames(
          expected!,
          actual!,
          CACHE_COMPARISON.channelThreshold,
        );
        const expectedText = mask(expected!, isBright);
        const actualText = mask(actual!, isBright);
        const comparison = {
          ...difference,
          width: expected!.width,
          height: expected!.height,
          directTextPixels: count(expectedText),
          cachedTextPixels: count(actualText),
          textAgreement: intersectionOverUnion(expectedText, actualText),
        };
        await Promise.all([
          writeFile(
            join(scenario.evidence.directory, `${name}-expected.png`),
            encodePng(expected!),
          ),
          writeFile(
            join(scenario.evidence.directory, `${name}-actual.png`),
            encodePng(actual!),
          ),
          writeFile(
            join(scenario.evidence.directory, `${name}-diff.png`),
            encodePng(
              differenceImage(
                expected!,
                actual!,
                CACHE_COMPARISON.channelThreshold,
              ),
            ),
          ),
          record(`${name}-comparison`, comparison),
        ]);
        assert.ok(comparison.directTextPixels > 500, "panel text is missing");
        assert.ok(
          comparison.changedFraction <= CACHE_COMPARISON.maxChangedFraction &&
            comparison.textAgreement >= CACHE_COMPARISON.minTextAgreement,
          `${name}: cached panel differs from direct presentation: ${JSON.stringify(comparison)}`,
        );
        await setMode("automatic");
        return comparison;
      };

      // Static content makes repaint counts exact: stop the scrolling trace.
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));

      // The authored camera is inside the direct distance.
      const authored = await g.inspect();
      const camera = transform(authored);
      const panel = fieldsWith(authored, "gui-demo", "sx");
      const authoredDistance = Math.hypot(
        ...["x", "y", "z"].map(
          (axis) => Number(panel[axis]) - Number(camera[axis]),
        ),
      );
      assert.ok(authoredDistance < CACHE_DIRECT_DISTANCE);
      const near = await observeUntil(
        "surface-cache-authored-near",
        (entry) => entry?.mode === "near",
      );
      assert.equal(near.record?.band, 0);
      assert.equal(near.record?.residentBytes, 0);
      assert.equal(near.backend.surfaceCacheDirect, 1);
      assert.equal(near.backend.surfaceCacheRepaints, 0);
      assert.equal(near.backend.surfaceCacheResidentBytes, 0);
      await record("authored-distance", authoredDistance);

      // Cold band-one frame: one allocation and one repaint, then reuse.
      const beforeCold = near;
      await place(26, false);
      const warm = await observeUntil("surface-cache-band1-warm", reused(1));
      assertWarm(warm, 1);
      assert.deepEqual(
        { ...cacheDelta(beforeCold, warm), reuses: 0 },
        { repaints: 1, allocations: 1, reuses: 0 },
      );
      assert.equal(warm.record?.repaints, 1);
      const aurora = await compareWithDirect("surface-cache-aurora-band1", 1);

      // Hysteresis: 42 m stays in band one (boundary 40 m + 10%), 46 m moves
      // to band two with one resize, 38 m stays there and 34 m returns.
      const bands: Array<readonly [number, number, number]> = [
        [2 * CACHE_DIRECT_DISTANCE * (1 + CACHE_HYSTERESIS) - 2, 1, 0],
        [2 * CACHE_DIRECT_DISTANCE * (1 + CACHE_HYSTERESIS) + 2, 2, 1],
        [2 * CACHE_DIRECT_DISTANCE * (1 - CACHE_HYSTERESIS) + 2, 2, 0],
        [2 * CACHE_DIRECT_DISTANCE * (1 - CACHE_HYSTERESIS) - 2, 1, 1],
      ];
      let previous = await observeUntil("surface-cache-readmitted", reused(1));
      const sizes: Array<readonly [number, number]> = [];
      for (const [distance, band, allocations] of bands) {
        await place(distance);
        const current = await observeUntil(
          `surface-cache-${distance}m`,
          reused(band),
        );
        assertWarm(current, band);
        assert.deepEqual(
          { ...cacheDelta(previous, current), reuses: 0 },
          { repaints: allocations, allocations, reuses: 0 },
          `${distance} m`,
        );
        sizes.push([current.record!.width, current.record!.height]);
        previous = current;
      }
      assert.ok(sizes[1]![0] < sizes[0]![0] && sizes[1]![1] < sizes[0]![1]);

      // A viewport resize keeps the fixed texel density: no repaint.
      await place(26);
      previous = await observeUntil("surface-cache-before-resize", reused(1));
      const viewport = g.page.viewportSize()!;
      await g.page.setViewportSize({
        width: viewport.width - 160,
        height: viewport.height - 80,
      });
      try {
        const resized = await observeUntil(
          "surface-cache-viewport-resized",
          reused(1),
        );
        assertWarm(resized, 1);
        assert.deepEqual(cacheDelta(previous, resized).repaints, 0);
        assert.deepEqual(cacheDelta(previous, resized).allocations, 0);
      } finally {
        await g.page.setViewportSize(viewport);
      }
      previous = await observeUntil(
        "surface-cache-viewport-restored",
        reused(1),
      );

      // A skin switch is paint, not a resource change: it repaints at the
      // band's cadence and settles on the latest state.
      await g.call(
        "galleryGuiAction",
        { role: "button", name: "EMBER" },
        { kind: "press" },
      );
      await g.page.waitForFunction(
        () => document.querySelector("#gui-skin")?.textContent === "ember",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      const reskinned = await observeUntil(
        "surface-cache-ember-settled",
        reused(1),
      );
      const reskin = cacheDelta(previous, reskinned);
      const reskinSeconds =
        (reskinned.record!.paintedAtMs - previous.record!.paintedAtMs) / 1000;
      assert.equal(reskin.allocations, 0);
      assert.ok(reskin.repaints >= 1, "the skin switch never repainted");
      assert.ok(
        reskin.repaints <= Math.ceil(reskinSeconds * CACHE_REFRESH_HZ) + 1,
        `skin repaints exceeded the ${CACHE_REFRESH_HZ} Hz cap: ${JSON.stringify({ reskin, reskinSeconds })}`,
      );
      await record("ember-repaints", { ...reskin, reskinSeconds });
      const ember = await compareWithDirect("surface-cache-ember-band1", 1);

      // Continuous scanning keeps the trace current without exceeding the
      // cap: the cached image repaints at most at the cap, and either keeps
      // repainting or the renderer presents the changing panel directly.
      previous = await observeUntil("surface-cache-before-scan", reused(1));
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "enabled",
      );
      const scanStart = await observe("surface-cache-scan-start");
      await new Promise((resolve) => setTimeout(resolve, 1_000));
      const scanEnd = await observe("surface-cache-scan-end");
      const scan = {
        ...cacheDelta(scanStart, scanEnd),
        direct: scanEnd.direct - scanStart.direct,
      };
      const scanSeconds =
        (scanEnd.record!.paintedAtMs - scanStart.record!.paintedAtMs) / 1000;
      assert.ok(
        scan.repaints >= 2 || scan.direct > 0,
        `continuous scanning starved presentation: ${JSON.stringify(scan)}`,
      );
      assert.ok(
        scan.repaints <= Math.ceil(scanSeconds * CACHE_REFRESH_HZ) + 1,
        `scan repaints exceeded the ${CACHE_REFRESH_HZ} Hz cap: ${JSON.stringify({ scan, scanSeconds })}`,
      );
      if (scan.direct === 0) assert.equal(scan.allocations, 0);
      await record("scan-repaints", { ...scan, scanSeconds });
      await g.call(
        "galleryGuiAction",
        { role: "checkbox" },
        { kind: "toggle" },
      );
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-autoscan")?.textContent === "standby",
      );
      await new Promise((resolve) => setTimeout(resolve, SKIN_SETTLE_MS));
      previous = await observeUntil("surface-cache-scan-stopped", reused(1));

      // Hover promotes the distant panel to direct presentation at once.
      const pulse = await g.call<ProjectedGuiNode>(
        "galleryGuiPoint",
        { role: "button", name: "PULSE" },
        0.5,
        0.5,
      );
      await g.page.mouse.move(pulse.clientX, pulse.clientY);
      const hovered = await observeUntil(
        "surface-cache-hover",
        (entry) => entry?.mode === "interaction",
      );
      assert.equal(hovered.backend.surfaceCacheDirect, 1);
      assert.equal(hovered.backend.surfaceCacheReuses, 0);
      assert.equal(hovered.backend.surfaceCacheRepaints, 0);
      const heldHover = await observe("surface-cache-hover-held");
      assert.equal(heldHover.record?.mode, "interaction");
      assert.equal(cacheDelta(hovered, heldHover).repaints, 0);
      // Leaving repaints before the changed image can be shown again. The
      // canvas corner shows only background, so the GUI observes the exit.
      const canvas = (await g.page.locator("#ipp-world-canvas").boundingBox())!;
      await g.page.mouse.move(canvas.x + 8, canvas.y + 8);
      const left = await observeUntil("surface-cache-hover-left", reused(1));
      assert.ok(cacheDelta(heldHover, left).repaints >= 1);
      assert.ok(left.record!.paintedAtMs > previous.record!.paintedAtMs);

      // Removing the GUI World content releases its image.
      await g.call("releaseGalleryGuiTransform");
      await g.navigate("shapes");
      const released = await observeUntil("surface-cache-released", () => true);
      assert.deepEqual(released.backend.surfaceCaches, []);
      assert.equal(released.backend.surfaceCacheEntries, 0);
      assert.equal(released.backend.surfaceCacheResidentBytes, 0);
      await record("summary", {
        aurora,
        ember,
        durationMs: performance.now() - started,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});
