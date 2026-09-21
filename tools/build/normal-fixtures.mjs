/** Test oracles derived from the real Rust sphere, shared by WebGL and GLES. */
export function normalFixtures(sphere) {
  const count = sphere.readUInt32LE(16);
  const vertices = sphere.readUInt32LE(8);
  let offset = 20 + count * 8;
  const streams = Array.from({ length: count }, (_, index) => {
    const descriptor = Buffer.from(
      sphere.subarray(20 + index * 8, 28 + index * 8),
    );
    const length = descriptor.readUInt32LE(4);
    const data = Buffer.from(sphere.subarray(offset, offset + length));
    offset += length;
    return { descriptor, data };
  });
  const indices = sphere.subarray(offset);
  const encode = (selected) => {
    const header = Buffer.from(sphere.subarray(0, 20));
    header.writeUInt32LE(selected.length, 16);
    return Buffer.concat([
      header,
      ...selected.map((s) => s.descriptor),
      ...selected.map((s) => s.data),
      indices,
    ]);
  };
  const flat = encode(streams.filter((s) => s.descriptor[0] !== 4));
  // Bake scale (1.6, .65, 1.1), then rotation .6 radians about +Y.
  // The normal oracle divides by scale before rotation, independently of GL's matrix helper.
  for (const { descriptor, data } of streams) {
    const semantic = descriptor[0];
    if (semantic !== 0 && semantic !== 4) continue;
    for (let vertex = 0; vertex < vertices; vertex++) {
      const factors =
        semantic === 0 ? [1.6, 0.65, 1.1] : [1 / 1.6, 1 / 0.65, 1 / 1.1];
      const [x, y, z] = factors.map(
        (factor, axis) => data.readFloatLE(vertex * 12 + axis * 4) * factor,
      );
      const values = [
        Math.cos(0.6) * x + Math.sin(0.6) * z,
        y,
        -Math.sin(0.6) * x + Math.cos(0.6) * z,
      ];
      const length = semantic === 4 ? Math.hypot(...values) : 1;
      values.forEach((value, axis) =>
        data.writeFloatLE(value / length, vertex * 12 + axis * 4),
      );
    }
  }
  return { "sphere-flat": flat, "sphere-baked": encode(streams) };
}
