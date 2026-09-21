import type {
  GeometryEncoder,
  Command,
  ComponentDescriptor,
  ComponentFieldValue,
  EntityRef,
  FieldWrite,
  FrameCapture,
  RenderWorldClient,
  RenderStatePatch,
  RenderStateUpdatedEvent,
  AssetResourceSnapshot,
} from "@ipp/client";
import {
  activateFixtureCamera,
  successfulBatch,
} from "../integration/camera-fixtures.js";
import {
  BACKGROUND_RGB,
  compareImages,
  summarizeImage,
  VIEWPORT,
} from "./image-assertions.js";

export interface DebugDeclaration {
  name: string;
  geometry: Readonly<
    Record<string, number | boolean | Uint8Array<ArrayBuffer>>
  >;
  transform?: Readonly<Record<string, number>>;
}

interface State {
  client: RenderWorldClient;
  encodeGeometry: GeometryEncoder;
  captures: Map<string, FrameCapture>;
  entities: bigint[];
  resourceEvents: AssetResourceSnapshot[];
  updates: RenderStateUpdatedEvent[];
  unsubscribe: (() => void)[];
}

let active: State | undefined;

export async function initialize(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  await close();
  const canvas = document.createElement("canvas");
  canvas.id = "debug-geometry-canvas";
  canvas.width = VIEWPORT.width;
  canvas.height = VIEWPORT.height;
  document.body.replaceChildren(canvas);
  const contract = await import(configuration.generatedModuleUrl);
  const client: RenderWorldClient = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10_000 },
  );
  try {
    if (!client.presentation || !client.components.BoundingGeometry)
      throw new Error("Debug fixture requires a rendering scene contract");
    active = {
      client,
      encodeGeometry: contract.encodeBoundingShape,
      captures: new Map(),
      entities: [],
      resourceEvents: [],
      updates: [],
      unsubscribe: [],
    };
    const current = state();
    current.unsubscribe.push(
      client.onResourceChange((event) => current.resourceEvents.push(event)),
      client.onRenderStateUpdated((event) => current.updates.push(event)),
    );
    await activateFixtureCamera(client);
    return observe();
  } catch (error) {
    await close();
    throw error;
  }
}

/** Replace only fixture-owned entities; the camera retains its session lifetime. */
export async function replaceScene(declarations: readonly DebugDeclaration[]) {
  const current = state();
  const commands: Command[] = current.entities.map((id) => ({
    kind: "delete",
    entity: { kind: "handle", id },
  }));
  for (const [index, declaration] of declarations.entries()) {
    const alias = index + 1;
    const entity = { kind: "alias", alias } as const;
    commands.push(
      {
        kind: "create",
        alias,
        metadata: { symbolicId: declaration.name, classes: ["debug-fixture"] },
      },
      insert("Transform", entity, declaration.transform ?? {}),
      // Without a MeshInstance this creates no visual draw. Its different color
      // proves that debug geometry does not consume ordinary material factors.
      insert("UnlitMaterial", entity, { r: 1, g: 0, b: 1 }),
      insert("BoundingGeometry", entity, geometryFields(declaration.geometry)),
    );
  }
  const outcome = successfulBatch(await current.client.batch(commands));
  current.entities = declarations.map((_declaration, index) => {
    const result = outcome.aliases.find((entry) => entry.alias === index + 1);
    if (!result) throw new Error("Debug entity creation omitted an alias");
    return result.id;
  });
  return observe();
}

export async function updateGeometry(
  index: number,
  values: Readonly<Record<string, number | boolean | Uint8Array<ArrayBuffer>>>,
) {
  const current = state();
  const id = current.entities[index];
  if (id === undefined) throw new Error(`No debug entity at ${index}`);
  const component = descriptor("BoundingGeometry");
  successfulBatch(
    await current.client.batch(
      fields(component, values).map((field) => ({
        kind: "setField",
        entity: { kind: "handle", id },
        component: component.id,
        field,
      })),
    ),
  );
  return observe();
}

export async function patch(changes: RenderStatePatch) {
  const current = state();
  const seen = current.updates.length;
  current.client.sendCommand({
    type: "RenderStateUpdateCommand",
    changes,
  });
  const inspection = await current.client.inspect();
  await current.client.waitForFrame(inspection.tick);
  return {
    session: current.client.session,
    tick: inspection.tick,
    notifications: current.updates.slice(seen),
  };
}

export async function observe() {
  const current = state();
  const inspection = await current.client.inspect();
  const component = descriptor("BoundingGeometry");
  return {
    session: current.client.session,
    tick: inspection.tick,
    debugEnabled: current.client.capabilities.debugGeometry,
    resources: inspection.resources,
    resourceEvents: [...current.resourceEvents],
    renderDiagnostics: inspection.renderDiagnostics,
    entities: inspection.entities
      .filter((entity) => current.entities.includes(entity.id))
      .map((entity) => ({
        id: entity.id,
        name: entity.metadata.symbolicId,
        base: namedFields(
          entity.base.find((value) => value.component === component.id)?.fields,
        ),
        effective: namedFields(
          entity.effective.find((value) => value.component === component.id)
            ?.fields,
        ),
      })),
  };
}

export async function capture(label: string) {
  const observation = await observe();
  const current = state();
  const frame = await current.client.presentation!.capture(observation.tick);
  if (frame.session !== observation.session || frame.tick <= observation.tick)
    throw new Error("Debug capture must follow its acknowledged scene state");
  current.captures.set(label, frame);
  return {
    ...captureMetadata(label),
    summary: summarizeImage(frame),
    observation,
  };
}

export function captureMetadata(label: string) {
  const { pixels: _pixels, ...metadata } = frame(label);
  return metadata;
}

export function captureDataUrl(label: string) {
  return dataUrl(frame(label));
}

/** Count all foreground colors, including the plane's differently colored arrow. */
export function uniformColor(label: string, linear: readonly number[]) {
  const captured = frame(label);
  const bytes = new Uint8Array(captured.pixels);
  const expected = linear.map((channel) =>
    Math.round(
      (channel <= 0.0031308
        ? 12.92 * channel
        : 1.055 * channel ** (1 / 2.4) - 0.055) * 255,
    ),
  );
  let foreground = 0;
  let matching = 0;
  for (let offset = 0; offset < bytes.length; offset += 4) {
    if (
      ![0, 1, 2].some(
        (channel) =>
          Math.abs(bytes[offset + channel]! - BACKGROUND_RGB[channel]!) > 8,
      )
    )
      continue;
    foreground++;
    if (
      expected.every(
        (value, channel) => Math.abs(bytes[offset + channel]! - value) <= 2,
      )
    )
      matching++;
  }
  return { foreground, matching, expected };
}

export function difference(first: string, second: string) {
  return compareImages(frame(first), frame(second));
}

export function differenceDataUrl(first: string, second: string) {
  const a = frame(first);
  const b = frame(second);
  const bytesA = new Uint8Array(a.pixels);
  const bytesB = new Uint8Array(b.pixels);
  const pixels = new Uint8Array(a.pixels.byteLength);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    for (let channel = 0; channel < 3; channel++)
      pixels[offset + channel] = Math.min(
        255,
        Math.abs(bytesA[offset + channel]! - bytesB[offset + channel]!) * 4,
      );
    pixels[offset + 3] = 255;
  }
  return dataUrl({ ...a, pixels: pixels.buffer });
}

export async function recoverContext() {
  const current = state();
  current.client.presentation!.loseContext();
  // Match the maintained texture fixture's actual browser presentation barrier.
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  current.client.presentation!.restoreContext();
}

export async function close() {
  const current = active;
  active = undefined;
  try {
    for (const unsubscribe of current?.unsubscribe ?? []) unsubscribe();
    await current?.client.close();
  } finally {
    document.querySelector("#debug-geometry-canvas")?.remove();
  }
}

function state() {
  if (!active) throw new Error("Debug fixture is not initialized");
  return active;
}

function descriptor(name: string): ComponentDescriptor {
  const component = state().client.components[name];
  if (!component) throw new Error(`Missing generated component ${name}`);
  return component;
}

function geometryFields(
  values: Readonly<Record<string, number | boolean | Uint8Array<ArrayBuffer>>>,
) {
  const { shape = 0, radius = 1, height = 2, ...style } = values;
  const r = Number(radius);
  const h = Number(height);
  return {
    ...style,
    geometry: state().encodeGeometry(
      shape === 1
        ? { type: "sphere", radius: r }
        : shape === 2
          ? {
              type: "pill",
              radius: r,
              start: [0, -h / 2 + r, 0],
              end: [0, h / 2 - r, 0],
            }
          : { type: "box", min: [-1, -1, -1], max: [1, 1, 1] },
    ),
  };
}

function fields(
  component: ComponentDescriptor,
  values: Readonly<Record<string, number | boolean | Uint8Array<ArrayBuffer>>>,
): FieldWrite[] {
  return Object.entries(values).map(([name, value]) => {
    const field = component.fields[name];
    if (!field) throw new Error(`Missing generated field ${name}`);
    return {
      offset: field.offset,
      value:
        value instanceof Uint8Array
          ? { kind: "bytes", value }
          : typeof value === "boolean"
            ? { kind: "bool", value }
            : field.kind === 3
              ? { kind: "u32", value }
              : { kind: "f32", value },
    };
  });
}

function insert(
  name: string,
  entity: EntityRef,
  values: Readonly<Record<string, number | boolean | Uint8Array<ArrayBuffer>>>,
): Command {
  const component = descriptor(name);
  return {
    kind: "insertComponent",
    entity,
    component: component.id,
    fields: fields(component, values),
  };
}

function namedFields(values: Record<string, ComponentFieldValue> | undefined) {
  if (!values)
    throw new Error("Debug component missing from scene observation");
  return values;
}

function frame(label: string) {
  const captured = state().captures.get(label);
  if (!captured) throw new Error(`Missing debug capture ${label}`);
  return captured;
}

function dataUrl(frame: Pick<FrameCapture, "width" | "height" | "pixels">) {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Canvas 2D is unavailable for frame evidence");
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
