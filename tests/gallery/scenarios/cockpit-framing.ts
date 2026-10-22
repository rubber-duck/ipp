/** Equal-height views must add side content, preserving every central ray. */
import assert from "node:assert/strict";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export async function exerciseCockpitFraming(driver: {
  capture(width: number, height: number, name: string): Promise<RgbaImage>;
  illumination(value: number): Promise<void>;
  tint(value: readonly [number, number, number]): Promise<void>;
}) {
  const normal = await driver.capture(1024, 576, "bridge-16x9");
  const wide = await driver.capture(2048, 576, "bridge-32x9");
  const restored = await driver.capture(1024, 576, "bridge-16x9-restored");
  assert.deepEqual(
    [normal.width, normal.height, wide.width, wide.height],
    [1024, 576, 2048, 576],
  );
  let changed = 0;
  let error = 0;
  let cabin = 0;
  for (let y = 0; y < normal.height; y++) {
    for (let x = 0; x < normal.width; x++) {
      const a = (y * normal.width + x) * 4;
      const b = (y * wide.width + x + normal.width / 2) * 4;
      let maximum = 0;
      for (let channel = 0; channel < 3; channel++) {
        const delta = Math.abs(
          normal.pixels[a + channel]! - wide.pixels[b + channel]!,
        );
        maximum = Math.max(maximum, delta);
        error += delta;
        assert.ok(
          Math.abs(
            normal.pixels[a + channel]! - restored.pixels[a + channel]!,
          ) <= 2,
          "returning to the original viewport preserves the static environment",
        );
      }
      if (maximum > 2) changed++;
    }
    for (let x = 0; x < wide.width; x++) {
      if (x >= 512 && x < 1536) continue;
      const i = (y * wide.width + x) * 4;
      const [r, g, b] = [
        wide.pixels[i]!,
        wide.pixels[i + 1]!,
        wide.pixels[i + 2]!,
      ];
      if (Math.min(r, g, b) > 80 && Math.max(r, g, b) - Math.min(r, g, b) < 45)
        cabin++;
    }
  }
  const pixels = normal.width * normal.height;
  assert.ok(
    changed < pixels * 0.005,
    `ultrawide center retains scale and composition: ${changed} differing pixels`,
  );
  assert.ok(
    error / (pixels * 3) < 0.25,
    "equal-height center crops retain the same rendered detail",
  );
  assert.ok(
    cabin > pixels * 0.15,
    `additional side regions reveal cabin architecture: ${cabin} pixels`,
  );
  await driver.illumination(0);
  const off = await driver.capture(2048, 576, "bridge-lights-off");
  await driver.illumination(1);
  const on = await driver.capture(2048, 576, "bridge-lights-on");
  let bridgeLamps = 0;
  let steadyMetal = 0;
  // Outer quarters contain bridge architecture and exclude the dashboard.
  for (let y = 0; y < on.height; y++)
    for (let x = 0; x < on.width; x++) {
      if (x >= 512 && x < 1536) continue;
      const i = (y * on.width + x) * 4;
      const difference = Math.max(
        ...[0, 1, 2].map(
          (channel) => on.pixels[i + channel]! - off.pixels[i + channel]!,
        ),
      );
      if (difference > 20) bridgeLamps++;
      if (
        difference <= 1 &&
        Math.min(off.pixels[i]!, off.pixels[i + 1]!, off.pixels[i + 2]!) > 80
      )
        steadyMetal++;
    }
  assert.ok(
    bridgeLamps > 100,
    `separate bridge emission visibly responds: ${bridgeLamps} pixels`,
  );
  assert.ok(
    steadyMetal > pixels * 0.15,
    "light controls retain the surrounding metalwork",
  );
  await driver.tint([1, 0, 0]);
  const red = await driver.capture(2048, 576, "bridge-lights-red");
  await driver.tint([0, 0, 1]);
  const blue = await driver.capture(2048, 576, "bridge-lights-blue");
  let coloredLamps = 0;
  for (let i = 0; i < off.pixels.length; i += 4) {
    // A scalar contribution changes only the selected channel. This catches
    // colored source images, swapped channels and color-space mistakes.
    for (const channel of [1, 2])
      assert.ok(
        Math.abs(red.pixels[i + channel]! - off.pixels[i + channel]!) <= 2,
      );
    for (const channel of [0, 1])
      assert.ok(
        Math.abs(blue.pixels[i + channel]! - off.pixels[i + channel]!) <= 2,
      );
    if (
      red.pixels[i]! - off.pixels[i]! > 20 &&
      blue.pixels[i + 2]! - off.pixels[i + 2]! > 20
    )
      coloredLamps++;
  }
  assert.ok(
    coloredLamps > 100,
    "shader RGB recolors the same registered lamp pixels",
  );
  // Independently measured console bar on the original 1672 x 941 plate.
  // Its unpowered lens is visible, its lit center changes, and its casing holds.
  const sample = (frame: RgbaImage, plateX: number, plateY: number) => {
    const scale = frame.height / 941;
    const x = Math.round((frame.width - 1672 * scale) / 2 + plateX * scale);
    const y = Math.round(plateY * scale);
    return Array.from(
      frame.pixels.slice(
        (y * frame.width + x) * 4,
        (y * frame.width + x) * 4 + 3,
      ),
    );
  };
  const lens = sample(off, 834, 749);
  assert.ok(
    lens.every((value) => value > 25 && value < 180),
    `unpowered lens remains present: ${lens}`,
  );
  assert.ok(
    sample(on, 834, 749)[0]! - lens[0]! > 30,
    "contribution lands on the physical lens",
  );
  assert.deepEqual(
    sample(off, 834, 733),
    sample(on, 834, 733),
    "surrounding console casing does not emit",
  );
  return {
    coloredLamps,
    bridgeLamps,
    steadyMetal,
    changedCenterPixels: changed,
    meanChannelError: error / (pixels * 3),
    additionalCabinPixels: cabin,
  };
}
