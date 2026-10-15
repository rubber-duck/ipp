import assert from "node:assert/strict";

export interface RenderScaleDriver {
  skin(skin: "glass" | "cybertron"): Promise<void>;
  select(scale: number): Promise<void>;
  snapshot(): Promise<{ scale: number; selected: string[] }>;
  capture(
    name: string,
  ): Promise<{ width: number; height: number; foregroundPixels: number }>;
}

/** Repeated physical edits must agree across the app, native control and actual frame. */
export async function exerciseRenderScale(driver: RenderScaleDriver) {
  for (const skin of ["glass", "cybertron"] as const) {
    await driver.skin(skin);
    for (const [index, scale] of [1.5, 1, 0.75, 1.5, 1].entries()) {
      await driver.select(scale);
      const snapshot = await driver.snapshot();
      assert.equal(snapshot.scale, scale);
      assert.deepEqual(snapshot.selected, [String(scale)]);
      const frame = await driver.capture(
        `${skin}-resolution-${index}-${scale}`,
      );
      assert.equal(frame.width, 960 * scale);
      assert.equal(frame.height, 720 * scale);
      assert.ok(
        frame.foregroundPixels > 1000,
        "the changed resolution renders scene content",
      );
    }
  }
}
