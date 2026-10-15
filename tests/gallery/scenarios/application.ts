/** Shared application assertions; process launch and transport belong to drivers. */
import assert from "node:assert/strict";
import type { GalleryOptions } from "../../../examples/demos/shared/scene.js";
import type { WorldReference } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface ApplicationState {
  readonly options: GalleryOptions;
  readonly root: WorldReference;
  readonly sceneWorlds: readonly WorldReference[];
  readonly worlds: readonly bigint[];
  readonly pendingCleanup: readonly unknown[];
  readonly failedCandidates: number;
  readonly error?: string | undefined;
  readonly toolbar: readonly number[];
}
export interface ApplicationDriver {
  state(): Promise<ApplicationState>;
  press(...ids: string[]): Promise<void>;
  action(name: string, value?: unknown): Promise<unknown>;
  capture(name: string): Promise<RgbaImage>;
  waitForOption(name: string, value: unknown): Promise<void>;
  wait(milliseconds: number): Promise<void>;
  retiredCapture(): Promise<unknown>;
}

function content(frame: RgbaImage) {
  const background = frame.pixels.slice(0, 3);
  let different = 0;
  // The lower scene area excludes the toolbar and its glass backing.
  for (let y = Math.floor(frame.height / 3); y < frame.height; y++)
    for (let x = 0; x < frame.width; x++) {
      const offset = (y * frame.width + x) * 4;
      if (
        background.some(
          (value, axis) => Math.abs(value - frame.pixels[offset + axis]!) > 24,
        )
      )
        different++;
    }
  assert.ok(
    different > frame.width * frame.height * 0.02,
    "the child scene contributes meaningful rendered pixels",
  );
}

export async function exerciseApplication(driver: ApplicationDriver) {
  const initial = await driver.state();
  assert.equal(initial.options.scene, "shapes");
  assert.equal(
    initial.worlds.length,
    2,
    "one persistent Canvas and one scene World",
  );
  content(await driver.capture("initial"));
  await driver.press("demo-scene-picker", "demo-scene-picker/options/lighting");
  await driver.waitForOption("scene", "lighting");
  const lighting = await driver.state();
  assert.deepEqual(
    lighting.root,
    initial.root,
    "scene selection preserves the Canvas World",
  );
  assert.notDeepEqual(lighting.sceneWorlds, initial.sceneWorlds);
  assert.equal(
    lighting.worlds.length,
    2,
    "the previous scene World is retired",
  );
  await assert.rejects(driver.retiredCapture(), /retired|abort/i);
  content(await driver.capture("lighting"));
  await driver.press(
    "demo-display",
    "demo-skin-picker",
    "demo-skin-picker/options/cybertron",
  );
  await driver.waitForOption("skin", "cybertron");
  const beforeScale = await driver.state();
  await driver.action("renderScale", 1.5);
  const scaled = await driver.capture("scaled");
  assert.equal(scaled.width, 1440);
  assert.equal(scaled.height, 960);
  const density = await driver.state();
  assert.deepEqual(
    density.toolbar,
    beforeScale.toolbar,
    "render scale preserves logical GUI bounds",
  );
  assert.deepEqual(density.root, initial.root);
  for (const [id, reason] of [
    ["particles", /mount failed/],
    ["gui", /source failed/],
  ] as const) {
    await assert.rejects(driver.action("scene", id), reason);
    const failed = await driver.state();
    assert.equal(
      failed.options.scene,
      "lighting",
      "failed replacement leaves the active scene usable",
    );
    assert.equal(
      failed.worlds.length,
      2,
      "failed candidate resources are retired",
    );
    assert.equal(failed.failedCandidates, 0);
    content(await driver.capture(`failed-${id}`));
  }
  await driver.action("scene", "platformer");
  await assert.rejects(driver.action("scene", "shapes"), /cleanup failed/);
  const pending = await driver.state();
  assert.equal(
    pending.options.scene,
    "shapes",
    "published replacement remains active while retirement is retried",
  );
  assert.equal(pending.pendingCleanup.length, 1);
  assert.equal(
    pending.worlds.length,
    3,
    "failed retirement remains owned and observable",
  );
  await driver.action("retryCleanup");
  const retried = await driver.state();
  assert.equal(retried.pendingCleanup.length, 0);
  assert.equal(retried.worlds.length, 2);
  assert.doesNotMatch(retried.error ?? "", /retired application fixture error/);
  // Healthy mounted scenes must survive the preparation deadline.
  await driver.wait(61_000);
  await driver.action("resetCamera");
  content(await driver.capture("healthy-after-deadline"));
}
