import type { CameraWorldClient, Command } from "@ipp/client";

/** The gallery camera lives for the canvas session, across world mount toggles. */
export async function initializeCamera(
  client: CameraWorldClient,
): Promise<bigint> {
  const transform = client.components.Transform;
  const camera = client.components.Camera;
  if (!transform || !camera) throw new Error("Gallery requires camera support");

  const yaw = Math.atan2(3, 5) / 2;
  const pitch = -Math.atan2(2, Math.hypot(3, 5)) / 2;
  const values = {
    x: 3,
    y: 2,
    z: 5,
    qx: Math.sin(pitch) * Math.cos(yaw),
    qy: Math.cos(pitch) * Math.sin(yaw),
    qz: -Math.sin(pitch) * Math.sin(yaw),
    qw: Math.cos(pitch) * Math.cos(yaw),
  };
  const entity = { kind: "alias", alias: 1 } as const;
  const commands: Command[] = [
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: "gallery-camera", classes: ["camera"] },
    },
    {
      kind: "insertComponent",
      entity,
      component: transform.id,
      fields: Object.entries(values).map(([field, value]) => ({
        offset: transform.fields[field]!.offset,
        value: { kind: "f32", value },
      })),
    },
    {
      kind: "insertComponent",
      entity,
      component: camera.id,
      fields: [
        {
          offset: camera.fields.focus_distance!.offset,
          value: { kind: "f32", value: Math.hypot(3, 2, 5) },
        },
      ],
    },
  ];
  const outcome = await client.batch(commands);
  if (!outcome.ok)
    throw new Error(`Camera creation failed: ${outcome.error.reason}`);
  const created = outcome.aliases.find((alias) => alias.alias === 1);
  if (!created) throw new Error("Camera creation returned no entity");

  client.sendCommand({
    type: "CameraActivateCommand",
    entity: created.id,
  });
  return created.id;
}

export type CameraView =
  | "shapes"
  | "lighting"
  | "platformer"
  | "particles"
  | "gui";

/** Keep the protected session camera alive while changing its view. */
export async function setCameraView(
  client: CameraWorldClient,
  id: bigint,
  view: CameraView,
): Promise<void> {
  const lighting = view !== "shapes";
  const [x, y, z] =
    view === "gui"
      ? [-8.2, 3.2, 18.2]
      : view === "particles"
        ? [4, 2.5, 7]
        : lighting
          ? [7, 7, 10]
          : [3, 2, 5];
  const targetY =
    view === "gui" ? -0.03 : view === "particles" ? -0.2 : lighting ? 0.5 : 0;
  const targetX = view === "gui" ? 0.5 : 0;
  const targetZ = view === "gui" ? 2.3 : 0;
  const yaw = Math.atan2(x - targetX, z - targetZ) / 2;
  const pitch =
    -Math.atan2(y - targetY, Math.hypot(x - targetX, z - targetZ)) / 2;
  const values = {
    Transform: {
      x,
      y,
      z,
      qx: Math.sin(pitch) * Math.cos(yaw),
      qy: Math.cos(pitch) * Math.sin(yaw),
      qz: -Math.sin(pitch) * Math.sin(yaw),
      qw: Math.cos(pitch) * Math.cos(yaw),
    },
    Camera: {
      projection: 0,
      fov_y: view === "gui" ? (21 * Math.PI) / 180 : Math.PI / 4,
      focus_distance: Math.hypot(x - targetX, y - targetY, z - targetZ),
    },
  };
  const operations: Command[] = Object.entries(values).flatMap(
    ([name, fields]) => {
      const component = client.components[name]!;
      return Object.entries(fields).map(([field, value]) => ({
        kind: "setField",
        entity: { kind: "handle", id },
        component: component.id,
        field: {
          offset: component.fields[field]!.offset,
          value: { kind: field === "projection" ? "u32" : "f32", value },
        },
      }));
    },
  );
  const result = await client.batch(operations);
  if (!result.ok)
    throw new Error(`Camera update failed: ${result.error.reason}`);
}
