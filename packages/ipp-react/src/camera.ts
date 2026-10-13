/**
 * Declarative camera rigs: one posed Camera entity without client batches.
 *
 * A rig is an ordinary Entity holding a Transform and a Camera. Aim it with
 * `lookAt` (a World-space point) or `target` (an entity handle to track);
 * either way the runtime LookAt System owns the evaluated orientation, with
 * World +Y up. Bind the declared camera as an output through the root or
 * scope `bindOutput`.
 */
import { createElement, type ReactElement } from "react";
import { Camera, Entity, LookAt, Transform } from "./components.js";

/** A finite three-component vector: position or aim point. */
export type CameraVec3 = readonly [number, number, number];

function finiteVec3(name: string, value: CameraVec3): void {
  if (value.length !== 3 || !value.every(Number.isFinite))
    throw new RangeError(`CameraRig ${name} must contain finite numbers`);
}

export interface CameraRigProps {
  /** Symbolic id of the camera Entity, for `bindOutput` and references. */
  readonly id: string;
  /** World-space camera position. */
  readonly position: CameraVec3;
  /** World-space point to aim at. */
  readonly lookAt?: CameraVec3 | undefined;
  /** Entity handle to track; the LookAt System aims the camera each frame. */
  readonly target?: bigint | undefined;
  /** Vertical field of view in radians; perspective only. */
  readonly fovY?: number | undefined;
  /** Orthographic view height in metres; selects orthographic projection. */
  readonly orthoHeight?: number | undefined;
  /** Near plane distance in metres. */
  readonly near?: number | undefined;
  /** Far plane distance in metres. */
  readonly far?: number | undefined;
  /** Focus distance in metres; the Camera default applies when omitted. */
  readonly focusDistance?: number | undefined;
}

/** Declare one posed Camera entity. See this module's header for aim modes. */
export function CameraRig(props: CameraRigProps): ReactElement {
  const {
    id,
    position,
    lookAt,
    target,
    fovY,
    orthoHeight,
    near,
    far,
    focusDistance,
  } = props;
  finiteVec3("position", position);
  if (lookAt !== undefined) finiteVec3("lookAt", lookAt);
  if (lookAt !== undefined && target !== undefined)
    throw new Error("CameraRig takes either lookAt or target, not both");
  if (orthoHeight !== undefined && fovY !== undefined)
    throw new Error("CameraRig takes either orthoHeight or fovY, not both");
  if (fovY !== undefined && !(fovY > 0 && fovY < Math.PI))
    throw new RangeError("CameraRig fovY must be between 0 and PI radians");
  if (orthoHeight !== undefined && !(orthoHeight > 0))
    throw new RangeError("CameraRig orthoHeight must be positive");
  if (near !== undefined && !(near > 0))
    throw new RangeError("CameraRig near must be positive");
  if (far !== undefined && near !== undefined && !(far > near))
    throw new RangeError("CameraRig far must exceed near");
  if (focusDistance !== undefined && !(focusDistance > 0))
    throw new RangeError("CameraRig focusDistance must be positive");

  return createElement(
    Entity,
    { id },
    createElement(Transform, {
      x: position[0],
      y: position[1],
      z: position[2],
    }),
    createElement(Camera, {
      projection: orthoHeight !== undefined ? 1 : 0,
      ...(fovY !== undefined ? { fov_y: fovY } : {}),
      ...(orthoHeight !== undefined ? { ortho_height: orthoHeight } : {}),
      ...(near !== undefined ? { near } : {}),
      ...(far !== undefined ? { far } : {}),
      ...(focusDistance !== undefined ? { focus_distance: focusDistance } : {}),
    }),
    ...(lookAt !== undefined
      ? [
          createElement(LookAt, {
            key: "look-at",
            has_point: true,
            point_x: lookAt[0],
            point_y: lookAt[1],
            point_z: lookAt[2],
          }),
        ]
      : target !== undefined
        ? [createElement(LookAt, { key: "look-at", target })]
        : []),
  );
}
