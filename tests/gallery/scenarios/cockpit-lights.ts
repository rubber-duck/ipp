import assert from "node:assert/strict";
import type { Inspection } from "@ipp/client";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { cockpitPlate, type CockpitViewport } from "../cockpit-oracle.js";

interface LightDriver {
  cameraViewport(): Promise<CockpitViewport>;
  state(): Promise<{
    scene: Inspection;
    options: Readonly<Record<string, unknown>>;
  }>;
  action(name: string, value: unknown): Promise<unknown>;
  capture(name: string): Promise<RgbaImage>;
  advance(seconds: number): Promise<void>;
}

function phases(scene: Inspection) {
  const console = scene.entities.find(
    ({ metadata }) => metadata.symbolicId === "cockpit-console-shell",
  );
  const material = console?.components.find(
    ({ properties }) => properties?.cabinPhase,
  );
  assert.ok(material?.properties);
  return ["cabinPhase", "dashboardPhase", "buttonsPhase", "dialsPhase"].map(
    (name) => material.properties![name],
  );
}

function dialPixels(frame: RgbaImage, viewport: CockpitViewport) {
  // Camera framing preserves vertical scale until a square view; narrower
  // views fit the central panel rather than the whole original image.
  // This rectangle contains only physical dial rings, below the floating GUI.
  const { scale, left, top } = cockpitPlate(frame, viewport);
  const height = 941 * scale;
  const width = 1672 * scale;
  const values: number[] = [];
  for (let y = Math.ceil(top + height * 0.87); y < top + height * 0.95; y++)
    for (
      let x = Math.max(0, Math.ceil(left + width * 0.28));
      x < Math.min(frame.width, left + width * 0.72);
      x++
    ) {
      const i = (y * frame.width + x) * 4;
      values.push(frame.pixels[i]!, frame.pixels[i + 1]!, frame.pixels[i + 2]!);
    }
  return values;
}

function casingPixels(frame: RgbaImage, viewport: CockpitViewport) {
  // Silver strips below the button banks, measured in the authored camera view.
  // They sit inside the broad lamp regions but are not emissive themselves.
  const { scale, left, top } = cockpitPlate(frame, viewport);
  const values: number[] = [];
  for (const center of [620, 1052])
    for (let y = Math.ceil(top + 800 * scale); y < top + 805 * scale; y++)
      for (
        let x = Math.ceil(left + (center - 8) * scale);
        x < left + (center + 8) * scale;
        x++
      ) {
        const offset = (y * frame.width + x) * 4;
        values.push(
          frame.pixels[offset]!,
          frame.pixels[offset + 1]!,
          frame.pixels[offset + 2]!,
        );
      }
  return values;
}

/** Assert visible default motion, not merely a moving animation clock. */
export async function exerciseCockpitLights(driver: LightDriver) {
  assert.equal((await driver.state()).options.lightsPlaying, true);
  await driver.action("scan", false);
  await driver.action("atmospherePlayback", false);
  const viewport = await driver.cameraViewport();
  const firstFrame = await driver.capture("cockpit-lights-motion-0");
  const first = dialPixels(firstFrame, viewport);
  const casing = casingPixels(firstFrame, viewport);
  assert.ok(
    casing.length > 0 && casing.every((value) => value > 80),
    "casing regions contain the visible silver body",
  );
  const minimum = [...first];
  const maximum = [...first];
  for (let frame = 1; frame <= 4; frame++) {
    await driver.advance(0.37);
    const captured = await driver.capture(`cockpit-lights-motion-${frame}`);
    const next = dialPixels(captured, viewport);
    assert.ok(
      casingPixels(captured, viewport).every(
        (value, index) => Math.abs(value - casing[index]!) <= 2,
      ),
      "lamp animation must not pulse the silver casing",
    );
    next.forEach((value, i) => {
      minimum[i] = Math.min(minimum[i]!, value);
      maximum[i] = Math.max(maximum[i]!, value);
    });
  }
  let visible = 0;
  let maximumChange = 0;
  for (let i = 0; i < first.length; i += 3) {
    const change = Math.max(
      ...[0, 1, 2].map((c) => maximum[i + c]! - minimum[i + c]!),
    );
    if (change > 32) visible++;
    maximumChange = Math.max(maximumChange, change);
  }
  const changedFraction = visible / (first.length / 3);
  assert.ok(
    changedFraction > 0.01,
    `dial motion is perceptible: ${visible} pixels (${changedFraction}), max ${maximumChange}`,
  );

  await driver.action("lightsPlayback", false);
  const paused = phases((await driver.state()).scene);
  const still = dialPixels(
    await driver.capture("cockpit-lights-paused"),
    viewport,
  );
  await driver.advance(0.4);
  const held = dialPixels(
    await driver.capture("cockpit-lights-held"),
    viewport,
  );
  assert.deepEqual(
    phases((await driver.state()).scene),
    paused,
    "light phases hold while paused",
  );
  assert.ok(
    still.every((value, i) => Math.abs(value - held[i]!) <= 1),
    "physical lights stop changing when paused",
  );
  await driver.action("lightsPlayback", true);
  await driver.advance(0.4);
  assert.notDeepEqual(
    phases((await driver.state()).scene),
    paused,
    "light motion resumes",
  );
  return { changedFraction, maximumChange };
}
