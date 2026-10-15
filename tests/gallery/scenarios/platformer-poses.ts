import assert from "node:assert/strict";
import type { AnimationPlaybackControl, Inspection } from "@ipp/client";
import type { PlatformerPoseOrigins } from "../../../examples/demos/scenes/platformer/pose-origins.js";
import type { PlatformerMode } from "../../../examples/demos/scenes/platformer/session.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface PlatformerPoseDriver {
  readonly poses: PlatformerPoseOrigins;
  state(): Promise<Inspection>;
  action(name: string, args?: unknown): Promise<unknown>;
  control(id: bigint, value: AnimationPlaybackControl): Promise<void>;
  reference(pose: PlatformerPoseOrigins["baseline"] | null): Promise<void>;
  capture(name: string): Promise<RgbaImage>;
}

function layers(state: Inspection) {
  const controllers = state.controllers ?? [];
  const origin = controllers.find((entry) =>
    entry.description.drivers.some((driver) =>
      driver.source.includes("platformer-pose-origin-"),
    ),
  );
  const gait = controllers.find((entry) =>
    entry.description.drivers.some(
      (driver) =>
        driver.source.startsWith("https://platformer.ipp.invalid/") &&
        "joints" in driver.property,
    ),
  );
  assert.ok(origin && gait, "both ordered pose layers are live");
  assert.ok(origin.id < gait.id, "pose origin composes before gait rotations");
  return [origin, gait] as const;
}

function difference(a: RgbaImage, b: RgbaImage) {
  assert.equal(a.width, b.width);
  assert.equal(a.height, b.height);
  let changed = 0;
  let total = 0;
  // Exclude controls and compare the robot's silhouette and articulated detail.
  for (let y = Math.round(a.height * 0.36); y < a.height * 0.8; y++) {
    for (let x = Math.round(a.width * 0.3); x < a.width * 0.7; x++) {
      const at = (y * a.width + x) * 4;
      const delta = Math.max(
        ...[0, 1, 2].map((channel) =>
          Math.abs(a.pixels[at + channel]! - b.pixels[at + channel]!),
        ),
      );
      if (delta > 12) changed++;
      total++;
    }
  }
  return changed / total;
}

async function settle(driver: PlatformerPoseDriver) {
  const initial = layers(await driver.state());
  await Promise.all(
    initial.map((layer) =>
      driver.control(layer.id, { action: "playAtSpeed", speed: 0 }),
    ),
  );
  const deadline = performance.now() + 10_000;
  let maximumSkew = 0;
  for (;;) {
    const pair = layers(await driver.state());
    if (pair.every((layer) => !layer.transition)) break;
    if (pair.every((layer) => layer.transition)) {
      maximumSkew = Math.max(
        maximumSkew,
        Math.abs(pair[0].transition!.elapsed - pair[1].transition!.elapsed),
      );
    }
    assert.ok(performance.now() < deadline, "paired source-pose fades settle");
  }
  await Promise.all(
    initial.map((layer) => driver.control(layer.id, { action: "pause" })),
  );
  assert.ok(
    maximumSkew < 0.08,
    `paired fade clock skew stays below 80 ms (${maximumSkew}s)`,
  );
  return maximumSkew;
}

/** Compare animation with absolute poses sampled directly from the packed Blender rig. */
export async function exercisePlatformerPoses(driver: PlatformerPoseDriver) {
  await driver.action("playback", false);
  await driver.action("resetCamera");
  await driver.action("reset");
  let maximumSkew = 0;
  let maximumImageDifference = 0;
  for (const mode of ["walk", "run", "crawl"] as const) {
    await driver.action("setMode", mode);
    maximumSkew = Math.max(maximumSkew, await settle(driver));
    for (const [index, sample] of driver.poses.references[mode].entries()) {
      let [origin, gait] = layers(await driver.state());
      await driver.control(origin.id, { action: "seek", time: 1 });
      await driver.control(gait.id, { action: "seek", time: sample.time });
      const actual = await driver.capture(`pose-${mode}-${index}-animated`);
      await Promise.all(
        [origin, gait].map((layer) =>
          driver.control(layer.id, { action: "stop" }),
        ),
      );
      try {
        await driver.reference(sample.pose);
        const expected = await driver.capture(`pose-${mode}-${index}-blender`);
        const error = difference(actual, expected);
        maximumImageDifference = Math.max(maximumImageDifference, error);
        assert.ok(
          error < 0.002,
          `${mode} sample ${index} agrees with Blender's absolute pose (${error})`,
        );
      } finally {
        await driver.reference(null);
      }
      [origin, gait] = layers(await driver.state());
      await Promise.all(
        [origin, gait].map((layer) =>
          driver.control(layer.id, { action: "playAtSpeed", speed: 0 }),
        ),
      );
      await Promise.all(
        [origin, gait].map((layer) =>
          driver.control(layer.id, { action: "pause" }),
        ),
      );
      await driver.control(origin.id, { action: "seek", time: 1 });
      await driver.control(gait.id, { action: "seek", time: sample.time });
    }
  }
  // Paused selection holds both contributions; resume/interrupt in both directions.
  for (const mode of [
    "walk",
    "crawl",
  ] as const satisfies readonly PlatformerMode[]) {
    await driver.action("setMode", mode === "walk" ? "crawl" : "walk");
    maximumSkew = Math.max(maximumSkew, await settle(driver));
    await driver.action("setMode", mode);
    const held = layers(await driver.state());
    assert.ok(held.every((layer) => layer.state === "paused"));
    const initial = await driver.capture(`fade-${mode}-paused`);
    const later = await driver.capture(`fade-${mode}-still-paused`);
    assert.equal(
      difference(initial, later),
      0,
      "paused mode selection leaves robot pose steady",
    );
    await Promise.all(
      held.map((layer) =>
        driver.control(layer.id, { action: "playAtSpeed", speed: 0 }),
      ),
    );
    const deadline = performance.now() + 10_000;
    while (layers(await driver.state())[0].transition?.elapsed! < 0.12) {
      assert.ok(performance.now() < deadline);
    }
    await driver.action("playback", false);
    const before = await driver.capture(`fade-${mode}-midpoint`);
    await driver.action("setMode", mode === "walk" ? "crawl" : "walk");
    const after = await driver.capture(`fade-${mode}-interrupted`);
    assert.ok(
      difference(before, after) < 0.002,
      "midfade retarget holds its composite pose while paused",
    );
    maximumSkew = Math.max(maximumSkew, await settle(driver));
  }
  await driver.action("setMode", "crawl");
  await driver.action("reset");
  const reset = layers(await driver.state());
  assert.equal(reset[0].time, 1, "reset retains the selected gait origin");
  assert.equal(
    reset[1].time,
    0,
    "reset returns the gait clock to its first pose",
  );
  assert.ok(
    reset.every((layer) => !layer.transition),
    "reset cancels both in-progress pose fades",
  );
  await driver.capture("pose-reset-during-fade");
  return { maximumSkew, maximumImageDifference };
}
