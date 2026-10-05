import type {
  AnimationWorldClient,
  Client,
  HostClientBase,
  Inspection,
} from "@ipp/client";
import {
  AnimationFixture,
  check,
  type AnimationContract,
  type AnimationRecord,
} from "../animation-fixtures.js";
import { SURFACE, selectSystems } from "../system-selections.js";

/** Host-clock transitions use the same retained endpoint on every Surface kind. */
export async function surfaceSpacingTransitions(
  host: HostClientBase<Client>,
  contract: AnimationContract,
  record: AnimationRecord,
) {
  const owned = await host.createWorld({
    selectedSystems: selectSystems(SURFACE),
    symbolicId: "surface-spacing-transitions",
  });
  let connection: Client | undefined;
  try {
    connection = await host.openWorld(owned.reference);
    const client = connection as AnimationWorldClient;
    const fixture = new AnimationFixture(client, contract, record);
    const duration = 0.8;
    let intermediates = 0;
    for (const [index, component] of [
      "FlatSurface",
      "CylinderSurface",
      "SphereSurface",
    ].entries()) {
      const entity = await fixture.create(`transition-${component}`, {
        [component]: { layer_spacing: 0 },
      });
      const definition = client.components[component]!;
      const source = await fixture.upload(
        {
          duration: 0.6,
          tracks: [
            {
              property: {
                component: definition.id,
                offsets: [definition.fields.layer_spacing!.offset],
              },
              keys: [
                {
                  time: 0,
                  value: { kind: "f32", value: 0 },
                  interpolation: { kind: "linear" },
                },
                { time: 0.6, value: { kind: "f32", value: 1 } },
              ],
            },
          ],
        },
        901n + BigInt(index),
      );
      const driver = fixture.driver(entity, source, 0, component, [
        "layer_spacing",
      ]);
      const controller = await fixture.controller([{ ...driver, weight: 0 }]);
      await client.controlAnimationController(controller, { action: "play" });
      await client.controlAnimationController(controller, {
        action: "seek",
        time: 0.6,
      });
      const retarget = (weight: number) =>
        client.transitionAnimationController(controller, {
          description: { drivers: [{ ...driver, weight }], speed: 0 },
          duration,
          easing: "linear",
          startTime: { policy: "seek", time: 0.6 },
        });
      const spacing = (inspection: Inspection) =>
        fixture.value(inspection, entity, component, "layer_spacing");
      const close = (actual: number, expected: number) =>
        check(
          Math.abs(actual - expected) < 0.0001,
          `${component} spacing ${actual} differs from ${expected}`,
        );
      const sample = async (origin: number, target: number) => {
        await client.controlAnimationController(controller, { action: "play" });
        await client.waitForFrame();
        // Freeze the Host fade and await publication so component and controller
        // observations cannot straddle an advancing frame.
        await client.controlAnimationController(controller, {
          action: "pause",
        });
        await client.waitForFrame();
        const inspection = await fixture.inspect();
        const state = fixture.state(inspection, controller);
        check(state.time === 0.6, `${component} clip endpoint moved`);
        check(state.description.speed === 0, `${component} clip speed changed`);
        const progress = state.transition
          ? state.transition.elapsed / duration
          : 1;
        close(spacing(inspection), origin + (target - origin) * progress);
        return { inspection, state };
      };

      await retarget(0.9);
      let mid: Awaited<ReturnType<typeof sample>> | undefined;
      for (let attempt = 0; attempt < 120; attempt++) {
        const observed = await sample(0, 0.9);
        const fade = observed.state.transition;
        if (
          fade &&
          !fade.pending &&
          fade.elapsed > 0 &&
          fade.elapsed < duration
        ) {
          mid = observed;
          intermediates++;
          break;
        }
        check(
          fade,
          `${component} fade completed without an observed intermediate`,
        );
      }
      check(mid, `${component} transition remained pending at held endpoint`);

      // Pause provides an observable origin before interrupting the first fade.
      await client.controlAnimationController(controller, { action: "pause" });
      const origin = spacing(await fixture.inspect());
      await retarget(0.15);
      let ready = false;
      for (let attempt = 0; attempt < 120; attempt++) {
        await client.waitForFrame();
        const inspection = await fixture.inspect();
        const state = fixture.state(inspection, controller);
        close(spacing(inspection), origin);
        check(state.state === "paused", `${component} retarget resumed itself`);
        check(
          state.transition?.elapsed === 0,
          `${component} paused fade advanced`,
        );
        if (state.transition?.pending === false) {
          ready = true;
          break;
        }
      }
      check(ready, `${component} interrupted transition failed to prepare`);
      await client.controlAnimationController(controller, { action: "play" });
      let complete = false;
      let retargetIntermediate = false;
      for (let attempt = 0; attempt < 180; attempt++) {
        const observed = await sample(origin, 0.15);
        const fade = observed.state.transition;
        if (!fade) {
          complete = true;
          break;
        }
        if (!fade.pending && fade.elapsed > 0 && fade.elapsed < duration)
          retargetIntermediate = true;
      }
      check(complete, `${component} interrupted transition did not complete`);
      check(
        retargetIntermediate,
        `${component} interrupted fade never interpolated`,
      );
      await client.controlAnimationController(controller, { action: "stop" });
      close(spacing(await fixture.inspect()), 0);
      await client.deleteAnimationController(controller);
    }
    return { providers: 3, intermediates, heldEndpoint: 0.6, speed: 0 };
  } finally {
    try {
      await connection?.close();
    } finally {
      await host.destroyWorld(owned.reference);
    }
  }
}
