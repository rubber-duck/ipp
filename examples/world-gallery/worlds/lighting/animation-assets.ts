import { clientAssetSource } from "@ipp/client";
import type {
  AnimationWorldClient,
  AnimationClipSource,
  AnimationTrack,
  GeometryEncoder,
} from "@ipp/client";
import { INITIAL_OBJECTS, type Vec3 } from "./model.js";

export const ANIMATION_DURATION = 8;
export const BEAM_ASSET = 920010n;
export const SKELETON_ASSET = 920011n;
export const SKIN_ASSET = 920012n;
export const BEAM_PICKING_ASSET = 920013n;
export const beamPickingSource = (session: bigint) =>
  clientAssetSource(session, 6, BEAM_PICKING_ASSET).source;
type Four = [number, number, number, number];
interface MeshData {
  positions: Vec3[];
  normals: Vec3[];
  joints: Four[];
  weights: Four[];
  indices: number[];
}
interface AnimationModule {
  GEOMETRY_TYPE: number;
  encodeBoundingShape: GeometryEncoder;
  encodeSkinnedMesh(source: MeshData): Uint8Array<ArrayBuffer>;
  encodeSkeletonAsset(
    source: readonly { parent: number | null; translation?: Vec3 }[],
  ): Uint8Array<ArrayBuffer>;
  encodeSkinAsset(
    source: readonly { joint: number; inverseBind: number[] }[],
  ): Uint8Array<ArrayBuffer>;
}
const uploaded = new WeakMap<AnimationWorldClient, Promise<void>>();

/** Upload the closed 3D rig; AnimationAsset declarations supply its motion. */
export function uploadRigAssets(
  client: AnimationWorldClient,
  moduleUrl: string,
) {
  let pending = uploaded.get(client);
  if (!pending) {
    pending = (async () => {
      const module: AnimationModule = await import(moduleUrl);
      const upload = async (
        kind: number,
        asset: bigint,
        bytes: Uint8Array<ArrayBuffer>,
      ) => {
        const result = clientAssetSource(client.session, kind, asset);
        await client.registerAsset(result, bytes.buffer);
      };
      await upload(1, BEAM_ASSET, module.encodeSkinnedMesh(createBeam()));
      await upload(
        3,
        SKELETON_ASSET,
        module.encodeSkeletonAsset([
          { parent: null, translation: [0, -1, 0] },
          { parent: 0, translation: [0, 1, 0] },
          // The tip follows the bend joint without adding a skin influence.
          { parent: 1, translation: [0, 1, 0] },
        ]),
      );
      await upload(
        5,
        SKIN_ASSET,
        module.encodeSkinAsset(
          [0, 1].map((joint) => ({
            joint,
            inverseBind: [
              1,
              0,
              0,
              0,
              0,
              1,
              0,
              0,
              0,
              0,
              1,
              0,
              0,
              joint === 0 ? 1 : 0,
              0,
              1,
            ],
          })),
        ),
      );
      // One pill per bone, thick enough for the square cross-section's corners.
      // These authored pick volumes also supply the selected debug contour.
      const radius = Math.hypot(0.25, 0.25);
      await upload(
        module.GEOMETRY_TYPE,
        BEAM_PICKING_ASSET,
        module.encodeBoundingShape({
          type: "compound",
          parts: [
            { type: "pill", joints: [0, 1], radius },
            { type: "pill", joints: [1, 2], radius },
          ],
        }),
      );
    })();
    uploaded.set(client, pending);
  }
  return pending;
}

/** Nine square sections with separate face normals and closed end caps. */
export function createBeam(): MeshData {
  const mesh: MeshData = {
    positions: [],
    normals: [],
    joints: [],
    weights: [],
    indices: [],
  };
  const vertex = (position: Vec3, normal: Vec3) => {
    const index = mesh.positions.length;
    const upper = Math.max(0, Math.min(1, position[1] + 0.5));
    mesh.positions.push(position);
    mesh.normals.push(normal);
    mesh.joints.push([0, 1, 0, 0]);
    mesh.weights.push([1 - upper, upper, 0, 0]);
    return index;
  };
  // Counterclockwise perimeter viewed from above; sides keep hard corners.
  const corners: [number, number][] = [
    [-0.25, -0.25],
    [-0.25, 0.25],
    [0.25, 0.25],
    [0.25, -0.25],
  ];
  const normals: Vec3[] = [
    [-1, 0, 0],
    [0, 0, 1],
    [1, 0, 0],
    [0, 0, -1],
  ];
  for (let side = 0; side < 4; side++) {
    const a = corners[side]!;
    const b = corners[(side + 1) % 4]!;
    const start = mesh.positions.length;
    for (let row = 0; row <= 8; row++) {
      const y = row / 4 - 1;
      vertex([a[0], y, a[1]], normals[side]!);
      vertex([b[0], y, b[1]], normals[side]!);
    }
    for (let row = 0; row < 8; row++) {
      const a = start + row * 2;
      mesh.indices.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
    }
  }
  for (const y of [-1, 1]) {
    const start = mesh.positions.length;
    for (const [x, z] of corners) vertex([x, y, z], [0, y, 0]);
    mesh.indices.push(
      ...(y > 0 ? [0, 1, 2, 0, 2, 3] : [0, 2, 1, 0, 3, 2]).map(
        (index) => start + index,
      ),
    );
  }
  return mesh;
}

export function animationClips(
  client: AnimationWorldClient,
): AnimationClipSource[] {
  const transform = client.components.Transform!;
  const numeric = (field: string, samples: number[]): AnimationTrack => ({
    property: {
      component: transform.id,
      offsets: [transform.fields[field]!.offset],
    },
    keys: samples.map((value, index) => ({
      time: (index * ANIMATION_DURATION) / (samples.length - 1),
      value: { kind: "f32", value },
    })),
  });
  const spot = INITIAL_OBJECTS["lighting-spot"].position;
  const steps = 32;
  const samples = Array.from(
    { length: steps + 1 },
    (_, index) => (index * Math.PI * 2) / steps,
  );
  return [
    {
      duration: ANIMATION_DURATION,
      tracks: ["x", "y", "z"].map((field, axis) =>
        numeric(
          field,
          samples.map((angle) => -spot[axis]! * 0.175 * (1 - Math.cos(angle))),
        ),
      ),
    },
    {
      duration: ANIMATION_DURATION,
      tracks: [
        numeric(
          "x",
          samples.map(
            (angle) => Math.SQRT2 * 2 * Math.cos(angle + Math.PI / 4) - 2,
          ),
        ),
        numeric(
          "z",
          samples.map(
            (angle) => Math.SQRT2 * 2 * Math.sin(angle + Math.PI / 4) - 2,
          ),
        ),
      ],
    },
    {
      duration: ANIMATION_DURATION,
      tracks: [
        {
          property: {
            component: transform.id,
            offsets: ["qx", "qy", "qz", "qw"].map(
              (field) => transform.fields[field]!.offset,
            ),
          },
          keys: [0, 1, 2, 3, 4].map((quarter) => ({
            time: quarter * 2,
            value: {
              kind: "rotation",
              value: [
                Math.sin((quarter * Math.PI) / 4),
                0,
                0,
                Math.cos((quarter * Math.PI) / 4),
              ],
            },
          })),
        },
      ],
    },
    {
      duration: ANIMATION_DURATION,
      tracks: [
        {
          joints: [0, 1],
          keys: [0, 4, 8].map((time) => ({
            time,
            value: {
              kind: "pose",
              value: [
                { translation: [0, -1, 0] },
                {
                  translation: [0, 1, 0],
                  rotation:
                    time === 4
                      ? [0, 0, Math.sin(Math.PI * 0.2), Math.cos(Math.PI * 0.2)]
                      : [0, 0, 0, 1],
                },
              ],
            },
          })),
        },
      ],
    },
  ];
}
