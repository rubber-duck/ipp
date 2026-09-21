/** Rotation math checks; real core/transport/rendering coverage is tests/render. */
import assert from "node:assert/strict";
import test from "node:test";
import { Transform, type TransformProps } from "../src/components.js";

type Vec3 = readonly [number, number, number];

// Rotate about independent axes rather than repeat the component's quaternion formula.
function rotateEuler([x, y, z]: Vec3, [rx, ry, rz]: Vec3): Vec3 {
  const zx = x * Math.cos(rz) - y * Math.sin(rz);
  const zy = x * Math.sin(rz) + y * Math.cos(rz);
  const yx = zx * Math.cos(ry) + z * Math.sin(ry);
  const yz = -zx * Math.sin(ry) + z * Math.cos(ry);
  return [
    yx,
    zy * Math.cos(rx) - yz * Math.sin(rx),
    zy * Math.sin(rx) + yz * Math.cos(rx),
  ];
}

function rotateQuaternion(
  { qx = 0, qy = 0, qz = 0, qw = 1 }: TransformProps,
  [x, y, z]: Vec3,
): Vec3 {
  const tx = 2 * (qy * z - qz * y);
  const ty = 2 * (qz * x - qx * z);
  const tz = 2 * (qx * y - qy * x);
  return [
    x + qw * tx + qy * tz - qz * ty,
    y + qw * ty + qz * tx - qx * tz,
    z + qw * tz + qx * ty - qy * tx,
  ];
}

test("Transform Euler conversion matches intrinsic XYZ axis rotations", () => {
  const rotations: Vec3[] = [
    [Math.PI / 2, 0, 0],
    [0, Math.PI / 2, 0],
    [0, 0, Math.PI / 2],
    [0.37, -0.61, 1.12],
    [-2.1, 0.8, -0.4],
  ];
  const basis: Vec3[] = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ];
  for (const angles of rotations) {
    const [rx, ry, rz] = angles;
    const result = Transform({ rx, ry, rz, x: 7, bound: true });
    assert.equal(result.type, "ipp-transform");
    assert.equal(result.props.x, 7);
    assert.equal(result.props.bound, true);
    for (const key of ["rx", "ry", "rz"])
      assert.ok(!Object.hasOwn(result.props, key));
    for (const key of ["qx", "qy", "qz", "qw"])
      assert.ok(Object.hasOwn(result.props, key));
    for (const axis of basis) {
      const expected = rotateEuler(axis, angles);
      const actual = rotateQuaternion(result.props, axis);
      for (let i = 0; i < 3; i += 1) {
        assert.ok(
          Math.abs(actual[i]! - expected[i]!) < 1e-12,
          `angles ${angles}, axis ${axis}: ${actual} != ${expected}`,
        );
      }
    }
  }
});

test("Transform distinguishes an explicit zero Euler rotation from omitted rotation", () => {
  assert.deepEqual(Transform({ rx: 0 }).props, { qx: 0, qy: 0, qz: 0, qw: 1 });
  assert.deepEqual(Transform({ rx: undefined, x: 2 }).props, { x: 2 });
  assert.deepEqual(Transform({ ry: undefined, qz: 0.3 }).props, { qz: 0.3 });
  assert.deepEqual(Transform({ rx: 0, qw: undefined }).props, {
    qx: 0,
    qy: 0,
    qz: 0,
    qw: 1,
  });
  const quarterTurn = Transform({ ry: Math.PI / 2 }).props;
  assert.equal(quarterTurn.qx, 0);
  assert.equal(quarterTurn.qz, 0);
  assert.ok(Math.abs(quarterTurn.qy! - Math.SQRT1_2) < 1e-12);
  assert.ok(Math.abs(quarterTurn.qw! - Math.SQRT1_2) < 1e-12);
});

test("Transform rejects every mixed Euler/quaternion field combination, including zero", () => {
  for (const euler of ["rx", "ry", "rz"]) {
    for (const quaternion of ["qx", "qy", "qz", "qw"]) {
      assert.throws(
        () => Transform({ [euler]: 0, [quaternion]: 0 }),
        /cannot mix/,
      );
    }
  }
});

test("Transform rejects invalid Euler inputs instead of coercing them", () => {
  for (const field of ["rx", "ry", "rz"]) {
    for (const value of [NaN, Infinity, -Infinity, null, "1"]) {
      assert.throws(() => Transform({ [field]: value }), /finite radians/);
    }
  }
});
