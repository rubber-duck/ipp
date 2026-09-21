import assert from "node:assert/strict";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import {
  openGallery,
  galleryEnvironment,
  entity,
  position,
  transform,
} from "./gallery-driver.js";
import { ANIMATED_IDS } from "../../examples/world-gallery/worlds/lighting/model.js";

function near(actual: number[], expected: number[]) {
  actual.forEach((value, index) =>
    assert.ok(
      Math.abs(value - expected[index]!) < 0.002,
      `${actual} != ${expected}`,
    ),
  );
}

test("combined scene animates radial, orbital and sun motion plus a solid skinned beam with independent playback", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "combined light and skin animations",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("lighting");
      const playing = await g.inspect();
      assert.equal(playing.controllers?.length, 4);
      assert.ok(
        playing.controllers!.every((controller) =>
          controller.description.drivers.every(
            (driver) =>
              driver.source.startsWith("client://") &&
              driver.source.includes("#"),
          ),
        ),
        "Lighting controllers use named declarative AnimationAsset sources",
      );
      assert.ok(
        playing.controllers!.every((player) => player.state === "playing"),
      );
      await g.seek(0);
      // Keep the first player's displayed cursor at zero while another plays.
      // Seeking all to the same displayed value must still send a real UI input.
      await g.page
        .locator("#animation-target")
        .selectOption("lighting-skinning");
      await g.page.locator("#animation-play").click();
      const mixed = await g.waitFor(
        (inspection) =>
          inspection.controllers?.filter((player) => player.state === "playing")
            .length === 1,
      );
      assert.equal(
        mixed.controllers?.filter(
          (player) => player.state === "paused" && player.time === 0,
        ).length,
        3,
      );
      await g.page.locator("#animation-target").selectOption("all");
      await g.page.waitForFunction(
        () =>
          document.querySelector<HTMLInputElement>("#animation-seek")?.value ===
          "0",
      );
      await scenario.evidence.record("same-value-seek-before", {
        input: await g.page.locator("#animation-seek").inputValue(),
        controllers: mixed.controllers,
      });
      await g.seek(0);
      const rest = await g.capture("rest");
      near(position(rest.inspection, "lighting-point"), [2, 2, 2]);
      near(position(rest.inspection, "lighting-spot"), [-3, 5, 3]);
      for (const id of ANIMATED_IDS.slice(0, 3)) {
        const value = entity(rest.inspection, id);
        assert.ok(value.effective.some((entry) => "intensity" in entry.fields));
        assert.ok(
          value.effective.some((entry) =>
            String(entry.fields.source).includes("mesh"),
          ),
        );
      }
      await g.seek(2);
      const quarter = await g.capture("quarter-orbit");
      near(position(quarter.inspection, "lighting-point"), [-2, 2, 2]);
      assert.notDeepEqual(
        transform(quarter.inspection, "lighting-fill"),
        transform(rest.inspection, "lighting-fill"),
      );
      await g.seek(4);
      const middle = await g.capture("inward-spot");
      near(position(middle.inspection, "lighting-spot"), [-1.95, 3.25, 1.95]);
      near(position(middle.inspection, "lighting-point"), [-2, 2, -2]);
      assert.ok(
        (await g.difference("rest", "inward-spot")).changedPixels > 1000,
      );

      // Isolate the skin deformation while all lights stay at exactly their rest pose.
      await g.seek(0);
      await g.capture("beam-rest");
      await g.seek(4, "lighting-skinning");
      const bent = await g.capture("beam-bent");
      assert.equal(bent.frame.drawCalls, 8);
      assert.equal(bent.frame.triangles, 3150);
      assert.ok(
        (await g.difference("beam-rest", "beam-bent")).changedPixels > 100,
      );
      const beam = entity(bent.inspection, "lighting-skinning");
      assert.ok(beam.effective.some((entry) => "roughness" in entry.fields));
      assert.ok(beam.effective.some((entry) => "skeleton" in entry.fields));
      assert.ok(
        bent.inspection.resources.some(
          (resource) =>
            resource.source.endsWith("/assets/1/920010#immutable") &&
            resource.status === "loaded",
        ),
      );
      for (const id of ANIMATED_IDS.slice(0, 3))
        near(position(bent.inspection, id), position(rest.inspection, id));

      await g.page.locator("#animation-target").selectOption("lighting-point");
      await g.page.locator("#animation-speed").selectOption("0.5");
      await g.page.locator("#animation-loop").uncheck();
      await g.page.locator("#animation-play").click();
      await g.waitFor(
        (inspection) =>
          inspection.controllers?.filter((player) => player.state === "playing")
            .length === 1,
      );
      const independent = await g.inspect();
      assert.equal(
        independent.controllers?.filter((player) => player.state === "paused")
          .length,
        3,
      );
      await g.page.locator("#animation-stop").click();
      await g.waitFor(
        (inspection) =>
          inspection.controllers?.some(
            (player) => player.state === "stopped" && player.time === 0,
          ) === true,
      );

      const originalIds = playing.controllers!.map((player) => player.id);
      await g.navigate("shapes");
      await g.waitFor((inspection) => !inspection.controllers?.length);
      assert.equal(
        (await g.capture("geometry-after-animation")).frame.drawCalls,
        12,
      );
      // Hold a real controller creation acknowledgement, then leave during setup.
      await g.call("delayNextControllerCreation");
      try {
        await g.selectScene("lighting");
        await g.held();
        await scenario.evidence.record(
          "held-setup-outcome",
          await g.call("heldReplyOutcome"),
        );
        const pending = await g.inspect();
        await scenario.evidence.record("held-player-setup", pending);
        assert.ok((pending.controllers?.length ?? 0) >= 1);
        await g.navigate("shapes");
        await g.call("releaseQuery");
        await g.waitFor((inspection) => !inspection.controllers?.length);
        assert.equal(
          (await g.capture("canceled-player-startup")).frame.drawCalls,
          12,
        );
      } finally {
        await g.call("releaseQuery");
      }
      await g.navigate("lighting");
      await g.seek(0);
      const reentered = await g.capture("combined-reentered");
      assert.equal(reentered.inspection.controllers?.length, 4);
      assert.ok(
        reentered.inspection.controllers!.every(
          (player) => !originalIds.includes(player.id),
        ),
      );
      assert.deepEqual(g.errors, []);
    },
  );
});
