import assert from "node:assert/strict";
import {
  assertBoundedChartInput,
  baselineBarPointer,
  focusChart,
  waitForCharts,
  type GalleryChartsDriver,
} from "./gallery-charts.js";

export interface ChartInputDriver extends GalleryChartsDriver {
  burst(
    actions: readonly { name: string; args: unknown }[],
  ): Promise<{ inspections: number; notifications: number }>;
}

/** Same-turn intent bursts exercise the real scene lanes before transport replies. */
export async function exerciseChartInput(
  driver: ChartInputDriver,
  aspect: number,
) {
  await focusChart(driver, "center");
  const before = await driver.inspect();
  const work = await driver.burst(
    Array.from({ length: 64 }, () => ({
      name: "navigate",
      args: { kind: "rotate", yaw: 0.005, pitch: 0 },
    })),
  );
  const after = await driver.inspect();
  assertBoundedChartInput(after);
  assert.equal(
    after.input.camera.submitted - before.input.camera.submitted,
    64,
  );
  assert.ok(after.input.camera.executed - before.input.camera.executed <= 2);
  assert.ok(after.input.camera.coalesced - before.input.camera.coalesced >= 62);
  const yaw = (state: typeof before) =>
    2 * Math.atan2(state.camera.transform.qy, state.camera.transform.qw);
  assert.ok(
    Math.abs(yaw(after) - yaw(before) - 0.32) < 0.005,
    "coalescing preserves accumulated camera motion",
  );
  assert.equal(
    work.inspections,
    0,
    "camera input avoids full World inspection",
  );
  assert.ok(
    work.notifications <= 2,
    "coalesced camera work publishes only committed changes",
  );
  await driver.record("camera-burst", {
    before: before.input,
    after: after.input,
    work,
  });

  await driver.burst([
    ...Array.from({ length: 48 }, () => ({
      name: "navigate",
      args: { kind: "rotate", yaw: 0.02, pitch: 0 },
    })),
    { name: "focus", args: "grid-bars" },
    { name: "focus", args: "bars" },
  ]);
  const latest = await waitForCharts(
    driver,
    (state) => state.focus?.chart === "bars" && state.focus.time >= 1.999,
    "burst-newest-focus",
  );
  assertBoundedChartInput(latest);
  assert.equal(latest.focus?.chart, "bars");
  assert.equal(
    latest.world.controllers?.length,
    1,
    "superseded focus leaves one camera controller",
  );
  const point = baselineBarPointer(latest, aspect);
  const hoverBefore = latest.input.hover;
  await driver.burst(
    Array.from({ length: 64 }, (_, index) => ({
      name: "hover",
      args: { x: point.x + (63 - index) * 0.001, y: point.y },
    })),
  );
  const hovered = await driver.inspect();
  assertBoundedChartInput(hovered);
  assert.equal(
    hovered.hover?.rowId,
    2n,
    "latest hover wins after a pending-query burst",
  );
  assert.equal(hovered.input.hover.submitted - hoverBefore.submitted, 64);
  assert.ok(hovered.input.hover.executed - hoverBefore.executed <= 2);
  assert.ok(hovered.input.hover.coalesced - hoverBefore.coalesced >= 62);
  await driver.capture("chart-burst-newest-row");
  await driver.record("hover-burst", {
    before: hoverBefore,
    after: hovered.input.hover,
    hover: hovered.hover,
  });
}
