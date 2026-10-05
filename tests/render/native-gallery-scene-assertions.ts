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
  probeIndependent?: () => Promise<void>,
  readGuiPanel?: () => Promise<Inspection>,
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
    assert.ok(
      readGuiPanel,
      "native scanner inspects its actual child Canvas World",
    );
    const panel = readGuiPanel;
    const waitPanel = async (ready: (state: Inspection) => boolean) => {
      const deadline = performance.now() + 15_000;
      let current: Inspection;
      do {
        signal.throwIfAborted();
        current = await panel();
        if (ready(current)) return current;
      } while (performance.now() < deadline);
      throw new Error(
        `Scanner did not reach its expected completed frame: ${current.entities.map(({ metadata }) => metadata.symbolicId).join(", ")}`,
      );
    };
    const contains = (state: Inspection, name: string) =>
      state.entities.some(({ metadata }) => metadata.symbolicId === name);
    const login = await waitPanel((state) => contains(state, "gui-login"));
    assert.ok(!contains(login, "gui-workspace"));
    const passwordId = entity(login, "gui-password").id;
    await action("app", { method: "setPassword", args: ["native-secret"] });
    await waitPanel(
      (state) => field(state, "gui-password", "text") === "native-secret",
    );
    assert.equal(field(await panel(), "gui-password", "masked"), true);
    const masked = await capture("login-masked");
    await action("app", { method: "setReveal", args: [true] });
    const revealed = await waitPanel(
      (state) => field(state, "gui-password", "masked") === false,
    );
    assert.equal(entity(revealed, "gui-password").id, passwordId);
    assert.equal(field(revealed, "gui-password", "text"), "native-secret");
    changed(masked, await capture("login-revealed"));
    const enterWorkspace = async () => {
      const before = await panel();
      await action("app", { method: "login" });
      const workspace = await waitPanel(
        (state) =>
          contains(state, "gui-workspace") &&
          contains(state, "gui-scan") &&
          contains(state, "gui-pulse") &&
          !contains(state, "gui-login-card"),
      );
      assert.ok(
        workspace.time - before.time >= 3.5,
        "Host-owned time delivers the connection sequence before the workspace",
      );
      return workspace;
    };
    await enterWorkspace();
    assert.ok(contains(await panel(), "gui-scanner-close"));
    assert.ok(!contains(await panel(), "gui-logout"));
    await action("app", { method: "setRange", args: [1.5] });
    await waitPanel(
      (state) => Number(field(state, "gui-scan-range/slider", "value")) === 1.5,
    );
    assert.ok(
      String(field(await panel(), "gui-scanner-status", "text")).includes(
        "2 CONTACTS",
      ),
    );
    await action("app", { method: "setStrength", args: [0.5] });
    await action("app", { method: "charge" });
    await waitPanel(
      (state) => Number(field(state, "gui-charge-progress/fill", "width")) > 0,
    );
    await waitPanel(
      (state) =>
        String(field(state, "gui-charge-progress/readout", "text")) === "READY",
    );
    close(field(await panel(), "gui-charge-progress/fill", "width"), 329.5);
    await action("pulse");
    await waitPanel(
      (state) =>
        String(field(state, "gui-pulse-progress/readout", "text")) !== "IDLE",
    );
    await action("app", { method: "logout" });
    await waitPanel((state) => contains(state, "gui-login-card"));
    await enterWorkspace();
    close(field(await panel(), "gui-charge-progress/fill", "width"), 0);
    close(field(await panel(), "gui-scan-range/slider", "value"), 4);
    await action("app", { method: "settings", args: [true] });
    await action("app", { method: "settingsPage", args: ["scene"] });
    await waitPanel((state) => contains(state, "gui-scene-tree"));
    assert.equal(field(await panel(), "gui-settings", "visible"), true);
    await action("app", { method: "settings", args: [false] });
    await waitPanel(
      (state) => field(state, "gui-settings", "visible") === false,
    );
    const settled = async (spacing = 0.6) => {
      const deadline = performance.now() + 15_000;
      while (true) {
        const current = await inspect(driver, signal);
        if (
          Math.abs(
            Number(field(current.state, "gui-demo", "layer_spacing")) - spacing,
          ) < 1e-4
        )
          return;
        if (performance.now() > deadline)
          throw new Error("GUI exploded spacing did not settle");
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
    };
    // Inspect from the app's side view so its Canvas does not occlude the base.
    await options({ exploded: true, layerStep: 0.6, autoscan: false });
    await action("resetCamera");
    await settled();
    const before = await capture("cyan");
    const initial = (await inspect(driver, signal)).state;
    const identities = [
      "gui-projector-floor",
      "gui-projector-base",
      "gui-demo",
    ].map((name) => entity(initial, name).id);
    for (const selected of ["floor", "base", "stage"] as const) {
      await action("tuning", { method: "setFocus", args: [selected] });
      const current = (await inspect(driver, signal)).state;
      for (const part of ["floor", "base"] as const) {
        const gain = entity(current, `gui-projector-${part}`).components.find(
          ({ properties }) => properties?.gain,
        )?.properties?.gain;
        assert.equal(gain?.kind, "f32");
        if (gain?.kind === "f32")
          close(
            gain.value,
            selected === part || selected === "stage" ? 1.35 : 1,
          );
      }
      assert.deepEqual(
        ["gui-projector-floor", "gui-projector-base", "gui-demo"].map(
          (name) => entity(current, name).id,
        ),
        identities,
      );
      const selectedFrame = await capture(`${selected}-selected`);
      assert.equal(selectedFrame.width, before.width);
      assert.equal(selectedFrame.height, before.height);
      let brightened = 0;
      for (let y = Math.floor(before.height * 0.5); y < before.height; y++) {
        for (let x = 0; x < before.width * 0.45; x++) {
          const at = (y * before.width + x) * 4;
          const delta = [0, 1, 2].map(
            (axis) =>
              selectedFrame.pixels[at + axis]! - before.pixels[at + axis]!,
          );
          if (
            delta.every((value) => value >= 0) &&
            delta.reduce((sum, value) => sum + value, 0) > 12
          )
            brightened++;
        }
      }
      assert.ok(
        brightened > 64,
        `${selected} must brighten the baked stage in GLES pixels`,
      );
      await probeIndependent?.();
    }
    await action("tuning", { method: "setFocus", args: [] });
    await action("setExploded", false);
    await options({
      gain: 0.37,
      callsign: "NATIVE-9",
      autoscan: false,
      layerStep: 0.6,
    });
    await action("setAccent", "amber");
    await action("toggleExplode");
    const controls = await waitPanel(
      (state) => field(state, "gui-scan", "checked") === false,
    );
    close(field(controls, "gui-gain/dial", "value"), 0.37);
    const verify = async () => {
      const current = await inspect(driver, signal);
      close(current.options.gain, 0.37);
      assert.equal(current.options.callsign, "NATIVE-9");
      assert.equal(current.options.autoscan, false);
      assert.equal(current.options.accent, "amber");
      assert.equal(current.options.exploded, true);
      close(current.options.layerStep, 0.6);
      close(
        field(current.state, "gallery-camera", "fov_y"),
        (23 * Math.PI) / 180,
      );
      assert.ok(
        Number(field(current.state, "gallery-camera", "x")) < -4,
        "initial/restored exploded mode must frame the inspection stack",
      );
      close(field(current.state, "gui-projector-light", "intensity"), 1.066);
      close(field(current.state, "gui-projector-beam", "weight"), 0);
      assert.ok(
        current.state.entities.some(
          ({ metadata }) => metadata.symbolicId === "gui-demo",
        ),
        "the scanner is the actual attached runtime GUI",
      );
    };
    await verify();
    const flatBeamSource = entity(
      (await inspect(driver, signal)).state,
      "gui-projector-beam",
    ).components.find(({ fields }) => "weight" in fields)?.fields.source;
    await settled();
    const amber = await capture("amber-exploded");
    visible(amber);
    changed(before, amber);
    await options({ layerStep: 0.15 });
    await settled(0.15);
    const compact = await capture("layers-compact");
    await options({ layerStep: 1 });
    await settled(1);
    const separated = await capture("layers-separated");
    changed(compact, separated);
    await options({ surfaceShape: "sphere", surfaceFacing: "inside" });
    await settled(1);
    visible(await capture("layers-sphere-inside"));
    const curvedBeam = entity(
      (await inspect(driver, signal)).state,
      "gui-projector-beam",
    ).components.find(({ fields }) => "weight" in fields)?.fields;
    assert.ok(
      curvedBeam,
      "the curved beam uses the production MeshPose component",
    );
    close(curvedBeam.weight, 1);
    // Native assets use immutable client aliases; the browser checks exact target paths.
    assert.notEqual(curvedBeam.source, flatBeamSource);
    const curvedState = (await inspect(driver, signal)).state;
    assert.ok(
      curvedState.resources.some(
        ({ kind, source, status }) =>
          kind === 1 && source === curvedBeam.source && status === "loaded",
      ),
    );
    const curvature = entity(curvedState, "gui-projector-beam").components.find(
      ({ properties }) => properties?.curvature,
    )?.properties?.curvature;
    assert.equal(curvature?.kind, "vec2");
    if (curvature?.kind === "vec2") {
      close(curvature.value[0], -1 / 5.6);
      close(curvature.value[1], 1);
    }
    // Native reload uses the same persisted high-level options as the browser.
    await options({
      surfaceShape: "flat",
      surfaceFacing: "outside",
      layerStep: 0.6,
    });
    await settled();
    await driver.call("reload", [], signal);
    await enterWorkspace();
    await verify();
    await settled();
    visible(await capture("reloaded"));
    await options({
      vectorOnly: true,
      surfaceShape: "sphere",
      surfaceFacing: "inside",
    });
    await driver.call("reload", [], signal);
    await enterWorkspace();
    const isolated = await inspect(driver, signal);
    assert.equal(isolated.options.vectorOnly, true);
    assert.equal(isolated.options.surfaceShape, "sphere");
    assert.ok(
      !isolated.state.entities.some(
        ({ metadata }) => metadata.symbolicId === "gui-projector-beam",
      ),
    );
    visible(await capture("saved-isolation-reloaded"));
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
