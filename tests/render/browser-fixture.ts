import { activateFixtureCamera } from "../integration/camera-fixtures.js";
import * as React from "react";
import type { ReactNode } from "react";
import type { Client } from "@ipp/client";
import type {
  BatchOutcome,
  Command,
  EntityRef,
  Inspection,
  AssetResourceSnapshot,
} from "@ipp/client";
import type { ClientPresentation, FrameCapture } from "@ipp/client";
import {
  createRoot,
  Entity as SceneEntity,
  MeshInstance as SceneMeshInstance,
  type ReactWorldRoot,
  Transform as SceneTransform,
  UnlitMaterial as SceneUnlitMaterial,
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

export interface RenderRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly meshSource: string;
  readonly timeoutMs: number;
  readonly logLevel?: "trace" | "debug" | "info" | "warn" | "error" | "off";
  readonly providerScenario?: {
    readonly duplicateReference?: boolean;
    readonly unaffectedSource?: string;
    readonly waitForSelected?: boolean;
  };
}

interface ComponentCommands {
  readonly id: number;
  insert(
    entity: EntityRef,
    values?: Readonly<Record<string, unknown>>,
  ): Command;
  setR?(entity: EntityRef, value: number): Command;
  setG?(entity: EntityRef, value: number): Command;
  setB?(entity: EntityRef, value: number): Command;
  setQx?(entity: EntityRef, value: number): Command;
  setQy?(entity: EntityRef, value: number): Command;
  setQz?(entity: EntityRef, value: number): Command;
  setQw?(entity: EntityRef, value: number): Command;
  setSource?(entity: EntityRef, value: string): Command;
}

interface GeneratedModule {
  readonly IppClient: {
    connectWorker(
      workerUrl: string | URL,
      wasmUrl: string | URL,
      options: {
        readonly timeoutMs: number;
        readonly canvas: OffscreenCanvas;
        readonly logLevel?: RenderRuntimeConfiguration["logLevel"];
      },
    ): Promise<Client>;
  };
  readonly Entity: {
    create(
      alias: number,
      metadata?: {
        readonly symbolicId?: string;
        readonly classes?: readonly string[];
      },
    ): Command;
    alias(alias: number): EntityRef;
    handle(id: bigint): EntityRef;
    delete(entity: EntityRef): Command;
  };
  readonly Transform: ComponentCommands;
  readonly UnlitMaterial: ComponentCommands;
  readonly MeshInstance: ComponentCommands;
}

interface FixtureState {
  readonly contract: GeneratedModule;
  readonly client: Client;
  readonly presentation: ClientPresentation;
  readonly root: ReactWorldRoot;
  readonly entity: EntityRef;
  readonly meshSource: string;
  readonly captures: Map<string, FrameCapture>;
  readonly resourceEntities: EntityRef[];
  mutationBatches: number;
  ownedRoot: ReactWorldRoot | undefined;
  transform: Readonly<Record<string, number>> | undefined;
  material: Readonly<Record<string, number>> | undefined;
  diagnosticEntity: EntityRef | undefined;
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
}

export interface AssetSetupReport {
  readonly entity: bigint;
  readonly transformComponent: number;
  readonly resource: AssetResourceSnapshot;
  readonly mutationBatches: number;
}

export interface ResourceSceneObservation {
  readonly session: bigint;
  readonly mutationBatches: number;
  readonly entityIds: readonly string[];
  readonly resources: readonly AssetResourceSnapshot[];
}

let active: FixtureState | undefined;

export async function initializeCube(
  configuration: RenderRuntimeConfiguration,
): Promise<AssetSetupReport> {
  await closeCube();

  const canvas = document.createElement("canvas");
  canvas.id = "ipp-cube-canvas";
  canvas.width = VIEWPORT.width;
  canvas.height = VIEWPORT.height;
  canvas.style.width = `${VIEWPORT.width}px`;
  canvas.style.height = `${VIEWPORT.height}px`;
  canvas.style.display = "block";
  const canvasHost = document.querySelector(".stage") ?? document.body;
  canvasHost.replaceChildren(canvas);

  if (typeof canvas.transferControlToOffscreen !== "function") {
    canvas.remove();
    throw new Error("Chromium does not expose OffscreenCanvas transfer");
  }
  const offscreen = canvas.transferControlToOffscreen();
  let contract: GeneratedModule;
  let client: Client;
  try {
    contract = (await import(
      configuration.generatedModuleUrl
    )) as GeneratedModule;
    client = await contract.IppClient.connectWorker(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      {
        timeoutMs: configuration.timeoutMs,
        canvas: offscreen,
        ...(configuration.logLevel === undefined
          ? {}
          : { logLevel: configuration.logLevel }),
      },
    );
  } catch (error) {
    canvas.remove();
    throw error;
  }
  let root: ReactWorldRoot | undefined;
  try {
    if (!client.capabilities.spatial || !client.capabilities.stateOverlays) {
      throw new Error("render fixture requires scene and overlay capabilities");
    }
    const presentation = client.presentation;
    if (presentation === undefined) {
      throw new Error("render worker did not expose presentation");
    }
    presentation.resize(VIEWPORT.width, VIEWPORT.height);

    const scenario = configuration.providerScenario;
    const selectedTransform = scenario
      ? { x: 0.55, sx: 0.46, sy: 0.46, sz: 0.46 }
      : undefined;
    const operations: Command[] = [
      contract.Entity.create(1, {
        symbolicId: "cube",
        classes: ["cube-fixture"],
      }),
      contract.Transform.insert(contract.Entity.alias(1), selectedTransform),
      contract.UnlitMaterial.insert(contract.Entity.alias(1), {
        r: 0.72,
        g: 0.72,
        b: 0.72,
      }),
      contract.MeshInstance.insert(contract.Entity.alias(1), {
        source: configuration.meshSource,
        variant: 0,
      }),
    ];
    if (scenario?.duplicateReference) {
      operations.push(
        contract.Entity.create(2, {
          symbolicId: "cube-shared-source",
          classes: ["resource-fixture"],
        }),
        contract.Transform.insert(contract.Entity.alias(2), {
          x: 1.1,
          sx: 0.32,
          sy: 0.32,
          sz: 0.32,
        }),
        contract.UnlitMaterial.insert(contract.Entity.alias(2), {
          r: 0.25,
          g: 0.52,
          b: 0.95,
        }),
        contract.MeshInstance.insert(contract.Entity.alias(2), {
          source: configuration.meshSource,
          variant: 0,
        }),
      );
    }
    if (scenario?.unaffectedSource) {
      operations.push(
        contract.Entity.create(3, {
          symbolicId: "cube-unaffected",
          classes: ["resource-fixture"],
        }),
        contract.Transform.insert(contract.Entity.alias(3), {
          x: -0.7,
          sx: 0.5,
          sy: 0.5,
          sz: 0.5,
        }),
        contract.UnlitMaterial.insert(contract.Entity.alias(3), {
          r: 0.2,
          g: 0.9,
          b: 0.3,
        }),
        contract.MeshInstance.insert(contract.Entity.alias(3), {
          source: scenario.unaffectedSource,
          variant: 0,
        }),
      );
    }
    const created = await client.batch(operations);
    requireBatchSuccess(created, "create cube");
    const id = created.ok
      ? created.aliases.find(({ alias }) => alias === 1)?.id
      : undefined;
    if (id === undefined) throw new Error("cube create omitted alias 1");

    await activateFixtureCamera(client);
    root = createRoot(client);
    const handles = [1, 2, 3]
      .map((alias) =>
        created.ok
          ? created.aliases.find((entry) => entry.alias === alias)?.id
          : undefined,
      )
      .filter((value): value is bigint => value !== undefined)
      .map((handle) => contract.Entity.handle(handle));
    active = {
      contract,
      client,
      presentation,
      root,
      entity: contract.Entity.handle(id),
      meshSource: configuration.meshSource,
      captures: new Map(),
      resourceEntities: handles,
      mutationBatches: 1,
      ownedRoot: undefined,
      transform: undefined,
      material: undefined,
      diagnosticEntity: undefined,
    };
    if (scenario?.unaffectedSource) {
      await waitForResource(client, 1, scenario.unaffectedSource, "loaded");
    }
    const inspection =
      scenario?.waitForSelected === false
        ? await client.inspect()
        : await waitForResource(client, 1, configuration.meshSource, "loaded");
    return {
      entity: id,
      transformComponent: contract.Transform.id,
      resource: requireResource(inspection, 1, configuration.meshSource),
      mutationBatches: 1,
    };
  } catch (error) {
    const cleanupFailures: unknown[] = [];
    if (root) {
      try {
        await root.unmount();
      } catch (cleanupError) {
        cleanupFailures.push(cleanupError);
      }
    }
    try {
      await client.close();
    } catch (cleanupError) {
      cleanupFailures.push(cleanupError);
    }
    canvas.remove();
    if (cleanupFailures.length > 0) {
      throw new AggregateError(
        [error, ...cleanupFailures],
        "cube fixture startup and cleanup failed",
      );
    }
    throw error;
  }
}

export async function createDiagnosticEntity(): Promise<bigint> {
  const state = requireActive();
  const outcome = await state.client.batch(
    [
      state.contract.Entity.create(90, {
        symbolicId: "diagnostic-probe",
        classes: ["diagnostics"],
      }),
    ],
    910n,
  );
  requireBatchSuccess(outcome, "create diagnostic entity");
  const id = outcome.ok
    ? outcome.aliases.find(({ alias }) => alias === 90)?.id
    : undefined;
  if (id === undefined) throw new Error("diagnostic create omitted alias 90");
  state.diagnosticEntity = state.contract.Entity.handle(id);
  return id;
}

export async function rejectDiagnosticEntityMutation(): Promise<BatchOutcome> {
  const state = requireActive();
  const entity = state.diagnosticEntity;
  if (entity === undefined) throw new Error("diagnostic entity is missing");
  const outcome = await state.client.batch(
    [
      state.contract.Entity.create(91, {
        symbolicId: "diagnostic-partial",
      }),
      state.contract.Entity.delete(entity),
      state.contract.Entity.delete(
        state.contract.Entity.handle(0xffff_ffff_ffff_ffffn),
      ),
    ],
    911n,
  );
  if (outcome.ok) throw new Error("invalid diagnostic batch completed");
  const inspection = await state.client.inspect();
  if (
    inspection.entities.some(
      ({ metadata }) => metadata.symbolicId === "diagnostic-probe",
    )
  )
    throw new Error("failed diagnostic batch lost its applied deletion");
  const partial = outcome.aliases.find(({ alias }) => alias === 91);
  if (!partial || !inspection.entities.some(({ id }) => id === partial.id))
    throw new Error("failed diagnostic batch omitted its created entity");
  state.diagnosticEntity = state.contract.Entity.handle(partial.id);
  return outcome;
}

export async function observeDiagnosticIdleFrames(
  count: number,
): Promise<bigint[]> {
  const state = requireActive();
  if (!Number.isInteger(count) || count < 1 || count > 8) {
    throw new RangeError("diagnostic idle frame count must be in [1, 8]");
  }
  const ticks: bigint[] = [];
  let afterTick: bigint | undefined;
  for (let index = 0; index < count; index += 1) {
    const frame = await state.client.waitForFrame(afterTick);
    ticks.push(frame.tick);
    afterTick = frame.tick;
  }
  return ticks;
}

export async function deleteDiagnosticEntity(): Promise<void> {
  const state = requireActive();
  const entity = state.diagnosticEntity;
  if (entity === undefined) throw new Error("diagnostic entity is missing");
  const outcome = await state.client.batch(
    [state.contract.Entity.delete(entity)],
    912n,
  );
  requireBatchSuccess(outcome, "delete diagnostic entity");
  state.diagnosticEntity = undefined;
}

export async function showMaterialOverride(): Promise<void> {
  const state = requireActive();
  state.material = { r: 0.16, g: 0.95, b: 0.28 };
  await renderState(state);
}

export async function updateHiddenProducerColor(): Promise<Inspection> {
  const state = requireActive();
  const { UnlitMaterial } = state.contract;
  if (!UnlitMaterial.setR || !UnlitMaterial.setG || !UnlitMaterial.setB) {
    throw new Error("generated material field setters are missing");
  }
  const outcome = await state.client.batch([
    UnlitMaterial.setR(state.entity, 0.95),
    UnlitMaterial.setG(state.entity, 0.18),
    UnlitMaterial.setB(state.entity, 0.12),
  ]);
  requireBatchSuccess(outcome, "update hidden producer material");
  return await state.client.inspect();
}

export async function removeProducerMaterial(): Promise<Inspection> {
  const state = requireActive();
  const outcome = await state.client.batch([
    {
      kind: "removeComponent",
      entity: state.entity,
      component: state.contract.UnlitMaterial.id,
    },
  ]);
  requireBatchSuccess(outcome, "remove producer material under Auto overlay");
  return await state.client.inspect();
}

export async function reinsertProducerMaterial(): Promise<Inspection> {
  const state = requireActive();
  const outcome = await state.client.batch([
    state.contract.UnlitMaterial.insert(state.entity, {
      r: 0.88,
      g: 0.12,
      b: 0.06,
    }),
  ]);
  requireBatchSuccess(outcome, "reinsert producer material under Auto overlay");
  return await state.client.inspect();
}

export async function clearMaterialOverride(): Promise<void> {
  const state = requireActive();
  state.material = {};
  await renderState(state);
}

export async function unmountReactScene(): Promise<void> {
  const state = requireActive();
  await state.root.render(null);
  await state.root.flush();
  state.material = undefined;
  state.transform = undefined;
}

export async function moveAndScaleCube(): Promise<void> {
  const state = requireActive();
  state.material = undefined;
  state.transform = { x: 1.15, y: 0, z: 0, sx: 0.55, sy: 0.55, sz: 0.55 };
  await renderState(state);
}

export async function setReactTransform(
  transform?: Readonly<Record<string, number>>,
): Promise<Inspection> {
  const state = requireActive();
  state.transform = transform;
  await renderState(state);
  return await state.client.inspect();
}

export async function attemptReactTransform(
  transform: Readonly<Record<string, number>>,
): Promise<{ readonly message: string; readonly inspection: Inspection }> {
  const state = requireActive();
  state.transform = transform;
  try {
    await renderState(state);
  } catch (error) {
    return {
      message: error instanceof Error ? error.message : String(error),
      inspection: await state.client.inspect(),
    };
  }
  throw new Error("invalid React transform unexpectedly succeeded");
}

export async function updateProducerRotation(rotation: {
  readonly qx: number;
  readonly qy: number;
  readonly qz: number;
  readonly qw: number;
}): Promise<Inspection> {
  const state = requireActive();
  const transform = state.contract.Transform;
  if (
    !transform.setQx ||
    !transform.setQy ||
    !transform.setQz ||
    !transform.setQw
  ) {
    throw new Error("generated Transform quaternion setters are missing");
  }
  requireBatchSuccess(
    await state.client.batch([
      transform.setQx(state.entity, rotation.qx),
      transform.setQy(state.entity, rotation.qy),
      transform.setQz(state.entity, rotation.qz),
      transform.setQw(state.entity, rotation.qw),
    ]),
    "update producer rotation",
  );
  return await state.client.inspect();
}

export async function waitForSelectedResource(
  status: AssetResourceSnapshot["status"],
): Promise<ResourceSceneObservation> {
  const state = requireActive();
  await waitForResource(state.client, 1, state.meshSource, status);
  return await inspectResourceScene();
}

export async function inspectResourceScene(): Promise<ResourceSceneObservation> {
  const state = requireActive();
  const inspection = await state.client.inspect();
  return {
    session: state.client.session,
    mutationBatches: state.mutationBatches,
    entityIds: inspection.entities
      .map(({ metadata }) => metadata.symbolicId)
      .filter((value): value is string => value !== null),
    resources: inspection.resources,
  };
}

export async function changeSharedResourceSource(
  source: string,
): Promise<ResourceSceneObservation> {
  const state = requireActive();
  const entity = state.resourceEntities[1];
  if (!entity) throw new Error("shared resource entity is missing");
  const setSource = state.contract.MeshInstance.setSource;
  if (!setSource)
    throw new Error("generated MeshInstance.setSource is missing");
  requireBatchSuccess(
    await state.client.batch([setSource(entity, source)]),
    "change shared resource source",
  );
  state.mutationBatches += 1;
  return await inspectResourceScene();
}

export async function removePrimaryResourceEntity(): Promise<ResourceSceneObservation> {
  const state = requireActive();
  const entity = state.resourceEntities[0];
  if (!entity) throw new Error("primary resource entity is missing");
  requireBatchSuccess(
    await state.client.batch([state.contract.Entity.delete(entity)]),
    "remove pending resource entity",
  );
  state.mutationBatches += 1;
  state.resourceEntities.splice(0, 1);
  return await inspectResourceScene();
}

export async function recoverContext(): Promise<{
  readonly beforeGeneration: number;
  readonly afterGeneration: number;
}> {
  const state = requireActive();
  const before = requireCapture(state, "before-context-loss");
  state.presentation.loseContext();
  await compositorBarrier();
  const resources = (await state.client.inspect()).resources;
  state.presentation.restoreContext();
  for (const resource of resources) {
    await waitForResource(
      state.client,
      resource.kind,
      resource.source,
      "loaded",
    );
  }
  const after = await captureCube("after-context-restore");
  if (after.contextGeneration <= before.contextGeneration) {
    throw new Error("context generation did not advance after restoration");
  }
  return {
    beforeGeneration: before.contextGeneration,
    afterGeneration: after.contextGeneration,
  };
}

export async function deleteOwnedCubeInFailedBatch(): Promise<void> {
  const state = requireActive();
  const outcome = await state.client.batch([
    state.contract.Entity.delete(state.entity),
    state.contract.Entity.delete(
      state.contract.Entity.handle(0xffff_ffff_ffff_ffffn),
    ),
  ]);
  if (outcome.ok || outcome.error.operation !== 1)
    throw new Error("expected failure after applied cube deletion");
}

export async function mountReactOwnedCube(): Promise<void> {
  const state = requireActive();
  if (state.ownedRoot) throw new Error("React-owned cube is already mounted");
  const root = createRoot(state.client);
  state.ownedRoot = root;
  await root.render(
    React.createElement(
      SceneEntity,
      { id: "react-owned-cube" },
      React.createElement(SceneTransform, { key: "transform", bound: false }),
      React.createElement(SceneUnlitMaterial, {
        key: "material",
        bound: false,
        r: 0.72,
        g: 0.72,
        b: 0.72,
      }),
      React.createElement(SceneMeshInstance, {
        key: "mesh",
        bound: false,
        source: state.meshSource,
      }),
    ),
  );
  await root.flush();
  await waitForResource(state.client, 1, state.meshSource, "loaded");
}

export async function unmountReactOwnedCube(): Promise<{
  readonly entityExists: boolean;
}> {
  const state = requireActive();
  const root = state.ownedRoot;
  if (!root) throw new Error("React-owned cube is not mounted");
  state.ownedRoot = undefined;
  await root.unmount();
  const inspection = await state.client.inspect();
  return {
    entityExists: inspection.entities.some(
      ({ metadata }) => metadata.symbolicId === "react-owned-cube",
    ),
  };
}

export async function captureCube(label: string): Promise<CaptureReport> {
  const state = requireActive();
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
  state.captures.set(label, {
    ...frame,
    pixels: frame.pixels.slice(0),
  });
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
  };
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

export function sampleCaptured(
  label: string,
  x: number,
  y: number,
): readonly [number, number, number, number] {
  const frame = requireCapture(requireActive(), label);
  if (
    !Number.isInteger(x) ||
    !Number.isInteger(y) ||
    x < 0 ||
    y < 0 ||
    x >= frame.width ||
    y >= frame.height
  ) {
    throw new RangeError("sample coordinates are outside the capture");
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
    pixels[offset] = Math.min(
      255,
      Math.abs((a[offset] ?? 0) - (b[offset] ?? 0)) * 4,
    );
    pixels[offset + 1] = Math.min(
      255,
      Math.abs((a[offset + 1] ?? 0) - (b[offset + 1] ?? 0)) * 4,
    );
    pixels[offset + 2] = Math.min(
      255,
      Math.abs((a[offset + 2] ?? 0) - (b[offset + 2] ?? 0)) * 4,
    );
    pixels[offset + 3] = 255;
  }
  return await rgbaDataUrl(first.width, first.height, pixels);
}

export async function backgroundDataUrl(): Promise<string> {
  const pixels = new Uint8ClampedArray(VIEWPORT.width * VIEWPORT.height * 4);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    pixels.set([...BACKGROUND_RGB, 255], offset);
  }
  return await rgbaDataUrl(VIEWPORT.width, VIEWPORT.height, pixels);
}

export async function differenceFromBackgroundDataUrl(
  label: string,
): Promise<string> {
  const frame = requireCapture(requireActive(), label);
  const pixels = new Uint8ClampedArray(frame.width * frame.height * 4);
  const source = new Uint8Array(frame.pixels);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    pixels[offset] = Math.min(
      255,
      Math.abs((source[offset] ?? 0) - BACKGROUND_RGB[0]) * 4,
    );
    pixels[offset + 1] = Math.min(
      255,
      Math.abs((source[offset + 1] ?? 0) - BACKGROUND_RGB[1]) * 4,
    );
    pixels[offset + 2] = Math.min(
      255,
      Math.abs((source[offset + 2] ?? 0) - BACKGROUND_RGB[2]) * 4,
    );
    pixels[offset + 3] = 255;
  }
  return await rgbaDataUrl(frame.width, frame.height, pixels);
}

export async function closeCube(): Promise<void> {
  const state = active;
  active = undefined;
  if (!state) return;
  const failures: unknown[] = [];
  if (state.ownedRoot) {
    try {
      await state.ownedRoot.unmount();
    } catch (error) {
      failures.push(error);
    }
  }
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
  document.querySelector("#ipp-cube-canvas")?.remove();
  if (failures.length > 0) {
    throw new AggregateError(failures, "cube fixture cleanup failed");
  }
}

async function renderState(state: FixtureState): Promise<void> {
  const children: ReactNode[] = [];
  if (state.transform !== undefined) {
    children.push(
      React.createElement(SceneTransform, {
        key: "transform",
        ...state.transform,
      }),
    );
  }
  if (state.material !== undefined) {
    children.push(
      React.createElement(SceneUnlitMaterial, {
        key: "material",
        ...state.material,
      }),
    );
  }
  await state.root.render(
    React.createElement(SceneEntity, { bindTo: "cube" }, ...children),
  );
  await state.root.flush();
}

function requireActive(): FixtureState {
  if (!active) throw new Error("cube fixture is not initialized");
  return active;
}

function requireCapture(state: FixtureState, label: string): FrameCapture {
  const frame = state.captures.get(label);
  if (!frame) throw new Error(`missing captured frame '${label}'`);
  return frame;
}

function requireBatchSuccess(outcome: BatchOutcome, label: string): void {
  if (!outcome.ok) {
    throw new Error(`${label} failed: ${outcome.error.reason}`);
  }
}

async function waitForResource(
  client: Client,
  kind: AssetResourceSnapshot["kind"],
  source: string,
  status: AssetResourceSnapshot["status"],
): Promise<Inspection> {
  const deadline = performance.now() + 10_000;
  while (true) {
    const inspection = await client.inspect();
    const resource = inspection.resources.find(
      (candidate) => candidate.kind === kind && candidate.source === source,
    );
    if (resource?.status === status) return inspection;
    if (resource?.status === "failed" && status !== "failed") {
      throw new Error(
        `${kind} source ${source} failed: ${resource.error ?? "unknown error"}`,
      );
    }
    if (performance.now() >= deadline) {
      throw new Error(`Timed out waiting for ${kind} source ${source}`);
    }
    await compositorBarrier();
  }
}

function requireResource(
  inspection: Inspection,
  kind: AssetResourceSnapshot["kind"],
  source: string,
): AssetResourceSnapshot {
  const resource = inspection.resources.find(
    (candidate) => candidate.kind === kind && candidate.source === source,
  );
  if (!resource) throw new Error(`missing ${kind} resource ${source}`);
  return resource;
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
