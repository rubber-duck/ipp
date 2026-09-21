import assert from "node:assert/strict";
import test from "node:test";
import type { Inspection } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery, galleryEnvironment, entity } from "./gallery-driver.js";

const emitter = (inspection: Inspection) =>
  entity(inspection, "particle-fountain").effective.find(
    (component) => "lifetime_random" in component.fields,
  )!.fields;

test("particle gallery renders sprites and meshes, updates settings and cleans up on navigation", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "particle gallery",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      await g.navigate("particles");
      const initial = await g.waitFor((inspection) =>
        inspection.entities.some(
          (e) => e.metadata.symbolicId === "particle-fountain",
        ),
      );
      const id = entity(initial, "particle-fountain").id;
      // Wait for visible density using real Host frames, without advancing simulation from the client.
      const deadline = performance.now() + 10_000;
      let sprites = await g.capture("particle-sprites");
      while (sprites.frame.triangles < 1000) {
        assert.ok(performance.now() < deadline, "Fountain did not fill");
        sprites = await g.capture("particle-sprites");
      }
      assert.equal(
        sprites.frame.drawCalls,
        3,
        "One instance batch and two base meshes",
      );
      assert.equal(emitter(sprites.inspection).rate, 900);
      await g.page.locator("#particle-rate").fill("300");
      await g.waitFor((inspection) => emitter(inspection).rate === 300);
      await g.page.locator("#particle-presentation").selectOption("meshes");
      await g.waitFor((inspection) =>
        entity(inspection, "particle-fountain").effective.some(
          (component) =>
            component.fields.source ===
            "ipp://mesh/cube?width=1&height=1&length=1",
        ),
      );
      const meshes = await g.capture("particle-meshes");
      assert.equal(
        entity(meshes.inspection, "particle-fountain").id,
        id,
        "Presentation switch preserves the producer",
      );
      assert.equal(meshes.frame.drawCalls, 3);
      assert.ok(
        (await g.difference("particle-sprites", "particle-meshes"))
          .changedPixels > 100,
      );
      await g.page.locator("#particle-emitting").click();
      await g.waitFor((inspection) => emitter(inspection).enabled === false);
      const drainDeadline = performance.now() + 10_000;
      let drained = await g.capture("particle-drained");
      while (drained.frame.drawCalls !== 2) {
        assert.ok(
          performance.now() < drainDeadline,
          "Stopped particles did not drain",
        );
        drained = await g.capture("particle-drained");
      }
      assert.ok(
        (await g.difference("particle-meshes", "particle-drained"))
          .changedPixels > 100,
      );
      await g.page.locator("#particle-restart").click();
      await g.waitFor(
        (inspection) =>
          emitter(inspection).enabled === true &&
          emitter(inspection).restart === 1,
      );
      await g.capture("particle-restarted");
      await g.navigate("shapes");
      await g.waitFor((inspection) =>
        inspection.entities.every(
          (e) => !e.metadata.symbolicId?.startsWith("particle-"),
        ),
      );
      assert.equal(
        (await g.capture("geometry-after-particles")).frame.drawCalls,
        12,
      );
      await g.navigate("particles");
      const returned = await g.waitFor((inspection) =>
        inspection.entities.some(
          (e) => e.metadata.symbolicId === "particle-fountain",
        ),
      );
      assert.notEqual(entity(returned, "particle-fountain").id, id);
      assert.equal(
        emitter(returned).rate,
        300,
        "Gallery retains authored controls across navigation",
      );
      assert.deepEqual(g.errors, []);
    },
  );
});
