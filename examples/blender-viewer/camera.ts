import {
  sameOutputReference,
  type Client,
  type FieldWrite,
  type OutputReference,
} from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";

export function requireBlenderClient(
  client: Client,
): asserts client is BlenderClient {
  const methods = [
    "registerAsset",
    "releaseAsset",
    "createAsset",
    "onResourceChange",
    "encodeAnimationClip",
    "createAnimationController",
    "updateAnimationController",
    "transitionAnimationController",
    "deleteAnimationController",
    "controlAnimationController",
    "playback",
    "onPlaybackEvent",
    "sendCommand",
  ];
  if (methods.some((name) => typeof Reflect.get(client, name) !== "function"))
    throw new Error("Blender viewer requires the animation and spatial client");
}

export function sameOutput(
  left: OutputReference | null | undefined,
  right: OutputReference | null | undefined,
): boolean {
  return !!left && !!right && sameOutputReference(left, right);
}

export async function createViewingCamera(client: Client): Promise<bigint> {
  const transform = client.components.Transform;
  const camera = client.components.Camera;
  if (!transform || !camera)
    throw new Error("Blender viewer target lacks Transform or Camera");
  const yaw = Math.atan2(3, 5);
  const pitch = -Math.atan2(2, Math.hypot(3, 5));
  const fields: FieldWrite[] = Object.entries({
    x: 3,
    y: 2,
    z: 5,
    qx: Math.cos(yaw / 2) * Math.sin(pitch / 2),
    qy: Math.sin(yaw / 2) * Math.cos(pitch / 2),
    qz: -Math.sin(yaw / 2) * Math.sin(pitch / 2),
    qw: Math.cos(yaw / 2) * Math.cos(pitch / 2),
  }).map(([name, value]) => {
    const field = transform.fields[name];
    if (!field || field.kind !== 1)
      throw new Error(`Missing generated Transform float field ${name}`);
    return { offset: field.offset, value: { kind: "f32", value } };
  });
  const result = await client.batch([
    {
      kind: "create",
      alias: 1,
      metadata: { symbolicId: "__blender-viewing-camera", classes: [] },
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: transform.id,
      fields,
    },
    {
      kind: "insertComponent",
      entity: { kind: "alias", alias: 1 },
      component: camera.id,
      fields: [],
    },
  ]);
  if (!result.ok) throw new Error(result.error.reason);
  const entity = result.aliases.find((entry) => entry.alias === 1)?.id;
  if (entity === undefined)
    throw new Error(
      "Viewing camera creation omitted its acknowledged identity",
    );
  return entity;
}
