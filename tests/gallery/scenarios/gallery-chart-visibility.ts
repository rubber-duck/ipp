import assert from "node:assert/strict";
import {
  assertChartImageChanged,
  type GalleryChartsDriver,
} from "./gallery-charts.js";

export interface ProgramObservation {
  programs: number;
  calls: { method: string; milliseconds: number }[];
  dropped: number;
}

export interface ChartVisibilityDriver
  extends Pick<GalleryChartsDriver, "action" | "capture" | "record"> {
  programs(): Promise<ProgramObservation>;
}

/** Revisit the real chart ring with unchanged data, shaders and parameter values. */
export async function exerciseChartVisibility(driver: ChartVisibilityDriver) {
  await driver.action("skin", "glass");
  const initial = await driver.capture("visibility-start");
  const start = await driver.programs();
  for (const pass of ["first", "revisit"]) {
    const before = await driver.programs();
    for (let step = 0; step < 12; step++) {
      await driver.action("navigate", {
        kind: "rotate",
        yaw: Math.PI / 6,
        pitch: 0,
      });
      const frame = await driver.capture(`visibility-${pass}-${step}`);
      if (step === 5)
        assertChartImageChanged(
          initial,
          frame,
          "Rotation presents different native charts",
        );
    }
    const after = await driver.programs();
    assert.equal(
      after.dropped,
      0,
      "Program diagnostic evidence must not be truncated",
    );
    const calls = after.calls.slice(before.calls.length);
    await driver.record(`visibility-${pass}-programs`, {
      programs: after.programs - before.programs,
      setupMilliseconds: calls.reduce(
        (sum, call) => sum + call.milliseconds,
        0,
      ),
      maximumSynchronousCallMilliseconds: Math.max(
        0,
        ...calls.map((call) => call.milliseconds),
      ),
      calls,
    });
    if (pass === "revisit")
      assert.equal(
        after.programs,
        before.programs,
        "Revisiting unchanged charts must reuse compiled Canvas programs",
      );
  }
  assert.ok(start.programs > 0, "The probe must observe real shader programs");
}
