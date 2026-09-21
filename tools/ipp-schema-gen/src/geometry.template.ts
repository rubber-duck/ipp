import type { BoundingShape } from "./types.js";

/** Immutable shared geometry payload type for ordinary asset uploads. */
export const GEOMETRY_TYPE = WIRE.ASSET_GEOMETRY;

/** Encode one primitive or a compound, preserving leaf order as hit.part. */
export function encodeBoundingShape(
  shape: BoundingShape,
): Uint8Array<ArrayBuffer> {
  const parts: Exclude<BoundingShape, { type: "compound" }>[] = [];
  const ancestors = new Set<BoundingShape>();
  function append(value: BoundingShape): void {
    if (value === null || typeof value !== "object" || ancestors.has(value))
      fail("invalid geometry nesting");
    if (value.type === "compound") {
      if (!Array.isArray(value.parts) || value.parts.length === 0)
        fail("empty compound");
      ancestors.add(value);
      for (const part of value.parts) append(part);
      ancestors.delete(value);
    } else {
      parts.push(value);
    }
  }
  append(shape);
  const w = new Writer(Number.MAX_SAFE_INTEGER);
  w.raw(new TextEncoder().encode("IPPG"));
  w.u32(1);
  w.u32(parts.length);
  for (const part of parts) {
    let values: readonly number[];
    switch (part.type) {
      case "box":
        if (part.min.length !== 3 || part.max.length !== 3)
          fail("invalid box extents");
        for (let axis = 0; axis < 3; axis++)
          if (part.min[axis]! > part.max[axis]!) fail("inverted box");
        w.u32(0);
        values = [...part.min, ...part.max, 0];
        break;
      case "sphere": {
        const center = part.center ?? [0, 0, 0];
        if (center.length !== 3 || !(Math.fround(part.radius) > 0))
          fail("invalid sphere");
        w.u32(1);
        values = [...center, part.radius, 0, 0, 0];
        break;
      }
      case "pill": {
        const start = part.start ?? [0, 0, 0];
        const end = part.end ?? [0, 0, 0];
        if (
          start.length !== 3 ||
          end.length !== 3 ||
          !(Math.fround(part.radius) > 0)
        )
          fail("invalid pill");
        if (part.joints) {
          if (!CAPABILITIES.skeletalAnimation || part.joints.length !== 2)
            fail("joint mapping requires skeletons");
          for (const value of [...start, ...end])
            if (value !== 0) fail("joint pairs define pill endpoints");
        }
        w.u32(2);
        values = [...start, ...end, part.radius];
        break;
      }
      default:
        return fail("unknown bounding shape");
    }
    for (const value of values) w.f32(value);
    const t = part.transform?.translation ?? [0, 0, 0];
    const q = part.transform?.rotation ?? [0, 0, 0, 1];
    const s = part.transform?.scale ?? [1, 1, 1];
    if (t.length !== 3 || q.length !== 4 || s.length !== 3)
      fail("invalid shape transform");
    if (q[0] === 0 && q[1] === 0 && q[2] === 0 && q[3] === 0)
      fail("zero shape quaternion");
    for (const scale of s)
      if (!(Math.fround(scale) > 0)) fail("invalid shape scale");
    for (const value of [...t, ...q, ...s]) w.f32(value);
    const joints = part.type === "pill" ? part.joints : undefined;
    w.u32(joints ? uint(joints[0], 31) : 0xffff_ffff);
    w.u32(joints ? uint(joints[1], 31) : 0xffff_ffff);
  }
  return w.finish();
}
