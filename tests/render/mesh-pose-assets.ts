/** Analytic endpoints and independent baked references, reusable by GL runners. */
const base = [
  [-0.7, -0.65, 0],
  [0.7, -0.65, 0],
  [0.7, 0.65, 0],
  [-0.7, 0.65, 0],
] as const;
const target = [
  [-0.7, -0.65, 0],
  [0.7, -0.65, 0],
  [1.35, 0.85, 0.6],
  [-0.05, 0.85, 0.6],
] as const;

/** T(0, .15, 0) × Ry(30°) × S(.8, 1.2, .6), authored through the real client. */
export const affinePoseTransform = {
  y: 0.15,
  qy: Math.sin(Math.PI / 12),
  qw: Math.cos(Math.PI / 12),
  sx: 0.8,
  sy: 1.2,
  sz: 0.6,
} as const;

/** Analytic reference coordinates; intentionally independent of runtime matrices. */
export function affinePosePosition([x, y, z]: readonly number[]): number[] {
  const cosine = Math.sqrt(3) / 2;
  return [
    0.8 * cosine * x! + 0.3 * z!,
    1.2 * y! + 0.15,
    -0.4 * x! + 0.6 * cosine * z!,
  ];
}

/** Apply the independently known inverse-transpose, then normalize for IPPM. */
export function affinePoseNormal([x, y, z]: readonly number[]): number[] {
  const cosine = Math.sqrt(3) / 2;
  const normal = [
    (cosine * x!) / 0.8 + (0.5 * z!) / 0.6,
    y! / 1.2,
    (-0.5 * x!) / 0.8 + (cosine * z!) / 0.6,
  ];
  const length = Math.hypot(...normal);
  return normal.map((value) => value / length);
}

export function poseMesh(
  weight: number,
  options: {
    normals?: boolean;
    skin?: boolean;
    uvs?: boolean;
    reversed?: boolean;
    x?: number;
    affine?: boolean;
    aim?: boolean;
    joint?: boolean;
  } = {},
): Uint8Array<ArrayBuffer> {
  const streams: {
    semantic: number;
    format: number;
    bytes: Uint8Array<ArrayBuffer>;
  }[] = [];
  const floats = (values: readonly number[]) => {
    const bytes = new Uint8Array(values.length * 4);
    const view = new DataView(bytes.buffer);
    values.forEach((value, index) => view.setFloat32(index * 4, value, true));
    return bytes;
  };
  streams.push({
    semantic: 0,
    format: 1,
    bytes: floats(
      base.flatMap((point, i) => {
        let position = point.map(
          (value, axis) =>
            value * (1 - weight) +
            target[i]![axis]! * weight +
            (axis === 0 ? (options.x ?? 0) : 0),
        );
        if (options.joint) position = jointPoseVector(position);
        if (options.aim) position = aimedPoseVector(position);
        return options.affine ? affinePosePosition(position) : position;
      }),
    ),
  });
  if (options.uvs) {
    streams.push({
      semantic: 2,
      format: 2,
      bytes: floats([0, 1, 1, 1, 1, 0, 0, 0]),
    });
  }
  if (options.normals !== false) {
    const length = Math.hypot(0.6, 1.5);
    const normal = [
      0,
      (-0.6 / length) * weight,
      1 - weight + (1.5 / length) * weight,
    ];
    const magnitude = Math.hypot(...normal);
    let unit = normal.map((value) => value / magnitude);
    if (options.joint) unit = jointPoseVector(unit);
    if (options.aim) unit = aimedPoseVector(unit);
    const transformed = options.affine ? affinePoseNormal(unit) : unit;

    streams.push({
      semantic: 4,
      format: 1,
      bytes: floats(base.flatMap(() => transformed)),
    });
  }
  if (options.skin) {
    streams.push({ semantic: 5, format: 4, bytes: new Uint8Array(16) });
    streams.push({
      semantic: 6,
      format: 5,
      bytes: floats(base.flatMap(() => [1, 0, 0, 0])),
    });
  }
  const bytes = new Uint8Array(
    20 +
      streams.length * 8 +
      streams.reduce((sum, stream) => sum + stream.bytes.length, 0) +
      12,
  );
  const view = new DataView(bytes.buffer);
  bytes.set([0x49, 0x50, 0x50, 0x4d]);
  [3, 4, 6, streams.length].forEach((value, index) =>
    view.setUint32(4 + index * 4, value, true),
  );
  let cursor = 20 + streams.length * 8;
  streams.forEach((stream, index) => {
    bytes.set([stream.semantic, stream.format, 0, 0], 20 + index * 8);
    view.setUint32(24 + index * 8, stream.bytes.length, true);
    bytes.set(stream.bytes, cursor);
    cursor += stream.bytes.length;
  });
  (options.reversed ? [0, 2, 1, 0, 3, 2] : [0, 1, 2, 0, 2, 3]).forEach(
    (value, index) => view.setUint16(cursor + index * 2, value, true),
  );
  return bytes;
}

/** The fixture's single joint rotates 0.4 radians around Z before object placement. */
function jointPoseVector([x, y, z]: readonly number[]): number[] {
  return [
    Math.cos(0.4) * x! - Math.sin(0.4) * y!,
    Math.sin(0.4) * x! + Math.cos(0.4) * y!,
    z!,
  ];
}

/** Independent Ry(-45 degrees), aiming local -Z toward (1, 0, -1). */
export function aimedPoseVector([x, y, z]: readonly number[]): number[] {
  const c = Math.SQRT1_2;
  return [c * (x! - z!), y!, c * (x! + z!)];
}
