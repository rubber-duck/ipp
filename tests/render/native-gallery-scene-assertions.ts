import assert from "node:assert/strict";
import { join } from "node:path";
import type { Inspection } from "@ipp/client";
import type { RgbaImage } from "../../tools/shared-host/images.js";
import type { NativeGalleryDriver } from "./native-gallery-driver.js";

async function inspect(driver: NativeGalleryDriver, signal: AbortSignal) {
  const { report } = await driver.call("inspect", [], signal);
  return {
    options: report.options as Record<string, unknown>,
    state: report.state as unknown as Inspection,
  };
}

function entity(state: Inspection, name: string) {
  const found = state.entities.find(
    ({ metadata }) => metadata.symbolicId === name,
  );
  assert.ok(found, `${name} is acknowledged in the mounted World`);
  return found;
}

function field(state: Inspection, name: string, key: string) {
  const fields = entity(state, name).components.find(
    ({ fields }) => key in fields,
  )?.fields;
  assert.ok(fields, `${name} has ${key}`);
  return fields[key];
}

function close(actual: unknown, expected: number) {
  assert.equal(typeof actual, "number");
  assert.ok(
    Math.abs(Number(actual) - expected) < 0.0001,
    `${actual} matches ${expected}`,
  );
}

function visible(frame: RgbaImage) {
  let pixels = 0;
  for (let at = 0; at < frame.pixels.length; at += 4) {
    if (
      [0, 1, 2].some(
        (axis) => Math.abs(frame.pixels[at + axis]! - frame.pixels[axis]!) > 20,
      )
    )
      pixels++;
  }
  assert.ok(
    pixels > frame.width * frame.height * 0.005,
    "scene content covers a meaningful completed frame region",
  );
}

function changed(before: RgbaImage, after: RgbaImage) {
  assert.equal(after.width, before.width);
  assert.equal(after.height, before.height);
  let pixels = 0;
  for (let at = 0; at < before.pixels.length; at += 4) {
    if (
      [0, 1, 2].some(
        (axis) =>
          Math.abs(before.pixels[at + axis]! - after.pixels[at + axis]!) > 20,
      )
    )
      pixels++;
  }
  assert.ok(
    pixels > before.width * before.height * 0.001,
    "live scene changes reach captured pixels",
  );
}

/** Scene behavior assertions reuse the driver regardless of how its Host is launched. */
export async function exerciseNativeGalleryScene(
  driver: NativeGalleryDriver,
  scene: string,
  directory: string,
  signal: AbortSignal,
): Promise<void> {
  const action = (name: string, args?: unknown) =>
    driver.call(
      "action",
      [name, ...(args === undefined ? [] : [JSON.stringify(args)])],
      signal,
    );
  const options = (patch: object) =>
    driver.call("options", [JSON.stringify(patch)], signal);
  const capture = (name: string) =>
    driver.capture(join(directory, `${scene}-${name}`), signal);
  if (scene === "lighting") {
    await action("pause");
    await action("configure", { field: "speed", value: 2 });
    await action("configure", { field: "looping", value: false });
    await action("seek", { time: 1.25 });
    const before = await capture("paused");
    await action("select", "lighting-cube");
    await action("updateObject", {
      id: "lighting-cube",
      patch: { position: [0, 1.6, 0.5], scale: 1.3 },
    });
    const verify = async () => {
      const current = await inspect(driver, signal);
      assert.equal(current.options.selected, "lighting-cube");
      close(field(current.state, "lighting-cube", "x"), 0);
      close(field(current.state, "lighting-cube", "y"), 1.6);
      close(field(current.state, "lighting-cube", "sx"), 1.3);
      const controllers = current.state.controllers ?? [];
      assert.equal(
        controllers.length,
        4,
        "each moving light and the skinned beam has a real controller",
      );
      for (const controller of controllers) {
        assert.equal(controller.state, "paused");
        close(controller.time, 1.25);
        close(controller.description.speed, 2);
        assert.equal(controller.description.looping, false);
      }
      const playback = current.options.animations as Record<
        string,
        Record<string, unknown>
      >;
      assert.equal(Object.keys(playback).length, 4);
      for (const setting of Object.values(playback)) {
        assert.equal(setting.state, "paused");
        assert.equal(setting.speed, 2);
        assert.equal(setting.looping, false);
        assert.equal(setting.time, 1.25);
      }
    };
    await verify();
    const edited = await capture("edited");
    visible(edited);
    changed(before, edited);
    await driver.call("reload", [], signal);
    await verify();
    visible(await capture("reloaded"));
  } else if (scene === "particles") {
    await options({
      presentation: "meshes",
      rate: 420,
      lifetime: 3,
      color: "#ff5500",
      size: 0.18,
    });
    await action("restart");
    const emitting = await capture("mesh-emission");
    visible(emitting);
    await action("toggleEmission");
    const verify = async () => {
      const current = await inspect(driver, signal);
      assert.equal(current.options.presentation, "meshes");
      assert.equal(current.options.emitting, false);
      assert.equal(current.options.restart, 1);
      assert.equal(current.options.color, "#ff5500");
      close(field(current.state, "particle-fountain", "rate"), 420);
      close(field(current.state, "particle-fountain", "lifetime"), 3);
      close(field(current.state, "particle-fountain", "size"), 0.18);
      assert.equal(field(current.state, "particle-fountain", "enabled"), false);
      close(field(current.state, "particle-fountain", "restart"), 1);
      const fountain = entity(current.state, "particle-fountain");
      assert.ok(
        fountain.components.some(
          ({ fields }) =>
            fields.source === "ipp://mesh/cube?width=1&height=1&length=1",
        ),
        "mesh particles use the declared cube resource",
      );
      assert.ok(
        !fountain.components.some(({ fields }) => "end_size" in fields),
        "switching presentation removes the sprite component",
      );
    };
    await verify();
    await driver.call("reload", [], signal);
    await verify();
    visible(await capture("reloaded"));
  } else if (scene === "gui") {
    const before = await capture("cyan");
    await options({ gain: 0.37, callsign: "NATIVE-9", autoscan: false });
    await action("setAccent", "amber");
    await action("toggleExplode");
    const verify = async () => {
      const current = await inspect(driver, signal);
      close(current.options.gain, 0.37);
      assert.equal(current.options.callsign, "NATIVE-9");
      assert.equal(current.options.autoscan, false);
      assert.equal(current.options.accent, "amber");
      assert.equal(current.options.exploded, true);
      close(
        field(current.state, "gallery-camera", "fov_y"),
        (23 * Math.PI) / 180,
      );
      assert.ok(
        Number(field(current.state, "gallery-camera", "x")) < -4,
        "initial/restored exploded mode must frame the inspection stack",
      );
      close(field(current.state, "gui-projector-light", "intensity"), 1.066);
      assert.ok(
        current.state.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-demo",
        ),
        "the dashboard is the actual attached runtime GUI",
      );
    };
    await verify();
    const settled = async () => {
      const deadline = performance.now() + 15_000;
      while (true) {
        const current = await inspect(driver, signal);
        if (
          Math.abs(
            Number(field(current.state, "gui-demo", "layer_spacing")) - 0.9,
          ) < 1e-4
        )
          return;
        if (performance.now() > deadline)
          throw new Error("GUI exploded spacing did not settle");
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
    };
    await settled();
    const amber = await capture("amber-exploded");
    visible(amber);
    changed(before, amber);
    await driver.call("reload", [], signal);
    await verify();
    await settled();
    visible(await capture("reloaded"));
  } else if (scene === "platformer") {
    await action("setMode", "run");
    await action("reverse");
    await action("playback", false);
    const verify = async () => {
      const current = await inspect(driver, signal);
      assert.equal(current.options.mode, "run");
      assert.equal(current.options.direction, -1);
      assert.equal(current.options.playing, false);
      const controllers = current.state.controllers ?? [];
      assert.equal(
        controllers.length,
        4,
        "route, gait, facing and orb are real mounted controllers",
      );
      assert.ok(controllers.every(({ state }) => state === "paused"));
      assert.ok(
        controllers.some(({ description }) => (description.speed ?? 1) < 0),
        "reverse changes acknowledged route playback",
      );
      assert.ok(
        current.state.resources.some(
          ({ source, status }) =>
            source.startsWith("https://platformer.ipp.invalid/") &&
            status === "loaded",
        ),
        "the saved scene loads its authored resources through the native adapter",
      );
    };
    await verify();
    visible(await capture("run-reversed-paused"));
    await driver.call("reload", [], signal);
    await verify();
    visible(await capture("reloaded"));
  }
}
