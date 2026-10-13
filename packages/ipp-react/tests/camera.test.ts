/** CameraRig declaration shape; aiming itself is covered by core LookAt tests. */
import assert from "node:assert/strict";
import test from "node:test";
import { CameraRig } from "../src/camera.js";

interface ChildElement {
  readonly type: unknown;
  readonly props: Record<string, unknown>;
}

interface HostElement {
  readonly type: string;
  readonly props: Record<string, unknown>;
}

/** Resolve one component element to its host element, as the reconciler does. */
function host(child: ChildElement): HostElement {
  return typeof child.type === "function"
    ? (child.type as (props: Record<string, unknown>) => HostElement)(
        child.props,
      )
    : (child as HostElement);
}

function rigChildren(props: Parameters<typeof CameraRig>[0]): HostElement[] {
  const element = CameraRig(props) as unknown as {
    props: { children: ChildElement[] };
  };
  return element.props.children.map(host);
}

test("CameraRig declares a positioned perspective Camera with a point LookAt", () => {
  const children = rigChildren({
    id: "main",
    position: [0, 3, 8],
    lookAt: [0, 2, 0],
    near: 0.1,
    far: 50,
  });
  assert.equal(children.length, 3);
  const [transform, camera, lookAt] = children;
  assert.equal(transform!.type, "ipp-transform");
  assert.equal(transform!.props.x, 0);
  assert.equal(transform!.props.y, 3);
  assert.equal(transform!.props.z, 8);
  // No authored orientation: the runtime LookAt System owns aiming.
  assert.ok(!Object.hasOwn(transform!.props, "qx"));
  assert.equal(camera!.type, "ipp-camera");
  assert.equal(camera!.props.projection, 0);
  assert.equal(camera!.props.near, 0.1);
  assert.equal(camera!.props.far, 50);
  assert.equal(lookAt!.type, "ipp-look-at");
  assert.equal(lookAt!.props.has_point, true);
  assert.equal(lookAt!.props.point_x, 0);
  assert.equal(lookAt!.props.point_y, 2);
  assert.equal(lookAt!.props.point_z, 0);
});

test("CameraRig declares orthographic projection from orthoHeight", () => {
  const children = rigChildren({
    id: "ortho",
    position: [0, 0, 10],
    lookAt: [0, 0, 0],
    orthoHeight: 6,
  });
  const camera = children[1]!;
  assert.equal(camera.props.projection, 1);
  assert.equal(camera.props.ortho_height, 6);
  assert.ok(!Object.hasOwn(camera.props, "fov_y"));
});

test("CameraRig with target composes an entity LookAt", () => {
  const target = 42n;
  const children = rigChildren({
    id: "tracking",
    position: [4, 2, 4],
    target,
  });
  assert.equal(children.length, 3);
  const lookAt = children[2]!;
  assert.equal(lookAt.type, "ipp-look-at");
  assert.equal(lookAt.props.target, target);
  assert.ok(!Object.hasOwn(lookAt.props, "has_point"));
});

test("CameraRig without an aim declares no LookAt", () => {
  const children = rigChildren({ id: "fixed", position: [0, 1, 6] });
  assert.equal(children.length, 2);
});

test("CameraRig rejects conflicting or invalid props", () => {
  assert.throws(
    () =>
      CameraRig({
        id: "x",
        position: [0, 0, 5],
        lookAt: [0, 0, 0],
        target: 1n,
      }),
    /either lookAt or target/,
  );
  assert.throws(
    () =>
      CameraRig({ id: "x", position: [0, 0, 5], lookAt: [0, 0, 0], fovY: 4 }),
    RangeError,
  );
  assert.throws(
    () =>
      CameraRig({
        id: "x",
        position: [0, 0, 5],
        lookAt: [0, 0, 0],
        fovY: 1,
        orthoHeight: 4,
      }),
    /either orthoHeight or fovY/,
  );
  assert.throws(
    () => CameraRig({ id: "x", position: [0, 0, Number.NaN] }),
    RangeError,
  );
});
