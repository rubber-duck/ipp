import type { RenderWorldClient, Command, FrameCapture } from "@ipp/client";
import {
  activateFixtureCamera,
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

let client: RenderWorldClient | undefined;
const entities = new Map<string, bigint>();
const captures = new Map<string, FrameCapture>();
const ambientEvents: [number, number, number][] = [];

export async function initialize(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  await close();
  const canvas = document.createElement("canvas");
  canvas.id = "lighting-canvas";
  canvas.width = 480;
  canvas.height = 360;
  document.body.replaceChildren(canvas);
  const contract = await import(configuration.generatedModuleUrl);
  client = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10000 },
  );
  const current = active();
  current.onRenderStateUpdated((event) => {
    if (event.changes.ambientLight)
      ambientEvents.push(event.changes.ambientLight);
  });
  if (!current.capabilities.pbr || !current.capabilities.shadows)
    throw new Error("Lighting fixture requires PBR and shadows");
  const camera = await activateFixtureCamera(current);
  entities.set("camera", camera);
  await update("camera", "Transform", { x: 4, y: 5, z: 7, ...aim(4, 5, 7) });
  await update("camera", "Camera", { projection: 1, ortho_height: 6 });
  const scene = [
    {
      name: "floor",
      transform: { y: -0.08 },
      mesh: "ipp://mesh/cube?width=7&height=0.16&length=6",
      pbr: { r: 0.65, g: 0.65, b: 0.65, roughness: 0.9, cast_shadows: false },
    },
    {
      name: "cube",
      transform: { x: -0.45, y: 0.65 },
      mesh: "ipp://mesh/cube?width=1.3&height=1.3&length=1.3",
      pbr: { r: 0.6, g: 0.3, b: 0.15, roughness: 0.6 },
    },
    {
      name: "sentinel",
      transform: { x: -2.5, y: 1.3, z: 1.5 },
      mesh: "ipp://mesh/cube?width=0.45&height=0.45&length=0.45",
      unlit: { r: 0.12, g: 0.7, b: 0.2 },
    },
    {
      name: "spot",
      transform: { x: -2, y: 4, z: 2, ...aim(-2, 4, 2) },
      light: {
        kind: 2,
        intensity: 65,
        range: 15,
        inner_cone: 0.45,
        outer_cone: 0.85,
        cast_shadows: true,
      },
    },
    {
      name: "fill",
      transform: { ...aim(2, 4, -2) },
      light: { kind: 0, intensity: 0.25 },
    },
  ];
  const operations: Command[] = [];
  for (const [index, object] of scene.entries()) {
    const entity = { kind: "alias", alias: index + 1 } as const;
    operations.push(
      createEntity(entity.alias, object.name),
      insertComponent(current, "Transform", entity, object.transform),
    );
    if (object.mesh)
      operations.push(
        insertComponent(current, "MeshInstance", entity, {
          source: object.mesh,
        }),
      );
    if (object.pbr)
      operations.push(
        insertComponent(current, "PbrMaterial", entity, object.pbr),
      );
    if (object.unlit)
      operations.push(
        insertComponent(current, "UnlitMaterial", entity, object.unlit),
      );
    if (object.light)
      operations.push(insertComponent(current, "Light", entity, object.light));
  }
  const outcome = await current.batch(operations);
  for (const [index, object] of scene.entries())
    entities.set(object.name, aliasId(outcome, index + 1));
  return current.capabilities;
}

export async function update(
  name: string,
  component: string,
  values: Record<string, number | boolean | string>,
) {
  const current = active();
  const id = entities.get(name);
  if (id === undefined) throw new Error(`Missing fixture entity ${name}`);
  return successfulBatch(
    await current.batch(
      componentFields(current, component, values).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id },
        component: current.components[component]!.id,
        field,
      })),
    ),
  );
}

export async function moveLight(x: number) {
  await update("spot", "Transform", { x, ...aim(x, 4, 2) });
}

export async function failedLightUpdate() {
  const current = active();
  const before = await current.inspect();
  const outcome = await current.batch([
    createEntity(500, "partial-light-update"),
    ...componentFields(current, "Light", { intensity: 0 }).map((field) => ({
      kind: "setField" as const,
      entity: { kind: "handle" as const, id: entities.get("spot")! },
      component: current.components.Light!.id,
      field,
    })),
    { kind: "delete", entity: { kind: "alias", alias: 501 } },
  ]);
  const after = await current.inspect();
  return { outcome, before: before.entities, after: after.entities };
}

export async function correctLightUpdate() {
  const current = active();
  const partial = (await current.inspect()).entities.find(
    (entity) => entity.metadata.symbolicId === "partial-light-update",
  );
  if (!partial) throw new Error("Missing partially created entity");
  successfulBatch(
    await current.batch([
      { kind: "delete", entity: { kind: "handle", id: partial.id } },
    ]),
  );
  await update("spot", "Light", { intensity: 65 });
}

export async function capture(label: string) {
  const current = active();
  const deadline = performance.now() + 15000;
  for (;;) {
    const inspection = await current.inspect();
    if (inspection.resources.some((resource) => resource.status === "failed"))
      throw new Error("Lighting asset failed");
    if (
      inspection.resources.length >= 3 &&
      inspection.resources.every((resource) => resource.status === "loaded")
    ) {
      const frame = await current.presentation!.capture(inspection.tick);
      if (frame.tick <= inspection.tick)
        throw new Error("Capture did not follow scene state");
      captures.set(label, frame);
      return { ...captureMetadata(label), summary: summarizeImage(frame) };
    }
    if (performance.now() > deadline)
      throw new Error("Lighting assets did not become ready");
    await current.waitForFrame(inspection.tick);
  }
}

export async function automaticBounds() {
  const current = active();
  const bounds = current.components.BoundingGeometry!.id;
  const observe = async () =>
    (await current.inspect()).entities
      .filter((entity) =>
        ["floor", "cube", "sentinel"].includes(
          entity.metadata.symbolicId ?? "",
        ),
      )
      .map((entity) => ({
        name: entity.metadata.symbolicId,
        effective: entity.effective.some((value) => value.component === bounds),
        authored: entity.base.some((value) => value.component === bounds),
      }));
  const before = await observe();
  const entity = { kind: "handle", id: entities.get("cube")! } as const;
  successfulBatch(
    await current.batch([
      insertComponent(current, "BoundingGeometry", entity, {}),
    ]),
  );
  const authored = await observe();
  successfulBatch(
    await current.batch([
      { kind: "removeComponent", entity, component: bounds },
    ]),
  );
  return { before, authored, restored: await observe() };
}

export function shadowDifference(lit: string, shadowed: string) {
  const a = new Uint8Array(frame(lit).pixels),
    b = new Uint8Array(frame(shadowed).pixels);
  let darkened = 0,
    brightened = 0,
    x = 0,
    y = 0,
    sentinel = 0,
    changedSentinel = 0;
  for (let offset = 0; offset < a.length; offset += 4) {
    const delta =
      (a[offset]! +
        a[offset + 1]! +
        a[offset + 2]! -
        b[offset]! -
        b[offset + 1]! -
        b[offset + 2]!) /
      3;
    if (delta > 12) {
      darkened++;
      x += (offset / 4) % frame(lit).width;
      y += Math.floor(offset / 4 / frame(lit).width);
    }
    if (delta < -4) brightened++;
    // Independent sRGB transfer of the unlit sentinel's linear material.
    if (
      Math.abs(a[offset]! - 97) < 3 &&
      Math.abs(a[offset + 1]! - 218) < 3 &&
      Math.abs(a[offset + 2]! - 124) < 3
    ) {
      sentinel++;
      if (
        [0, 1, 2].some((channel) => a[offset + channel] !== b[offset + channel])
      )
        changedSentinel++;
    }
  }
  return {
    darkened,
    brightened,
    centroid: [x / darkened, y / darkened],
    sentinel,
    changedSentinel,
  };
}

/** Count changed shadow-edge pixels; illumination/materials remain fixed. */
export function softShadowDifference(lit: string, hard: string, soft: string) {
  const a = new Uint8Array(frame(lit).pixels),
    b = new Uint8Array(frame(hard).pixels),
    c = new Uint8Array(frame(soft).pixels);
  let softened = 0,
    spread = 0,
    brightened = 0;
  for (let i = 0; i < a.length; i += 4) {
    const sum = (pixels: Uint8Array) =>
      pixels[i]! + pixels[i + 1]! + pixels[i + 2]!;
    const full = sum(a),
      hardValue = sum(b),
      softValue = sum(c);
    if (full - hardValue > 60 && softValue - hardValue > 18) softened++;
    if (Math.abs(full - hardValue) < 6 && full - softValue > 18) spread++;
    if (softValue - full > 12) brightened++;
  }
  return { softened, spread, brightened };
}

export function difference(a: string, b: string) {
  return compareImages(frame(a), frame(b));
}
export function captureMetadata(label: string) {
  const { pixels: _, ...metadata } = frame(label);
  return metadata;
}
export function captureDataUrl(label: string) {
  const value = frame(label),
    canvas = document.createElement("canvas");
  canvas.width = value.width;
  canvas.height = value.height;
  canvas
    .getContext("2d")!
    .putImageData(
      new ImageData(
        new Uint8ClampedArray(value.pixels.slice(0)),
        value.width,
        value.height,
      ),
      0,
      0,
    );
  return canvas.toDataURL("image/png");
}

export async function recoverContext() {
  active().presentation!.loseContext();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  active().presentation!.restoreContext();
}

export async function close() {
  const previous = client;
  client = undefined;
  try {
    await previous?.close();
  } finally {
    entities.clear();
    ambientEvents.length = 0;
    captures.clear();
    document.querySelector("#lighting-canvas")?.remove();
  }
}

function active() {
  if (!client) throw new Error("Lighting fixture disconnected");
  return client;
}
function frame(label: string) {
  const value = captures.get(label);
  if (!value) throw new Error(`No capture ${label}`);
  return value;
}
function aim(x: number, y: number, z: number) {
  const yaw = Math.atan2(x, z) / 2,
    pitch = -Math.atan2(y, Math.hypot(x, z)) / 2;
  return {
    qx: Math.sin(pitch) * Math.cos(yaw),
    qy: Math.cos(pitch) * Math.sin(yaw),
    qz: -Math.sin(pitch) * Math.sin(yaw),
    qw: Math.cos(pitch) * Math.cos(yaw),
  };
}

/** Compare normal streams on identical geometry through ordinary source loading. */
export async function normalMesh(source: string, scaled = false) {
  await update("cube", "MeshInstance", { source });
  await update("cube", "Transform", {
    x: 0,
    y: 1.25,
    z: 0,
    sx: scaled ? 1.6 : 1,
    sy: scaled ? 0.65 : 1,
    sz: scaled ? 1.1 : 1,
    qx: 0,
    qy: scaled ? Math.sin(0.3) : 0,
    qz: 0,
    qw: scaled ? Math.cos(0.3) : 1,
  });
  await update("cube", "PbrMaterial", { roughness: 0.2, metallic: 0.25 });
  await update("spot", "Light", { kind: 0, intensity: 2, cast_shadows: false });
  await update("fill", "Light", { intensity: 0.25 });
}

/** Texture declarations use ordinary generated commands and built-in provider I/O. */
export async function baseColorTexture(enabled: boolean) {
  const current = active();
  const entity = { kind: "handle", id: entities.get("cube")! } as const;
  successfulBatch(
    await current.batch([
      enabled
        ? insertComponent(current, "BaseColorTexture", entity, {
            source:
              "ipp://texture/checkerboard?width=64&height=64&cellsX=8&cellsY=8",
          })
        : {
            kind: "removeComponent",
            entity,
            component: current.components.BaseColorTexture!.id,
          },
    ]),
  );
}

export function ambientLight(color: [number, number, number]) {
  active().sendCommand({
    type: "RenderStateUpdateCommand",
    changes: { ambientLight: color },
  });
}

export function ambientChanges() {
  return ambientEvents;
}

/** A small foot-like caster under a distant soft spotlight, viewed from above. */
export async function contactShadowFixture() {
  await update("camera", "Transform", {
    x: 0,
    y: 10,
    z: 0,
    qx: -Math.SQRT1_2,
    qy: 0,
    qz: 0,
    qw: Math.SQRT1_2,
  });
  await update("camera", "Camera", { ortho_height: 2 });
  await update("spot", "Transform", { x: -8, y: 16, z: 8, ...aim(-8, 16, 8) });
  await update("spot", "Light", {
    intensity: 1000,
    range: 60,
    shadow_near: 0.1,
    shadow_bias: 0.000002,
    shadow_radius: 0.65,
    cast_shadows: true,
  });
  await update("floor", "PbrMaterial", { cast_shadows: true });
  await update("cube", "Transform", {
    x: 0,
    y: 0.15,
    z: 0,
    sx: 0.15 / 1.3,
    sy: 0.3 / 1.3,
    sz: 0.15 / 1.3,
  });
}

/** These ground points lie inside the caster's analytic umbra, outside its footprint.
 * Light rays to (-8,16,8) intersect its top [-.075,.075]^2 at height .3.
 * The orthographic camera maps +X right and +Z down, at height 2.
 */
export function contactShadowSamples(lit: string, shadowed: string) {
  const a = frame(lit),
    b = frame(shadowed);
  const before = new Uint8Array(a.pixels),
    after = new Uint8Array(b.pixels);
  return [0.11, 0.12, 0.13, 0.14].map((x) => {
    const px = Math.floor(a.width / 2 + (x * a.height) / 2);
    const py = Math.floor(a.height / 2 - (x * a.height) / 2);
    const offset = (py * a.width + px) * 4;
    return (
      [0, 1, 2].reduce(
        (sum, channel) =>
          sum + before[offset + channel]! - after[offset + channel]!,
        0,
      ) / 3
    );
  });
}

/** Four independent atlas tiles; each light sees the same caster from another side. */
export async function multipleShadowSources(count = 4) {
  const current = active();
  await update("spot", "Light", {
    intensity: 18,
    cast_shadows: true,
    shadow_radius: 0,
  });
  if (count === 8) {
    const outcome = await current.batch([
      { kind: "delete", entity: { kind: "handle", id: entities.get("fill")! } },
    ]);
    if (!outcome.ok) throw new Error("Cannot release fill-light slot");
    entities.delete("fill");
  }
  const result = ["spot"];
  const positions = [
    [2, 4, 2],
    [-2, 4, -2],
    [2, 4, -2],
    [0, 5, 3],
    [0, 5, -3],
    [-3, 5, 0],
    [3, 5, 0],
  ];
  for (const [i, [x, y, z]] of positions.slice(0, count - 1).entries()) {
    const name = `shadow-${i + 1}`;
    const entity = { kind: "alias" as const, alias: 90 };
    const outcome = await current.batch([
      createEntity(90, name),
      insertComponent(current, "Transform", entity, {
        x: x!,
        y: y!,
        z: z!,
        ...aim(x!, y!, z!),
      }),
      insertComponent(current, "Light", entity, {
        kind: 2,
        intensity: 18,
        range: 15,
        inner_cone: 0.45,
        outer_cone: 0.85,
        cast_shadows: true,
      }),
    ]);
    entities.set(name, aliasId(outcome, 90));
    result.push(name);
  }
  return result;
}

/** Sixteen competing lights illuminate two separated objects through per-draw selection. */
export async function separatedLightGroups() {
  const current = active();
  await update("camera", "Transform", {
    x: 0,
    y: 0.65,
    z: 10,
    qx: 0,
    qy: 0,
    qz: 0,
    qw: 1,
  });
  await update("camera", "Camera", { ortho_height: 6 });
  await update("cube", "Transform", { x: -2, y: 0.65, z: 0 });
  await update("cube", "PbrMaterial", {
    r: 0.7,
    g: 0.7,
    b: 0.7,
    cast_shadows: false,
  });
  await update("sentinel", "Transform", {
    x: 2,
    y: 0.65,
    z: 0,
    sx: 2,
    sy: 2,
    sz: 2,
  });
  successfulBatch(
    await current.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: entities.get("sentinel")! },
        component: current.components.UnlitMaterial!.id,
      },
      insertComponent(
        current,
        "PbrMaterial",
        { kind: "handle", id: entities.get("sentinel")! },
        { r: 0.7, g: 0.7, b: 0.7, cast_shadows: false },
      ),
    ]),
  );
  for (const name of ["spot", "fill"])
    if (entities.has(name)) await update(name, "Light", { intensity: 0 });
  const operations: Command[] = [];
  for (let i = 0; i < 16; i++) {
    const left = i < 8;
    const entity = { kind: "alias" as const, alias: i + 1 };
    operations.push(
      createEntity(i + 1, `selected-light-${i}`),
      insertComponent(current, "Transform", entity, {
        x: (left ? -2 : 2) + ((i % 4) - 1.5) * 0.05,
        y: 0.65,
        z: 1.2,
      }),
      insertComponent(current, "Light", entity, {
        kind: 1,
        intensity: 0.4,
        range: 2,
        r: left ? 1 : 0,
        g: 0,
        b: left ? 0 : 1,
        cast_shadows: false,
      }),
    );
  }
  successfulBatch(await current.batch(operations));
}

export function selectedLightColors(label: string) {
  const frame = captures.get(label)!;
  const pixels = new Uint8Array(frame.pixels);
  return [frame.width / 4, (frame.width * 3) / 4].map((center) => {
    const sums = [0, 0, 0];
    for (let y = frame.height / 2 - 5; y < frame.height / 2 + 5; y++)
      for (let x = center - 5; x < center + 5; x++)
        for (let channel = 0; channel < 3; channel++)
          sums[channel]! += pixels[(y * frame.width + x) * 4 + channel]!;
    return sums.map((value) => value / 100);
  });
}
