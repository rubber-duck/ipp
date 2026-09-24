# Skeleton, Pose and Skin Payloads

[Skeleton and pose decoders](skeleton.rs) · [Skin binding decoder](skin_binding.rs) · [Authoring guide](../../../../../docs/development/skeletal-skinning.md) · [Mesh attributes](MESH_FORMAT.md)

| Format | Encoding |
| --- | --- |
| Skeleton/pose/skin v1 | Little-endian, exact-length; `IPPS`/`IPPP`/`IPPB`, u32 version/count |
| Skeleton joint | Parent u32 (`0xffffffff` root), then f32 translation xyz / quaternion xyzw / scale xyz; parents precede children |
| Pose | TRS entries; count matches skeleton |
| Skin | Skeleton ordinal u32, invertible affine column-major f32 matrix |
| IPPM v3 joints | Paired semantic 5/format 4 (`u8x4` indices), semantic 6/format 5 (`f32x4` weights) |

Per-instance joint overrides are not a payload: they are `Skeleton.joints` [schema rows](../../world/systems/skeleton/component.rs). Quaternions must be nonzero and normalize during evaluation; scales finite/positive. Decoders check exact lengths/joint limits without byte quotas. All joint indices must fit the palette, even zero-weight slots. Weights are finite 0..1 with positive sum, normalized once.
