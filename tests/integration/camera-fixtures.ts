import type {
  BatchOutcome,
  CameraWorldClient,
  Client,
  Command,
  EntityRef,
  FieldWrite,
} from "@ipp/client";

export const CAMERA_VIEWPORT = Object.freeze({ width: 320, height: 240 });
export const ORTHOGRAPHIC_CAMERA = Object.freeze({
  projection: 1,
  fov_y: Math.PI / 4,
  near: 0.1,
  far: 100,
  ortho_height: 4,
  focus_distance: 6,
});

/** Resolve writes from the connected target's generated component descriptors. */
export function componentFields(
  client: Client,
  name: string,
  values: Readonly<
    Record<string, number | string | boolean | Uint8Array<ArrayBuffer> | bigint>
  >,
): FieldWrite[] {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`Target does not expose ${name}`);
  return Object.entries(values).map(([name, value]) => {
    const field = descriptor.fields[name];
    if (!field) throw new Error(`Component has no generated field ${name}`);
    return {
      offset: field.offset,
      value:
        value instanceof Uint8Array
          ? { kind: "bytes", value }
          : typeof value === "bigint"
            ? { kind: "entity", value: { kind: "handle", id: value } }
            : typeof value === "boolean"
              ? { kind: "bool", value }
              : field.kind === 5
                ? { kind: "string", value: String(value) }
                : field.kind === 3
                  ? { kind: "u32", value: Number(value) }
                  : { kind: "f32", value: Number(value) },
    };
  });
}

export function insertComponent(
  client: Client,
  name: string,
  entity: EntityRef,
  values: Readonly<
    Record<string, number | string | boolean | Uint8Array<ArrayBuffer> | bigint>
  > = {},
): Command {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`Target does not expose ${name}`);
  return {
    kind: "insertComponent",
    entity,
    component: descriptor.id,
    fields: componentFields(client, name, values),
  };
}

export function createEntity(alias: number, symbolicId: string): Command {
  return { kind: "create", alias, metadata: { symbolicId, classes: [] } };
}

export function successfulBatch(
  outcome: BatchOutcome,
): Extract<BatchOutcome, { ok: true }> {
  if (!outcome.ok) {
    throw new Error(
      `World batch rejected at ${outcome.error.operation}: ${outcome.error.reason}`,
    );
  }
  return outcome;
}

export function aliasId(outcome: BatchOutcome, alias: number): bigint {
  const id = successfulBatch(outcome).aliases.find(
    (entry) => entry.alias === alias,
  )?.id;
  if (id === undefined) throw new Error(`World batch omitted alias ${alias}`);
  return id;
}

export function cameraClient(client: Client): CameraWorldClient {
  if (!("sendCommand" in client) || typeof client.sendCommand !== "function") {
    throw new Error("World target does not expose generated camera commands");
  }
  return client as CameraWorldClient;
}

/** Explicitly author the former fixture view: eye (3,2,5), looking at the origin. */
export async function activateFixtureCamera(client: Client): Promise<bigint> {
  const yaw = Math.atan2(3, 5);
  const pitch = -Math.atan2(2, Math.hypot(3, 5));
  const sy = Math.sin(yaw / 2);
  const cy = Math.cos(yaw / 2);
  const sx = Math.sin(pitch / 2);
  const cx = Math.cos(pitch / 2);
  const entity: EntityRef = { kind: "alias", alias: 60000 };
  const outcome = await client.batch([
    {
      kind: "create",
      alias: entity.alias,
      metadata: {
        symbolicId: "__fixture-camera",
        classes: ["fixture-camera"],
      },
    },
    insertComponent(client, "Transform", entity, {
      x: 3,
      y: 2,
      z: 5,
      qx: cy * sx,
      qy: sy * cx,
      qz: -sy * sx,
      qw: cy * cx,
    }),
    insertComponent(client, "Camera", entity, {
      ...ORTHOGRAPHIC_CAMERA,
      projection: 0,
    }),
  ]);
  const id = aliasId(outcome, entity.alias);
  cameraClient(client).sendCommand({
    type: "CameraActivateCommand",
    entity: id,
  });
  await client.inspect();
  return id;
}

/** Four quads surround a square hole; its box contains points with no triangles. */
export function createPickingRing(): ArrayBuffer {
  const positions = [
    [-1, -1, 0],
    [1, -1, 0],
    [1, 1, 0],
    [-1, 1, 0],
    [-0.3, -0.3, 0],
    [0.3, -0.3, 0],
    [0.3, 0.3, 0],
    [-0.3, 0.3, 0],
  ];
  const indices = [
    0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
  ];
  const bytes = new ArrayBuffer(
    16 + positions.length * 24 + indices.length * 2,
  );
  const view = new DataView(bytes);
  new Uint8Array(bytes, 0, 4).set([0x49, 0x50, 0x50, 0x4d]);
  [1, positions.length, indices.length].forEach((value, index) =>
    view.setUint32(4 + index * 4, value, true),
  );
  let offset = 16;
  for (const position of positions) {
    for (const value of [...position, 0.9, 0.3, 0.1]) {
      view.setFloat32(offset, value, true);
      offset += 4;
    }
  }
  for (const index of indices) {
    view.setUint16(offset, index, true);
    offset += 2;
  }
  return bytes;
}

/** A compound union keeps a square hole while sharing one immutable definition. */
export const PICKING_RING = {
  type: "compound",
  parts: [
    { type: "box", min: [-1, -1, 0], max: [1, -0.3, 0] },
    { type: "box", min: [0.3, -0.3, 0], max: [1, 0.3, 0] },
    { type: "box", min: [-1, 0.3, 0], max: [1, 1, 0] },
    { type: "box", min: [-1, -0.3, 0], max: [-0.3, 0.3, 0] },
  ],
} as const;
