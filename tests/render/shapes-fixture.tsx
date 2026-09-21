import { activateFixtureCamera } from "../integration/camera-fixtures.js";
import type { Client } from "@ipp/client";
import type {
  Command,
  ComponentFieldValue,
  EntityRef,
  Inspection,
  AssetResourceSnapshot,
} from "@ipp/client";
import type { ClientPresentation, FrameCapture } from "@ipp/client";
import {
  createRoot,
  Entity,
  MeshInstance,
  type ReactWorldRoot,
  Transform,
  UnlitMaterial,
  UnlitTexture,
} from "@ipp/react";
import {
  compareImages,
  type ImageDifference,
  type ImageSummary,
  summarizeImage,
  VIEWPORT,
} from "./image-assertions.js";

import {
  foregroundMask,
  colorMask,
  oraclePixels,
  analyticMask,
  projectedContours,
  projectedPlanePerimeter,
  projectedNormal,
  projectedArrowBounds,
  planeInteriorProbes,
  projectPoint,
  stableMaskValue,
  distanceToPolylines,
  samplePolylines,
  neighborhoodHasForeground,
  probeNeighborhood,
  maskBounds,
  checkerColors,
  pixelRgb,
  maximumDifference,
  linearToSrgb8,
  projectedVisibleLongitudeSeam,
} from "./shapes-oracle.js";

export const SHAPE_IDS = [
  "cube",
  "sphere",
  "pill",
  "plane",
  "cube-outline",
  "sphere-outline",
  "pill-outline",
  "plane-outline",
  "cone",
  "cone-outline",
  "cone-outline-rings",
] as const;
export type ShapeId = (typeof SHAPE_IDS)[number];

import type { Vec3, Point, BaseShape } from "./shapes-oracle.js";

export interface ShapeDefinition {
  readonly recipe: string;
  readonly base: BaseShape;
  readonly outline: boolean;
  readonly material: readonly [number, number, number];
  readonly rings?: number;
}

export const SHAPES: Readonly<Record<ShapeId, ShapeDefinition>> = Object.freeze(
  {
    cube: {
      recipe: "ipp://mesh/cube?width=2&height=2&length=2",
      base: "cube",
      outline: false,
      material: [0.95, 0.72, 0.78],
    },
    sphere: {
      // URL-encoded values and reordered parameters use the same geometry oracles.
      recipe: "ipp://mesh/sphere?radius=%31",
      base: "sphere",
      outline: false,
      material: [0.72, 0.95, 0.82],
    },
    pill: {
      recipe: "ipp://mesh/pill?height=2.8&radius=0.65",
      base: "pill",
      outline: false,
      material: [0.82, 0.78, 0.98],
    },
    plane: {
      recipe: "ipp://mesh/plane?size=2&normalLength=1.25&stroke=0.05",
      base: "plane",
      outline: false,
      material: [1, 1, 1],
    },
    "cube-outline": {
      recipe: "ipp://mesh/cube-outline?width=2&height=2&length=2&stroke=0.045",
      base: "cube",
      outline: true,
      material: [0.98, 0.86, 0.62],
    },
    "sphere-outline": {
      recipe: "ipp://mesh/sphere-outline?radius=1&stroke=0.045",
      base: "sphere",
      outline: true,
      material: [0.68, 0.92, 0.98],
    },
    "pill-outline": {
      recipe: "ipp://mesh/pill-outline?radius=0.65&height=2.8&stroke=0.045",
      base: "pill",
      outline: true,
      material: [0.96, 0.7, 0.94],
    },
    "plane-outline": {
      recipe: "ipp://mesh/plane-outline?size=2&normalLength=1.25&stroke=0.05",
      base: "plane",
      outline: true,
      material: [1, 1, 1],
    },
    cone: {
      recipe: "ipp://mesh/cone?radius=1&height=2",
      base: "cone",
      outline: false,
      material: [1, 0.88, 0.55],
    },
    "cone-outline": {
      recipe: "ipp://mesh/cone-outline?radius=1&height=2&stroke=0.045&rings=1",
      base: "cone",
      outline: true,
      rings: 1,
      material: [1, 0.88, 0.55],
    },
    "cone-outline-rings": {
      recipe: "ipp://mesh/cone-outline?radius=1&height=2&stroke=0.045&rings=3",
      base: "cone",
      outline: true,
      rings: 3,
      material: [1, 0.88, 0.55],
    },
  },
);

export const ARROW_GEOMETRY_CASES = [
  {
    label: "arrow",
    shape: "arrow",
    source: "ipp://mesh/arrow?length=1.25&stroke=0.05",
    axes: [2],
    colors: [[1, 1, 1]],
  },
  {
    label: "axis-default",
    shape: "axis",
    source: "ipp://mesh/axis?length=1.25&stroke=0.05",
    axes: [0, 1, 2],
    colors: [
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1],
    ],
  },
  {
    label: "axis-custom",
    shape: "axis",
    source:
      "ipp://mesh/axis?length=1.25&stroke=0.05&xColor=1,.25,0&yColor=.25,0,1&zColor=0,1,.25",
    axes: [0, 1, 2],
    colors: [
      [1, 0.25, 0],
      [0.25, 0, 1],
      [0, 1, 0.25],
    ],
  },
  {
    label: "axis-partial",
    shape: "axis",
    source: "ipp://mesh/axis?xColor=.5%2C.25%2C1&stroke=0.05&length=1.25",
    axes: [0, 1, 2],
    colors: [
      [0.5, 0.25, 1],
      [0, 1, 0],
      [0, 0, 1],
    ],
  },
] as const;

export const CHECKER_SOURCE =
  "ipp://texture/checkerboard?width=64&height=64&cellsX=8&cellsY=8";
export const ROTATED_PLANE_Y = Math.PI / 4;

interface GeneratedModule {
  readonly IppClient: {
    connectWorker(
      workerUrl: string | URL,
      wasmUrl: string | URL,
      options: { readonly timeoutMs: number; readonly canvas: OffscreenCanvas },
    ): Promise<Client>;
  };
  readonly UnlitMaterial: { readonly id: number };
  readonly Entity: {
    create(alias: number, metadata?: { readonly symbolicId?: string }): Command;
    alias(alias: number): EntityRef;
    delete(entity: EntityRef): Command;
  };
  readonly MeshInstance: {
    readonly id: number;
    insert(
      entity: EntityRef,
      values: Readonly<Record<string, unknown>>,
    ): Command;
  };
  readonly UnlitTexture: { readonly id: number };
  readonly Transform: { readonly id: number };
}

interface FixtureState {
  readonly contract: GeneratedModule;
  readonly client: Client;
  readonly presentation: ClientPresentation;
  readonly root: ReactWorldRoot;
  readonly captures: Map<string, FrameCapture>;
  readonly captureRotations: Map<string, number>;
  rotationY: number;
  selected: ShapeId | "arrow" | "axis";
  source: string;
}

export interface ShapeRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs: number;
}

export interface ShapeSetupReport {
  readonly resources: readonly AssetResourceSnapshot[];
  readonly inspection: ShapeInspection;
}

export interface ShapeInspection {
  readonly selected: ShapeId | "arrow" | "axis";
  readonly entityExists: boolean;
  readonly transform: Readonly<Record<string, ComponentFieldValue>> | null;
  readonly material: Readonly<Record<string, ComponentFieldValue>> | null;
  readonly mesh: Readonly<Record<string, ComponentFieldValue>> | null;
  readonly texture: Readonly<Record<string, ComponentFieldValue>> | null;
}

export interface ShapeCaptureReport {
  readonly label: string;
  readonly shape: ShapeId | "arrow" | "axis";
  readonly session: bigint;
  readonly tick: bigint;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly rotationY: number;
  readonly contextGeneration: number;
  readonly backend: Readonly<Record<string, unknown>>;
  readonly summary: ImageSummary;
  readonly inspection: ShapeInspection;
  readonly resourceCount: number;
}

export interface ShapeImageEvidence {
  readonly shape: ShapeId;
  readonly checkerCounts: readonly [number, number, number, number];
  readonly foregroundPixels: number;
  readonly unmatchedForegroundPixels: number;
  readonly analyticInteriorPixels: number;
  readonly analyticInteriorForegroundPixels: number;
  readonly analyticExteriorPixels: number;
  readonly analyticExteriorForegroundPixels: number;
  readonly contourSamples: number;
  readonly contourSampleHits: number;
  readonly foregroundNearContours: number;
  readonly longitudeSeamSamples: number;
  readonly longitudeSeamSampleHits: number;
  readonly expectedBounds: ImageSummary["bounds"];
  readonly planeSurfaceColorCounts: readonly [number, number, number, number];
  readonly planeSurfaceExpectedPixels: number;
  readonly planeSurfaceMatchedPixels: number;
  readonly planePerimeterSamples: number;
  readonly planePerimeterSampleHits: number;
  readonly normalSamples: number;
  readonly normalSampleHits: number;
  readonly normalOrigin: Point | null;
  readonly normalTip: Point | null;
  readonly solidArrowPixels: number;
  readonly expectedArrowBounds: ImageSummary["bounds"];
  readonly actualArrowBounds: ImageSummary["bounds"];
  readonly emptyProbePixels: number;
  readonly emptyProbeForegroundPixels: number;
}

export interface InvalidRecipeReport {
  readonly outcomes: readonly string[];
  readonly before: ShapeInspection;
  readonly after: ShapeInspection;
  readonly failedResources: readonly AssetResourceSnapshot[];
  readonly resourcesAfterCleanup: number;
}

let active: FixtureState | undefined;

export async function initializeShapes(
  configuration: ShapeRuntimeConfiguration,
): Promise<ShapeSetupReport> {
  await closeShapes();
  const canvas = document.createElement("canvas");
  canvas.id = "ipp-shapes-canvas";
  canvas.width = VIEWPORT.width;
  canvas.height = VIEWPORT.height;
  canvas.style.width = `${VIEWPORT.width}px`;
  canvas.style.height = `${VIEWPORT.height}px`;
  canvas.style.display = "block";
  document.body.replaceChildren(canvas);
  if (typeof canvas.transferControlToOffscreen !== "function") {
    canvas.remove();
    throw new Error("Chromium does not expose OffscreenCanvas transfer");
  }

  let client: Client | undefined;
  let root: ReactWorldRoot | undefined;
  try {
    const contract = (await import(
      configuration.generatedModuleUrl
    )) as GeneratedModule;
    client = await contract.IppClient.connectWorker(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      {
        timeoutMs: configuration.timeoutMs,
        canvas: canvas.transferControlToOffscreen(),
      },
    );
    if (
      !client.capabilities.spatial ||
      !client.capabilities.stateOverlays ||
      !client.capabilities.textures ||
      !client.capabilities.builtinAssets
    ) {
      throw new Error(
        "shape fixture requires scene, overlays, textures, and built-in assets",
      );
    }
    const presentation = client.presentation;
    if (!presentation)
      throw new Error("shape worker did not expose presentation");
    presentation.resize(VIEWPORT.width, VIEWPORT.height);

    await activateFixtureCamera(client);
    root = createRoot(client);
    active = {
      contract,
      client,
      presentation,
      root,
      captures: new Map(),
      captureRotations: new Map(),
      rotationY: 0,
      selected: "cube",
      source: SHAPES.cube.recipe,
    };
    await renderShape(active, "cube");
    const inspection = await waitForCurrentResources(active);
    return {
      resources: inspection.resources,
      inspection: await inspectShape(active),
    };
  } catch (error) {
    const failures: unknown[] = [error];
    if (root) {
      try {
        await root.unmount();
      } catch (cleanupError) {
        failures.push(cleanupError);
      }
    }
    if (client) {
      try {
        await client.close();
      } catch (cleanupError) {
        failures.push(cleanupError);
      }
    }
    canvas.remove();
    if (failures.length > 1)
      throw new AggregateError(failures, "shape fixture startup failed");
    throw error;
  }
}

export async function selectShape(shape: ShapeId): Promise<ShapeInspection> {
  const state = requireActive();
  await renderShape(state, shape);
  await waitForCurrentResources(state);
  return await inspectShape(state);
}

export async function selectArrowGeometry(
  index: number,
): Promise<ShapeInspection> {
  const definition = ARROW_GEOMETRY_CASES[index];
  if (!definition) throw new Error(`unknown arrow geometry case ${index}`);
  const state = requireActive();
  await state.root.render(
    <Entity id="shape-fixture">
      <Transform bound={false} />
      <UnlitMaterial bound={false} r={1} g={1} b={1} />
      <MeshInstance bound={false} source={definition.source} />
    </Entity>,
  );
  await state.root.flush();
  state.selected = definition.shape;
  state.source = definition.source;
  state.rotationY = 0;
  await waitForResources(
    state,
    (resource) => resource.source === state.source,
    "loaded",
    1,
  );
  return await inspectShape(state);
}

export function analyzeArrowGeometryCapture(label: string, index: number) {
  const definition = ARROW_GEOMETRY_CASES[index];
  if (!definition) throw new Error(`unknown arrow geometry case ${index}`);
  const frame = requireCapture(requireActive(), label);
  const allColors = definition.colors.map(
    (color) => color.map(linearToSrgb8) as unknown as Vec3,
  );
  const matched = colorMask(frame, allColors, 4);
  const foreground = foregroundMask(frame);
  return {
    unmatched: foreground.reduce(
      (sum, value, i) => sum + (value && !matched[i] ? 1 : 0),
      0,
    ),
    axes: definition.axes.map((axis, colorIndex) => {
      const mask = colorMask(frame, [allColors[colorIndex]!], 4);
      const samples = Array.from({ length: 24 }, (_, i) => {
        const point: [number, number, number] = [0, 0, 0];
        point[axis] = 1.25 * (0.15 + (0.8 * i) / 23);
        return projectPoint(point);
      });
      return {
        axis,
        pixels: mask.reduce((sum, value) => sum + value, 0),
        bounds: maskBounds(mask, frame.width, frame.height),
        expectedBounds: projectedArrowBounds(0, 1.25, 0, axis),
        samples: samples.length,
        hits: samples.filter(([x, y]) =>
          neighborhoodHasForeground(mask, frame.width, frame.height, x, y, 1),
        ).length,
      };
    }),
  };
}

export async function selectRotatedPlane(): Promise<ShapeInspection> {
  const state = requireActive();
  await renderShape(state, "plane", ROTATED_PLANE_Y);
  await waitForCurrentResources(state);
  return await inspectShape(state);
}

export async function selectPlanePointer(
  length: number,
  offset: number,
): Promise<ShapeInspection> {
  const state = requireActive();
  await renderShape(
    state,
    "plane",
    0,
    `ipp://mesh/plane?size=2&normalLength=${length}&stroke=0.05&normalOffset=${offset}`,
  );
  await waitForCurrentResources(state);
  return await inspectShape(state);
}

export function analyzePlanePointerCapture(
  label: string,
  length: number,
  offset: number,
) {
  const frame = requireCapture(requireActive(), label);
  // The textured grey square cannot produce the solid white pointer color.
  const arrow = colorMask(frame, [[255, 255, 255]], 4);
  const gap = projectPoint([0, 0, offset / 2]);
  return {
    bounds: maskBounds(arrow, frame.width, frame.height),
    expectedBounds: projectedArrowBounds(0, length, offset),
    gapWhitePixels: probeNeighborhood(
      arrow,
      frame.width,
      frame.height,
      gap,
      1,
    )[1],
  };
}

export async function captureShapeFrame(
  label: string,
): Promise<ShapeCaptureReport> {
  const state = requireActive();
  const inspection = await inspectShape(state);
  const runtimeInspection = await state.client.inspect();
  const frame = await state.presentation.capture(runtimeInspection.tick);
  if (
    frame.session !== state.client.session ||
    frame.tick < runtimeInspection.tick
  ) {
    throw new Error("shape capture is not from the requested session and tick");
  }
  if (frame.width !== VIEWPORT.width || frame.height !== VIEWPORT.height) {
    throw new Error(`unexpected capture size ${frame.width}x${frame.height}`);
  }
  state.captures.set(label, { ...frame, pixels: frame.pixels.slice(0) });
  state.captureRotations.set(label, state.rotationY);
  await compositorBarrier();
  return {
    label,
    shape: state.selected,
    session: frame.session,
    tick: frame.tick,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    rotationY: state.rotationY,
    contextGeneration: frame.contextGeneration,
    backend: frame.backend,
    summary: summarizeImage(frame),
    inspection,
    resourceCount: runtimeInspection.resources.length,
  };
}

export function analyzeShapeCapture(
  label: string,
  shape: ShapeId,
): ShapeImageEvidence {
  const frame = requireCapture(requireActive(), label);
  const rotationY = requireCaptureRotation(requireActive(), label);
  const definition = SHAPES[shape];
  const foreground = foregroundMask(frame);
  const analytic = analyticMask(frame, definition.base, rotationY);
  const expectedBounds = maskBounds(analytic, frame.width, frame.height);
  const checker = checkerColors(definition.material);
  const planeSurfaceChecker = checkerColors(
    definition.material.map((factor) => factor * 0.25),
  );
  const solidArrowColor = definition.material.map(
    linearToSrgb8,
  ) as unknown as Vec3;
  const arrowColors = definition.outline ? checker : [solidArrowColor];
  const arrowColorMask = colorMask(frame, arrowColors, 4);
  const checkerCounts: [number, number, number, number] = [0, 0, 0, 0];
  const planeSurfaceColorCounts: [number, number, number, number] = [
    0, 0, 0, 0,
  ];
  let foregroundPixels = 0;
  let unmatchedForegroundPixels = 0;
  for (let index = 0; index < foreground.length; index += 1) {
    if (!foreground[index]) continue;
    foregroundPixels += 1;
    const rgb = pixelRgb(frame, index);
    const match = checker.findIndex(
      (color) => maximumDifference(rgb, color) <= 4,
    );
    if (match < 0) unmatchedForegroundPixels += 1;
    else checkerCounts[match] = (checkerCounts[match] ?? 0) + 1;
  }

  let analyticInteriorPixels = 0;
  let analyticInteriorForegroundPixels = 0;
  let analyticExteriorPixels = 0;
  let analyticExteriorForegroundPixels = 0;
  let contourSamples = 0;
  let contourSampleHits = 0;
  let foregroundNearContours = 0;
  let longitudeSeamSamples = 0;
  let longitudeSeamSampleHits = 0;
  let planeSurfaceExpectedPixels = 0;
  let planeSurfaceMatchedPixels = 0;
  let planePerimeterSamples = 0;
  let planePerimeterSampleHits = 0;
  let normalSamples = 0;
  let normalSampleHits = 0;
  let emptyProbePixels = 0;
  let emptyProbeForegroundPixels = 0;
  const contours = definition.outline
    ? projectedContours(definition.base, rotationY, definition.rings)
    : [];
  for (let y = 2; y < frame.height - 2; y += 1) {
    for (let x = 2; x < frame.width - 2; x += 1) {
      const index = y * frame.width + x;
      const stableInside = stableMaskValue(analytic, frame.width, x, y, true);
      const stableOutside = stableMaskValue(analytic, frame.width, x, y, false);
      const contourDistance = definition.outline
        ? distanceToPolylines([x + 0.5, y + 0.5], contours)
        : Number.POSITIVE_INFINITY;
      if (stableInside && (!definition.outline || contourDistance > 8)) {
        analyticInteriorPixels += 1;
        if (foreground[index]) analyticInteriorForegroundPixels += 1;
      }
      if (stableOutside) {
        analyticExteriorPixels += 1;
        if (foreground[index]) analyticExteriorForegroundPixels += 1;
      }
      if (definition.outline && foreground[index] && contourDistance <= 2.5) {
        foregroundNearContours += 1;
      }
      if (definition.base === "plane" && stableInside) {
        planeSurfaceExpectedPixels += 1;
        const rgb = pixelRgb(frame, index);
        const surfaceMatch = planeSurfaceChecker.findIndex(
          (color) => maximumDifference(rgb, color) <= 4,
        );
        const arrowMatch = arrowColors.some(
          (color) => maximumDifference(rgb, color) <= 4,
        );
        if (surfaceMatch >= 0) {
          planeSurfaceColorCounts[surfaceMatch] =
            (planeSurfaceColorCounts[surfaceMatch] ?? 0) + 1;
        }
        if (surfaceMatch >= 0 || arrowMatch) {
          planeSurfaceMatchedPixels += 1;
        }
      }
    }
  }
  if (definition.outline) {
    const samples = samplePolylines(contours, 3);
    contourSamples = samples.length;
    contourSampleHits = samples.filter(([x, y]) =>
      neighborhoodHasForeground(foreground, frame.width, frame.height, x, y, 2),
    ).length;
  } else if (
    definition.base === "sphere" ||
    definition.base === "pill" ||
    definition.base === "cone"
  ) {
    const seam = projectedVisibleLongitudeSeam(definition.base);
    longitudeSeamSamples = seam.length;
    longitudeSeamSampleHits = seam.filter(([x, y]) =>
      neighborhoodHasForeground(foreground, frame.width, frame.height, x, y, 1),
    ).length;
  }
  const planePerimeter =
    definition.base === "plane" ? projectedPlanePerimeter(rotationY) : [];
  const normal =
    definition.base === "plane" ? projectedNormal(rotationY) : undefined;
  if (definition.base === "plane" && normal) {
    const perimeterPoints = samplePolylines(planePerimeter, 3);
    const normalPoints = samplePolylines([normal], 2);
    planePerimeterSamples = perimeterPoints.length;
    planePerimeterSampleHits = perimeterPoints.filter(([x, y]) =>
      neighborhoodHasForeground(foreground, frame.width, frame.height, x, y, 2),
    ).length;
    normalSamples = normalPoints.length;
    normalSampleHits = normalPoints.filter(([x, y]) =>
      neighborhoodHasForeground(
        arrowColorMask,
        frame.width,
        frame.height,
        x,
        y,
        3,
      ),
    ).length;
    if (definition.outline) {
      for (const probe of planeInteriorProbes(rotationY)) {
        const [pixels, foregroundPixels] = probeNeighborhood(
          foreground,
          frame.width,
          frame.height,
          probe,
          3,
        );
        emptyProbePixels += pixels;
        emptyProbeForegroundPixels += foregroundPixels;
      }
    }
  }
  return {
    shape,
    checkerCounts,
    foregroundPixels,
    unmatchedForegroundPixels,
    analyticInteriorPixels,
    analyticInteriorForegroundPixels,
    analyticExteriorPixels,
    analyticExteriorForegroundPixels,
    contourSamples,
    contourSampleHits,
    foregroundNearContours,
    longitudeSeamSamples,
    longitudeSeamSampleHits,
    expectedBounds,
    planeSurfaceColorCounts,
    planeSurfaceExpectedPixels,
    planeSurfaceMatchedPixels,
    planePerimeterSamples,
    planePerimeterSampleHits,
    normalSamples,
    normalSampleHits,
    normalOrigin: normal?.[0] ?? null,
    normalTip: normal?.[1] ?? null,
    solidArrowPixels: arrowColorMask.reduce((total, value) => total + value, 0),
    expectedArrowBounds:
      definition.base === "plane" ? projectedArrowBounds(rotationY) : null,
    actualArrowBounds:
      definition.base === "plane" && !definition.outline
        ? maskBounds(arrowColorMask, frame.width, frame.height)
        : null,
    emptyProbePixels,
    emptyProbeForegroundPixels,
  };
}

export async function rejectInvalidShapeRecipes(): Promise<InvalidRecipeReport> {
  const state = requireActive();
  const before = await inspectShape(state);
  const invalid = [
    "ipp://mesh/cube?width=0&height=2&length=2",
    "ipp://mesh/sphere?radius=0",
    "ipp://mesh/pill?radius=0.65&height=1",
    "ipp://mesh/plane?size=2&normalLength=1.25&stroke=0.16",
    "ipp://mesh/cube-outline?width=2&height=2&length=2&stroke=0.6",
    "ipp://mesh/sphere-outline?radius=1&stroke=0",
    "ipp://mesh/pill-outline?radius=0.65&height=2.8&stroke=NaN",
    "ipp://mesh/plane-outline?size=2&normalLength=0&stroke=0.05",
    "ipp://mesh/cube?width=2&height=2&length=2&typo=1",
    "ipp://mesh/cube?width=2&height=2&length=2&%77idth=2",
    "ipp://mesh/plane?size=2&normalLength=1.25",
    "ipp://mesh/sphere?radius=%00",
    "ipp://mesh/arrow?stroke=.05",
    "ipp://mesh/axis?length=1&stroke=.2",
    "ipp://mesh/axis?length=1&stroke=.05&xColor=1,2,0",
    "ipp://mesh/axis?length=1&stroke=.05&zColor=0,0,1&zColor=1,0,0",
  ];
  const operations: Command[] = [];
  for (const [index, recipe] of invalid.entries()) {
    const alias = 100 + index;
    operations.push(
      state.contract.Entity.create(alias, {
        symbolicId: `invalid-shape-source-${index}`,
      }),
      state.contract.MeshInstance.insert(state.contract.Entity.alias(alias), {
        source: recipe,
        variant: 0,
      }),
    );
  }
  const created = await state.client.batch(operations);
  if (!created.ok) {
    throw new Error(
      `invalid source declarations rejected: ${created.error.reason}`,
    );
  }
  const failedResources = await waitForResources(
    state,
    (resource) => invalid.includes(resource.source),
    "failed",
    invalid.length,
  );
  const outcomes = failedResources.map(
    ({ error, source }) => `${source}: ${error ?? "unknown failure"}`,
  );
  const handles = created.aliases
    .filter(({ alias }) => alias >= 100 && alias < 100 + invalid.length)
    .map(({ id }) => state.contract.Entity.delete({ kind: "handle", id }));
  const removed = await state.client.batch(handles);
  if (!removed.ok) {
    throw new Error(`invalid source cleanup rejected: ${removed.error.reason}`);
  }
  const resourcesAfterCleanup = (await state.client.inspect()).resources.length;
  const after = await inspectShape(state);
  return {
    outcomes,
    before,
    after,
    failedResources,
    resourcesAfterCleanup,
  };
}

export async function recoverShapeContext(
  beforeLabel: string,
  afterLabel = "after-context-restore",
): Promise<{
  readonly beforeGeneration: number;
  readonly after: ShapeCaptureReport;
  readonly resourceCountBefore: number;
  readonly resourceCountAfter: number;
}> {
  const state = requireActive();
  const before = requireCapture(state, beforeLabel);
  const resourceCountBefore = (await state.client.inspect()).resources.length;
  state.presentation.loseContext();
  await compositorBarrier();
  state.presentation.restoreContext();
  const after = await captureShapeFrame(afterLabel);
  if (after.contextGeneration <= before.contextGeneration) {
    throw new Error(
      "context generation did not advance after shape restoration",
    );
  }
  return {
    beforeGeneration: before.contextGeneration,
    after,
    resourceCountBefore,
    resourceCountAfter: after.resourceCount,
  };
}

export function compareShapeCaptures(
  firstLabel: string,
  secondLabel: string,
): ImageDifference {
  const state = requireActive();
  return compareImages(
    requireCapture(state, firstLabel),
    requireCapture(state, secondLabel),
  );
}

export async function shapeCaptureDataUrl(label: string): Promise<string> {
  return await frameDataUrl(requireCapture(requireActive(), label));
}

export function shapeCaptureMetadata(
  label: string,
): Omit<FrameCapture, "pixels"> {
  const { pixels: _pixels, ...metadata } = requireCapture(
    requireActive(),
    label,
  );
  return metadata;
}

export async function shapeDifferenceDataUrl(
  firstLabel: string,
  secondLabel: string,
): Promise<string> {
  const first = requireCapture(requireActive(), firstLabel);
  const second = requireCapture(requireActive(), secondLabel);
  const a = new Uint8Array(first.pixels);
  const b = new Uint8Array(second.pixels);
  const pixels = new Uint8ClampedArray(a.byteLength);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    for (let channel = 0; channel < 3; channel += 1) {
      pixels[offset + channel] = Math.min(
        255,
        Math.abs((a[offset + channel] ?? 0) - (b[offset + channel] ?? 0)) * 4,
      );
    }
    pixels[offset + 3] = 255;
  }
  return await rgbaDataUrl(first.width, first.height, pixels);
}

export async function shapeOracleDataUrl(
  label: string,
  shape: ShapeId,
): Promise<string> {
  const state = requireActive();
  const frame = requireCapture(state, label);
  return await rgbaDataUrl(
    frame.width,
    frame.height,
    oraclePixels(
      frame,
      SHAPES[shape],
      false,
      requireCaptureRotation(state, label),
    ),
  );
}

export async function shapeOracleDifferenceDataUrl(
  label: string,
  shape: ShapeId,
): Promise<string> {
  const state = requireActive();
  const frame = requireCapture(state, label);
  return await rgbaDataUrl(
    frame.width,
    frame.height,
    oraclePixels(
      frame,
      SHAPES[shape],
      true,
      requireCaptureRotation(state, label),
    ),
  );
}

export async function closeShapes(): Promise<void> {
  const state = active;
  active = undefined;
  if (!state) return;
  const failures: unknown[] = [];
  try {
    await state.root.unmount();
  } catch (error) {
    failures.push(error);
  }
  try {
    await state.client.close();
  } catch (error) {
    failures.push(error);
  }
  state.captures.clear();
  state.captureRotations.clear();
  document.querySelector("#ipp-shapes-canvas")?.remove();
  if (failures.length > 0)
    throw new AggregateError(failures, "shape fixture cleanup failed");
}

async function renderShape(
  state: FixtureState,
  shape: ShapeId,
  rotationY = 0,
  source = SHAPES[shape].recipe,
): Promise<void> {
  const definition = SHAPES[shape];
  await state.root.render(
    <Entity id="shape-fixture">
      <Transform
        bound={false}
        qy={Math.sin(rotationY / 2)}
        qw={Math.cos(rotationY / 2)}
      />
      <UnlitMaterial
        bound={false}
        r={definition.material[0]}
        g={definition.material[1]}
        b={definition.material[2]}
      />
      <MeshInstance bound={false} source={source} />
      <UnlitTexture bound={false} source={CHECKER_SOURCE} />
    </Entity>,
  );
  await state.root.flush();
  state.rotationY = rotationY;
  state.selected = shape;
  state.source = source;
}

async function inspectShape(state: FixtureState): Promise<ShapeInspection> {
  const inspection = await state.client.inspect();
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === "shape-fixture",
  );
  return {
    selected: state.selected,
    entityExists: entity !== undefined,
    transform: componentFields(entity?.effective, state.contract.Transform.id),
    material: componentFields(
      entity?.effective,
      state.contract.UnlitMaterial.id,
    ),
    mesh: componentFields(entity?.effective, state.contract.MeshInstance.id),
    texture: componentFields(entity?.effective, state.contract.UnlitTexture.id),
  };
}

function componentFields(
  components: Inspection["entities"][number]["effective"] | undefined,
  id: number,
): Inspection["entities"][number]["effective"][number]["fields"] | null {
  return components?.find(({ component }) => component === id)?.fields ?? null;
}

function requireActive(): FixtureState {
  if (!active) throw new Error("shape fixture is not initialized");
  return active;
}

function requireCapture(state: FixtureState, label: string): FrameCapture {
  const frame = state.captures.get(label);
  if (!frame) throw new Error(`missing captured shape frame '${label}'`);
  return frame;
}

function requireCaptureRotation(state: FixtureState, label: string): number {
  const rotation = state.captureRotations.get(label);
  if (rotation === undefined)
    throw new Error(`missing captured shape transform '${label}'`);
  return rotation;
}

async function waitForCurrentResources(
  state: FixtureState,
): Promise<Inspection> {
  await waitForResources(
    state,
    (resource) =>
      resource.source === state.source || resource.source === CHECKER_SOURCE,
    "loaded",
    2,
  );
  return await state.client.inspect();
}

async function waitForResources(
  state: FixtureState,
  matches: (resource: AssetResourceSnapshot) => boolean,
  status: AssetResourceSnapshot["status"],
  count: number,
): Promise<readonly AssetResourceSnapshot[]> {
  const deadline = performance.now() + 10_000;
  while (true) {
    const inspection = await state.client.inspect();
    const resources = inspection.resources.filter(matches);
    if (
      resources.length === count &&
      resources.every((resource) => resource.status === status)
    ) {
      return resources;
    }
    const failure = resources.find((resource) => resource.status === "failed");
    if (failure && status !== "failed") {
      throw new Error(
        `Resource ${failure.source} failed: ${failure.error ?? "unknown error"}`,
      );
    }
    if (performance.now() >= deadline) {
      throw new Error(`Timed out waiting for ${count} ${status} resources`);
    }
    await compositorBarrier();
  }
}

async function compositorBarrier(): Promise<void> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

async function frameDataUrl(frame: FrameCapture): Promise<string> {
  return await rgbaDataUrl(
    frame.width,
    frame.height,
    new Uint8ClampedArray(frame.pixels.slice(0)),
  );
}

async function rgbaDataUrl(
  width: number,
  height: number,
  pixels: Uint8ClampedArray<ArrayBuffer>,
): Promise<string> {
  const canvas = new OffscreenCanvas(width, height);
  const context = canvas.getContext("2d");
  if (!context) throw new Error("2D canvas is unavailable for shape evidence");
  context.putImageData(new ImageData(pixels, width, height), 0, 0);
  const blob = await canvas.convertToBlob({ type: "image/png" });
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}
