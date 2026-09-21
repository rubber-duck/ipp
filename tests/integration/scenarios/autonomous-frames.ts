import assert from "node:assert/strict";
import type { FrameObservation } from "../driver.js";
import type { ScenarioContext } from "../environment.js";

const FRAME_COUNT = 3;

export async function autonomousFramesAdvance(
  context: ScenarioContext,
): Promise<void> {
  assert.equal(
    await context.execute("inspect public client surface", {}, () =>
      context.driver.publicStepAvailable({ signal: context.signal }),
    ),
    false,
    "the public client must not expose step",
  );

  const frames: FrameObservation[] = [];
  for (let index = 0; index < FRAME_COUNT; index += 1) {
    const previous = frames.at(-1);
    const frame = await context.execute(
      "observe unsolicited runtime frame",
      {
        index,
        afterTick: previous?.tick,
        clientRequestsSinceBootstrap: 0,
      },
      () =>
        context.driver.waitForFrame(previous?.tick, {
          signal: context.signal,
        }),
    );
    assert.ok(frame.tick > 0n, "frame tick must be positive");
    assert.ok(Number.isFinite(frame.time), "frame time must be finite");
    assert.ok(frame.time >= 0, "frame time must be non-negative");
    if (previous !== undefined) {
      assert.ok(frame.tick > previous.tick, "frame tick must increase");
      assert.ok(frame.time > previous.time, "frame time must increase");
    }
    frames.push(frame);
  }
}
