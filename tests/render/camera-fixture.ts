import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  CameraMotion,
  FrameCapture,
  PickingWorldClient,
  StateOverlayRef,
} from "@ipp/client";
import {
  CAMERA_VIEWPORT,
  createPickingRing,
  PICKING_RING,
  componentFields,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import {
  CameraFixture,
  compoundPicking,
} from "../integration/scenarios/cameras-and-picking.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

interface State {
  readonly fixture: CameraFixture;
  readonly captures: Map<string, FrameCapture>;
  front: bigint;
  shifted: bigint;
  target: bigint;
  owner: StateOverlayRef | undefined;
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
  const client: PickingWorldClient = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10_000 },
  );
  try {
    if (!client.presentation || !client.capabilities.picking) {
      throw new Error("Camera render fixture requires WebGL and picking");
    }
    const record = (
      globalThis as unknown as {
        recordCamera(kind: string, value: unknown): Promise<void>;
      }
    ).recordCamera;
    active = {
      fixture: new CameraFixture(
        client,
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
      front: 0n,
      shifted: 0n,
      target: 0n,
      owner: undefined,
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
    selection: await current.fixture.activate(current.front),
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
  const presentation = current.fixture.client.presentation!;
  presentation.loseContext();
  presentation.restoreContext();
  const deadline = performance.now() + 10_000;
  for (;;) {
    const captured = await capture("gpu-recovered");
    if (
      captured.contextGeneration > before.contextGeneration &&
      captured.drawCalls === 2
    ) {
      return observeGpuFailure();
    }
    if (performance.now() >= deadline) {
      throw new Error(
        `GPU recovery did not restore both meshes: ${JSON.stringify(captured.backend)}`,
      );
    }
    await current.fixture.client.waitForFrame(captured.tick);
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
  return { target: current.target, noCamera: await current.fixture.pick() };
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

export async function resizeTinyCamera() {
  const current = state();
  current.fixture.client.presentation!.resize(1, 2048);
  return {
    selection: await current.fixture.activate(current.front, {
      width: 1,
      height: 2048,
    }),
    pick: await current.fixture.pick(0.5, 0.5, { width: 1, height: 2048 }),
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
    selection: await current.fixture.activate(current.front, {
      width: 1,
      height: 2048,
    }),
    pick: await current.fixture.pick(0.5, 0.5, { width: 1, height: 2048 }),
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

export async function navigate(motion: CameraMotion, x = 0.5, y = 0.5) {
  const current = state();
  const seen = current.fixture.changes.length;
  current.fixture.navigate(motion);
  const result = await current.fixture.pick(x, y, CAMERA_VIEWPORT, true);
  if (current.fixture.changes.length !== seen) {
    throw new Error(
      "Entity navigation must not emit camera-system state changes",
    );
  }
  return result;
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
    mismatch: await current.fixture.pick(0.5, 0.5, { width: 321, height: 240 }),
  };
}

export async function overlayCamera() {
  const current = state();
  const client = current.fixture.client;
  const owner = { kind: "alias", alias: 10 } as const;
  const outcome = successfulBatch(
    await current.fixture.batch([
      { kind: "createStateOverlayOwner", alias: 10 },
      {
        kind: "attachEntityOverlayBinding",
        owner,
        alias: 11,
        symbolicId: "shifted-camera",
        mode: "bound",
      },
      {
        kind: "attachComponentStateOverlay",
        owner,
        binding: { kind: "alias", alias: 11 },
        alias: 12,
        component: client.components.Transform!.id,
        mode: "bound",
        fields: componentFields(client, "Transform", { x: 0 }),
      },
    ]),
  );
  const owned = outcome.stateOverlays.find((resource) => resource.alias === 10);
  if (!owned) throw new Error("Camera overlay omitted owner handle");
  current.owner = { kind: "handle", id: owned.id };
  const inspection = await current.fixture.inspect();
  const camera = inspection.entities.find(
    (entity) => entity.id === current.shifted,
  );
  const transform = (layer: "base" | "effective") =>
    camera?.[layer].find(
      (component) => component.component === client.components.Transform!.id,
    )?.fields;
  return {
    base: transform("base"),
    effective: transform("effective"),
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

export async function releaseCameraOverlay() {
  const current = state();
  if (!current.owner) throw new Error("Camera overlay owner missing");
  successfulBatch(
    await current.fixture.batch([
      { kind: "releaseStateOverlayOwner", owner: current.owner },
      ...current.fixture.set(current.shifted, "Camera", { projection: 1 }),
    ]),
  );
  current.owner = undefined;
  return current.fixture.pick();
}

export async function capture(label: string) {
  const current = state();
  const inspection = await current.fixture.inspect();
  const frame = await current.fixture.client.presentation!.capture(
    inspection.tick,
  );
  if (
    frame.session !== current.fixture.client.session ||
    frame.tick <= inspection.tick
  ) {
    throw new Error(
      "Capture must identify a completed frame after the observed scene",
    );
  }
  current.captures.set(label, frame);
  return { ...captureMetadata(label), summary: summarizeImage(frame) };
}

export function captureMetadata(label: string) {
  const { pixels: _pixels, ...metadata } = frame(label);
  return metadata;
}

export function captureDataUrl(label: string) {
  return dataUrl(frame(label));
}

export function difference(first: string, second: string) {
  return compareImages(frame(first), frame(second));
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
  return dataUrl({ ...a, pixels: output.buffer });
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

function dataUrl(frame: Pick<FrameCapture, "width" | "height" | "pixels">) {
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
