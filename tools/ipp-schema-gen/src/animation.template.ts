/** Encode shared keyframes through the compiled immutable animation asset format. */
export function encodeAnimationClip(
  clip: AnimationClipSource,
): Uint8Array<ArrayBuffer> {
  if (!Number.isFinite(clip.duration) || clip.duration <= 0)
    fail("animation duration");
  if (!clip.tracks.length) fail("animation track count");
  const dynamic = clip.tracks.some(
    (track) =>
      track.property?.name !== undefined ||
      track.keys.some((key) => key.value.kind === "dynamic"),
  );
  const poses = clip.tracks.some((track) => track.joints !== undefined);
  if (poses && !CAPABILITIES.skeletalAnimation)
    fail("skeleton animation unavailable");
  const w = new Writer(Number.MAX_SAFE_INTEGER);
  for (const byte of [73, 80, 80, 65, dynamic ? 3 : poses ? 2 : 1, 0, 0, 0])
    w.u8(byte);
  w.f64(clip.duration);
  w.u32(clip.tracks.length);
  function value(v: AnimationValue) {
    switch (v.kind) {
      case "dynamic": {
        const bytes = encodeDynamicValue(v.value);
        w.u8(11);
        w.u32(bytes.length);
        w.raw(bytes);
        break;
      }
      case "f32":
        w.u8(1);
        w.f32(v.value);
        break;
      case "entity":
        w.u8(2);
        w.u64(v.value);
        break;
      case "u32":
        w.u8(3);
        w.u32(v.value);
        break;
      case "u64":
        w.u8(4);
        w.u64(v.value);
        break;
      case "string":
        w.u8(5);
        w.string(v.value, 0xffff_ffff);
        break;
      case "bytes":
        if (!(v.value instanceof Uint8Array)) fail("animation bytes required");
        w.u8(6);
        w.u32(v.value.length);
        w.raw(v.value);
        break;
      case "bool":
        w.u8(7);
        w.boolean(v.value);
        break;
      case "rotation":
        if (v.value.length !== 4 || v.value.every((x) => Math.fround(x) === 0))
          fail("animation rotation");
        w.u8(8);
        for (const number of v.value) w.f32(number);
        break;
      // #if skeletal-animation
      case "pose":
        if (!v.value.length || v.value.length > MAX_JOINTS)
          fail("animation pose count");
        w.u8(9);
        w.u32(v.value.length);
        for (const joint of v.value) writeJointTransform(w, joint);
        break;
      // #endif
      default:
        fail("animation value kind");
    }
  }
  for (const track of clip.tracks) {
    if (!track.keys.length) fail("animation key count");
    const kind = track.keys[0]!.value.kind;
    if (poses || dynamic)
      w.u8(
        track.property?.name !== undefined
          ? 2
          : track.joints === undefined
            ? 0
            : 1,
      );
    if (track.property?.name !== undefined) {
      if (kind !== "dynamic") fail("dynamic animation value required");
      w.u16(track.property.component);
      w.string(track.property.name, 0xffff_ffff);
    } else if (track.joints !== undefined) {
      if (
        !CAPABILITIES.skeletalAnimation ||
        !track.joints.length ||
        track.joints.length > 32 ||
        kind !== "pose"
      )
        fail("animation joint target");
      w.u32(track.joints.length);
      let previous = -1;
      for (const joint of track.joints) {
        uint(joint, 31);
        if (joint <= previous) fail("duplicate/unordered joint target");
        previous = joint;
        w.u32(joint);
      }
    } else {
      const { component, offsets } = track.property;
      if (![1, 4].includes(offsets.length)) fail("animation property offsets");
      if (new Set(offsets).size !== offsets.length)
        fail("duplicate animation property offset");
      if ((kind === "rotation") !== (offsets.length === 4) || kind === "pose")
        fail("animation property kind");
      w.u16(component);
      w.u8(offsets.length);
      for (const offset of offsets) w.u32(offset);
    }
    w.u32(track.keys.length);
    const validPose = (v: AnimationValue) =>
      v.kind !== "pose" || v.value.length === track.joints?.length;
    for (let index = 0; index < track.keys.length; index++) {
      const key = track.keys[index]!;
      const next = track.keys[index + 1];
      if (
        key.time < 0 ||
        key.time > clip.duration ||
        !validPose(key.value) ||
        key.value.kind !== kind ||
        (next && next.time <= key.time)
      )
        fail("animation key order/type");
      w.f64(key.time);
      value(key.value);
      const numeric =
        ["f32", "u32", "u64", "rotation", "pose"].includes(kind) ||
        (track.keys[0]!.value.kind === "dynamic" &&
          !["bool", "asset"].includes(track.keys[0]!.value.value.kind));
      const curve = key.interpolation ?? {
        kind: next && numeric ? "linear" : "step",
      };
      if (curve.kind === "step") w.u8(0);
      else if (curve.kind === "linear" && next && numeric) w.u8(1);
      else if (curve.kind === "bezier" && next && numeric) {
        if (
          curve.time1 < key.time ||
          curve.time2 < curve.time1 ||
          curve.time2 > next.time ||
          curve.value1.kind !== kind ||
          curve.value2.kind !== kind ||
          !validPose(curve.value1) ||
          !validPose(curve.value2)
        )
          fail("animation bezier handles");
        w.u8(2);
        w.f64(curve.time1);
        value(curve.value1);
        w.f64(curve.time2);
        value(curve.value2);
      } else fail("animation interpolation");
    }
  }
  return w.finish();
}
