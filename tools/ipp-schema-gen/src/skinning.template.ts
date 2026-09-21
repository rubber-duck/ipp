export interface SkinJoint {
  joint: number;
  /** Column-major mesh bind-space to joint bind-space matrix (16 floats). */
  inverseBind: readonly number[];
}
/** Immutable inverse binds in mesh palette order, independent of skeleton order. */
export function encodeSkinAsset(
  joints: readonly SkinJoint[],
): Uint8Array<ArrayBuffer> {
  const w = rigHeader("IPPB", joints.length);
  for (const entry of joints) {
    w.u32(uint(entry.joint, MAX_JOINTS - 1));
    if (entry.inverseBind.length !== 16)
      fail("inverse bind requires 16 floats");
    for (const value of entry.inverseBind) w.f32(value);
  }
  return w.finish();
}
export interface SkinnedMesh {
  positions: readonly (readonly [number, number, number])[];
  normals?: readonly (readonly [number, number, number])[];
  colors?: readonly (readonly [number, number, number])[];
  joints: readonly (readonly [number, number, number, number])[];
  weights: readonly (readonly [number, number, number, number])[];
  indices: readonly number[];
}
/** IPPM v3 with separate joint streams. Weights are normalized by the runtime. */
export function encodeSkinnedMesh(mesh: SkinnedMesh): Uint8Array<ArrayBuffer> {
  const count = mesh.positions.length;
  uint(count, 65536);
  if (
    !count ||
    mesh.joints.length !== count ||
    mesh.weights.length !== count ||
    (mesh.colors && mesh.colors.length !== count) ||
    (mesh.normals && mesh.normals.length !== count) ||
    !mesh.indices.length ||
    mesh.indices.length % 3
  )
    fail("invalid skinned mesh lengths");
  const streams: [number, number, number][] = [
    [0, 1, 12],
    ...(mesh.colors ? [[1, 1, 12] as [number, number, number]] : []),
    ...(mesh.normals ? [[4, 1, 12] as [number, number, number]] : []),
    [5, 4, 4],
    [6, 5, 16],
  ];
  const w = new Writer(Number.MAX_SAFE_INTEGER);
  w.raw(new TextEncoder().encode("IPPM"));
  for (const n of [3, count, mesh.indices.length, streams.length]) w.u32(n);
  for (const [semantic, format, width] of streams) {
    w.u8(semantic);
    w.u8(format);
    w.u16(0);
    w.u32(count * width);
  }
  for (const position of mesh.positions) {
    if (position.length !== 3) fail("invalid position");
    for (const v of position) w.f32(v);
  }
  if (mesh.colors)
    for (const color of mesh.colors) {
      if (color.length !== 3) fail("invalid color");
      for (const v of color) {
        if (v < 0 || v > 1) fail("invalid color");
        w.f32(v);
      }
    }
  if (mesh.normals)
    for (const normal of mesh.normals) {
      if (normal.length !== 3 || Math.hypot(...normal) < 1e-6)
        fail("invalid normal");
      for (const v of normal) w.f32(v);
    }
  for (const joints of mesh.joints) {
    if (joints.length !== 4) fail("four joint slots required");
    for (const joint of joints) w.u8(uint(joint, MAX_JOINTS - 1));
  }
  for (const weights of mesh.weights) {
    if (
      weights.length !== 4 ||
      weights.some((v) => v < 0 || v > 1) ||
      !weights.some((v) => Math.fround(v) > 0)
    )
      fail("invalid joint weights");
    for (const v of weights) w.f32(v);
  }
  for (const index of mesh.indices) w.u16(uint(index, count - 1));
  return w.finish();
}
