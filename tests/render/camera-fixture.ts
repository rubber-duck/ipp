import { renderDiagnostics } from "../../packages/ipp-client/src/diagnostics.js";
import {
  outputProducer,
  sameOutputReference,
} from "../../packages/ipp-client/src/references.js";
import { presentationTesting } from "../../packages/ipp-client/src/testing.js";
import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  CameraViewMotion,
  PickingWorldClient,
  PresentationView,
  PresentedCapture,
  RootBinding,
} from "@ipp/client";
import {
  CAMERA_VIEWPORT,
  createPickingRing,
  PICKING_RING,
  successfulBatch,
  type HostedWorldClient,
} from "../integration/camera-fixtures.js";
import {
  CameraFixture,
  compoundPicking,
} from "../integration/scenarios/cameras-and-picking.js";
import { compareImages, summarizeImage } from "./image-assertions.js";
import { capturedImage, captureSummary } from "./root-presentation.js";
import { SCENE, selectSystems } from "../integration/system-selections.js";

interface State {
  readonly fixture: CameraFixture;
  readonly captures: Map<string, PresentedCapture>;
  view: PresentationView | undefined;
  front: bigint;
  shifted: bigint;
  target: bigint;
  /** The shifted camera's x before `moveCamera`, written back on restore. */
  shiftedX: number | undefined;
}

let active: State | undefined;

export async function initialize(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  await close();
  const canvas = document.createElement("canvas");
  canvas.id = "camera-canvas";
  canvas.width = CAMERA_VIEWPORT.width;
  canvas.height = CAMERA_VIEWPORT.height;
  document.body.replaceChildren(canvas);
  const contract = await import(configuration.generatedModuleUrl);
  const client: HostedWorldClient<PickingWorldClient> =
    await contract.IppClient.connectWorker(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      {
        selectedSystems: selectSystems(SCENE),
        canvas: canvas.transferControlToOffscreen(),
        timeoutMs: 10_000,
      },
    );
  try {
    if (!renderDiagnostics(client.host)) {
      throw new Error("Camera render fixture requires WebGL diagnostics");
    }
    const record = (
      globalThis as unknown as {
        recordCamera(kind: string, value: unknown): Promise<void>;
      }
    ).recordCamera;
    active = {
      fixture: new CameraFixture(
        client,
        client.host,
        (kind, value) =>
          record(
            kind,
            JSON.parse(
              JSON.stringify(value, (_key, item: unknown) =>
                typeof item === "bigint" ? { $bigint: item.toString() } : item,
              ),
            ),
          ),
        contract.encodeBoundingShape,
      ),
      captures: new Map(),
      view: undefined,
      front: 0n,
      shifted: 0n,
      target: 0n,
      shiftedX: undefined,
    };
    return { schemaHash: client.schemaHash, session: client.session };
  } catch (error) {
    await client.close();
    canvas.remove();
    throw error;
  }
}

export async function cpuMeshScenario() {
  return compoundPicking(state().fixture);
}

export async function declarePendingMesh(source: string) {
  const current = state();
  current.target = await current.fixture.target(
    "http-ring-target",
    {},
    { source },
  );
  return current.target;
}

export async function observePendingMesh(source: string) {
  const fixture = state().fixture;
  const inspection = await fixture.inspect();
  return {
    resource: inspection.resources.find(
      (resource) => resource.source === source,
    ),
    query: await fixture.pick(),
  };
}

export async function completePendingMesh(source: string) {
  const fixture = state().fixture;
  const resource = await fixture.waitForGeometry(source);
  return {
    resource,
    hole: await fixture.pick(),
    rim: await fixture.pick(0.5 + 0.7 / ((4 * 320) / 240), 0.5),
  };
}

export async function prepareGpuFailureScene() {
  const current = state();
  current.front = await current.fixture.camera("gpu-failure-camera");
  const selection = await current.fixture.activate(current.front);
  await current.fixture.create("unaffected-ready-cube", {
    Transform: { x: -0.9, sx: 0.35, sy: 0.35, sz: 0.35 },
    MeshInstance: {
      source: "ipp://mesh/cube?width=2&height=2&length=2",
      variant: 0,
    },
    UnlitMaterial: { r: 0.2, g: 0.8, b: 0.3 },
  });
  await current.fixture.waitForMesh(
    "ipp://mesh/cube?width=2&height=2&length=2",
  );
  return selection;
}

export async function failGpuMesh() {
  const current = state();
  const outcome = clientAssetSource(current.fixture.client.session, 1, 700n);
  await current.fixture.client.registerAsset(outcome, createPickingRing());
  await current.fixture.record("gpu_failed_mesh_upload", outcome);
  current.target = await current.fixture.create("gpu-failed-ring", {
    Transform: { x: 0.9, sx: 0.7, sy: 0.7, sz: 0.7 },
    MeshInstance: {
      source: clientAssetSource(current.fixture.client.session, 1, 700n).source,
      variant: 0,
    },
    PickingGeometry: { geometry: current.fixture.encodeGeometry(PICKING_RING) },
    UnlitMaterial: { r: 0.9, g: 0.2, b: 0.1 },
  });
  await current.fixture.waitForMesh(
    clientAssetSource(current.fixture.client.session, 1, 700n).source,
    "failed",
  );
  return observeGpuFailure();
}

export async function observeGpuFailure() {
  const current = state();
  // Real host frames continue while the same failed draw remains demanded.
  for (let frame = 0; frame < 3; frame += 1) {
    await current.fixture.client.waitForFrame();
  }
  return {
    entity: current.target,
    // Keep the ready Camera binding selected while observing the unrelated failure.
    selection: await current.fixture.pick(),
    rim: await current.fixture.pick(
      0.5 + (0.9 + 0.7 * 0.7) / ((4 * 320) / 240),
      0.5,
    ),
    hole: await current.fixture.pick(0.5 + 0.9 / ((4 * 320) / 240), 0.5),
    inspection: await current.fixture.inspect(),
  };
}

export async function recoverGpuFailure() {
  const current = state();
  const before = frame("gpu-allocation-failed");
  const diagnostics = renderDiagnostics(current.fixture.host)!;
  presentationTesting(diagnostics).loseContext();
  presentationTesting(diagnostics).restoreContext();
  const deadline = performance.now() + 10_000;
  for (;;) {
    const surface = await current.fixture.host.presentation
      .surface()
      .catch((error: unknown) => {
        if (presentationFailure(error) === "unavailable") return undefined;
        throw error;
      });
    if (surface && surface.context > before.view.surface.context) {
      const captured = await capture("gpu-recovered", false);
      if (captured.drawCalls === 2) return observeGpuFailure();
      if (performance.now() >= deadline) {
        throw new Error(
          `GPU recovery did not restore both meshes: ${JSON.stringify({ drawCalls: captured.drawCalls, failedDrawCalls: captured.failedDrawCalls })}`,
        );
      }
    } else if (performance.now() >= deadline) {
      throw new Error("Graphics context was not restored");
    }
    await current.fixture.client.waitForFrame();
  }
}

export async function prepareVisibleScene() {
  const current = state();
  current.target = await current.fixture.create("visible-pick-target", {
    Transform: { sx: 0.5, sy: 0.5, sz: 0.5 },
    MeshInstance: {
      source: "ipp://mesh/cube?width=2&height=2&length=2",
      variant: 0,
    },
    UnlitMaterial: { r: 0.9, g: 0.15, b: 0.08 },
    PickingGeometry: {
      geometry: current.fixture.encodeGeometry({
        type: "box",
        min: [-1, -1, -1],
        max: [1, 1, 1],
      }),
    },
  });
  await current.fixture.waitForMesh(
    "ipp://mesh/cube?width=2&height=2&length=2",
  );
  const unselected = await current.fixture.camera("unselected-camera");
  return {
    target: current.target,
    rootBinding: await current.fixture.rootBinding(),
    noCamera: await current.fixture.pick(
      0.5,
      0.5,
      CAMERA_VIEWPORT,
      undefined,
      await current.fixture.output(unselected),
    ),
  };
}

export async function selectTinyCamera() {
  const current = state();
  current.front = await current.fixture.camera(
    "tiny-orthographic-camera",
    { z: 6 },
    { ortho_height: 1e-38 },
  );
  return {
    selection: await current.fixture.activate(current.front),
    pick: await current.fixture.pick(),
  };
}

const TALL_VIEWPORT = Object.freeze({
  width: 1,
  height: 2048,
  devicePixelRatio: 1,
});

export async function resizeTinyCamera() {
  const current = state();
  const selection = await current.fixture.activate(
    current.front,
    TALL_VIEWPORT,
  );
  return {
    selection,
    binding: rootSummary(await current.fixture.rootBinding()),
    pick: await current.fixture.pick(0.5, 0.5, TALL_VIEWPORT),
  };
}

export async function repairTinyCamera() {
  const current = state();
  successfulBatch(
    await current.fixture.batch(
      current.fixture.set(current.front, "Camera", { ortho_height: 4 }),
    ),
  );
  return {
    selection: await current.fixture.activate(current.front, TALL_VIEWPORT),
    binding: rootSummary(await current.fixture.rootBinding()),
    pick: await current.fixture.pick(0.5, 0.5, TALL_VIEWPORT),
  };
}

export async function selectFront() {
  const current = state();
  current.front = await current.fixture.camera("front-camera");
  current.shifted = await current.fixture.camera("shifted-camera", {
    x: 1.5,
    z: 6,
  });
  return {
    selection: await current.fixture.activate(current.front),
    pick: await current.fixture.pick(),
  };
}

export async function prepareNavigation(projection: number) {
  const current = state();
  if (current.front === 0n) {
    current.front = await current.fixture.camera("navigation-camera");
  }
  successfulBatch(
    await current.fixture.batch([
      ...current.fixture.set(current.front, "Transform", {
        x: 0,
        y: 0,
        z: 6,
        qx: 0,
        qy: 0,
        qz: 0,
        qw: 1,
      }),
      ...current.fixture.set(current.front, "Camera", {
        projection,
        focus_distance: 6,
        ortho_height: 4,
      }),
    ]),
  );
  return current.fixture.activate(current.front);
}

export async function navigate(motion: CameraViewMotion, x = 0.5, y = 0.5) {
  const current = state();
  await current.fixture.navigate(motion);
  return current.fixture.pick(x, y, CAMERA_VIEWPORT, true);
}

export async function selectShifted() {
  const current = state();
  const selection = await current.fixture.activate(current.shifted);
  const deletion = await current.fixture.batch([
    current.fixture.delete(current.front),
  ]);
  successfulBatch(deletion);
  return {
    selection,
    deletion,
    center: await current.fixture.pick(),
    projected: await current.fixture.pick(0.5 - 1.5 / ((4 * 320) / 240), 0.5),
    mismatch: await current.fixture.pick(0.5, 0.5, {
      ...CAMERA_VIEWPORT,
      width: 321,
    }),
  };
}

/** The shifted camera's current Transform x. */
async function shiftedCameraX(): Promise<number> {
  const current = state();
  const client = current.fixture.client;
  const inspection = await current.fixture.inspect();
  const x = inspection.entities
    .find((entity) => entity.id === current.shifted)
    ?.components.find(
      (component) => component.component === client.components.Transform!.id,
    )?.fields.x;
  if (typeof x !== "number") throw new Error("Shifted camera has no x");
  return x;
}

/** Move the shifted camera to x = 0 with a plain field write, keeping its
 * previous x to write back on restore. */
export async function moveCamera() {
  const current = state();
  const before = await shiftedCameraX();
  current.shiftedX = before;
  successfulBatch(
    await current.fixture.batch(
      current.fixture.set(current.shifted, "Transform", { x: 0 }),
    ),
  );
  return {
    before,
    after: await shiftedCameraX(),
    pick: await current.fixture.pick(),
  };
}

export async function perspectiveCamera() {
  const current = state();
  successfulBatch(
    await current.fixture.batch(
      current.fixture.set(current.shifted, "Camera", { projection: 0 }),
    ),
  );
  return current.fixture.pick();
}

/** Write the kept x and the orthographic projection back in one batch. */
export async function restoreCamera() {
  const current = state();
  const x = current.shiftedX;
  if (x === undefined) throw new Error("Camera was not moved");
  successfulBatch(
    await current.fixture.batch([
      ...current.fixture.set(current.shifted, "Transform", { x }),
      ...current.fixture.set(current.shifted, "Camera", { projection: 1 }),
    ]),
  );
  current.shiftedX = undefined;
  return current.fixture.pick();
}

/** Select the current root binding on the worker surface when it changed. */
async function selectedView() {
  const current = state();
  const binding = current.fixture.selected();
  if (
    current.view?.binding.generation.host !== binding.generation.host ||
    current.view.binding.generation.serial !== binding.generation.serial ||
    current.view.surface.context !==
      (await current.fixture.host.presentation.surface()).context
  ) {
    current.view = await current.fixture.host.presentation.select(
      await current.fixture.host.presentation.surface(),
      binding,
    );
  }
  return current.view;
}

/**
 * A completed draw after the observed scene. A draw that skips failed
 * resources completes, but never witnesses inclusion of its output, so such
 * scenes wait for a later draw sequence instead of the output's evaluation cut.
 */
async function completedCapture(included = true) {
  const current = state();
  const inspection = await current.fixture.inspect();
  const output = current.fixture.selected().output;
  const previous = [...current.captures.values()].at(-1);
  const frame = await current.fixture.host.presentation.capture(
    await selectedView(),
    included
      ? { afterOutputs: [output] }
      : previous
        ? { afterSequence: previous.sequence }
        : {},
  );
  const source = frame.sources.find((source) =>
    sameOutputReference(source.output, output),
  );
  if (included && (!source || source.tick <= inspection.tick)) {
    throw new Error(
      "Capture must identify a completed frame after the observed scene",
    );
  }
  return frame;
}

export async function capture(label: string, included = true) {
  const current = state();
  const frame = await completedCapture(included);
  current.captures.set(label, frame);
  return {
    ...captureMetadata(label),
    statistics: await renderDiagnostics(current.fixture.host)!.statistics(),
    summary: summarizeImage(capturedImage(frame)),
  };
}

/** An invalid selected camera has no successful draw to capture. */
export async function captureFailure() {
  const failure = await completedCapture().then(
    () => undefined,
    (error: unknown) => error,
  );
  const reason = presentationFailure(failure);
  if (reason === undefined)
    throw new Error(`Expected a presentation failure, got ${String(failure)}`);
  return { reason };
}

/** The generated client bundles its own PresentationError class. */
function presentationFailure(error: unknown) {
  return error instanceof Error &&
    error.name === "PresentationError" &&
    "reason" in error &&
    typeof error.reason === "string"
    ? error.reason
    : undefined;
}

export function captureMetadata(label: string) {
  return captureSummary(frame(label));
}

export function captureDataUrl(label: string) {
  return dataUrl(capturedImage(frame(label)));
}

export function difference(first: string, second: string) {
  return compareImages(
    capturedImage(frame(first)),
    capturedImage(frame(second)),
  );
}

export function differenceDataUrl(first: string, second: string) {
  const a = frame(first);
  const b = frame(second);
  const inputA = new Uint8Array(a.pixels);
  const inputB = new Uint8Array(b.pixels);
  const output = new Uint8Array(a.pixels.byteLength);
  for (let offset = 0; offset < output.length; offset += 4) {
    for (let channel = 0; channel < 3; channel += 1) {
      output[offset + channel] = Math.min(
        255,
        Math.abs(inputA[offset + channel]! - inputB[offset + channel]!) * 4,
      );
    }
    output[offset + 3] = 255;
  }
  return dataUrl({ ...capturedImage(a), pixels: output.buffer });
}

export async function close() {
  const current = active;
  active = undefined;
  try {
    await current?.fixture.client.close();
  } finally {
    document.querySelector("#camera-canvas")?.remove();
  }
}

function state() {
  if (!active) throw new Error("Camera fixture has not been initialized");
  return active;
}

function frame(label: string) {
  const capture = state().captures.get(label);
  if (!capture) throw new Error(`Missing camera capture ${label}`);
  return capture;
}

function rootSummary(binding: RootBinding | null) {
  return (
    binding && {
      entity: outputProducer(binding.output)?.entity,
      width: binding.viewport.width,
      height: binding.viewport.height,
    }
  );
}

function dataUrl(frame: ReturnType<typeof capturedImage>) {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D unavailable for evidence");
  context.putImageData(
    new ImageData(
      new Uint8ClampedArray(frame.pixels.slice(0)),
      frame.width,
      frame.height,
    ),
    0,
    0,
  );
  return canvas.toDataURL("image/png");
}
