import type { RenderStatisticsSnapshot } from "@ipp/client";
import { guiNodeStyleOffset } from "@ipp/client";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import { writeFile } from "node:fs/promises";
import test from "node:test";
import type {
  ComponentDescriptor,
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
  pixelDifference,
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
  dashboard: "\ueacd",
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
    ({ data, style }) =>
      data.kind === "container" &&
      data.containerKind === "stack" &&
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
    ({ data }) => data.kind === "text" && data.text === text,
  );
  assert.equal(matches.length, 1, `expected one text leaf ${text}`);
  const semantic = state.semantic.nodes.find(({ id }) => id === matches[0]!.id);
  assert.ok(semantic, `text leaf ${text} is absent from semantics`);
  return semantic.bounds;
}

const guiEnvironment = {
  ...galleryEnvironment,
  evidenceParent: resolve("target/integration-artifacts/gallery-gui"),
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

/**
 * Waveform scan and pulse probes. Their controllers animate one GuiRoot
 * `node_style.position` each; driver offsets map back to nodes through the
 * generated row offsets of the connected contract, never a restated layout.
 */
function waveformProbes(guiRoot: ComponentDescriptor) {
  const first = guiNodeStyleOffset(guiRoot, 1, "position");
  const stride = guiNodeStyleOffset(guiRoot, 2, "position") - first;

  /** Node whose position this offset names; other offsets name no node. */
  const drivenNode = (offset: number): number | undefined => {
    const relative = offset - first;
    if (relative < 0 || relative % stride !== 0) return undefined;
    return relative / stride + 1;
  };

  const animation = (inspection: Inspection, pulse = false) => {
    const target = sceneEntity(inspection, "gui-demo").id;
    const controller = inspection.controllers?.find(
      ({ description }) =>
        description.drivers.length === 1 &&
        description.looping === !pulse &&
        description.drivers.every((driver) => driver.target === target) &&
        description.drivers.some(
          (driver) =>
            "offsets" in driver.property &&
            driver.property.offsets?.length === 1 &&
            drivenNode(driver.property.offsets[0]!) !== undefined,
        ),
    );
    assert.ok(
      controller,
      `missing GUI waveform ${pulse ? "pulse" : "scan"} animation`,
    );
    return controller;
  };

  /** Effective node style row of the node a waveform controller animates. */
  const style = (
    inspection: Inspection,
    pulse: boolean,
  ): Record<string, unknown> => {
    const property = animation(inspection, pulse).description.drivers[0]!
      .property as { offsets?: readonly number[] };
    const node = drivenNode(property.offsets?.[0] ?? -1);
    assert.ok(node !== undefined, "waveform driver targets no node position");

    // The browser helper hands rows tables over as records keyed by slot.
    const table = fieldsWith(inspection, "gui-demo", "node_style")
      .node_style as unknown as {
      rows: Readonly<Record<number, Record<string, unknown>>>;
    };
    const row = table.rows[node];
    assert.ok(row, `waveform node ${node} has no style row`);
    return row;
  };

  return {
    animation,

    /** Effective horizontal position of the scan (or pulse) waveform node. */
    x(inspection: Inspection, pulse = false): number {
      const position = style(inspection, pulse).position;
      assert.ok(Array.isArray(position) && position.length === 2);
      return (position as readonly number[])[0]!;
    },

    /** Effective opacity of the scan (or pulse) waveform node. */
    opacity(inspection: Inspection, pulse = false): number {
      return Number(style(inspection, pulse).opacity);
    },
  };
}

function waveformViewport(state: GalleryGuiState) {
  const node = state.detailed.nodes.find(
    ({ data, style }) =>
      data.kind === "container" &&
      data.containerKind === "scrollView" &&
      style.enabled === false &&
      Math.abs((style.width ?? 0) - 3.54) < 1e-5 &&
      Math.abs((style.height ?? 0) - 0.46) < 1e-5,
  );
  assert.ok(node, "missing retained waveform viewport");
  return node;
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
      const waveform = waveformProbes(
        await g.call<ComponentDescriptor>("galleryGuiRootDescriptor"),
      );
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
      // `background` reads the node's live channels, which carry the animated
      // appearance; `background_disabled` reads its theme's authored disabled
      // state.
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
      assert.equal(coldDirect.frame.failedDrawCalls, 0);
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
        waveform.animation(loadingFrame.inspection).state,
        "playing",
      );
      assert.notEqual(
        waveform.animation(loadingFrame.inspection, true).state,
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
      assert.notEqual(waveform.animation(resourcesReady).state, "playing");
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
      // The event log VirtualList nests inside the telemetry ScrollView.
      telemetryScrollViews(initial);

      const buttons = initial.semantic.nodes.filter(
        ({ role }) => role === "button",
      );
      assert.deepEqual(buttons.map(({ name }) => name).sort(), [
        "AURORA",
        "EMBER",
        "NEON",
        "PULSE",
        "PURGE",
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
      assert.equal(waveform.animation(overviewInspection).state, "playing");
      assertNoScanner(overviewInspection);
      const initialWaveform = waveform.animation(overviewInspection);
      await g.waitFor((inspection) => {
        const time = waveform.animation(inspection).time;
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
        initial.detailed.nodes.filter(({ data }) => data.kind === "drawing")
          .length,
        3,
        "waveform geometry must not expand into hundreds of GUI nodes",
      );
      assert.ok(Number(overview.frame.statistics!.gui!.guiBatches) > 0);
      assert.ok(Number(overview.frame.statistics!.gui!.glyphPages) > 0);
      const glyphTexts = initial.detailed.nodes.flatMap(({ data }) =>
        data.kind === "text" ? [data.text] : [],
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
      assert.equal(overview.frame.failedDrawCalls, 0);
      assert.ok(overview.summary.coverage > 0.08);
      assert.deepEqual(overview.inspection.renderDiagnostics, []);

      // UPLINK is mounted disabled while SCAN runs. Its disabled state is an
      // ordinary theme part row and paints a solid dim fill.
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
        (inspection) => waveform.animation(inspection).state === "paused",
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

      const pausedWaveform = waveform.animation(await g.inspect());
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
        waveform.animation(await g.inspect()).time,
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
      assert.ok(
        waveform.x(loopEnd.inspection) < -3.5,
        "SCAN must travel left through the second authored tile",
      );
      const seamDifference = await waveformDifference(
        "gui-waveform-loop-start",
        "gui-waveform-loop-end",
      );
      await recordWaveform("seam", {
        difference: seamDifference,
        position: waveform.x(loopEnd.inspection),
        controller: waveform.animation(loopEnd.inspection),
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
        waveform.animation(await g.inspect()).id,
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
      assert.equal(obliqueFrame.frame.failedDrawCalls, 0);
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
        const controller = waveform.animation(inspection, true);
        return controller.state === "playing" && controller.time > 0.32;
      });
      const activeWavePulse = waveform.animation(pulsing, true);
      assert.equal(activeWavePulse.state, "playing");
      assert.equal(
        waveform.animation(pulsing).state,
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
      const capturedWavePulse = waveform.animation(pulseFrame.inspection, true);
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
      await recordWaveform("pulse", {
        started: activeWavePulse,
        controller: capturedWavePulse,
        scan: waveform.animation(pulseFrame.inspection),
        opacity: waveform.opacity(pulseFrame.inspection, true),
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
      assert.ok(
        Math.abs(waveform.opacity(pulseFrame.inspection) - 0.35) < 1e-6,
        "PULSE must preserve the visible paused sine trace",
      );
      const pulseToggle = await point("checkbox");
      await g.page.mouse.click(pulseToggle.clientX, pulseToggle.clientY);
      await g.waitFor(
        (inspection) =>
          waveform.animation(inspection).state === "playing" &&
          waveform.animation(inspection).time > 0.2,
      );
      // Running SCAN disables UPLINK again; its skin transition animates the
      // background part's ordinary properties into the disabled sample.
      current = await waitForGui(
        ({ semantic }) => !semanticNode(semantic, "button", "UPLINK").enabled,
      );
      await waitForPartOpacity(current.semantic, enabledUplink, 0.45);
      const toggledPulse = await g.capture("gui-waveform-pulse-scan-toggled");
      assert.ok(
        waveform.opacity(toggledPulse.inspection) > 0.85,
        "the moving sine must remain visible alongside PULSE",
      );
      assert.equal(
        waveform.animation(toggledPulse.inspection).state,
        "playing",
      );
      assert.equal(
        waveform.animation(toggledPulse.inspection, true).time,
        capturedWavePulse.time,
      );
      assert.equal(
        waveform.opacity(toggledPulse.inspection, true),
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
          waveform.animation(inspection, true).time >
            capturedWavePulse.time + 0.12 &&
          waveform.x(inspection, true) <
            waveform.x(pulseFrame.inspection, true) - 0.3,
      );
      assert.equal(waveform.animation(advancedPulse).state, "playing");
      assert.equal(waveform.animation(advancedPulse, true).state, "playing");
      assert.ok(
        waveform.x(advancedPulse, true) <
          waveform.x(pulseFrame.inspection, true) - 0.3,
        "PULSE must travel right to left alongside SCAN",
      );
      await recordWaveform("direction", {
        pulseStart: waveform.x(pulseFrame.inspection, true),
        pulseAdvanced: waveform.x(advancedPulse, true),
        scan: waveform.animation(advancedPulse),
        pulse: waveform.animation(advancedPulse, true),
      });
      const advancedPulseTime = waveform.animation(advancedPulse, true).time;
      await g.page.mouse.click(pulse.clientX, pulse.clientY);
      await g.waitFor((inspection) => {
        const controller = waveform.animation(inspection, true);
        return (
          controller.id === activeWavePulse.id &&
          controller.state === "playing" &&
          controller.time > 0.02 &&
          controller.time < advancedPulseTime - 0.1
        );
      });
      await g.waitFor(
        (inspection) =>
          waveform.animation(inspection, true).state === "completed",
      );
      await g.waitFor((inspection) => waveform.opacity(inspection, true) === 0);
      const finishedPulse = await g.capture("gui-waveform-pulse-finished");
      assert.equal(
        waveform.animation(finishedPulse.inspection).state,
        "playing",
        "pulse completion interrupted SCAN",
      );
      assert.equal(waveform.opacity(finishedPulse.inspection, true), 0);
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
        (inspection) => waveform.animation(inspection).state === "paused",
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
      assert.equal(controlsFrame.frame.failedDrawCalls, 0);
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
      // SCAN is a capsule switch whose knob slides to the right end while on
      // and the left end while off. Rows through the knob, inset from the
      // capsule ends, weigh each sample by its contrast with the row median
      // (the track), so the knob ink centroid falls on the knob's side.
      const knobInk = async (label: string) => {
        const [x, y, width, height] = semanticNode(
          current.semantic,
          "checkbox",
        ).bounds;
        const columns = 64;
        const rows = [0.3, 0.4, 0.5, 0.6, 0.7];
        const xs = Array.from(
          { length: columns },
          (_, column) => x + 0.06 + ((width - 0.12) * column) / (columns - 1),
        );
        const samples = await g.call<readonly (readonly number[])[]>(
          "sampleGalleryGuiCapture",
          label,
          rows.flatMap((row) =>
            xs.map((sampleX) => [sampleX, y + height * row] as const),
          ),
        );
        let weight = 0;
        let moment = 0;
        rows.forEach((_, row) => {
          const line = samples.slice(row * columns, (row + 1) * columns);
          const median = [0, 1, 2].map((channel) => {
            const values = line
              .map((pixel) => pixel[channel]!)
              .sort((a, b) => a - b);
            return values[values.length >> 1]!;
          });
          line.forEach((pixel, column) => {
            const contrast = Math.max(
              ...median.map((value, channel) =>
                Math.abs(pixel[channel]! - value),
              ),
            );
            const ink = Math.max(0, contrast - 24);
            weight += ink;
            moment += ink * xs[column]!;
          });
        });
        assert.ok(weight > 0, `${label} shows no SCAN knob`);
        return { centroid: moment / weight, centre: x + width / 2, weight };
      };
      const knobOn = await knobInk("gui-demo-overview");
      const knobOff = await knobInk("gui-demo-controls-active");
      await recordRegions("scan-knob", { on: knobOn, off: knobOff });
      assert.ok(
        knobOn.centroid > knobOn.centre + 0.1,
        `SCAN knob is not at the right end while on: ${JSON.stringify(knobOn)}`,
      );
      assert.ok(
        knobOff.centroid < knobOff.centre - 0.1,
        `SCAN knob is not at the left end while off: ${JSON.stringify(knobOff)}`,
      );
      // Pointer clicks take toggle focus without the focus ring: the
      // toggle's top border row after the click carries the blue track
      // border and no amber ring pixel (the pre-fix build stroked the
      // whole border with palette.focus here).
      const [toggleX, toggleY, toggleWidth] = semanticNode(
        current.semantic,
        "checkbox",
      ).bounds;
      const borderRow = Array.from(
        { length: 12 },
        (_, column) =>
          [
            toggleX + toggleWidth * (0.2 + (0.6 * column) / 11),
            toggleY + 0.008,
          ] as const,
      );
      const borderSamples = await g.call<readonly (readonly number[])[]>(
        "sampleGalleryGuiCapture",
        "gui-demo-controls-active",
        borderRow,
      );
      const ringPixels = borderSamples.filter(
        ([red, , blue]) => red! > 150 && red! > blue! + 30,
      );
      assert.equal(
        ringPixels.length,
        0,
        `pointer-clicked SCAN toggle shows focus ring pixels: ${JSON.stringify(borderSamples)}`,
      );
      assert.ok(
        borderSamples.filter(
          ([, , blue], index) => blue! > borderSamples[index]![0]! + 30,
        ).length >= 6,
        `SCAN border row missed the track border: ${JSON.stringify(borderSamples)}`,
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
      // The gain fill stays flush with the track: it is not inset by the
      // track border (ipp-jtst.7). Samples just inside the track's left
      // edge, across the track band, carry the bright cyan fill for any
      // committed value. Thresholds read the rendered frame, which tone
      // maps brighter than authored values: the muted border (b 158) and
      // the dark panel stay well below the green/blue floors.
      const [sliderX, sliderY, , sliderHeight] = semanticNode(
        current.semantic,
        "slider",
      ).bounds;
      const fillColumn = [-0.01, 0, 0.01].map(
        (dy) => [sliderX + 0.05, sliderY + sliderHeight / 2 + dy] as const,
      );
      const fillSamples = await g.call<readonly (readonly number[])[]>(
        "sampleGalleryGuiCapture",
        "gui-demo-controls-active",
        fillColumn,
      );
      for (const [, green, blue] of fillSamples) {
        assert.ok(
          green! > 180 && blue! > 200,
          `gain fill is not flush with the track start: ${JSON.stringify(fillSamples)}`,
        );
      }
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
        scanController: waveform.animation(sceneBeforeSkin).id,
        wavePulseController: waveform.animation(sceneBeforeSkin, true).id,
      };
      const callsignBeforeSkin = semanticNode(
        identityBeforeSkin,
        "textInput",
        "CALLSIGN",
      );
      assert.deepEqual(identityBeforeSkin.focused, {
        id: callsignBeforeSkin.id,
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
        waveform.animation(emberFrame.inspection).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        waveform.animation(emberFrame.inspection, true).id,
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
      assert.equal(emberFrame.frame.failedDrawCalls, 0);
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
        waveform.animation(neonFrame.inspection).id,
        projectorBeforeSkin.scanController,
        "reskin replaced the waveform scan controller",
      );
      assert.equal(
        waveform.animation(neonFrame.inspection, true).id,
        projectorBeforeSkin.wavePulseController,
        "reskin replaced the waveform pulse controller",
      );
      const neonDifference = await g.difference(
        "gui-demo-ember-active",
        "gui-demo-neon-active",
      );
      assert.ok(neonDifference.changedPixels > 1_000);
      assert.ok(neonDifference.meanAbsoluteChannelDifference > 1);
      assert.equal(neonFrame.frame.failedDrawCalls, 0);
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
        // SPAN, UPLINK and PURGE labels sit centred in their buttons
        // (ipp-jtst.8).
        // Button labels lay out left-aligned and are not separate text
        // leaves, so the specimen tunes each button's left padding and the
        // check weighs each sample by its contrast with the row median
        // (the button fill), comparing the ink centroid to the centre.
        for (const name of ["SPAN", "UPLINK", "PURGE"] as const) {
          const [bx, by, bw, bh] = semanticNode(
            detail.semantic,
            "button",
            name,
          ).bounds;
          const columns = 48;
          const labelXs = Array.from(
            { length: columns },
            (_, column) => bx + 0.05 + ((bw - 0.1) * column) / (columns - 1),
          );
          const labelSamples = await g.call<readonly (readonly number[])[]>(
            "sampleGalleryGuiCapture",
            "gui-detail-neon",
            [0.4, 0.5, 0.6].flatMap((row) =>
              labelXs.map((sampleX) => [sampleX, by + bh * row] as const),
            ),
          );
          let weight = 0;
          let moment = 0;
          for (const [row, pixel] of labelSamples.entries()) {
            const line = Math.floor(row / columns);
            const median = [0, 1, 2].map((channel) => {
              const values = labelSamples
                .slice(line * columns, (line + 1) * columns)
                .map((sample) => sample[channel]!)
                .sort((a, b) => a - b);
              return values[values.length >> 1]!;
            });
            const contrast = Math.max(
              ...median.map((value, channel) =>
                Math.abs(pixel[channel]! - value),
              ),
            );
            const ink = Math.max(0, contrast - 24);
            weight += ink;
            moment += ink * labelXs[row % columns]!;
          }
          assert.ok(weight > 0, `${name} button shows no label ink`);
          const offset = Math.abs(moment / weight - (bx + bw / 2));
          assert.ok(offset < 0.015, `${name} label is off-centre by ${offset}`);
        }
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
        const dashboard = textBounds(detail, ICON_CODE_POINTS.dashboard);
        near(dashboard[0], px, "dashboard icon x");
        near(
          dashboard[1] + dashboard[3] / 2,
          title[1] + title[3] / 2,
          "dashboard icon y",
        );
        assert.ok(dashboard[0] + dashboard[2] <= title[0]);
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
        // Painted ink, not just the layout box, is centred on each icon's
        // intrinsic box, which the checks above pin to its cell: an icon
        // box sized apart from its glyph leaves the ink off-centre.
        const inkBounds = await g.call<Record<string, LogicalRect>>(
          "galleryGuiInkBounds",
          "gui-detail-neon",
          Object.fromEntries(
            Object.entries(iconRects).map(([name, [x0, y0, x1, y1]]) => [
              name,
              [x0 - 0.03, y0 - 0.03, x1 + 0.03, y1 + 0.03] as LogicalRect,
            ]),
          ),
        );
        evidence.iconInk = inkBounds;
        for (const [name, [x0, y0, x1, y1]] of Object.entries(iconRects)) {
          const ink = inkBounds[name]!;
          for (const [axis, inkCentre, boxCentre] of [
            ["x", (ink[0] + ink[2]) / 2, (x0 + x1) / 2],
            ["y", (ink[1] + ink[3]) / 2, (y0 + y1) / 2],
          ] as const)
            assert.ok(
              Math.abs(inkCentre - boxCentre) < 0.012,
              `${name} icon ink ${axis} centre ${inkCentre} is not ${boxCentre}: ${JSON.stringify(ink)}`,
            );
        }
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
      // Wheel over the telemetry readouts, above the nested event log.
      const telemetry = telemetryScrollViews(await waitForGui()).outer;
      const [scroll] = await projectContent(g, [
        [
          telemetry.bounds[0] + telemetry.bounds[2] * 0.5,
          telemetry.bounds[1] + 0.15,
        ],
      ]);
      const cameraBeforeScroll = transform(await g.inspect());
      await g.capture("gui-demo-before-scroll");
      await g.page.mouse.move(scroll!.clientX, scroll!.clientY);
      // One 100 px wheel notch scrolls the telemetry view by the gallery's
      // wheel step, an eighth of its 0.78 viewport.
      const scrollState = (state: GalleryGuiState) =>
        state.semantic.nodes.find(({ id }) => id === telemetry.id)!;
      const beforeNotch = scrollState(await waitForGui());
      assert.ok(beforeNotch.scroll, "telemetry ScrollView has no scroll state");
      const notchTarget = Math.min(
        beforeNotch.scroll.offset[1] + 0.78 / 8,
        beforeNotch.scroll.maxOffset[1],
      );
      assert.ok(
        notchTarget > beforeNotch.scroll.offset[1] + 0.05,
        `telemetry has no room for a wheel notch: ${JSON.stringify(beforeNotch.scroll)}`,
      );
      await g.page.mouse.wheel(0, 100);
      const afterNotch = scrollState(
        await waitForGui(
          (state) =>
            Math.abs(scrollState(state).scroll!.offset[1] - notchTarget) < 1e-4,
        ),
      );
      await scenario.evidence.record("telemetry-wheel-notch", {
        before: beforeNotch.scroll,
        after: afterNotch.scroll,
        viewportHeight: afterNotch.bounds[3],
      });
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
      const mountedScan = waveform.animation(await g.inspect());
      const mountedWavePulse = waveform.animation(await g.inspect(), true);
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
      const returnedScanner = waveform.animation(returnedInspection);
      assertNoScanner(returnedInspection);
      assert.notEqual(
        waveform.animation(returnedInspection, true).id,
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
        returnedPulse: waveform.animation(returnedInspection, true).id,
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
  readonly statistics: RenderStatisticsSnapshot | undefined;
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
      evidenceParent: resolve(
        "target/integration-artifacts/gallery-gui/surface-cache",
      ),
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
        const statistics = frame.statistics;
        const surfaces = statistics?.surfaces;
        assert.ok(surfaces, "Surface cache diagnostics are unavailable");
        const records = surfaces.surfaceCaches;
        const observation = {
          label,
          record:
            entity === undefined
              ? undefined
              : records.find((entry) => entry.entity === entity),
          statistics,
          repaints: surfaces.totalSurfaceCacheRepaints,
          allocations: surfaces.totalSurfaceCacheAllocations,
          reuses: surfaces.totalSurfaceCacheReuses,
          direct: surfaces.totalSurfaceCacheDirect,
        };
        const { ingress: _ingress, ...stats } = statistics!;
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
        assert.equal(observation.statistics!.surfaces!.surfaceCacheReuses, 1);
        assert.equal(observation.statistics!.surfaces!.surfaceCacheRepaints, 0);
        assert.equal(observation.statistics!.surfaces!.surfaceCacheDirect, 0);
        assert.equal(
          observation.statistics!.surfaces!.surfaceCacheAllocations,
          0,
        );
        assert.equal(observation.statistics!.frame.uploadedBytes, 0);
        assert.equal(
          observation.statistics!.surfaces!.surfaceCacheResidentBytes,
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
        assert.equal(direct.statistics!.surfaces!.surfaceCacheEntries, 0);
        assert.equal(direct.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
        assert.equal(direct.statistics!.surfaces!.surfaceCacheDirect, 0);
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
      assert.equal(near.statistics!.surfaces!.surfaceCacheDirect, 1);
      assert.equal(near.statistics!.surfaces!.surfaceCacheRepaints, 0);
      assert.equal(near.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
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
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheDirect, 1);
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheReuses, 0);
      assert.equal(hovered.statistics!.surfaces!.surfaceCacheRepaints, 0);
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
      assert.deepEqual(released.statistics!.surfaces!.surfaceCaches, []);
      assert.equal(released.statistics!.surfaces!.surfaceCacheEntries, 0);
      assert.equal(released.statistics!.surfaces!.surfaceCacheResidentBytes, 0);
      await record("summary", {
        aurora,
        ember,
        durationMs: performance.now() - started,
      });
      assert.deepEqual(g.errors, []);
    },
  );
});

type Gallery = Awaited<ReturnType<typeof openGallery>>;

interface ProjectedPoint {
  readonly x: number;
  readonly y: number;
  readonly clientX: number;
  readonly clientY: number;
}

/** Surface half extents, restated from the panel's 7.4 x 4.8 Surface. */
const PANEL_HALF = [3.7, 2.4] as const;

/** Runtime scroll bar thickness: a twentieth of the viewport's shorter side. */
const SCROLL_BAR_THICKNESS = 0.05;

/** Shortest thumb, in bar thicknesses. */
const SCROLL_THUMB_MIN = 2;

/** The gallery's wheel step: an eighth of the 0.78 telemetry viewport. */
const WHEEL_STEP = 0.78 / 8;

/** Project Surface content points through the actual panel and camera. */
function projectContent(
  g: Gallery,
  points: readonly (readonly [number, number])[],
) {
  return g.call<readonly ProjectedPoint[]>(
    "projectGalleryPoints",
    "gui-demo",
    points.map(([x, y]) => [x - PANEL_HALF[0], PANEL_HALF[1] - y, 0]),
  );
}

async function waitForGuiState(
  g: Gallery,
  predicate: (state: GalleryGuiState) => boolean = () => true,
): Promise<GalleryGuiState> {
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
}

/** The telemetry ScrollView and the event log VirtualList nested inside it. */
function telemetryScrollViews(state: GalleryGuiState) {
  const views = state.detailed.nodes.filter(
    ({ data, style }) =>
      data.kind === "container" &&
      (data.containerKind === "scrollView" ||
        data.containerKind === "virtualList") &&
      style.enabled !== false,
  );
  assert.equal(views.length, 2, "expected the telemetry and event log views");
  const semantic = (id: number) => {
    const node = state.semantic.nodes.find((candidate) => candidate.id === id);
    assert.ok(node, `ScrollView ${id} is absent from semantics`);
    return node;
  };
  const nested = (id: number) => {
    for (let node = semantic(id); node.parent !== undefined; ) {
      if (views.some((view) => view.id === node.parent)) return true;
      node = semantic(node.parent);
    }
    return false;
  };
  const inner = views.filter(({ id }) => nested(id));
  const outer = views.filter(({ id }) => !nested(id));
  assert.equal(inner.length, 1, "the event log is not nested");
  assert.equal(outer.length, 1);
  assert.equal(
    inner[0]!.data.kind === "container" && inner[0]!.data.containerKind,
    "virtualList",
  );
  assert.equal(
    outer[0]!.data.kind === "container" && outer[0]!.data.containerKind,
    "scrollView",
  );
  const pair = { outer: semantic(outer[0]!.id), inner: semantic(inner[0]!.id) };
  assert.ok(pair.outer.scroll && pair.inner.scroll, "missing scroll state");
  return pair as {
    outer: GuiSemanticNode & { scroll: NonNullable<GuiSemanticNode["scroll"]> };
    inner: GuiSemanticNode & { scroll: NonNullable<GuiSemanticNode["scroll"]> };
  };
}

/** Event log entries restated from the demo: a Text at least 0.28 high
 * with a 0.05 gap below it, at 0.16 type. */
const EVENT_MIN_HEIGHT = 0.28;
const EVENT_GAP = 0.05;
const EVENT_FONT_SIZE = 0.16;

/** Monospaced advance and line height per unit font size, measured from a
 * single-line label. */
interface TextMetrics {
  readonly advance: number;
  readonly lineHeight: number;
}

/**
 * The event log VirtualList: its semantic scroll and item state, authored
 * item properties, and the declared items in index order. Declared items
 * are the list's children, each an item wrapper holding one entry Text;
 * their index follows the loaded range, since the list orders children by
 * item index.
 */
function eventLog(state: GalleryGuiState) {
  const { outer, inner } = telemetryScrollViews(state);
  const list = inner.virtualList;
  assert.ok(list, "the event log has no VirtualList semantics");
  const inspected = new Map(
    state.detailed.nodes.map((node) => [node.id, node]),
  );
  const detail = inspected.get(inner.id)!;
  const { itemCount, itemExtent, overscan } = detail.values;
  assert.ok(
    itemCount !== undefined &&
      itemExtent !== undefined &&
      overscan !== undefined,
    "the event log has no authored item properties",
  );
  const items = detail.children.map((id, position) => {
    const wrapper = inspected.get(id);
    assert.ok(wrapper && wrapper.children.length === 1, "bad item wrapper");
    const entry = inspected.get(wrapper.children[0]!);
    assert.ok(entry && entry.data.kind === "text", "item is not a Text");
    const semantic = state.semantic.nodes.find(
      (candidate) => candidate.id === entry.id,
    );
    assert.ok(semantic, `event log item ${entry.id} is absent from semantics`);
    return {
      index: list.loadedFirst + position,
      text: entry.data.text,
      bounds: semantic.bounds,
    };
  });
  return {
    outer,
    node: inner,
    list,
    itemCount,
    itemExtent,
    overscan,
    items,
  };
}

/**
 * Independent event log layout: every item takes the estimate except the
 * declared ones, which measure their greedily wrapped lines (at least the
 * minimum height) plus the gap. Positions, content extent, capacity and the
 * wanted range follow from those extents, the viewport and the overscan.
 */
function expectedEventLog(
  log: ReturnType<typeof eventLog>,
  metrics: TextMetrics,
) {
  const glyph = metrics.advance * EVENT_FONT_SIZE;
  const line = metrics.lineHeight * EVENT_FONT_SIZE;
  const width = log.items[0]?.bounds[2] ?? 0;
  const columns = Math.floor(width / glyph + 1e-4);
  const estimate = log.itemExtent;
  const count = log.itemCount;
  const viewport = log.node.bounds[3];
  const first = log.items[0]?.index ?? 0;
  const lines = log.items.map(({ text }) => wrapColumns(text, columns));
  const extents = lines.map(
    (wrapped) => Math.max(wrapped.length * line, EVENT_MIN_HEIGHT) + EVENT_GAP,
  );
  const starts = extents.map((_, at) =>
    extents
      .slice(0, at)
      .reduce((sum, extent) => sum + extent, first * estimate),
  );
  const declaredEnd = first + extents.length;
  const excess =
    extents.reduce((sum, extent) => sum + extent, 0) -
    extents.length * estimate;
  const position = (index: number) =>
    index < first
      ? index * estimate
      : index < declaredEnd
        ? starts[index - first]!
        : index * estimate + excess;
  const extent = (index: number) =>
    index >= first && index < declaredEnd ? extents[index - first]! : estimate;
  const content = count * estimate + excess;
  const capacity = Math.max(0, content - viewport);
  // Item containing a main-axis offset; the log holds a few hundred items
  // at most, so a walk from the first is enough.
  const itemAt = (offset: number) => {
    let index = 0;
    while (index < count - 1 && offset >= position(index) + extent(index))
      index += 1;
    return index;
  };
  const wanted = (offset: number): [number, number] => {
    if (count === 0) return [0, 0];
    const firstVisible = itemAt(offset);
    const end = offset + viewport;
    let lastVisible = itemAt(end);
    if (lastVisible > firstVisible && position(lastVisible) >= end)
      lastVisible -= 1;
    return [
      Math.max(0, firstVisible - log.overscan),
      Math.min(count, lastVisible + 1 + log.overscan),
    ];
  };
  return {
    glyph,
    line,
    columns,
    lines,
    extents,
    position,
    content,
    capacity,
    wanted,
  };
}

/**
 * Expected vertical scroll bar of one ScrollView at `[x, y]` on screen:
 * a track along the right edge as thick as a twentieth of the shorter
 * viewport side, and a thumb whose length is the visible fraction of the
 * track, never shorter than two thicknesses, placed by offset / capacity.
 */
function expectedScrollBar(
  node: GuiSemanticNode & { scroll: NonNullable<GuiSemanticNode["scroll"]> },
  x: number,
  y: number,
) {
  const [, , width, height] = node.bounds;
  const thickness = SCROLL_BAR_THICKNESS * Math.min(width, height);
  const capacity = node.scroll.maxOffset[1];
  const extent = height + capacity;
  const length = Math.min(
    height,
    Math.max(height * (height / extent), SCROLL_THUMB_MIN * thickness),
  );
  const fraction = capacity > 0 ? node.scroll.offset[1] / capacity : 0;
  const top = y + (height - length) * fraction;
  return {
    thickness,
    track: [x + width - thickness, y, x + width, y + height] as LogicalRect,
    thumb: [x + width - thickness, top, x + width, top + length] as LogicalRect,
  };
}

/** Convex hull of projected points, in order around the hull. */
function convexHull(points: readonly ProjectedPoint[]): ProjectedPoint[] {
  const sorted = [...points].sort(
    (a, b) => a.clientX - b.clientX || a.clientY - b.clientY,
  );
  const cross = (o: ProjectedPoint, a: ProjectedPoint, b: ProjectedPoint) =>
    (a.clientX - o.clientX) * (b.clientY - o.clientY) -
    (a.clientY - o.clientY) * (b.clientX - o.clientX);
  const half = (ordered: readonly ProjectedPoint[]) => {
    const chain: ProjectedPoint[] = [];
    for (const point of ordered) {
      while (
        chain.length >= 2 &&
        cross(chain[chain.length - 2]!, chain[chain.length - 1]!, point) <= 0
      )
        chain.pop();
      chain.push(point);
    }
    chain.pop();
    return chain;
  };
  return [...half(sorted), ...half([...sorted].reverse())];
}

/** Greedy word wrap of monospaced text into lines of at most `columns`. */
function wrapColumns(text: string, columns: number): string[] {
  const lines: string[] = [];
  let line = "";
  for (const word of text.split(" ")) {
    const candidate = line === "" ? word : `${line} ${word}`;
    if (candidate.length <= columns || line === "") line = candidate;
    else {
      lines.push(line);
      line = word;
    }
  }
  if (line !== "") lines.push(line);
  return lines;
}

/**
 * Compare a colour classification of one completed-frame area with the
 * logical rectangles expected to hold it, and write the expected mask, the
 * actual crop, the actual mask and their difference as PNG evidence. The
 * area maps to frame pixels through its projected corners, so the detail
 * view or the authored camera both work while the panel stays planar.
 */
async function compareMask(
  g: Gallery,
  directory: string,
  name: string,
  capture: {
    readonly label: string;
    readonly width: number;
    readonly height: number;
  },
  area: LogicalRect,
  expected: readonly LogicalRect[],
  select: (pixel: readonly [number, number, number]) => boolean,
) {
  const [x0, y0, x1, y1] = area;
  const corners = await projectContent(g, [
    [x0, y0],
    [x1, y0],
    [x0, y1],
    [x1, y1],
  ]);
  const [a, b, d] = corners as [ProjectedPoint, ProjectedPoint, ProjectedPoint];
  const region = await g.call<{
    left: number;
    top: number;
    width: number;
    height: number;
    pixels: string;
  }>("viewerCaptureRegionPixels", capture.label, [
    Math.min(...corners.map(({ x }) => x)),
    Math.min(...corners.map(({ y }) => y)),
    Math.max(...corners.map(({ x }) => x)),
    Math.max(...corners.map(({ y }) => y)),
  ]);
  const crop = decodeRegion(region);
  const det = (b.x - a.x) * (d.y - a.y) - (b.y - a.y) * (d.x - a.x);
  const expectedImage = new Uint8Array(crop.pixels.length);
  const actualImage = new Uint8Array(crop.pixels.length);
  let expectedPixels = 0;
  let actualPixels = 0;
  let both = 0;
  for (let row = 0; row < crop.height; row++) {
    for (let column = 0; column < crop.width; column++) {
      const index = row * crop.width + column;
      const px = (region.left + column + 0.5) / capture.width;
      const py = (region.top + row + 0.5) / capture.height;
      const u = ((px - a.x) * (d.y - a.y) - (py - a.y) * (d.x - a.x)) / det;
      const v = ((b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)) / det;
      const inside = u >= 0 && u <= 1 && v >= 0 && v <= 1;
      const lx = x0 + u * (x1 - x0);
      const ly = y0 + v * (y1 - y0);
      const wanted =
        inside &&
        expected.some(
          ([ex0, ey0, ex1, ey1]) =>
            lx >= ex0 && lx <= ex1 && ly >= ey0 && ly <= ey1,
        );
      const offset = index * 4;
      const found =
        inside &&
        select([
          crop.pixels[offset]!,
          crop.pixels[offset + 1]!,
          crop.pixels[offset + 2]!,
        ]);
      expectedPixels += Number(wanted);
      actualPixels += Number(found);
      both += Number(wanted && found);
      const outside = inside ? 0 : 64;
      expectedImage.set(
        wanted ? [255, 255, 255, 255] : [outside, outside, outside, 255],
        offset,
      );
      actualImage.set(
        found ? [255, 255, 255, 255] : [outside, outside, outside, 255],
        offset,
      );
    }
  }
  const expectedFrame = { ...crop, pixels: expectedImage };
  const actualMask = { ...crop, pixels: actualImage };
  await Promise.all([
    writeFile(
      join(directory, `${name}-expected.png`),
      encodePng(expectedFrame),
    ),
    writeFile(join(directory, `${name}-actual.png`), encodePng(crop)),
    writeFile(
      join(directory, `${name}-actual-mask.png`),
      encodePng(actualMask),
    ),
    writeFile(
      join(directory, `${name}-diff.png`),
      encodePng(differenceImage(expectedFrame, actualMask, 128)),
    ),
  ]);
  const union = expectedPixels + actualPixels - both;
  return {
    expectedPixels,
    actualPixels,
    intersectionOverUnion: union === 0 ? 1 : both / union,
    precision: actualPixels === 0 ? 1 : both / actualPixels,
    recall: expectedPixels === 0 ? 1 : both / expectedPixels,
  };
}

test("Gallery GUI settings panel wraps notes, nests scrolling and blocks input at its shield", {
  timeout: 180_000,
}, async (context) => {
  await runBrowserEnvironment(
    "GUI settings panel",
    {
      ...galleryEnvironment,
      evidenceParent: resolve(
        "target/integration-artifacts/gallery-gui/settings-panel",
      ),
    },
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario, { initialPage: "gui" });
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLOutputElement>("#status")?.dataset
            .state === "ready",
      );
      const directory = scenario.evidence.directory;
      const evidence: Record<string, unknown> = {};
      const record = async (name: string, value: unknown) => {
        evidence[name] = value;
        await writeFile(
          join(directory, "settings-panel-evidence.json"),
          JSON.stringify(
            evidence,
            (_key, value) =>
              typeof value === "bigint" ? String(value) : value,
            2,
          ) + "\n",
        );
      };
      const text = async (selector: string) =>
        documentStatus(await g.page.locator(selector).textContent());
      const capture = async (label: string) => {
        const { frame } = await g.capture(label);
        assert.equal(frame.failedDrawCalls, 0);
        return { label, width: frame.width, height: frame.height };
      };
      // Hold the waveform still so completed frames differ only by the
      // inputs under test.
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
      await g.page.mouse.move(1, 1);

      // Wrapped notes. The monospaced advance and line height come from the
      // single-line TELEMETRY label; the expected lines are a greedy word
      // wrap of the notes into the columns their fixed width holds.
      const initial = await waitForGuiState(g);
      const title = textBounds(initial, "TELEMETRY");
      const advance = title[2] / "TELEMETRY".length / 0.24;
      const lineHeight = title[3] / 0.24;
      const notesNode = initial.detailed.nodes.find(
        ({ data }) => data.kind === "text" && data.text.length > 80,
      );
      assert.ok(notesNode && notesNode.data.kind === "text", "missing notes");
      const notesText = notesNode.data.text;
      const notes = initial.semantic.nodes.find(
        ({ id }) => id === notesNode.id,
      )!;
      const glyph = advance * 0.13;
      const line = lineHeight * 0.13;
      const lines = wrapColumns(
        notesText,
        Math.floor(notes.bounds[2] / glyph + 1e-4),
      );
      await record("notes", {
        bounds: notes.bounds,
        advance,
        lineHeight,
        lines,
      });
      assert.ok(lines.length >= 3, `notes wrapped into ${lines.length} lines`);
      near(notes.bounds[2], 2.86, "notes width");
      assert.ok(
        Math.abs(notes.bounds[3] - lines.length * line) < 1e-3,
        `notes height ${notes.bounds[3]} is not ${lines.length} lines of ${line}`,
      );

      const views = telemetryScrollViews(initial);
      await record("scroll-views", views);
      assert.deepEqual(views.outer.scroll.offset, [0, 0]);
      assert.deepEqual(views.inner.scroll.offset, [0, 0]);
      assert.ok(views.outer.scroll.maxOffset[1] > 0.5);
      assert.ok(views.inner.scroll.maxOffset[1] > 1);

      // The event log is a VirtualList over the demo's whole history that
      // declares only its wanted range. Once the declared window follows
      // the committed offset, the loaded range, the capacity and each
      // declared item's position match an independent layout of the
      // declared entries, the sidebar shows the range the list reported to
      // React, and entries run newest first by sequence number.
      const metrics = { advance, lineHeight };
      const eventLogMatches = async (state: GalleryGuiState) => {
        const log = eventLog(state);
        const expected = expectedEventLog(log, metrics);
        const offset = log.node.scroll.offset[1];
        const { loadedFirst, loadedLast } = log.list;
        const [wantedFirst, wantedLast] = expected.wanted(offset);
        assert.equal(log.list.itemCount, log.itemCount);
        assert.equal(
          log.items.length,
          loadedLast - loadedFirst,
          "the declared children do not fill the loaded range",
        );
        assert.ok(
          log.items.length < log.itemCount || log.itemCount <= 1,
          `the event log declares all ${log.itemCount} items`,
        );
        assert.deepEqual(
          [loadedFirst, loadedLast],
          [wantedFirst, wantedLast],
          `loaded range at offset ${offset}`,
        );
        assert.ok(
          Math.abs(log.node.scroll.maxOffset[1] - expected.capacity) < 1e-3,
          `capacity ${log.node.scroll.maxOffset[1]} is not ${expected.capacity}`,
        );
        for (const item of log.items)
          assert.ok(
            Math.abs(
              item.bounds[1] -
                (log.node.bounds[1] + expected.position(item.index)),
            ) < 1e-3,
            `item ${item.index} lies at ${item.bounds[1]}, not ${log.node.bounds[1] + expected.position(item.index)}`,
          );
        const sequences = log.items.map(({ text }) =>
          Number(/^(\d+) \/\/ /.exec(text)?.[1]),
        );
        sequences.forEach((sequence, at) =>
          assert.equal(sequence, sequences[0]! - at, "entries out of order"),
        );
        assert.equal(
          await text("#gui-events"),
          `items ${loadedFirst}-${loadedLast} of ${log.itemCount}`,
        );
        return { log, expected };
      };
      const settledEventLog = async (
        name: string,
        predicate: (log: ReturnType<typeof eventLog>) => boolean = () => true,
      ) => {
        const deadline = performance.now() + 15_000;
        let failure: unknown = new Error(
          `${name}: the event log did not settle`,
        );
        while (performance.now() < deadline) {
          try {
            const state = await waitForGuiState(g);
            const settled = await eventLogMatches(state);
            if (predicate(settled.log)) {
              const { log, expected } = settled;
              await record(name, {
                scroll: log.node.scroll,
                virtualList: log.list,
                itemExtent: log.itemExtent,
                overscan: log.overscan,
                items: log.items.map((item, at) => ({
                  ...item,
                  extent: expected.extents[at],
                  lines: expected.lines[at],
                })),
                capacity: expected.capacity,
              });
              return { state, ...settled };
            }
            failure = new Error(
              `${name}: event log ${JSON.stringify(settled.log.list)} at ${JSON.stringify(settled.log.node.scroll)}`,
            );
          } catch (error) {
            failure = error;
          }
          await new Promise((resolve) => setTimeout(resolve, 50));
        }
        throw failure;
      };
      const initialLog = await settledEventLog("event-log-top");
      assert.deepEqual(
        [initialLog.log.list.loadedFirst, initialLog.log.list.anchorIndex],
        [0, 0],
      );
      assert.ok(
        initialLog.log.itemCount >= 90 &&
          initialLog.expected.capacity > 20 * views.inner.bounds[3],
        "the event log history does not span many viewports",
      );
      assert.ok(
        initialLog.expected.extents.some(
          (extent) => Math.abs(extent - initialLog.log.itemExtent) > 0.05,
        ) &&
          new Set(initialLog.expected.lines.map(({ length }) => length)).size >
            1,
        "declared entries all measure like one another or the estimate",
      );
      near(views.outer.bounds[3], 0.78, "telemetry viewport height");
      assert.ok(
        views.inner.bounds[1] + views.inner.bounds[3] >
          views.outer.bounds[1] + views.outer.bounds[3],
        "the event log must start below the telemetry fold",
      );

      // Scroll bar frames: each visible thumb paints where an independent
      // geometry calculation puts it for the committed offsets.
      const barFrame = async (
        frame: Awaited<ReturnType<typeof capture>>,
        state: ReturnType<typeof telemetryScrollViews>,
        name: string,
        withLog: boolean,
      ) => {
        const [ox, oy] = state.outer.bounds;
        const bars = {
          outer: expectedScrollBar(state.outer, ox, oy),
          // Nested bounds are laid out at zero ancestor scroll; the outer
          // offset moves the event log on screen.
          ...(withLog
            ? {
                inner: expectedScrollBar(
                  state.inner,
                  state.inner.bounds[0],
                  state.inner.bounds[1] - state.outer.scroll.offset[1],
                ),
              }
            : {}),
        };
        const tracks = Object.fromEntries(
          Object.entries(bars).map(([bar, { track }]) => [
            bar,
            [track[0] - 0.01, track[1], track[2] + 0.01, track[3]],
          ]),
        ) as Record<string, LogicalRect>;
        const ink = await g.call<Record<string, LogicalRect>>(
          "galleryGuiInkBounds",
          frame.label,
          tracks,
        );
        const masks: Record<string, unknown> = {};
        for (const [bar, expected] of Object.entries(bars)) {
          const mask = await compareMask(
            g,
            directory,
            `${name}-${bar}-bar`,
            frame,
            [
              expected.track[0] - 0.03,
              expected.track[1],
              expected.track[2] + 0.03,
              expected.track[3],
            ],
            [expected.thumb],
            (pixel) => Math.max(...pixel) >= 150,
          );
          masks[bar] = mask;
          const [, top, , bottom] = ink[bar]!;
          assert.ok(
            Math.abs(top - expected.thumb[1]) < 0.03 &&
              Math.abs(bottom - expected.thumb[3]) < 0.03,
            `${name} ${bar} thumb spans ${top}..${bottom}, expected ${expected.thumb[1]}..${expected.thumb[3]}`,
          );
          assert.ok(
            mask.intersectionOverUnion > 0.5,
            `${name} ${bar} thumb mask: ${JSON.stringify(mask)}`,
          );
        }
        await record(name, { bars, ink, masks });
      };
      // Event log frames: within the part of the log the telemetry view
      // shows, entry ink lies on the independently wrapped lines of the
      // declared items at their offset positions, and each fully visible
      // line paints from its left edge to its last glyph, so a wrong item,
      // position or wrap would move or cut the ink.
      const eventFrame = async (
        frame: Awaited<ReturnType<typeof capture>>,
        settled: Awaited<ReturnType<typeof settledEventLog>>,
        name: string,
      ) => {
        const { log, expected } = settled;
        const listTop = log.node.bounds[1] - log.outer.scroll.offset[1];
        const offset = log.node.scroll.offset[1];
        const top = Math.max(listTop, log.outer.bounds[1]);
        const bottom = Math.min(
          listTop + log.node.bounds[3],
          log.outer.bounds[1] + log.outer.bounds[3],
        );
        const lineRects: LogicalRect[] = [];
        const lineEnds: Record<string, number> = {};
        const regions: Record<string, LogicalRect> = {};
        log.items.forEach((item, at) => {
          const x = item.bounds[0];
          const itemTop = listTop + expected.position(item.index) - offset;
          const lines = expected.lines[at]!;
          // A one-line entry sits in its minimum-height box.
          const boxHeight =
            lines.length === 1 ? EVENT_MIN_HEIGHT : expected.line;
          lines.forEach((content, index) => {
            const y0 = itemTop + index * expected.line;
            const y1 = y0 + boxHeight;
            const clipped = [
              x,
              Math.max(y0, top),
              x + content.length * expected.glyph,
              Math.min(y1, bottom),
            ] as LogicalRect;
            if (clipped[3] > clipped[1]) lineRects.push(clipped);
            if (y0 >= top && y1 <= bottom) {
              const key = `item${item.index}-line${index}`;
              lineEnds[key] = x + content.length * expected.glyph;
              regions[key] = [x - 0.02, y0, x + item.bounds[2], y1];
            }
          });
        });
        assert.ok(
          Object.keys(regions).length >= 1,
          `${name}: no event log line is fully visible`,
        );
        const ink = await g.call<Record<string, LogicalRect>>(
          "galleryGuiInkBounds",
          frame.label,
          regions,
        );
        const width = log.items[0]!.bounds[2];
        const x = log.items[0]!.bounds[0];
        const logMask = await compareMask(
          g,
          directory,
          name,
          frame,
          [x - 0.03, top, x + width + 0.03, bottom],
          lineRects,
          (pixel) => Math.max(...pixel) >= 110,
        );
        await record(`${name}-frame`, { lineRects, lineEnds, ink, logMask });
        for (const [key, end] of Object.entries(lineEnds)) {
          const [start, , stop] = ink[key]!;
          assert.ok(
            start > x - 0.015 && start < x + expected.glyph,
            `${name} ${key} ink starts at ${start}, not ${x}`,
          );
          assert.ok(
            stop > end - expected.glyph && stop < end + 0.015,
            `${name} ${key} ink ends at ${stop}, not before ${end}`,
          );
        }
        assert.ok(logMask.actualPixels > 150, `${name}: no entry ink`);
        assert.ok(
          logMask.precision > 0.95,
          `${name}: entry ink escaped its wrapped lines: ${JSON.stringify(logMask)}`,
        );
      };

      await g.call("faceGalleryGuiToCamera");
      try {
        const detail = await capture("settings-bars-top");

        // Each wrapped line paints ink from the left edge to its last glyph,
        // once the telemetry view scrolls the notes into its viewport.
        const checkNotes = async (
          frame: typeof detail,
          outerOffset: number,
        ) => {
          const nx = notes.bounds[0];
          const ny = notes.bounds[1] - outerOffset;
          const lineRects = lines.map(
            (content, index) =>
              [
                nx,
                ny + index * line,
                nx + content.length * glyph,
                ny + (index + 1) * line,
              ] as LogicalRect,
          );
          const inkBounds = await g.call<Record<string, LogicalRect>>(
            "galleryGuiInkBounds",
            frame.label,
            Object.fromEntries(
              lineRects.map(([x0, y0, , y1], index) => [
                `line${index}`,
                [x0 - 0.02, y0, x0 + notes.bounds[2] + 0.02, y1] as LogicalRect,
              ]),
            ),
          );
          const notesMask = await compareMask(
            g,
            directory,
            "settings-notes",
            frame,
            [
              nx - 0.03,
              ny - 0.03,
              nx + notes.bounds[2] + 0.03,
              ny + notes.bounds[3] + 0.03,
            ],
            lineRects,
            (pixel) => Math.max(...pixel) >= 110,
          );
          await record("notes-frame", { lineRects, inkBounds, notesMask });
          lineRects.forEach(([x0, , x1], index) => {
            const ink = inkBounds[`line${index}`]!;
            assert.ok(
              ink[0] > x0 - 0.015 && ink[0] < x0 + glyph,
              `line ${index} ink starts at ${ink[0]}, not ${x0}`,
            );
            assert.ok(
              ink[2] > x1 - glyph && ink[2] < x1 + 0.015,
              `line ${index} ink ends at ${ink[2]}, not before ${x1}: ${lines[index]}`,
            );
          });
          assert.ok(notesMask.actualPixels > 200, "notes painted no ink");
          assert.ok(
            notesMask.precision > 0.97,
            `notes ink escaped its wrapped lines: ${JSON.stringify(notesMask)}`,
          );
        };

        await barFrame(detail, views, "settings-bars-top", false);

        const cameraBefore = transform(await g.inspect());
        const wheelAt = async (point: ProjectedPoint, deltaY: number) => {
          await g.page.mouse.move(point.clientX, point.clientY);
          await g.page.mouse.wheel(0, deltaY);
        };
        const until = (
          predicate: (
            views: ReturnType<typeof telemetryScrollViews>,
          ) => boolean,
        ) =>
          waitForGuiState(g, (state) =>
            predicate(telemetryScrollViews(state)),
          ).then(telemetryScrollViews);
        const at = (actual: number, expected: number) =>
          Math.abs(actual - expected) < 1e-4;
        const dragThumb = async (
          bar: ReturnType<typeof expectedScrollBar>,
          toward: "start" | "end",
        ) => {
          const x = (bar.track[0] + bar.track[2]) / 2;
          const [from, to] = await projectContent(g, [
            [x, (bar.thumb[1] + bar.thumb[3]) / 2],
            [x, toward === "end" ? bar.track[3] + 0.3 : bar.track[1] - 0.3],
          ]);
          await g.drag(
            [from!.clientX, from!.clientY],
            [to!.clientX, to!.clientY],
          );
        };

        // A wheel notch over the readouts scrolls the telemetry view.
        const [readouts] = await projectContent(g, [
          [
            views.outer.bounds[0] + views.outer.bounds[2] * 0.4,
            views.outer.bounds[1] + 0.15,
          ],
        ]);
        await wheelAt(readouts!, 100);
        const notched = await until(({ outer }) =>
          at(outer.scroll.offset[1], WHEEL_STEP),
        );
        assert.deepEqual(notched.inner.scroll.offset, [0, 0]);

        // Seven more notches bring the wrapped notes fully into view.
        for (let notch = 0; notch < 7; notch++) await wheelAt(readouts!, 100);
        const notesShown = await until(({ outer }) =>
          at(outer.scroll.offset[1], 8 * WHEEL_STEP),
        );
        assert.deepEqual(notesShown.inner.scroll.offset, [0, 0]);
        const notesTop = notes.bounds[1] - notesShown.outer.scroll.offset[1];
        assert.ok(
          notesTop >= notesShown.outer.bounds[1] &&
            notesTop + notes.bounds[3] <=
              notesShown.outer.bounds[1] + notesShown.outer.bounds[3],
          "the scrolled notes are clipped by the telemetry view",
        );
        await g.page.mouse.move(1, 1);
        await checkNotes(
          await capture("settings-notes"),
          notesShown.outer.scroll.offset[1],
        );

        // Dragging the telemetry thumb past its track end opens the log.
        await dragThumb(
          expectedScrollBar(
            notesShown.outer,
            notesShown.outer.bounds[0],
            notesShown.outer.bounds[1],
          ),
          "end",
        );
        const opened = await until(({ outer }) =>
          at(outer.scroll.offset[1], outer.scroll.maxOffset[1]),
        );
        assert.deepEqual(opened.inner.scroll.offset, [0, 0]);

        // The log sits inside the telemetry viewport once opened.
        const [ix, iy, iw, ih] = opened.inner.bounds;
        const logTop = iy - opened.outer.scroll.offset[1];
        assert.ok(
          logTop >= opened.outer.bounds[1] - 1e-4 &&
            logTop + ih <=
              opened.outer.bounds[1] + opened.outer.bounds[3] + 1e-4,
          `opened log ${logTop}..${logTop + ih} is clipped by the telemetry view`,
        );
        const [log] = await projectContent(g, [
          [ix + iw * 0.4, logTop + ih / 2],
        ]);

        // A notch over the log scrolls the log alone.
        await wheelAt(log!, 100);
        const logNotched = await until(({ inner }) =>
          at(inner.scroll.offset[1], WHEEL_STEP),
        );
        assert.deepEqual(
          logNotched.outer.scroll.offset,
          opened.outer.scroll.offset,
          "a wheel over the event log also scrolled the telemetry view",
        );
        // The notch moves the log within its first item: the anchor keeps
        // that item and the offset into it.
        const wheeled = await settledEventLog("event-log-wheeled", (log) =>
          at(log.node.scroll.offset[1], WHEEL_STEP),
        );
        assert.deepEqual(
          [wheeled.log.list.anchorIndex, wheeled.log.list.loadedFirst],
          [0, 0],
        );
        assert.ok(at(wheeled.log.list.anchorOffset, WHEEL_STEP));
        await g.page.mouse.move(1, 1);
        const wheeledFrame = await capture("settings-log-wheeled");
        await barFrame(
          wheeledFrame,
          telemetryScrollViews(wheeled.state),
          "settings-log-wheeled-bars",
          true,
        );
        await eventFrame(wheeledFrame, wheeled, "settings-log-wheeled");

        // Dragging the log thumb past its track end scrolls it to its end.
        const logBar = (state: typeof opened) =>
          expectedScrollBar(
            state.inner,
            state.inner.bounds[0],
            state.inner.bounds[1] - state.outer.scroll.offset[1],
          );
        // The declared window follows to the oldest entries; measuring
        // them keeps the offset at the end of the shortened content.
        await dragThumb(logBar(logNotched), "end");
        await until(({ inner }) =>
          at(inner.scroll.offset[1], inner.scroll.maxOffset[1]),
        );
        const dragged = await settledEventLog(
          "event-log-dragged",
          (log) =>
            log.list.loadedLast === log.itemCount &&
            at(log.node.scroll.offset[1], log.node.scroll.maxOffset[1]),
        );
        const logEnd = telemetryScrollViews(dragged.state);
        assert.deepEqual(
          logEnd.outer.scroll.offset,
          opened.outer.scroll.offset,
        );
        assert.ok(
          dragged.log.list.loadedFirst > 0 &&
            dragged.log.list.anchorIndex > dragged.log.list.loadedFirst,
          `the dragged log did not move its window: ${JSON.stringify(dragged.log.list)}`,
        );
        await g.page.mouse.move(1, 1);
        const scrolledFrame = await capture("settings-bars-scrolled");
        await barFrame(scrolledFrame, logEnd, "settings-bars-scrolled", true);
        await eventFrame(scrolledFrame, dragged, "settings-log-dragged");
        assert.ok(
          (await g.difference(detail.label, scrolledFrame.label))
            .changedPixels > 200,
          "nested scrolling did not visibly move the telemetry content",
        );

        // Back at its start, the log passes unused upward movement outward:
        // the telemetry view scrolls up a notch while the log holds still.
        await dragThumb(logBar(logEnd), "start");
        const logStart = await until(({ inner }) =>
          at(inner.scroll.offset[1], 0),
        );
        await settledEventLog(
          "event-log-returned",
          (log) => log.list.loadedFirst === 0 && log.list.anchorIndex === 0,
        );
        assert.deepEqual(
          logStart.outer.scroll.offset,
          opened.outer.scroll.offset,
        );
        await wheelAt(log!, -100);
        const passed = await until(({ outer }) =>
          at(
            outer.scroll.offset[1],
            opened.outer.scroll.offset[1] - WHEEL_STEP,
          ),
        );
        assert.deepEqual(passed.inner.scroll.offset, [0, 0]);
        await record("nested-scrolling", {
          notched: notched.outer.scroll,
          opened: opened.outer.scroll,
          logNotched: logNotched.inner.scroll,
          logEnd: logEnd.inner.scroll,
          passed: { outer: passed.outer.scroll, inner: passed.inner.scroll },
        });
        await g.page.mouse.move(1, 1);
        assert.deepEqual(
          transform(await g.inspect()),
          cameraBefore,
          "GUI scrolling was also processed as a camera gesture",
        );
      } finally {
        await g.call("releaseGalleryGuiTransform");
      }

      // The armed shield is scene picking geometry marked as a blocker: a
      // press and a wheel notch aimed at PURGE through the glass are
      // blocked, reach neither the panel nor the camera, and are observable
      // as blocked input.
      const purge = async () => {
        const state = await waitForGuiState(g);
        const node = semanticNode(state.semantic, "button", "PURGE");
        const [x, y, width, height] = node.bounds;
        const [centre] = await projectContent(g, [
          [x + width / 2, y + height / 2],
        ]);
        return { node, centre: centre! };
      };
      const armed = await purge();
      // The click point lies inside the shield's projected front face.
      const shieldFace = await g.call<readonly ProjectedPoint[]>(
        "projectGalleryPoints",
        "gui-input-shield",
        [
          [-0.5, 0.5, 0.5],
          [0.5, 0.5, 0.5],
          [0.5, -0.5, 0.5],
          [-0.5, -0.5, 0.5],
        ],
      );
      const insideQuad = (
        quad: readonly ProjectedPoint[],
        { clientX, clientY }: ProjectedPoint,
      ) =>
        quad.every((corner, index) => {
          const next = quad[(index + 1) % quad.length]!;
          return (
            (next.clientX - corner.clientX) * (clientY - corner.clientY) -
              (next.clientY - corner.clientY) * (clientX - corner.clientX) >=
            0
          );
        }) ||
        quad.every((corner, index) => {
          const next = quad[(index + 1) % quad.length]!;
          return (
            (next.clientX - corner.clientX) * (clientY - corner.clientY) -
              (next.clientY - corner.clientY) * (clientX - corner.clientX) <=
            0
          );
        });
      assert.ok(
        insideQuad(shieldFace, armed.centre),
        "PURGE is not behind the input shield from the authored camera",
      );
      assert.equal(await text("#gui-shield"), "armed, 0 blocked");
      const commandBefore = await text("#gui-command");
      const cameraAuthored = transform(await g.inspect());
      const armedFrame = await capture("settings-shield-armed");
      await g.page.mouse.click(armed.centre.clientX, armed.centre.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-shield")?.textContent ===
          "armed, 1 blocked",
      );
      await g.page.mouse.wheel(0, 100);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-shield")?.textContent ===
          "armed, 2 blocked",
      );
      await g.page.mouse.move(1, 1);
      await g.settle();
      assert.equal(await text("#gui-command"), commandBefore);
      assert.deepEqual(
        transform(await g.inspect()),
        cameraAuthored,
        "blocked input reached the camera",
      );
      const blocked = await waitForGuiState(g);
      const blockedPurge = semanticNode(blocked.semantic, "button", "PURGE");
      assert.equal(blockedPurge.revision, armed.node.revision);
      const logTexts = blocked.detailed.nodes.flatMap(({ data }) =>
        data.kind === "text" ? [data.text] : [],
      );
      assert.ok(
        logTexts.some((entry) => entry.endsWith("SHIELD BLOCKED PRESS")) &&
          logTexts.some((entry) => entry.endsWith("SHIELD BLOCKED WHEEL")),
        "the event log did not record the blocked inputs",
      );

      // Lifting the shield keeps the glass in front of PURGE but stops
      // marking it: the same click now presses PURGE through the glass.
      await g.page.locator("#gui-shield-toggle").click();
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-shield")?.textContent ===
          "lifted, 2 blocked",
      );
      await g.page.mouse.move(1, 1);
      const liftedFrame = await capture("settings-shield-lifted");
      const lifted = await g.inspect();
      assert.ok(
        lifted.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-input-shield",
        ),
        "lifting the shield removed its glass",
      );
      // Only the shield's marks change between the armed and lifted
      // frames: every changed pixel lies on the projected box, and the
      // changed frame spans it.
      const shieldBox = convexHull(
        await g.call<readonly ProjectedPoint[]>(
          "projectGalleryPoints",
          "gui-input-shield",
          [-0.5, 0.5].flatMap((x) =>
            [-0.5, 0.5].flatMap((y) => [-0.5, 0.5].map((z) => [x, y, z])),
          ),
        ),
      );
      const faceBounds = [
        Math.min(...shieldBox.map(({ x }) => x)),
        Math.min(...shieldBox.map(({ y }) => y)),
        Math.max(...shieldBox.map(({ x }) => x)),
        Math.max(...shieldBox.map(({ y }) => y)),
      ] as const;
      const pad = 3 / armedFrame.width;
      const bounds = [
        faceBounds[0] - pad,
        faceBounds[1] - pad,
        faceBounds[2] + pad,
        faceBounds[3] + pad,
      ];
      const regions = await Promise.all(
        [armedFrame.label, liftedFrame.label].map((label) =>
          g.call<{
            left: number;
            top: number;
            width: number;
            height: number;
            pixels: string;
          }>("viewerCaptureRegionPixels", label, bounds),
        ),
      );
      const [before, after] = regions.map(decodeRegion) as [
        RgbaFrame,
        RgbaFrame,
      ];
      const { left, top } = regions[0]!;
      const face = shieldBox.map(
        ({ x, y }) =>
          ({
            x,
            y,
            clientX: x * armedFrame.width - left,
            clientY: y * armedFrame.height - top,
          }) as ProjectedPoint,
      );
      const expectedGlass = new Uint8Array(before.pixels.length);
      const changedGlass = new Uint8Array(before.pixels.length);
      const changedExtent = [Infinity, Infinity, -Infinity, -Infinity];
      let facePixels = 0;
      let changedPixels = 0;
      let changedOnFace = 0;
      for (let row = 0; row < before.height; row++) {
        for (let column = 0; column < before.width; column++) {
          const index = row * before.width + column;
          const inside = insideQuad(face, {
            x: 0,
            y: 0,
            clientX: column + 0.5,
            clientY: row + 0.5,
          });
          const changed = pixelDifference(before, after, index) > 24;
          facePixels += Number(inside);
          changedPixels += Number(changed);
          changedOnFace += Number(inside && changed);
          if (changed && inside) {
            changedExtent[0] = Math.min(changedExtent[0]!, column);
            changedExtent[1] = Math.min(changedExtent[1]!, row);
            changedExtent[2] = Math.max(changedExtent[2]!, column + 1);
            changedExtent[3] = Math.max(changedExtent[3]!, row + 1);
          }
          expectedGlass.set(
            inside ? [255, 255, 255, 255] : [0, 0, 0, 255],
            index * 4,
          );
          changedGlass.set(
            changed ? [255, 255, 255, 255] : [0, 0, 0, 255],
            index * 4,
          );
        }
      }
      const expectedFrame = { ...before, pixels: expectedGlass };
      const changedFrame = { ...before, pixels: changedGlass };
      await Promise.all([
        writeFile(
          join(directory, "settings-shield-expected.png"),
          encodePng(expectedFrame),
        ),
        writeFile(
          join(directory, "settings-shield-armed-crop.png"),
          encodePng(before),
        ),
        writeFile(
          join(directory, "settings-shield-actual.png"),
          encodePng(after),
        ),
        writeFile(
          join(directory, "settings-shield-actual-mask.png"),
          encodePng(changedFrame),
        ),
        writeFile(
          join(directory, "settings-shield-diff.png"),
          encodePng(differenceImage(expectedFrame, changedFrame, 128)),
        ),
      ]);
      const faceExtent = [
        Math.min(...face.map(({ clientX }) => clientX)),
        Math.min(...face.map(({ clientY }) => clientY)),
        Math.max(...face.map(({ clientX }) => clientX)),
        Math.max(...face.map(({ clientY }) => clientY)),
      ];
      const glass = {
        facePixels,
        changedPixels,
        precision: changedOnFace / Math.max(1, changedPixels),
        changedExtent,
        faceExtent,
      };
      await record("shield-glass", glass);
      assert.ok(facePixels > 400, "the shield covers too few pixels");
      assert.ok(
        changedPixels > 150,
        `lifting the shield barely changed its marks: ${JSON.stringify(glass)}`,
      );
      assert.ok(
        glass.precision > 0.95,
        `lifting the shield changed pixels off its glass: ${JSON.stringify(glass)}`,
      );
      changedExtent.forEach((value, side) =>
        assert.ok(
          Math.abs(value - faceExtent[side]!) < 2.5,
          `the changed shield frame does not span its face: ${JSON.stringify(glass)}`,
        ),
      );

      await g.page.mouse.click(armed.centre.clientX, armed.centre.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-command")?.textContent === "Log purged",
      );
      const purged = await waitForGuiState(g, (state) =>
        state.detailed.nodes.some(
          ({ data }) =>
            data.kind === "text" && data.text.endsWith("LOG PURGED"),
        ),
      );
      const purgedViews = telemetryScrollViews(purged);
      assert.equal(purgedViews.inner.scroll.maxOffset[1], 0);
      assert.equal(await text("#gui-shield"), "lifted, 2 blocked");

      // PURGE resets the list to its one entry and anchors it at the top.
      await settledEventLog(
        "event-log-purged",
        (log) =>
          log.itemCount === 1 &&
          log.list.anchorIndex === 0 &&
          log.list.anchorOffset === 0,
      );
      // With nothing left to scroll, a wheel notch over the log passes to
      // the telemetry view, which reaches its end with the log in view.
      {
        const [ix, iy, iw] = purgedViews.inner.bounds;
        const [over] = await projectContent(g, [
          [ix + iw * 0.4, iy - purgedViews.outer.scroll.offset[1] + 0.15],
        ]);
        await g.page.mouse.move(over!.clientX, over!.clientY);
        await g.page.mouse.wheel(0, 100);
        await waitForGuiState(g, (state) => {
          const { outer } = telemetryScrollViews(state);
          return (
            Math.abs(outer.scroll.offset[1] - outer.scroll.maxOffset[1]) < 1e-4
          );
        });
        await g.page.mouse.move(1, 1);
      }
      const emptied = await settledEventLog(
        "event-log-emptied",
        (log) =>
          log.itemCount === 1 &&
          log.node.scroll.offset[1] === 0 &&
          log.outer.scroll.offset[1] === log.outer.scroll.maxOffset[1],
      );
      assert.ok(emptied.log.items[0]!.text.endsWith("LOG PURGED"));
      await g.call("faceGalleryGuiToCamera");
      try {
        const purgedFrame = await capture("settings-log-purged");
        await barFrame(
          purgedFrame,
          telemetryScrollViews(emptied.state),
          "settings-log-purged-bars",
          true,
        );
        await eventFrame(purgedFrame, emptied, "settings-log-purged");
      } finally {
        await g.call("releaseGalleryGuiTransform");
      }

      // Re-arming marks the glass again.
      await g.page.locator("#gui-shield-toggle").click();
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-shield")?.textContent ===
          "armed, 2 blocked",
      );
      await g.page.mouse.click(armed.centre.clientX, armed.centre.clientY);
      await g.page.waitForFunction(
        () =>
          document.querySelector("#gui-shield")?.textContent ===
          "armed, 3 blocked",
      );
      await g.page.mouse.move(1, 1);
      await capture("settings-panel-final");
      assert.deepEqual(g.errors, []);
    },
  );
});
