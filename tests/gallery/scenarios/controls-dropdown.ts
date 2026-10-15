import assert from "node:assert/strict";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

export interface ControlsDropdownDriver {
  press(id: string): Promise<void>;
  state(): Promise<{ presentation: unknown; open: boolean }>;
  capture(name: string): Promise<RgbaImage>;
}

/** A live input context opens, paints and chooses an overlay inside a scroll view. */
export async function exerciseControlsDropdown(driver: ControlsDropdownDriver) {
  await driver.press("demo-controls-toggle");
  const closed = await driver.capture("controls-closed-dropdown");
  await driver.press("particles-presentation");
  assert.equal((await driver.state()).open, true);
  const open = await driver.capture("controls-open-dropdown");
  let changed = 0;
  for (let offset = 0; offset < closed.pixels.length; offset += 4) {
    if (
      [0, 1, 2].some(
        (axis) =>
          Math.abs(
            closed.pixels[offset + axis]! - open.pixels[offset + axis]!,
          ) > 20,
      )
    )
      changed++;
  }
  assert.ok(
    changed > 100,
    "the native dropdown paints above its scroll content",
  );
  await driver.press("particles-presentation/options/meshes");
  assert.deepEqual(await driver.state(), {
    presentation: "meshes",
    open: false,
  });
  await driver.capture("controls-selected-dropdown");
}
