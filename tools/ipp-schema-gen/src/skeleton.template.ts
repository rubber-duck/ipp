/** Portable initial rig limit, matching the compiled skeleton implementation. */
export const MAX_JOINTS = 32;
export interface JointTransform {
  translation?: readonly [number, number, number];
  rotation?: readonly [number, number, number, number];
  scale?: readonly [number, number, number];
}
export interface SkeletonJoint extends JointTransform {
  parent: number | null;
}
export interface JointOverride extends JointTransform {
  joint: number;
}
function writeJointTransform(w: Writer, value: JointTransform): void {
  const t = value.translation ?? [0, 0, 0];
  const r = value.rotation ?? [0, 0, 0, 1];
  const s = value.scale ?? [1, 1, 1];
  if (
    t.length !== 3 ||
    r.length !== 4 ||
    s.length !== 3 ||
    r.every((v) => Math.fround(v) === 0) ||
    s.some((v) => Math.fround(v) <= 0)
  )
    fail("invalid joint TRS");
  for (const v of [...t, ...r, ...s]) w.f32(v);
}
function rigHeader(magic: string, count: number): Writer {
  if (count < 1) fail("empty rig asset");
  const w = new Writer();
  w.raw(new TextEncoder().encode(magic));
  w.u32(1);
  w.u32(uint(count, MAX_JOINTS));
  return w;
}
/** Immutable hierarchy/rest source; parents precede children, roots use null. */
export function encodeSkeletonAsset(
  joints: readonly SkeletonJoint[],
): Uint8Array<ArrayBuffer> {
  const w = rigHeader("IPPS", joints.length);
  joints.forEach((joint, index) => {
    w.u32(joint.parent === null ? 0xffff_ffff : uint(joint.parent, index - 1));
    writeJointTransform(w, joint);
  });
  return w.finish();
}
/** Reusable local pose in the skeleton's ordinal joint order. */
export function encodePoseAsset(
  joints: readonly JointTransform[],
): Uint8Array<ArrayBuffer> {
  const w = rigHeader("IPPP", joints.length);
  for (const joint of joints) writeJointTransform(w, joint);
  return w.finish();
}
/** Sparse absolute local overrides; omission reveals the pose asset or rest pose. */
export function encodeJointOverrides(
  joints: readonly JointOverride[],
): Uint8Array<ArrayBuffer> {
  const w = new Writer();
  let previous = -1;
  for (const joint of joints) {
    if (joint.joint <= previous)
      fail("joint overrides must be unique and ascending");
    previous = uint(joint.joint, MAX_JOINTS - 1);
    w.u32(previous);
    writeJointTransform(w, joint);
  }
  return w.finish();
}
