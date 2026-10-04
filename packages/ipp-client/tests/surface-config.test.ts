import assert from "node:assert/strict";
import test from "node:test";
import {
  curvedSurfaceFromRadius,
  cylinderSurfaceFromAngles,
  sphereSurfaceFromAngles,
  surfaceCurvatureFromRadius,
} from "../src/surface-config.js";

test("radius and facing recipes store signed curvature and signed spacing", () => {
  assert.equal(surfaceCurvatureFromRadius(4), 0.25);
  assert.equal(surfaceCurvatureFromRadius(4, "inside"), -0.25);
  assert.deepEqual(
    curvedSurfaceFromRadius({
      width: 2,
      height: 1,
      radius: 4,
      facing: "inside",
      layer_spacing: -0.03,
    }),
    { width: 2, height: 1, curvature: -0.25, layer_spacing: -0.03 },
  );
});

test("angular recipes describe cylinder arc width and sphere chart extents", () => {
  assert.deepEqual(
    cylinderSurfaceFromAngles({ radius: 3, angle: Math.PI / 2, height: 2 }),
    { width: (3 * Math.PI) / 2, height: 2, curvature: 1 / 3, layer_spacing: 0 },
  );
  assert.deepEqual(
    sphereSurfaceFromAngles({
      radius: 2,
      angle_x: 0.6,
      angle_y: 0.4,
      facing: "inside",
    }),
    { width: 1.2, height: 0.8, curvature: -0.5, layer_spacing: 0 },
  );
});

test("recipes reject nonfinite or nonpositive dimensions and radius", () => {
  for (const radius of [0, -1, Infinity, NaN, Number.MIN_VALUE])
    assert.throws(() => surfaceCurvatureFromRadius(radius), RangeError);
  assert.throws(
    () => curvedSurfaceFromRadius({ width: 0, height: 1, radius: 1 }),
    RangeError,
  );
  assert.throws(
    () =>
      curvedSurfaceFromRadius({
        width: 1,
        height: 1,
        radius: 1,
        layer_spacing: Infinity,
      }),
    RangeError,
  );
  assert.throws(
    () => cylinderSurfaceFromAngles({ radius: 1, angle: 0, height: 1 }),
    RangeError,
  );
  assert.throws(
    () => sphereSurfaceFromAngles({ radius: 1, angle_x: 1, angle_y: NaN }),
    RangeError,
  );
});
