import assert from "node:assert/strict";
import type { Inspection } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

type Mode = "walk" | "run" | "crawl";
export interface PlatformerStreamingDriver {
  state(): Promise<Inspection>;
  mode(mode: Mode): Promise<void>;
  playing(playing: boolean): Promise<void>;
  reverse(): Promise<void>;
  seek(controller: bigint, time: number): Promise<void>;
  play(controller: bigint, speed?: number): Promise<void>;
  pause(controller: bigint): Promise<void>;
  capture(name: string): Promise<RgbaImage>;
  release(): Promise<void>;
  requests(): Promise<Record<Mode, number>>;
  close(): Promise<void>;
  leave(): Promise<void>;
  readonly clips: Record<Mode, { source: string; duration: number }>;
}
function entity(state: Inspection, id: string) {
  const found = state.entities.find(
    (entry) => entry.metadata.symbolicId === id,
  );
  assert.ok(found, `${id} is present`);
  return found;
}
function pose(state: Inspection, id: string, fields: readonly string[]) {
  const component = entity(state, id).components.find(
    (entry) => "qx" in entry.fields,
  );
  assert.ok(component, `${id} has its authored Transform`);
  return fields.map((field) => Number(component.fields[field]));
}
const position = (state: Inspection) =>
  pose(state, "platformer-root", ["x", "y", "z"]);
const rotation = (state: Inspection) =>
  pose(state, "platformer-rig", ["qx", "qy", "qz", "qw"]);
const distance = (left: number[], right: number[]) =>
  Math.hypot(...left.map((value, i) => value - right[i]!));
function route(state: Inspection) {
  const root = entity(state, "platformer-root").id;
  const controller = state.controllers?.find((entry) =>
    entry.description.drivers.some((driver) => driver.target === root),
  );
  assert.ok(controller, "authored route controller is live");
  return controller;
}
function gait(
  state: Inspection,
  driver: PlatformerStreamingDriver,
  mode: Mode,
) {
  return state.controllers?.find((entry) =>
    entry.description.drivers.some(
      (binding) => binding.source === driver.clips[mode].source,
    ),
  );
}
function facing(state: Inspection, driver: PlatformerStreamingDriver) {
  const rig = entity(state, "platformer-rig").id;
  const sources = new Set(
    Object.values(driver.clips).map((clip) => clip.source),
  );
  const found = state.controllers?.find((entry) =>
    entry.description.drivers.some(
      (binding) =>
        binding.target === rig &&
        "component" in binding.property &&
        !sources.has(binding.source),
    ),
  );
  assert.ok(found, "local facing controller is live");
  return found;
}
async function wait(
  driver: PlatformerStreamingDriver,
  predicate: (state: Inspection) => boolean,
) {
  const deadline = performance.now() + 20_000;
  for (;;) {
    const state = await driver.state();
    if (predicate(state)) return state;
    assert.ok(
      performance.now() < deadline,
      "robot observation reaches its expected state",
    );
  }
}
function changed(left: RgbaImage, right: RgbaImage) {
  assert.equal(left.width, right.width);
  assert.equal(left.height, right.height);
  let pixels = 0;
  for (let y = Math.round(left.height * 0.43); y < left.height * 0.75; y++) {
    for (let x = Math.round(left.width * 0.25); x < left.width * 0.75; x++) {
      const at = (y * left.width + x) * 4;
      if (
        [0, 1, 2].some(
          (channel) =>
            Math.abs(left.pixels[at + channel]! - right.pixels[at + channel]!) >
            20,
        )
      )
        pixels++;
    }
  }
  assert.ok(pixels > 50, `articulated robot changes ${pixels} central pixels`);
}
function visible(frame: RgbaImage) {
  let ink = 0;
  for (let at = 0; at < frame.pixels.length; at += 4) {
    if (
      frame.pixels[at]! > 135 &&
      frame.pixels[at + 1]! > 150 &&
      frame.pixels[at + 2]! > 155
    )
      ink++;
  }
  assert.ok(ink > 300, "completed GLES frame contains the robot and track");
}

export async function exercisePlatformerContinuity(
  driver: PlatformerStreamingDriver,
) {
  const initial = await driver.state();
  const walk = gait(initial, driver, "walk")!;
  assert.equal(walk.state, "playing");
  const pending = driver.mode("run");
  void pending.catch(() => {});
  const loading = await wait(
    driver,
    (state) =>
      gait(state, driver, "run")?.state === "stopped" &&
      state.resources.some(
        (resource) =>
          resource.source === driver.clips.run.source &&
          resource.status !== "loaded",
      ),
  );
  assert.equal(
    gait(loading, driver, "walk")?.id,
    walk.id,
    "cold preparation preserves outgoing controller identity",
  );
  await wait(
    driver,
    (state) =>
      distance(position(state), position(loading)) > 0.08 &&
      gait(state, driver, "walk")!.time !== gait(loading, driver, "walk")!.time,
  );
  await driver.pause(route(loading).id);
  await driver.pause(facing(loading, driver).id);
  const isolated = await driver.state();
  const first = await driver.capture("pending-walk-a");
  visible(first);
  await wait(driver, (state) => {
    const elapsed =
      (gait(state, driver, "walk")!.time -
        gait(isolated, driver, "walk")!.time +
        driver.clips.walk.duration) %
      driver.clips.walk.duration;
    return elapsed > 0.16 && elapsed < 0.45;
  });
  const second = await driver.capture("pending-walk-b");
  changed(first, second);
  assert.ok(
    distance(position(await driver.state()), position(isolated)) < 0.001,
    "gait-only images exclude route translation",
  );
  await driver.playing(false);
  assert.equal(
    gait(await driver.state(), driver, "walk")!.state,
    "paused",
    "pause remains responsive while the incoming clip is pending",
  );
  await driver.release();
  await pending;
  const incoming = await driver.state();
  assert.equal(gait(incoming, driver, "run")!.id, walk.id);
  assert.equal(gait(incoming, driver, "run")!.transition?.easing, "smoothstep");
  await driver.playing(true);
  await wait(
    driver,
    (state) => gait(state, driver, "run")?.transition === undefined,
  );
  await driver.mode("walk");
  await wait(
    driver,
    (state) => gait(state, driver, "walk")?.transition === undefined,
  );
  assert.deepEqual(
    await driver.requests(),
    { walk: 1, run: 1, crawl: 0 },
    "reselection after outgoing demand ends reuses the Host clip cache",
  );
}

export async function exercisePlatformerCancellation(
  driver: PlatformerStreamingDriver,
  outcome: "newest" | "closing" | "leaving",
) {
  const pending = driver.mode("run");
  void pending.catch(() => {});
  await wait(
    driver,
    (state) => gait(state, driver, "run")?.state === "stopped",
  );
  if (outcome === "closing" || outcome === "leaving") {
    if (outcome === "closing") await driver.close();
    else await driver.leave();
    await pending;
    await driver.release();
  } else {
    await driver.mode("crawl");
    await pending;
    const state = await driver.state();
    assert.equal(
      state.controllers?.length,
      4,
      "superseded preparation releases its controller",
    );
    assert.equal(gait(state, driver, "run"), undefined);
    assert.equal(
      gait(state, driver, "crawl")!.state,
      "playing",
      "newest gait commits while previous bytes remain gated",
    );
    visible(await driver.capture("newest-crawl"));
    await driver.release();
    assert.equal(
      gait(await driver.state(), driver, "run"),
      undefined,
      "late producer completion cannot revive the superseded gait",
    );
  }
}

/** Independent authored course oracle: coordinates/speeds from Blender's route fixture. */
export async function exercisePlatformerRoute(
  driver: PlatformerStreamingDriver,
) {
  await driver.playing(false);
  const initial = await driver.state();
  const root = route(initial);
  const walk = gait(initial, driver, "walk")!;
  const phase = 0.37;
  await driver.seek(walk.id, phase * driver.clips.walk.duration);
  const walkFrame = await driver.capture("walk-phase");
  for (const mode of ["run", "crawl"] as const) {
    await driver.mode(mode);
    const current = gait(await driver.state(), driver, mode)!;
    assert.equal(current.id, walk.id);
    assert.ok(
      Math.abs(current.time / driver.clips[mode].duration - phase) < 0.015,
      `${mode} preserves normalized gait phase`,
    );
    assert.equal(current.transition?.easing, "smoothstep");
    assert.ok((current.transition?.duration ?? 0) > 0);
    await driver.play(current.id);
    await wait(
      driver,
      (state) => gait(state, driver, mode)?.transition === undefined,
    );
    await driver.pause(current.id);
    await driver.seek(current.id, phase * driver.clips[mode].duration);
  }
  changed(walkFrame, await driver.capture("crawl-phase"));
  const rampStart = 27 / 2.2;
  for (const [name, time, expected] of [
    ["ramp", rampStart + Math.sqrt(45) / 4.4, [9, 2.5, -3]],
    ["upper", rampStart + Math.sqrt(45) / 2.2 + 3 / 2.2 + 9 / 2.2, [0, 4, -9]],
  ] as const) {
    await driver.seek(root.id, time);
    await wait(
      driver,
      (state) => distance(position(state), [...expected]) < 0.01,
    );
    visible(await driver.capture(`route-${name}`));
  }
  for (const [mode, speed] of [
    ["walk", 2.2],
    ["run", 4.4],
    ["crawl", 1],
  ] as const) {
    await driver.mode(mode);
    await driver.seek(root.id, 2);
    const before = await driver.state();
    await driver.playing(true);
    await wait(
      driver,
      (state) => route(state).time - route(before).time > 0.35,
    );
    await driver.playing(false);
    // Inspection collections span successive Host frames. Read both endpoints
    // while paused so entity coordinates and controller time describe one pose.
    const after = await driver.state();
    const elapsed = route(after).time - route(before).time;
    assert.ok(
      Math.abs((position(after)[0]! - position(before)[0]!) / elapsed - 2.2) <
        0.08,
      `${mode} route clip samples its independent spatial speed oracle`,
    );
    assert.ok(
      Math.abs(route(after).description.speed! - speed / 2.2) < 1e-6,
      `${mode} selects authored metres-per-second ratio`,
    );
  }
  await driver.seek(root.id, 4);
  await driver.reverse();
  await driver.playing(true);
  const before = await driver.state();
  const after = await wait(
    driver,
    (state) =>
      route(before).time - route(state).time > 0.25 &&
      facing(state, driver).transition === undefined,
  );
  await driver.playing(false);
  assert.ok(
    position(after)[0]! < position(before)[0]!,
    "backward travel follows the previous route point without wrapping",
  );
  assert.ok(
    (gait(after, driver, "crawl")!.description.speed ?? 0) > 0,
    "backward travel preserves forward gait articulation",
  );
  const [qx, qy, qz, qw] = rotation(await driver.state());
  assert.ok(
    Math.abs(Math.abs(qy!) - 1) < 0.01 &&
      Math.abs(qx!) < 0.01 &&
      Math.abs(qz!) < 0.01 &&
      Math.abs(qw!) < 0.01,
    "facing completes the authored local 180-degree Y rotation",
  );
  visible(await driver.capture("route-backward"));
}
