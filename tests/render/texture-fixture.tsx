import { activateFixtureCamera } from "../integration/camera-fixtures.js";
import type { AssetWorldClient as Client } from "@ipp/client";
import type {
  ComponentFieldValue,
  Inspection,
  AssetResourceSnapshot,
} from "@ipp/client";
import { workerTransport } from "@ipp/client";
import {
  AssetSourceFixture,
  type SourceResult,
} from "./asset-source-fixture.js";
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
  BACKGROUND_RGB,
  compareImages,
  type FramePixels,
  type ImageDifference,
  type ImageSummary,
  summarizeImage,
  VIEWPORT,
} from "./image-assertions.js";

export const OPTIONAL_LAYOUTS = ["position", "color", "uv", "weight"] as const;

export const CUBE_MATERIAL = Object.freeze([0.78, 0.44, 0.91] as const);
export const QUAD_VERTEX_COLOR = Object.freeze([0.63, 0.47, 0.81] as const);
export const QUAD_MATERIAL = Object.freeze([0.72, 0.55, 0.88] as const);
export const LAYOUT_COLOR = Object.freeze([0.18, 0.72, 0.32] as const);
export const WEIGHT_COLOR = Object.freeze([0.7, 0.6, 0.5] as const);
export const WEIGHT_BYTES = Object.freeze([0, 85, 170, 255] as const);
export const PLANE_MATERIAL = Object.freeze([0.64, 0.38, 0.82] as const);
export const ASYMMETRIC_RGB = Object.freeze([
  [37, 83, 149],
  [211, 67, 129],
  [94, 203, 51],
  [173, 121, 237],
  [53, 229, 181],
  [241, 157, 43],
] as const);

export const CUBE_SOURCE = "ipp://mesh/cube?width=2&height=2&length=2";
export const PLANE_SOURCE =
  "ipp://mesh/plane?size=2&normalLength=1.25&stroke=0.05";
export const CHECKER_SOURCE =
  "ipp://texture/checkerboard?width=3072&height=2048&cellsX=8&cellsY=8";
const QUAD_HALF_WIDTH = 2;
const QUAD_HALF_HEIGHT = 1.5;
const QUAD_UV_EXTENT = 2;
const QUAD_VERTEX_COUNT = 4;
const QUAD_INDEX_COUNT = 6;
const QUAD_SOURCE_BYTES =
  16 +
  QUAD_VERTEX_COUNT * 8 * Float32Array.BYTES_PER_ELEMENT +
  QUAD_INDEX_COUNT * Uint16Array.BYTES_PER_ELEMENT;
const ASYMMETRIC_SOURCE_BYTES = 16 + 3 * 2 * 4;
const CAMERA_DISTANCE = Math.sqrt(38);
const CAMERA_BACK = [
  3 / CAMERA_DISTANCE,
  2 / CAMERA_DISTANCE,
  5 / CAMERA_DISTANCE,
] as const;
const CAMERA_RIGHT_LENGTH = Math.hypot(CAMERA_BACK[2], CAMERA_BACK[0]);
const CAMERA_RIGHT = [
  CAMERA_BACK[2] / CAMERA_RIGHT_LENGTH,
  0,
  -CAMERA_BACK[0] / CAMERA_RIGHT_LENGTH,
] as const;
const CAMERA_UP = [
  CAMERA_BACK[1] * CAMERA_RIGHT[2],
  CAMERA_BACK[2] * CAMERA_RIGHT[0] - CAMERA_BACK[0] * CAMERA_RIGHT[2],
  -CAMERA_BACK[1] * CAMERA_RIGHT[0],
] as const;
const OPTIONAL_QUAD_WIDTH = 1.5;
const OPTIONAL_QUAD_HEIGHT = 1.1;
const OPTIONAL_LAYOUT_CENTERS = Object.freeze({
  position: [-1.15, 0.75],
  color: [1.15, 0.75],
  uv: [-1.15, -0.75],
  weight: [1.15, -0.75],
} as const);

type OptionalLayout = (typeof OPTIONAL_LAYOUTS)[number];
type Vec2 = readonly [number, number];
type Vec3 = readonly [number, number, number];

interface GeneratedModule {
  readonly MAX_MESSAGE_BYTES: number;
  readonly IppClient: {
    connectTransport(
      transport: import("@ipp/client").MessageTransport,
      options: { readonly timeoutMs: number },
    ): Promise<Client>;
  };
  acceptBootstrap(bytes: Uint8Array): bigint;
  decodeResponse(
    bytes: Uint8Array,
    session: bigint,
  ): import("@ipp/client").Response;
  readonly UnlitTexture: { readonly id: number };
}

interface FixtureState {
  readonly contract: GeneratedModule;
  readonly client: Client;
  readonly presentation: ClientPresentation;
  readonly root: ReactWorldRoot;
  readonly producer: AssetSourceFixture;
  readonly sources: {
    readonly quad: string;
    readonly checker: string;
    readonly asymmetric: string;
    readonly legacyV1: string;
    readonly optional: Readonly<Record<OptionalLayout, string>>;
  };
  readonly captures: Map<string, FrameCapture>;
  scene:
    | "cube-textured"
    | "cube-untextured"
    | "optional-layouts"
    | "quad"
    | "plane-textured"
    | "plane-untextured";
}

export interface TextureRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs: number;
  readonly assetBaseUrl: string;
}

export interface TextureSetupReport {
  readonly resources: readonly AssetResourceSnapshot[];
  readonly componentId: number;
  readonly startupBackend: Readonly<Record<string, unknown>>;
  readonly inspection: WorldInspection;
}

export interface OptionalLayoutSetupReport {
  readonly resources: readonly AssetResourceSnapshot[];
}

interface OptionalLayoutEntry {
  readonly entity: bigint;
  readonly source: string;
  readonly slot: number;
}

export interface OptionalLayoutOrderReport {
  readonly before: readonly OptionalLayoutEntry[];
  readonly after: readonly OptionalLayoutEntry[];
}

export interface OptionalLayoutSample {
  readonly layout: OptionalLayout;
  readonly weightByte: number | null;
  readonly rgba: readonly [number, number, number, number];
}

export interface CaptureReport {
  readonly label: string;
  readonly session: bigint;
  readonly tick: bigint;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly contextGeneration: number;
  readonly backend: Readonly<Record<string, unknown>>;
  readonly summary: ImageSummary;
  readonly inspection: Inspection;
  readonly resourceCount: number;
}

export interface WorldInspection {
  readonly entityExists: boolean;
  readonly texture: Readonly<Record<string, ComponentFieldValue>> | null;
  readonly material: Readonly<Record<string, ComponentFieldValue>> | null;
}

export interface ColorEvidence {
  readonly counts: readonly number[];
  readonly foregroundPixels: number;
  readonly unmatchedForegroundPixels: number;
  readonly adjacentTransitions: number;
}

export interface SamplerObservation {
  readonly uv: readonly [number, number];
  readonly sourceIndex: number;
  readonly coordinate: readonly [number, number];
  readonly rgba: readonly [number, number, number, number];
}

export interface RejectionReport {
  readonly malformed: SourceResult;
  readonly duplicate: SourceResult;
  readonly weightWithoutUv: SourceResult;
  readonly texturedWithoutUv: string;
  readonly failedSource: AssetResourceSnapshot;
  readonly before: WorldInspection;
  readonly after: WorldInspection;
  readonly resourceCount: number;
}

let active: FixtureState | undefined;

export async function initializeTextures(
  configuration: TextureRuntimeConfiguration,
): Promise<TextureSetupReport> {
  await closeTextures();
  const canvas = document.createElement("canvas");
  canvas.id = "ipp-texture-canvas";
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
    const transport = workerTransport(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      contract.MAX_MESSAGE_BYTES,
      {
        canvas: canvas.transferControlToOffscreen(),
      },
    );
    client = await contract.IppClient.connectTransport(transport, {
      timeoutMs: configuration.timeoutMs,
    });
    const producer = new AssetSourceFixture(client);
    if (
      !client.capabilities.spatial ||
      !client.capabilities.stateOverlays ||
      !client.capabilities.textures ||
      !client.capabilities.builtinAssets
    ) {
      throw new Error(
        "texture fixture requires scene, overlays, textures, and builtin assets",
      );
    }
    if (contract.UnlitTexture.id !== 6) {
      throw new Error(
        `generated UnlitTexture id is ${contract.UnlitTexture.id}; expected 6`,
      );
    }
    const presentation = client.presentation;
    if (!presentation)
      throw new Error("texture worker did not expose presentation");
    presentation.resize(VIEWPORT.width, VIEWPORT.height);

    const startupInspection = await client.inspect();
    const startupFrame = await presentation.capture(startupInspection.tick);
    if (startupFrame.drawCalls !== 0 || startupFrame.triangles !== 0) {
      throw new Error("texture renderer drew before a scene was declared");
    }

    await activateFixtureCamera(client);
    root = createRoot(client);
    active = {
      contract,
      client,
      presentation,
      root,
      producer,
      sources: {
        quad: `${configuration.assetBaseUrl}/sampler-quad.mesh`,
        checker: `${configuration.assetBaseUrl}/checker.texture`,
        asymmetric: `${configuration.assetBaseUrl}/asymmetric.texture`,
        legacyV1: `${configuration.assetBaseUrl}/legacy-v1.texture`,
        optional: Object.fromEntries(
          OPTIONAL_LAYOUTS.map((layout) => [
            layout,
            `${configuration.assetBaseUrl}/optional-${layout}.mesh`,
          ]),
        ) as Record<OptionalLayout, string>,
      },
      captures: new Map(),
      scene: "cube-textured",
    };
    await renderCube(active, true);
    const resources = await waitForActiveResources(active);
    return {
      resources,
      componentId: contract.UnlitTexture.id,
      startupBackend: startupFrame.backend,
      inspection: await inspectScene(active, "texture-cube"),
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
    if (failures.length > 1) {
      throw new AggregateError(failures, "texture fixture startup failed");
    }
    throw error;
  }
}

export async function setCubeTextureEnabled(
  enabled: boolean,
): Promise<WorldInspection> {
  const state = requireActive();
  await renderCube(state, enabled);
  return await inspectScene(state, "texture-cube");
}

export async function mountOptionalLayoutScene(): Promise<OptionalLayoutSetupReport> {
  const state = requireActive();
  await state.root.render(null);
  await state.root.flush();
  await renderOptionalLayouts(state);
  return { resources: await waitForActiveResources(state) };
}

export async function reverseOptionalLayoutScene(): Promise<OptionalLayoutOrderReport> {
  const state = requireActive();
  if (state.scene !== "optional-layouts") {
    throw new Error("optional layout scene is not mounted");
  }
  const before = await inspectOptionalLayoutOrder(state);
  const reversed = [...before].reverse();
  const layouts: OptionalLayout[] = [];
  for (const [index, entry] of before.entries()) {
    const layout = OPTIONAL_LAYOUTS.find(
      (name) => state.sources.optional[name] === reversed[index]!.source,
    );
    if (!layout) throw new Error("optional layout mesh is not in the fixture");
    layouts[entry.slot] = layout;
  }
  await renderOptionalLayouts(state, layouts);
  return { before, after: await inspectOptionalLayoutOrder(state) };
}

export function sampleOptionalLayouts(label: string): OptionalLayoutSample[] {
  const frame = requireCapture(requireActive(), label);
  const samples: OptionalLayoutSample[] = [];
  for (const layout of ["position", "color", "uv"] as const) {
    const center = OPTIONAL_LAYOUT_CENTERS[layout];
    const coordinate = cameraPlanePixel(center[0], center[1]);
    samples.push({
      layout,
      weightByte: null,
      rgba: sample(frame, coordinate[0], coordinate[1]),
    });
  }
  const weightCenter = OPTIONAL_LAYOUT_CENTERS.weight;
  for (let index = 0; index < WEIGHT_BYTES.length; index += 1) {
    const stripeCenter =
      weightCenter[0] +
      ((index + 0.5) / WEIGHT_BYTES.length - 0.5) * OPTIONAL_QUAD_WIDTH;
    const coordinate = cameraPlanePixel(stripeCenter, weightCenter[1]);
    samples.push({
      layout: "weight",
      weightByte: WEIGHT_BYTES[index] ?? null,
      rgba: sample(frame, coordinate[0], coordinate[1]),
    });
  }
  return samples;
}

export async function mountSamplerQuad(): Promise<{
  readonly resources: readonly AssetResourceSnapshot[];
  readonly inspection: WorldInspection;
}> {
  const state = requireActive();
  await state.root.render(null);
  await state.root.flush();
  await renderQuad(state, state.sources.asymmetric, QUAD_MATERIAL);
  state.scene = "quad";
  return {
    resources: await waitForActiveResources(state),
    inspection: await inspectScene(state, "sampler-quad"),
  };
}

export async function rejectTextureMutations(): Promise<RejectionReport> {
  const state = requireActive();
  if (state.scene !== "quad") throw new Error("sampler quad is not mounted");
  const before = await inspectScene(state, "sampler-quad");

  const malformedBytes = createAsymmetricTexture().slice(0, 23);
  const malformed = await state.producer.register(2, 301n, malformedBytes);
  if (
    malformed.status !== "failed" ||
    !/InvalidAsset/.test(malformed.error ?? "")
  ) {
    throw new Error(
      `malformed IPPTv3 expected InvalidAsset, received ${receiptResult(malformed)}`,
    );
  }

  requireTextureSuccess(
    await state.producer.register(2, 303n, createAsymmetricTexture()),
    "distinct named asymmetric texture",
  );
  const duplicate = await state.producer.register(
    2,
    303n,
    createAsymmetricTexture(),
  );
  if (
    duplicate.status !== "failed" ||
    !/already registered|DuplicateAsset/.test(duplicate.error ?? "")
  ) {
    throw new Error(
      `duplicate texture expected DuplicateAsset, received ${receiptResult(duplicate)}`,
    );
  }

  const weightWithoutUv = await state.producer.register(
    1,
    302n,
    createWeightWithoutUvMesh(),
  );
  if (
    weightWithoutUv.status !== "failed" ||
    !/InvalidAsset/.test(weightWithoutUv.error ?? "")
  ) {
    throw new Error(
      `weight without UV expected InvalidAsset, received ${meshReceiptResult(weightWithoutUv)}`,
    );
  }

  await renderSingleMesh(
    state,
    "sampler-quad",
    state.sources.optional.position,
    state.sources.asymmetric,
    QUAD_MATERIAL,
  );
  await waitForActiveResources(state);
  const incompatibleInspection = await state.client.inspect();
  const texturedWithoutUv = incompatibleInspection.renderDiagnostics.find(
    ({ reason }) => reason === "InvalidAsset",
  )?.reason;
  if (!texturedWithoutUv) {
    throw new Error(
      "position-only textured mesh omitted its render diagnostic",
    );
  }

  await renderQuad(state, state.sources.legacyV1, [
    0.02,
    QUAD_MATERIAL[1],
    QUAD_MATERIAL[2],
  ]);
  const failedSource = await waitForResource(
    state,
    state.sources.legacyV1,
    "failed",
  );
  await renderQuad(state, state.sources.asymmetric, QUAD_MATERIAL);
  await waitForActiveResources(state);
  const after = await inspectScene(state, "sampler-quad");
  return {
    malformed,
    duplicate,
    weightWithoutUv,
    texturedWithoutUv,
    failedSource,
    before,
    after,
    resourceCount: (await state.client.inspect()).resources.length,
  };
}

export async function setPlaneTextureEnabled(
  enabled: boolean,
): Promise<WorldInspection> {
  const state = requireActive();
  await renderSingleMesh(
    state,
    "weighted-plane",
    PLANE_SOURCE,
    enabled ? state.sources.asymmetric : undefined,
    PLANE_MATERIAL,
  );
  state.scene = enabled ? "plane-textured" : "plane-untextured";
  return await inspectScene(state, "weighted-plane");
}

export async function recoverTextureContext(beforeLabel: string): Promise<{
  readonly beforeGeneration: number;
  readonly after: CaptureReport;
  readonly resourceCountBefore: number;
  readonly resourceCountAfter: number;
}> {
  const state = requireActive();
  const before = requireCapture(state, beforeLabel);
  const resourceCountBefore = (await state.client.inspect()).resources.length;
  state.presentation.loseContext();
  await compositorBarrier();
  state.presentation.restoreContext();
  const after = await captureTextureFrame("after-context-restore");
  if (after.contextGeneration <= before.contextGeneration) {
    throw new Error("context generation did not advance after restoration");
  }
  return {
    beforeGeneration: before.contextGeneration,
    after,
    resourceCountBefore,
    resourceCountAfter: after.resourceCount,
  };
}

/** A changed HTTP endpoint must fail recovery while preserving logical identity. */
export async function rejectedTextureRecovery(): Promise<{
  before: AssetResourceSnapshot;
  after: AssetResourceSnapshot;
  drawCalls: number;
}> {
  const state = requireActive();
  const before = await waitForResource(state, state.sources.checker, "loaded");
  state.presentation.loseContext();
  await compositorBarrier();
  state.presentation.restoreContext();
  const after = await waitForResource(state, state.sources.checker, "failed");
  const inspection = await state.client.inspect();
  const frame = await state.presentation.capture(inspection.tick);
  state.captures.set("rejected-recovery", frame);
  return { before, after, drawCalls: frame.drawCalls };
}

export async function captureTextureFrame(
  label: string,
): Promise<CaptureReport> {
  const state = requireActive();
  await waitForActiveResources(state);
  const inspection = await state.client.inspect();
  const frame = await state.presentation.capture(inspection.tick);
  if (frame.session !== state.client.session) {
    throw new Error("capture returned a different client session");
  }
  if (frame.tick < inspection.tick) {
    throw new Error("capture predates the inspected core tick");
  }
  if (frame.width !== VIEWPORT.width || frame.height !== VIEWPORT.height) {
    throw new Error(`unexpected capture size ${frame.width}x${frame.height}`);
  }
  state.captures.set(label, { ...frame, pixels: frame.pixels.slice(0) });
  await compositorBarrier();
  return {
    label,
    session: frame.session,
    tick: frame.tick,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    contextGeneration: frame.contextGeneration,
    backend: frame.backend,
    summary: summarizeImage(frame),
    inspection,
    resourceCount: inspection.resources.length,
  };
}

export function analyzeCapturedColors(
  label: string,
  expected: readonly (readonly [number, number, number])[],
  tolerance: number,
): ColorEvidence {
  const frame = requireCapture(requireActive(), label);
  const pixels = new Uint8Array(frame.pixels);
  const classes = new Int16Array(frame.width * frame.height).fill(-1);
  const counts = expected.map(() => 0);
  let foregroundPixels = 0;
  let unmatchedForegroundPixels = 0;
  for (let index = 0; index < classes.length; index += 1) {
    const offset = index * 4;
    const rgb = [
      pixels[offset] ?? 0,
      pixels[offset + 1] ?? 0,
      pixels[offset + 2] ?? 0,
    ] as const;
    if (maximumDifference(rgb, BACKGROUND_RGB) <= 8) continue;
    foregroundPixels += 1;
    const match = expected.findIndex(
      (color) => maximumDifference(rgb, color) <= tolerance,
    );
    if (match < 0) unmatchedForegroundPixels += 1;
    else {
      classes[index] = match;
      counts[match] = (counts[match] ?? 0) + 1;
    }
  }

  let adjacentTransitions = 0;
  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const index = y * frame.width + x;
      const value = classes[index] ?? -1;
      if (value < 0) continue;
      if (x + 1 < frame.width) {
        const right = classes[index + 1] ?? -1;
        if (right >= 0 && right !== value) adjacentTransitions += 1;
      }
      if (y + 1 < frame.height) {
        const below = classes[index + frame.width] ?? -1;
        if (below >= 0 && below !== value) adjacentTransitions += 1;
      }
    }
  }
  return {
    counts,
    foregroundPixels,
    unmatchedForegroundPixels,
    adjacentTransitions,
  };
}

export function sampleAsymmetricTexture(label: string): SamplerObservation[] {
  const frame = requireCapture(requireActive(), label);
  const samples: SamplerObservation[] = [];
  for (const v of [0.25, 0.75, 1.25, 1.75]) {
    for (const u of [1 / 6, 0.5, 5 / 6, 7 / 6, 1.5, 11 / 6]) {
      const coordinate = uvToPixel(u, v);
      samples.push({
        uv: [u, v],
        sourceIndex:
          modulo(Math.floor(v * 2), 2) * 3 + modulo(Math.floor(u * 3), 3),
        coordinate,
        rgba: sample(frame, coordinate[0], coordinate[1]),
      });
    }
  }
  return samples;
}

export function compareCaptured(
  firstLabel: string,
  secondLabel: string,
): ImageDifference {
  const state = requireActive();
  return compareImages(
    requireCapture(state, firstLabel),
    requireCapture(state, secondLabel),
  );
}

export async function captureDataUrl(label: string): Promise<string> {
  return await frameDataUrl(requireCapture(requireActive(), label));
}

export function captureMetadata(label: string): Omit<FrameCapture, "pixels"> {
  const { pixels: _pixels, ...metadata } = requireCapture(
    requireActive(),
    label,
  );
  return metadata;
}

export async function differenceDataUrl(
  firstLabel: string,
  secondLabel: string,
): Promise<string> {
  const state = requireActive();
  const first = requireCapture(state, firstLabel);
  const second = requireCapture(state, secondLabel);
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

export async function samplerExpectedDataUrl(label: string): Promise<string> {
  const frame = requireCapture(requireActive(), label);
  return await rgbaDataUrl(
    frame.width,
    frame.height,
    samplerExpectedPixels(frame),
  );
}

export async function samplerDifferenceDataUrl(label: string): Promise<string> {
  const frame = requireCapture(requireActive(), label);
  const expected = samplerExpectedPixels(frame);
  const actual = new Uint8Array(frame.pixels);
  const difference = new Uint8ClampedArray(actual.byteLength);
  for (let offset = 0; offset < difference.length; offset += 4) {
    for (let channel = 0; channel < 3; channel += 1) {
      difference[offset + channel] = Math.min(
        255,
        Math.abs(
          (actual[offset + channel] ?? 0) - (expected[offset + channel] ?? 0),
        ) * 4,
      );
    }
    difference[offset + 3] = 255;
  }
  return await rgbaDataUrl(frame.width, frame.height, difference);
}

export async function closeTextures(): Promise<void> {
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
  document.querySelector("#ipp-texture-canvas")?.remove();
  if (failures.length > 0) {
    throw new AggregateError(failures, "texture fixture cleanup failed");
  }
}

export function createOptionalLayoutMesh(layout: OptionalLayout): ArrayBuffer {
  const center = OPTIONAL_LAYOUT_CENTERS[layout];
  if (layout === "weight") {
    const positions: Vec3[] = [];
    const colors: number[] = [];
    const uvs: number[] = [];
    const weights: number[] = [];
    const indices: number[] = [];
    const stripeWidth = OPTIONAL_QUAD_WIDTH / WEIGHT_BYTES.length;
    for (let stripe = 0; stripe < WEIGHT_BYTES.length; stripe += 1) {
      const stripeCenter =
        center[0] - OPTIONAL_QUAD_WIDTH / 2 + stripeWidth * (stripe + 0.5);
      positions.push(
        ...cameraQuad(
          [stripeCenter, center[1]],
          stripeWidth,
          OPTIONAL_QUAD_HEIGHT,
        ),
      );
      for (let vertex = 0; vertex < 4; vertex += 1) {
        colors.push(...WEIGHT_COLOR);
        uvs.push(0.25, 0.25);
        weights.push(WEIGHT_BYTES[stripe] ?? 0);
      }
      const first = stripe * 4;
      indices.push(first, first + 1, first + 2, first, first + 2, first + 3);
    }
    return encodeV3Mesh(positions, indices, [
      { semantic: 1, format: 1, values: colors },
      { semantic: 2, format: 2, values: uvs },
      { semantic: 3, format: 3, values: weights },
    ]);
  }

  const positions = cameraQuad(
    center,
    OPTIONAL_QUAD_WIDTH,
    OPTIONAL_QUAD_HEIGHT,
  );
  if (layout === "position") return encodeV3Mesh(positions, QUAD_INDICES, []);
  if (layout === "color") {
    return encodeV3Mesh(positions, QUAD_INDICES, [
      { semantic: 1, format: 1, values: repeated(4, LAYOUT_COLOR) },
    ]);
  }
  return encodeV3Mesh(positions, QUAD_INDICES, [
    { semantic: 2, format: 2, values: repeated(4, [0.25, 0.25]) },
  ]);
}

function createWeightWithoutUvMesh(): ArrayBuffer {
  return encodeV3Mesh(cameraQuad([0, 0], 1, 1), QUAD_INDICES, [
    { semantic: 3, format: 3, values: [255, 255, 255, 255] },
  ]);
}

type V3Attribute =
  | {
      readonly semantic: 1;
      readonly format: 1;
      readonly values: readonly number[];
    }
  | {
      readonly semantic: 2;
      readonly format: 2;
      readonly values: readonly number[];
    }
  | {
      readonly semantic: 3;
      readonly format: 3;
      readonly values: readonly number[];
    };

const QUAD_INDICES = [0, 1, 2, 0, 2, 3] as const;

function encodeV3Mesh(
  positions: readonly Vec3[],
  indices: readonly number[],
  optionalAttributes: readonly V3Attribute[],
): ArrayBuffer {
  const positionValues = positions.flatMap((position) => [...position]);
  const attributes = [
    { semantic: 0, format: 1, values: positionValues },
    ...optionalAttributes,
  ] as const;
  const streamBytes = attributes.map(({ semantic, values }) =>
    semantic === 3 ? values.length : values.length * 4,
  );
  const sourceBytes =
    20 +
    attributes.length * 8 +
    streamBytes.reduce((total, bytes) => total + bytes, 0) +
    indices.length * 2;
  const buffer = new ArrayBuffer(sourceBytes);
  const view = new DataView(buffer);
  let offset = 0;
  for (const byte of [0x49, 0x50, 0x50, 0x4d]) {
    view.setUint8(offset, byte);
    offset += 1;
  }
  for (const value of [
    3,
    positions.length,
    indices.length,
    attributes.length,
  ]) {
    view.setUint32(offset, value, true);
    offset += 4;
  }
  for (const [index, attribute] of attributes.entries()) {
    view.setUint8(offset, attribute.semantic);
    view.setUint8(offset + 1, attribute.format);
    view.setUint16(offset + 2, 0, true);
    view.setUint32(offset + 4, streamBytes[index] ?? 0, true);
    offset += 8;
  }
  for (const attribute of attributes) {
    for (const value of attribute.values) {
      if (attribute.semantic === 3) {
        view.setUint8(offset, value);
        offset += 1;
      } else {
        view.setFloat32(offset, value, true);
        offset += 4;
      }
    }
  }
  for (const index of indices) {
    view.setUint16(offset, index, true);
    offset += 2;
  }
  if (offset !== buffer.byteLength) {
    throw new Error(
      `IPPMv3 fixture wrote ${offset} of ${buffer.byteLength} bytes`,
    );
  }
  return buffer;
}

function cameraQuad(center: Vec2, width: number, height: number): Vec3[] {
  return [
    cameraPlanePoint(center[0] - width / 2, center[1] - height / 2),
    cameraPlanePoint(center[0] + width / 2, center[1] - height / 2),
    cameraPlanePoint(center[0] + width / 2, center[1] + height / 2),
    cameraPlanePoint(center[0] - width / 2, center[1] + height / 2),
  ];
}

function cameraPlanePoint(x: number, y: number): Vec3 {
  return [
    CAMERA_RIGHT[0] * x + CAMERA_UP[0] * y,
    CAMERA_RIGHT[1] * x + CAMERA_UP[1] * y,
    CAMERA_RIGHT[2] * x + CAMERA_UP[2] * y,
  ];
}

function cameraPlanePixel(x: number, y: number): readonly [number, number] {
  const projection = 1 / Math.tan(Math.PI / 8);
  const ndcX =
    (projection / (VIEWPORT.width / VIEWPORT.height)) * (x / CAMERA_DISTANCE);
  const ndcY = projection * (y / CAMERA_DISTANCE);
  return [
    Math.round(((ndcX + 1) * VIEWPORT.width) / 2),
    Math.round(((1 - ndcY) * VIEWPORT.height) / 2),
  ];
}

function repeated(count: number, values: readonly number[]): number[] {
  return Array.from({ length: count }, () => values).flat();
}

/** Reproducible camera-facing IPPMv2 quad, with UV 0..2 in both axes. */
export function createSamplerQuadMesh(): ArrayBuffer {
  const eyeLength = Math.sqrt(38);
  const back = [3 / eyeLength, 2 / eyeLength, 5 / eyeLength] as const;
  const rightLength = Math.hypot(back[2], back[0]);
  const right = [back[2] / rightLength, 0, -back[0] / rightLength] as const;
  const up = [
    back[1] * right[2],
    back[2] * right[0] - back[0] * right[2],
    -back[1] * right[0],
  ] as const;
  const corner = (x: number, y: number): readonly [number, number, number] => [
    right[0] * x + up[0] * y,
    right[1] * x + up[1] * y,
    right[2] * x + up[2] * y,
  ];
  const vertices = [
    [corner(-QUAD_HALF_WIDTH, -QUAD_HALF_HEIGHT), [0, QUAD_UV_EXTENT]],
    [
      corner(QUAD_HALF_WIDTH, -QUAD_HALF_HEIGHT),
      [QUAD_UV_EXTENT, QUAD_UV_EXTENT],
    ],
    [corner(QUAD_HALF_WIDTH, QUAD_HALF_HEIGHT), [QUAD_UV_EXTENT, 0]],
    [corner(-QUAD_HALF_WIDTH, QUAD_HALF_HEIGHT), [0, 0]],
  ] as const;
  const buffer = new ArrayBuffer(QUAD_SOURCE_BYTES);
  const view = new DataView(buffer);
  let offset = 0;
  for (const byte of [0x49, 0x50, 0x50, 0x4d]) {
    view.setUint8(offset, byte);
    offset += 1;
  }
  for (const value of [2, QUAD_VERTEX_COUNT, QUAD_INDEX_COUNT]) {
    view.setUint32(offset, value, true);
    offset += 4;
  }
  for (const [position, uv] of vertices) {
    for (const value of [...position, ...QUAD_VERTEX_COLOR, ...uv]) {
      view.setFloat32(offset, value, true);
      offset += 4;
    }
  }
  for (const index of [0, 1, 2, 0, 2, 3]) {
    view.setUint16(offset, index, true);
    offset += 2;
  }
  if (offset !== buffer.byteLength) {
    throw new Error(
      `quad fixture wrote ${offset} of ${buffer.byteLength} bytes`,
    );
  }
  return buffer;
}

/** Reproducible odd-row IPPTv3 source with unique nonbinary RGB in every texel. */
export function createAsymmetricTexture(): ArrayBuffer {
  const buffer = new ArrayBuffer(ASYMMETRIC_SOURCE_BYTES);
  const view = new DataView(buffer);
  let offset = 0;
  for (const byte of [0x49, 0x50, 0x50, 0x54]) {
    view.setUint8(offset, byte);
    offset += 1;
  }
  for (const value of [3, 3, 2]) {
    view.setUint32(offset, value, true);
    offset += 4;
  }
  for (const rgb of ASYMMETRIC_RGB) {
    for (const value of [...rgb, 255]) {
      view.setUint8(offset, value);
      offset += 1;
    }
  }
  if (offset !== buffer.byteLength) {
    throw new Error(
      `texture fixture wrote ${offset} of ${buffer.byteLength} bytes`,
    );
  }
  return buffer;
}

/** Complete former IPPTv1 RGBA payload, retained only as a rejection fixture. */
export function createLegacyV1Texture(): ArrayBuffer {
  const buffer = new ArrayBuffer(16 + 2 * 2 * 4);
  const view = new DataView(buffer);
  let offset = 0;
  for (const byte of [0x49, 0x50, 0x50, 0x54]) {
    view.setUint8(offset, byte);
    offset += 1;
  }
  for (const value of [1, 2, 2]) {
    view.setUint32(offset, value, true);
    offset += 4;
  }
  for (const rgb of ASYMMETRIC_RGB.slice(0, 4)) {
    for (const value of [...rgb, 255]) {
      view.setUint8(offset, value);
      offset += 1;
    }
  }
  return buffer;
}

async function renderCube(
  state: FixtureState,
  textured: boolean,
): Promise<void> {
  await state.root.render(
    <Entity id="texture-cube">
      <Transform bound={false} />
      <UnlitMaterial
        bound={false}
        r={CUBE_MATERIAL[0]}
        g={CUBE_MATERIAL[1]}
        b={CUBE_MATERIAL[2]}
      />
      <MeshInstance bound={false} source={CUBE_SOURCE} />
      {textured ? (
        <UnlitTexture bound={false} source={state.sources.checker} />
      ) : null}
    </Entity>,
  );
  await state.root.flush();
  state.scene = textured ? "cube-textured" : "cube-untextured";
}

async function renderQuad(
  state: FixtureState,
  texture: string,
  material: readonly [number, number, number],
): Promise<void> {
  await state.root.render(
    <Entity id="sampler-quad">
      <Transform bound={false} />
      <UnlitMaterial
        bound={false}
        r={material[0]}
        g={material[1]}
        b={material[2]}
      />
      <MeshInstance bound={false} source={state.sources.quad} />
      <UnlitTexture bound={false} source={texture} />
    </Entity>,
  );
  await state.root.flush();
}

async function renderOptionalLayouts(
  state: FixtureState,
  layouts: readonly OptionalLayout[] = ["position", "color", "uv", "weight"],
): Promise<void> {
  await state.root.render(
    // Keep entity slots stable while swapping their meshes: changing keyed
    // child order alone does not change the core's entity-ordered draw list.
    layouts.map((layout, index) => (
      <Entity id={`optional-slot-${index}`} key={index}>
        <Transform bound={false} />
        <UnlitMaterial bound={false} />
        <MeshInstance bound={false} source={state.sources.optional[layout]} />
        {layout === "uv" || layout === "weight" ? (
          <UnlitTexture bound={false} source={state.sources.asymmetric} />
        ) : null}
      </Entity>
    )),
  );
  await state.root.flush();
  state.scene = "optional-layouts";
}

async function inspectOptionalLayoutOrder(
  state: FixtureState,
): Promise<OptionalLayoutOrderReport["before"]> {
  const inspection = await state.client.inspect();
  return inspection.entities
    .filter(({ metadata }) => metadata.symbolicId?.startsWith("optional-slot-"))
    .map((entity) => {
      const source = componentFields(entity.effective, 5)?.source;
      if (typeof source !== "string") {
        throw new Error("optional layout entity has no effective mesh");
      }
      return {
        entity: entity.id,
        source,
        slot: Number(
          entity.metadata.symbolicId!.slice("optional-slot-".length),
        ),
      };
    });
}

async function renderSingleMesh(
  state: FixtureState,
  id: string,
  mesh: string,
  texture: string | undefined,
  material: readonly [number, number, number],
): Promise<void> {
  await state.root.render(
    <Entity id={id}>
      <Transform bound={false} />
      <UnlitMaterial
        bound={false}
        r={material[0]}
        g={material[1]}
        b={material[2]}
      />
      <MeshInstance bound={false} source={mesh} />
      {texture ? <UnlitTexture bound={false} source={texture} /> : null}
    </Entity>,
  );
  await state.root.flush();
}

function requireActive(): FixtureState {
  if (!active) throw new Error("texture fixture is not initialized");
  return active;
}

function requireCapture(state: FixtureState, label: string): FrameCapture {
  const frame = state.captures.get(label);
  if (!frame) throw new Error(`missing captured frame '${label}'`);
  return frame;
}

async function waitForActiveResources(
  state: FixtureState,
): Promise<readonly AssetResourceSnapshot[]> {
  const deadline = performance.now() + 10_000;
  while (true) {
    const inspection = await state.client.inspect();
    const sources = new Set(
      inspection.entities.flatMap((entity) =>
        entity.effective
          .map((component) => component.fields.source)
          .filter((source): source is string => typeof source === "string"),
      ),
    );
    const resources = inspection.resources.filter((resource) =>
      sources.has(resource.source),
    );
    const failed = resources.find((resource) => resource.status === "failed");
    if (failed) {
      throw new Error(
        `Resource ${failed.source} failed: ${failed.error ?? "unknown error"}`,
      );
    }
    if (
      resources.length > 0 &&
      resources.every((resource) => resource.status === "loaded")
    ) {
      return resources;
    }
    if (performance.now() >= deadline) {
      throw new Error("Timed out waiting for active texture resources");
    }
    await state.client.waitForFrame(inspection.tick);
  }
}

async function waitForResource(
  state: FixtureState,
  source: string,
  status: AssetResourceSnapshot["status"],
): Promise<AssetResourceSnapshot> {
  const deadline = performance.now() + 10_000;
  while (true) {
    const inspection = await state.client.inspect();
    const resource = inspection.resources.find(
      (candidate) => candidate.source === source,
    );
    if (resource?.status === status) return resource;
    if (performance.now() >= deadline) {
      throw new Error(`Timed out waiting for ${source} to become ${status}`);
    }
    await state.client.waitForFrame(inspection.tick);
  }
}

async function inspectScene(
  state: FixtureState,
  symbolicId: string,
): Promise<WorldInspection> {
  const inspection = await state.client.inspect();
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === symbolicId,
  );
  return {
    entityExists: entity !== undefined,
    texture: componentFields(entity?.effective, 6),
    material: componentFields(entity?.effective, 4),
  };
}

function componentFields(
  components: Inspection["entities"][number]["effective"] | undefined,
  id: number,
): Inspection["entities"][number]["effective"][number]["fields"] | null {
  return components?.find(({ component }) => component === id)?.fields ?? null;
}

function requireTextureSuccess(result: SourceResult, label: string): void {
  if (result.status !== "loaded")
    throw new Error(`${label} failed: ${result.error}`);
}

function receiptResult(result: SourceResult): string {
  return result.status === "loaded" ? "success" : (result.error ?? "failed");
}

const meshReceiptResult = receiptResult;

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

function uvToPixel(u: number, v: number): readonly [number, number] {
  const bounds = quadPixelBounds();
  return [
    Math.round(bounds.left + (u / QUAD_UV_EXTENT) * bounds.width),
    Math.round(bounds.top + (v / QUAD_UV_EXTENT) * bounds.height),
  ];
}

function quadPixelBounds(): {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
} {
  const distance = Math.sqrt(38);
  const projection = 1 / Math.tan(Math.PI / 8);
  const halfNdcX =
    (projection / (VIEWPORT.width / VIEWPORT.height)) *
    (QUAD_HALF_WIDTH / distance);
  const halfNdcY = projection * (QUAD_HALF_HEIGHT / distance);
  const left = ((1 - halfNdcX) * VIEWPORT.width) / 2;
  const top = ((1 - halfNdcY) * VIEWPORT.height) / 2;
  const width = halfNdcX * VIEWPORT.width;
  const height = halfNdcY * VIEWPORT.height;
  return { left, top, width, height };
}

function sample(
  frame: FrameCapture,
  x: number,
  y: number,
): readonly [number, number, number, number] {
  if (x < 0 || y < 0 || x >= frame.width || y >= frame.height) {
    throw new RangeError(`sample (${x}, ${y}) is outside the capture`);
  }
  const pixels = new Uint8Array(frame.pixels);
  const offset = (y * frame.width + x) * 4;
  return [
    pixels[offset] ?? 0,
    pixels[offset + 1] ?? 0,
    pixels[offset + 2] ?? 0,
    pixels[offset + 3] ?? 0,
  ];
}

function samplerExpectedPixels(
  frame: FrameCapture,
): Uint8ClampedArray<ArrayBuffer> {
  const actual = new Uint8Array(frame.pixels);
  const expected = new Uint8ClampedArray(actual.byteLength);
  const bounds = quadPixelBounds();
  const shaded = ASYMMETRIC_RGB.map((rgb) =>
    rgb.map((encoded, channel) =>
      linearToSrgb8(
        srgb8ToLinear(encoded) *
          (QUAD_VERTEX_COLOR[channel] ?? 0) *
          (QUAD_MATERIAL[channel] ?? 0),
      ),
    ),
  );
  for (let y = 0; y < frame.height; y += 1) {
    for (let x = 0; x < frame.width; x += 1) {
      const offset = (y * frame.width + x) * 4;
      const actualRgb = [
        actual[offset] ?? 0,
        actual[offset + 1] ?? 0,
        actual[offset + 2] ?? 0,
      ] as const;
      let rgb: readonly number[] = BACKGROUND_RGB;
      if (maximumDifference(actualRgb, BACKGROUND_RGB) > 8) {
        const u = ((x - bounds.left) / bounds.width) * QUAD_UV_EXTENT;
        const v = ((y - bounds.top) / bounds.height) * QUAD_UV_EXTENT;
        const sourceIndex =
          modulo(Math.floor(v * 2), 2) * 3 + modulo(Math.floor(u * 3), 3);
        rgb = shaded[sourceIndex] ?? BACKGROUND_RGB;
      }
      expected[offset] = rgb[0] ?? 0;
      expected[offset + 1] = rgb[1] ?? 0;
      expected[offset + 2] = rgb[2] ?? 0;
      expected[offset + 3] = 255;
    }
  }
  return expected;
}

function modulo(value: number, divisor: number): number {
  return ((value % divisor) + divisor) % divisor;
}

function srgb8ToLinear(value: number): number {
  const encoded = value / 255;
  return encoded <= 0.04045
    ? encoded / 12.92
    : ((encoded + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb8(value: number): number {
  const encoded =
    value <= 0.003_130_8 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, encoded)) * 255);
}

async function compositorBarrier(): Promise<void> {
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
}

async function frameDataUrl(frame: FramePixels): Promise<string> {
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
  if (!context)
    throw new Error("2D canvas is unavailable for visual artifacts");
  context.putImageData(new ImageData(pixels, width, height), 0, 0);
  const blob = await canvas.convertToBlob({ type: "image/png" });
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}
