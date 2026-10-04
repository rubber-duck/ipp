import type { CameraWorldClient } from "@ipp/client";
import { setCameraPose, setCameraView } from "../../shared/camera.js";
import { PROJECTED_PANEL_TRANSFORM as panel } from "./projector.js";

/** Inspect the whole stack from its left side, with space for popup/dialog shells. */
export async function frameGuiCamera(
  client: CameraWorldClient,
  camera: bigint,
  exploded: boolean,
): Promise<void> {
  if (!exploded) return setCameraView(client, camera, "gui");
  const { qx, qy, qz, qw } = panel;
  const worldPoint = ([x, y, z]: readonly [number, number, number]): [
    number,
    number,
    number,
  ] => [
    panel.x +
      (1 - 2 * (qy * qy + qz * qz)) * x +
      2 * (qx * qy - qw * qz) * y +
      2 * (qx * qz + qw * qy) * z,
    panel.y +
      2 * (qx * qy + qw * qz) * x +
      (1 - 2 * (qx * qx + qz * qz)) * y +
      2 * (qy * qz - qw * qx) * z,
    panel.z +
      2 * (qx * qz - qw * qy) * x +
      2 * (qy * qz + qw * qx) * y +
      (1 - 2 * (qx * qx + qy * qy)) * z,
  ];
  await setCameraPose(
    client,
    camera,
    worldPoint([-9, 0.8, 9.5]),
    worldPoint([0, 0, 1.6]),
    (23 * Math.PI) / 180,
  );
}
