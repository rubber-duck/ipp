import assert from "node:assert/strict";
import type { Inspection } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface PlatformerControlsState {
  readonly scene: Inspection;
  readonly panel: Inspection;
  readonly options: Readonly<Record<string, unknown>>;
  readonly componentIds: Readonly<Record<string, number>>;
}

/** Inputs and observations stay independent of Host launch and transport. */
export interface PlatformerControlsDriver {
  state(): Promise<PlatformerControlsState>;
  capture(name: string): Promise<RgbaImage>;
  press(id: string): Promise<void>;
  rotate(yaw: number): Promise<void>;
  action(name: string, args?: unknown): Promise<unknown>;
  advance(seconds: number): Promise<void>;
}

function entity(state: Inspection, id: string) {
  const found = state.entities.find(
    (entry) => entry.metadata.symbolicId === id,
  );
  assert.ok(found, `${id} exists in its declared World`);
  return found;
}

function fields(state: Inspection, id: string, component: number) {
  const found = entity(state, id).components.find(
    (entry) => entry.component === component,
  );
  assert.ok(found, `${id} has ${component}`);
  return found.fields;
}

function visible(frame: RgbaImage) {
  let light = 0;
  for (let y = Math.round(frame.height * 0.12); y < frame.height * 0.65; y++) {
    for (let x = Math.round(frame.width * 0.25); x < frame.width * 0.75; x++) {
      const at = (y * frame.width + x) * 4;
      if (
        frame.pixels[at]! > 135 &&
        frame.pixels[at + 1]! > 150 &&
        frame.pixels[at + 2]! > 155
      )
        light++;
    }
  }
  assert.ok(
    light > 300,
    "completed central frame contains the robot and readable control ink",
  );
}

/** Independent intended geometry: font 16 controls inside a 420 × 160 canvas. */
function movementGeometry(state: PlatformerControlsState) {
  const boxes = [
    "platformer-reverse",
    "platformer-pause",
    "platformer-reset",
  ].map((id) => fields(state.panel, id, state.componentIds.CanvasBounds!));
  for (const box of boxes) {
    assert.ok(
      Math.abs(Number(box.height) - 36) < 0.1,
      "movement actions retain the full control height",
    );
    assert.ok(
      Number(box.x) >= 24 - 0.1 &&
        Number(box.x) + Number(box.width) <= 420 - 24 + 0.1,
      "movement actions retain horizontal container insets",
    );
    assert.ok(
      Number(box.y) >= 24 &&
        Number(box.y) + Number(box.height) <= 160 - 24 + 0.1,
      "movement actions fit above the bottom container inset",
    );
    assert.ok(
      Math.abs(Number(box.width) - Number(boxes[0]!.width)) < 0.1,
      "movement actions have equal widths",
    );
  }
  const surface = fields(
    state.scene,
    "platformer-movement-surface",
    state.componentIds.FlatSurface!,
  );
  const placement = fields(
    state.scene,
    "platformer-movement-facing",
    state.componentIds.Transform!,
  );
  assert.ok(
    Number(placement.y) - Number(surface.height) / 2 > 2.2,
    "resized panel still clears the standing robot head",
  );
}

export async function exercisePlatformerControls(
  driver: PlatformerControlsDriver,
) {
  const option = async (name: string, expected: unknown) => {
    const deadline = performance.now() + 20_000;
    for (;;) {
      const current = (await driver.state()).options[name];
      if (current === expected) return current;
      assert.ok(
        performance.now() < deadline,
        `${name} becomes ${String(expected)} after routed input`,
      );
      await driver.advance(0.03);
    }
  };
  await driver.action("playback", false);
  await driver.action("resetCamera");
  await driver.action("reset");
  await driver.action("setMode", "walk");
  const before = await driver.state();
  assert.ok(
    !before.scene.entities.some(
      (entry) => entry.metadata.symbolicId === "platformer-orb",
    ),
    "decorative orb is absent from the robot demo",
  );
  assert.equal(
    before.scene.controllers?.length,
    4,
    "only route, pose origin, gait and facing controllers remain",
  );
  const root = entity(before.scene, "platformer-root");
  const facing = entity(before.scene, "platformer-movement-facing");
  const camera = entity(before.scene, "platformer-camera");
  assert.equal(
    facing.link.parent,
    root.id,
    "controls follow the robot entity transform through native hierarchy",
  );
  assert.ok(
    !facing.components.some(
      (entry) => entry.component === before.componentIds.ParentJoint,
    ),
    "controls inherit no animated joint",
  );
  const placement = fields(
    before.scene,
    "platformer-movement-facing",
    before.componentIds.Transform!,
  );
  assert.deepEqual(
    [placement.x, placement.y, placement.z],
    [0, 2.75, 0],
    "panel clears the standing head above the robot entity origin",
  );
  assert.equal(
    fields(
      before.scene,
      "platformer-movement-facing",
      before.componentIds.LookAt!,
    ).target,
    camera.id,
  );
  assert.ok(
    !camera.components.some(
      (entry) => entry.component === before.componentIds.LookAt,
    ),
    "the camera is a valid unconstrained aiming target",
  );
  const controls = [
    "platformer-mode/walk",
    "platformer-mode/run",
    "platformer-mode/crawl",
    "platformer-reverse",
    "platformer-pause",
    "platformer-reset",
  ];
  const initialIds = controls.map((id) => entity(before.panel, id).id);
  visible(await driver.capture("robot-root-controls"));
  movementGeometry(await driver.state());

  await exercisePlatformerGaitSelection(driver);
  await driver.press("platformer-reverse");
  assert.equal(await option("direction", -1), -1);
  await driver.press("platformer-pause");
  assert.equal(await option("playing", true), true);
  await driver.advance(0.5);
  await driver.action("playback", false);
  visible(await driver.capture("robot-running-reversed"));

  await driver.rotate(0.42);
  visible(await driver.capture("robot-controls-camera-rotated"));
  await driver.press("platformer-mode/crawl");
  assert.equal(
    await option("mode", "crawl"),
    "crawl",
    "projected input stays usable after camera rotation",
  );
  await driver.action("playback", true);
  await driver.advance(0.6);
  await driver.action("playback", false);
  visible(await driver.capture("robot-crawl-root-controls"));
  await driver.press("platformer-reset");
  assert.equal(await option("direction", 1), 1);

  await driver.action("skin", "cybertron");
  const cybertron = await driver.state();
  assert.deepEqual(
    controls.map((id) => entity(cybertron.panel, id).id),
    initialIds,
    "skin changes preserve control identities",
  );
  visible(await driver.capture("robot-controls-cybertron"));
  movementGeometry(await driver.state());
  await driver.action("skin", "glass");
  visible(await driver.capture("robot-controls-glass-restored"));
  assert.equal(
    (await driver.state()).options.mode,
    "crawl",
    "skin changes preserve gait selection",
  );
  assert.equal(
    (await driver.state()).options.direction,
    1,
    "skin changes preserve reset playback intent",
  );
}

/** Returning to earlier choices must keep native selection and actual playback aligned. */
export async function exercisePlatformerGaitSelection(
  driver: Pick<PlatformerControlsDriver, "state" | "press" | "advance">,
) {
  for (const mode of ["run", "walk", "crawl", "run", "walk", "run"]) {
    await driver.press(`platformer-mode/${mode}`);
    const deadline = performance.now() + 20_000;
    while ((await driver.state()).options.mode !== mode) {
      assert.ok(
        performance.now() < deadline,
        `mode becomes ${mode} after routed input`,
      );
      await driver.advance(0.03);
    }
    const state = await driver.state();
    assert.deepEqual(
      ["walk", "run", "crawl"].filter(
        (key) =>
          fields(
            state.panel,
            `platformer-mode/${key}`,
            state.componentIds.GuiButton!,
          ).selected,
      ),
      [mode],
      "the native selected button agrees with playback after returning to a gait",
    );
    assert.ok(
      state.scene.controllers?.some(({ description }) =>
        description.drivers.some(({ source }) =>
          String(source).includes(`platformer-pose-origin-${mode}`),
        ),
      ),
      "the selected gait's actual pose controller is active",
    );
  }
}
