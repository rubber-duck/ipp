/** Seated yaw uses real view-qualified Host navigation and completed images. */
import assert from "node:assert/strict";
import type { CockpitDriver, CockpitState } from "./cockpit.js";
import { cockpitFields } from "../cockpit-oracle.js";

export function cockpitCamera(
  state: Pick<CockpitState, "scene" | "componentIds">,
) {
  return {
    pose: cockpitFields(
      state.scene,
      "cockpit-camera",
      state.componentIds.Transform!,
    ),
    projection: cockpitFields(
      state.scene,
      "cockpit-camera",
      state.componentIds.Camera!,
    ),
  };
}

export function assertSeatedYaw(
  before: ReturnType<typeof cockpitCamera>,
  after: ReturnType<typeof cockpitCamera>,
) {
  assert.deepEqual(
    after.projection,
    before.projection,
    "looking around preserves projection",
  );
  for (const name of ["x", "y", "z", "sx", "sy", "sz", "qx", "qz"])
    assert.equal(
      after.pose[name],
      before.pose[name],
      `looking around preserves ${name}`,
    );
  assert.ok(
    Math.abs(Number(after.pose.qy) - Number(before.pose.qy)) > 0.01,
    "horizontal drag rotates around Y",
  );
}

export async function exerciseCockpitCamera(driver: CockpitDriver) {
  await driver.options({
    lightsPlaying: false,
    atmospherePlaying: false,
    scanning: false,
  });
  const initial = await driver.state();
  assert.equal(initial.navigation, "yaw");
  const before = cockpitCamera(initial);
  const forward = await driver.capture("cockpit-camera-forward");
  await driver.navigate({ kind: "rotate", yaw: Math.PI / 2, pitch: 0.7 });
  const left = cockpitCamera(await driver.state());
  assertSeatedYaw(before, left);
  const turned = await driver.capture("cockpit-camera-left");
  let changed = 0;
  for (let i = 0; i < forward.pixels.length; i += 4)
    if (Math.abs(forward.pixels[i]! - turned.pixels[i]!) > 24) changed++;
  assert.ok(
    changed > forward.width * forward.height * 0.1,
    "turning reveals another part of the bridge",
  );
  for (const motion of [
    { kind: "rotate", yaw: 0, pitch: 1 },
    { kind: "pan", x: 0.3, y: 0.2 },
    { kind: "zoom", amount: 1 },
  ] as const)
    await driver.navigate(motion);
  assert.deepEqual(
    cockpitCamera(await driver.state()),
    left,
    "pitch, pan and zoom cannot move the seated camera",
  );
  await driver.navigate({ kind: "turn", yaw: -Math.PI, pitch: -0.5 });
  const right = cockpitCamera(await driver.state());
  assertSeatedYaw(before, right);
  await driver.capture("cockpit-camera-right");
  const focus = Number((await driver.state()).options.carousel);
  const drag = await driver.drag(3, "cockpit-header-3", -0.1);
  assert.ok(
    drag.some((state) => Number(state.options.carousel) > focus + 0.05),
    "pane dragging works after turning 90 degrees",
  );
  for (const state of drag) assert.deepEqual(cockpitCamera(state), right);
  await driver.action("resetCamera");
  assert.deepEqual(
    cockpitCamera(await driver.state()),
    before,
    "Reset view restores the forward view",
  );
  await driver.capture("cockpit-camera-reset");
  return { changedPixels: changed, before, left };
}
