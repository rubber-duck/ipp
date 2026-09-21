import {
  affinePoseNormal,
  affinePosePosition,
  poseMesh,
} from "./mesh-pose-assets.js";

test("affine mesh reference preserves transformation order and inverse-transpose normals", () => {
  const close = (actual: readonly number[], expected: readonly number[]) => {
    actual.forEach((value, index) =>
      assert.ok(Math.abs(value - expected[index]!) < 1e-6),
    );
  };
  const cosine = Math.sqrt(3) / 2;
  close(affinePosePosition([0, 0, 0]), [0, 0.15, 0]);
  close(affinePosePosition([1, 2, 3]), [
    0.8 * cosine + 0.9,
    2.55,
    -0.4 + 1.8 * cosine,
  ]);
  close(affinePoseNormal([0, 0, 1]), [0.5, 0, cosine]);
  // The tangent (0, 1, -1) remains orthogonal to its normal (0, 1, 1).
  const origin = affinePosePosition([0, 0, 0]);
  const tangent = affinePosePosition([0, 1, -1]).map(
    (value, axis) => value - origin[axis]!,
  );
  const normal = affinePoseNormal([0, 1, 1]);
  assert.ok(
    Math.abs(
      tangent.reduce((sum, value, axis) => sum + value * normal[axis]!, 0),
    ) < 1e-12,
  );
  const bytes = poseMesh(0.5, { affine: true });
  const view = new DataView(bytes.buffer);
  const positionStart = 20 + view.getUint32(16, true) * 8;
  close(
    [0, 1, 2].map((axis) => view.getFloat32(positionStart + axis * 4, true)),
    [-0.56 * cosine, -0.63, 0.28],
  );
});

import assert from "node:assert/strict";
import test from "node:test";
import {
  BACKGROUND_RGB,
  compareImages,
  requireBlank,
  requireShiftedAndScaled,
  requireVisible,
  summarizeImage,
} from "./image-assertions.js";

test("image assertions measure visibility, motion, scale, and differences", () => {
  const background = solidFrame(40, 30, BACKGROUND_RGB);
  const base = rectangleFrame(40, 30, 12, 8, 27, 23, [220, 70, 30]);
  const moved = rectangleFrame(40, 30, 28, 11, 35, 18, [220, 70, 30]);

  requireBlank(summarizeImage(background), "background");
  requireVisible(summarizeImage(base), "base");
  requireShiftedAndScaled(summarizeImage(base), summarizeImage(moved));
  assert.ok(compareImages(base, moved).changedFraction > 0.1);
});

function solidFrame(
  width: number,
  height: number,
  color: readonly [number, number, number],
) {
  const pixels = new Uint8Array(width * height * 4);
  for (let offset = 0; offset < pixels.length; offset += 4) {
    pixels.set([...color, 255], offset);
  }
  return { width, height, pixels: pixels.buffer };
}

function rectangleFrame(
  width: number,
  height: number,
  left: number,
  top: number,
  right: number,
  bottom: number,
  color: readonly [number, number, number],
) {
  const frame = solidFrame(width, height, BACKGROUND_RGB);
  const pixels = new Uint8Array(frame.pixels);
  for (let y = top; y <= bottom; y += 1) {
    for (let x = left; x <= right; x += 1) {
      pixels.set([...color, 255], (y * width + x) * 4);
    }
  }
  return frame;
}

test("the gallery skinned beam is a closed volume with outward normals and two-joint weights", async () => {
  const { createBeam } = await import(
    "../../examples/world-gallery/worlds/lighting/animation-assets.js"
  );
  const mesh = createBeam();
  assert.equal(mesh.positions.length, 80);
  assert.equal(mesh.indices.length, 204);
  const edges = new Map<string, number>();
  let volume = 0;
  for (let triangle = 0; triangle < mesh.indices.length; triangle += 3) {
    const indices = mesh.indices.slice(triangle, triangle + 3);
    const a = mesh.positions[indices[0]!]!;
    const b = mesh.positions[indices[1]!]!;
    const c = mesh.positions[indices[2]!]!;
    const u = b.map((value, axis) => value - a[axis]!);
    const v = c.map((value, axis) => value - a[axis]!);
    const cross = [
      u[1]! * v[2]! - u[2]! * v[1]!,
      u[2]! * v[0]! - u[0]! * v[2]!,
      u[0]! * v[1]! - u[1]! * v[0]!,
    ];
    for (const index of indices)
      assert.ok(
        cross.reduce(
          (sum, value, axis) => sum + value * mesh.normals[index]![axis]!,
          0,
        ) > 0,
      );
    volume += a.reduce((sum, value, axis) => sum + value * cross[axis]!, 0) / 6;
    for (let edge = 0; edge < 3; edge++) {
      const endpoints = [
        mesh.positions[indices[edge]!]!.join(","),
        mesh.positions[indices[(edge + 1) % 3]!]!.join(","),
      ]
        .sort()
        .join("|");
      edges.set(endpoints, (edges.get(endpoints) ?? 0) + 1);
    }
  }
  assert.ok([...edges.values()].every((count) => count === 2));
  assert.ok(Math.abs(volume - 0.5) < 1e-6);
  assert.deepEqual(
    new Set(mesh.positions.map((point) => point[2])),
    new Set([-0.25, 0.25]),
  );
  mesh.weights.forEach((weights) =>
    assert.equal(
      weights.reduce((sum, value) => sum + value, 0),
      1,
    ),
  );
});
