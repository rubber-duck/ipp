/** Wheel navigation shares native GUI priority with the cockpit carousel. */
import assert from "node:assert/strict";
import type { GuiInputRoutingOutcome } from "@ipp/client";
import type { CockpitDriver, CockpitState } from "./cockpit.js";
import {
  assertCylinder,
  assertFlat,
  cockpitFields,
  surfaceSample,
  rotate,
} from "../cockpit-oracle.js";

export interface CockpitWheelEvidence {
  outcomes: GuiInputRoutingOutcome[];
  admissionMs: number[];
  viewport: readonly [number, number];
  controller: string;
  retainedController: string;
  samples: {
    pose: number;
    target: number;
    elapsed: number | undefined;
    tick: string;
  }[];
}

export interface CockpitWheelDriver extends CockpitDriver {
  wheel(
    panel: number | "background" | "shell",
    id: string,
    delta: readonly [number, number],
    fraction?: number,
    count?: number,
  ): Promise<CockpitWheelEvidence>;
}

function renderedFocus(state: CockpitState) {
  const camera = cockpitFields(
    state.scene,
    "cockpit-camera",
    state.componentIds.Transform!,
  );
  const surface = state.scene.entities.find(
    (entity) => entity.metadata.symbolicId === "cockpit-screen-0",
  )!;
  const fields = surface.components.find(
    (component) =>
      component.component === state.componentIds.CylinderSurface ||
      component.component === state.componentIds.FlatSurface,
  )!.fields;
  const center = surfaceSample(
    state.scene,
    state.componentIds,
    0,
    0.5,
    0.5,
  ).position;
  const local = rotate(
    center.map((value, axis) => value - Number(camera[["x", "y", "z"][axis]!])),
    camera,
    true,
  );
  const angle = Math.atan2(-local[0]!, -local[2]!);
  const radius = Number(state.options.projection);
  return angle / ((Number(fields.width) + 0.07) / radius);
}

export async function exerciseCockpitWheel(driver: CockpitWheelDriver) {
  await driver.options({ carousel: 1 });
  await driver.advance(0.3);
  const initial = await driver.state();
  const focus = async () => Number((await driver.state()).options.carousel);
  const navigationControl = async (id: string, enabled: boolean) => {
    const deadline = performance.now() + 15000;
    for (;;) {
      const state = await driver.state();
      const entity = state.panels[0]!.entities.find(
        (entity) => entity.metadata.symbolicId === id,
      );
      const behavior = entity?.components.find(
        (component) => component.component === state.componentIds.GuiBehavior,
      )?.fields;
      const bounds = entity?.components.find(
        (component) => component.component === state.componentIds.CanvasBounds,
      )?.fields;
      if (
        behavior?.enabled === enabled &&
        bounds &&
        Number(bounds.width) > 0 &&
        Number(bounds.height) > 0
      )
        return entity!.id;
      assert.ok(
        performance.now() < deadline,
        `${id} mounts and becomes ${enabled ? "enabled" : "disabled"}`,
      );
      await driver.advance(0.03);
    }
  };
  const close = (actual: number, expected: number, reason: string) =>
    assert.ok(
      Math.abs(actual - expected) < 0.0001,
      `${reason}: ${actual} vs ${expected}`,
    );
  const camera = cockpitFields(
    initial.scene,
    "cockpit-camera",
    initial.componentIds.Transform!,
  );
  const timings: number[] = [];
  const wheel: CockpitWheelDriver["wheel"] = async (...args) => {
    const result = await driver.wheel(...args);
    assert.ok(
      result.outcomes.every(
        (outcome) => !outcome.error && !outcome.rejected && !outcome.cancelled,
      ),
    );
    timings.push(...result.admissionMs);
    return result;
  };

  // No pointer move or press precedes this wheel: its native hit must be fresh.
  await wheel(1, "cockpit-header-1", [40, 0]);
  close(
    await focus(),
    1.25,
    "horizontal trackpad movement advances a quarter pane",
  );
  await wheel(1, "cockpit-header-1", [0, 40]);
  close(
    await focus(),
    1.5,
    "vertical wheel provides the same navigation fallback",
  );
  // At this angle the middle header sits behind the app toolbar; use the
  // exposed next header so the burst exercises carousel input, not the overlay.
  const burst = await wheel(2, "cockpit-header-2", [4, 0], 0.5, 10);
  assert.equal(
    burst.controller,
    burst.retainedController,
    "wheel retargets retain the prepared controller",
  );
  if (burst.viewport[0] <= 960) {
    assert.ok(
      burst.samples.some(
        (sample) =>
          sample.pose > 1.5001 &&
          sample.pose < sample.target - 0.0001 &&
          sample.elapsed !== undefined,
      ),
      "the focused 960px fixture observes a native pose inside an active wheel transition",
    );
  }
  close(await focus(), 1.75, "a rapid trackpad burst accumulates every delta");
  const reverse = await wheel(2, "cockpit-header-2", [-4, 0], 0.5, 10);
  close(
    await focus(),
    1.5,
    "a rapid direction reversal accumulates without an older target returning",
  );
  const curved = await driver.capture("cockpit-wheel-curved");
  let colored = 0;
  for (let offset = 0; offset < curved.pixels.length; offset += 4)
    if (
      curved.pixels[offset + 2]! > curved.pixels[offset]! + 20 &&
      curved.pixels[offset + 1]! > 70
    )
      colored++;
  assert.ok(
    colored > 500,
    "completed wheel frame retains the cockpit and window paint",
  );
  const afterBurst = await driver.state();
  close(
    renderedFocus(afterBurst),
    1.5,
    "the actual evaluated rotor reaches the accumulated wheel target",
  );
  assertCylinder(
    afterBurst.scene,
    afterBurst.componentIds,
    Number(afterBurst.options.projection),
  );

  await driver.options({ carousel: 2 });
  await driver.advance(0.3);
  await wheel(2, "cockpit-content-2", [0, -40], 0.96);
  close(
    await focus(),
    1.75,
    "unused window content passes unconsumed wheel to the carousel",
  );

  await driver.action("skin", "cybertron");
  await driver.options({ carousel: 0 });
  await driver.advance(0.3);
  await driver.contentHeight(0, 200);
  const scrollBefore = await driver.state();
  const bodyBefore = cockpitFields(
    scrollBefore.panels[0]!,
    "cockpit-content-0",
    scrollBefore.componentIds.GuiScrollView!,
  );
  assert.ok(
    Number(bodyBefore.capacity_y) > 0,
    "the native scroll fixture actually overflows",
  );
  const amount = Math.min(20, Number(bodyBefore.capacity_y) / 2);
  const scrolled = await wheel(0, "cockpit-content-0", [0, amount], 0.96);
  close(
    await focus(),
    0,
    "scrollable content consumes its wheel before carousel navigation",
  );
  assert.ok(
    scrolled.outcomes.every(
      (outcome) =>
        !outcome.remaining || outcome.remaining.every((value) => value === 0),
    ),
  );
  const scrollAfter = await driver.state();
  const bodyAfter = cockpitFields(
    scrollAfter.panels[0]!,
    "cockpit-content-0",
    scrollAfter.componentIds.GuiScrollView!,
  );
  assert.ok(
    Number(bodyAfter.offset_y) > Number(bodyBefore.offset_y),
    "the body actually scrolled",
  );

  await driver.contentHeight(0, null);
  await driver.options({ gain: 1 });
  await wheel(0, "cockpit-gain/slider", [0, -40]);
  close(
    await focus(),
    0,
    "a value control at its bound still owns wheel input",
  );
  close(
    Number((await driver.state()).options.gain),
    1,
    "the bound value remains clamped",
  );
  await wheel(0, "cockpit-pulse", [40, 0]);
  close(await focus(), 0, "button hits never become unused chrome");
  await wheel("background", "", [40, 0]);
  close(await focus(), 0, "the scene background does not move the windows");
  await wheel("shell", "demo-controls-toggle", [40, 0]);
  close(
    await focus(),
    0,
    "parent overlay controls do not move the child carousel",
  );

  await driver.options({ surface: "flat", carousel: 1 });
  await driver.advance(0.3);
  await wheel(1, "cockpit-header-1", [-40, 0]);
  close(await focus(), 0.75, "flat windows use the same wheel navigation");
  const flat = await driver.state();
  close(
    renderedFocus(flat),
    0.75,
    "the flat window rotor reaches its wheel target",
  );
  assertFlat(flat.scene, flat.componentIds, Number(flat.options.projection));
  await driver.capture("cockpit-wheel-flat");
  await driver.options({ carousel: 0 });
  await driver.advance(0.3);
  await driver.press(0, "cockpit-control-tab/gui");
  const previous = await navigationControl("cockpit-previous", false);
  // Prepare wheel tracking while already clamped at zero. Later deltas reuse
  // that controller without reauthoring the surrounding pane Worlds.
  await wheel(0, "cockpit-header-0", [-40, 0]);
  close(await focus(), 0, "wheel navigation clamps at the first pane");
  await wheel(0, "cockpit-header-0", [40, 0]);
  close(
    await focus(),
    0.25,
    "wheel leaves the first endpoint without a full pane render",
  );
  assert.equal(
    await navigationControl("cockpit-previous", true),
    previous,
    "tracking enables the mounted Previous button without replacing it",
  );
  await driver.press(0, "cockpit-previous");
  await driver.advance(0.3);
  close(
    await focus(),
    0,
    "Previous becomes enabled and reads the latest wheel intent",
  );
  assert.equal(
    await navigationControl("cockpit-previous", false),
    previous,
    "returning to the endpoint disables the same Previous control",
  );
  await wheel(0, "cockpit-header-0", [-40, 0]);
  close(await focus(), 0, "wheel navigation clamps at the first pane");
  await driver.options({ carousel: 2 });
  await driver.advance(0.3);
  await wheel(2, "cockpit-header-2", [40, 0]);
  close(await focus(), 2, "wheel navigation clamps at the last pane");
  await driver.capture("cockpit-wheel-last");
  const final = await driver.state();
  assert.deepEqual(
    cockpitFields(final.scene, "cockpit-camera", final.componentIds.Transform!),
    camera,
    "wheel movement keeps the authored Camera fixed",
  );
  assert.deepEqual(
    final.panelWorlds,
    initial.panelWorlds,
    "wheel navigation retains all five Worlds",
  );
  return {
    admissionMs: timings,
    forward: burst.samples,
    reverse: reverse.samples,
    worlds: final.panelWorlds.length,
  };
}
