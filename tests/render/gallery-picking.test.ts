import assert from "node:assert/strict";
import test from "node:test";
import type { GeometryPickResultEvent } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { SCENE_OBJECTS } from "../../examples/world-gallery/worlds/lighting/model.js";
import {
  openGallery,
  galleryEnvironment,
  entity,
  selected,
} from "./gallery-driver.js";
import type {
  projectGalleryPoints,
  captureColorRegion,
} from "./viewer-browser-helper.js";

test("combined gallery picks every object, keeps one outline and edits independent properties", {
  timeout: 90_000,
}, async (context) => {
  await runBrowserEnvironment(
    "combined selection and inspector",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      assert.equal(
        await g.page.locator("#scene-picker .scene-option").count(),
        5,
      );
      await g.navigate("lighting");
      await g.seek(0);
      const before = await g.capture("combined-unselected");
      assert.equal(before.frame.drawCalls, 8);
      assert.deepEqual(selected(before.inspection), []);
      for (const object of SCENE_OBJECTS) {
        const inspection = await g.click(object.id);
        assert.deepEqual(selected(inspection), [object.id]);
        assert.equal(
          await g.page.locator("#object-select").inputValue(),
          object.id,
        );
        assert.equal(
          await g.page.locator(".object-inspector").getAttribute("data-object"),
          object.id,
        );
        const frame = await g.capture(`selected-${object.id}`);
        assert.equal(
          frame.frame.drawCalls,
          object.id === "lighting-skinning" ? 10 : 9,
        );
        const yellow = await g.call<number[]>(
          "countViewerColors",
          frame.label,
          [[255, 231, 0]],
        );
        assert.ok(
          yellow[0]! > 10,
          `${object.name} has a visible selection outline`,
        );
        if (object.id === "lighting-spot" || object.id === "lighting-fill") {
          // The actual marker recipes extend only along -Z from the light origin.
          const halfWidth = object.id === "lighting-spot" ? 0.3 : 1;
          const depth = object.id === "lighting-spot" ? 0.7 : 1.12;
          const corners: number[][] = [];
          for (const x of [-halfWidth, halfWidth])
            for (const y of [-halfWidth, halfWidth])
              for (const z of [-depth, 0]) corners.push([x, y, z]);
          const projected = await g.call<
            Awaited<ReturnType<typeof projectGalleryPoints>>
          >("projectGalleryPoints", object.id, corners);
          const left = Math.min(...projected.map((point) => point.x));
          const right = Math.max(...projected.map((point) => point.x));
          const top = Math.min(...projected.map((point) => point.y));
          const bottom = Math.max(...projected.map((point) => point.y));
          const contour = await g.call<ReturnType<typeof captureColorRegion>>(
            "captureColorRegion",
            frame.label,
            [255, 231, 0],
          );
          await scenario.evidence.record("marker-contour", {
            object: object.id,
            contour,
            expected: { left, right, top, bottom },
          });
          assert.ok(contour.bounds);
          assert.ok(contour.bounds.left >= left * frame.frame.width - 2);
          assert.ok(contour.bounds.right <= right * frame.frame.width + 2);
          assert.ok(contour.bounds.top >= top * frame.frame.height - 2);
          assert.ok(contour.bounds.bottom <= bottom * frame.frame.height + 2);
          for (const [x, y] of [
            [(left + right) / 2, top - 0.005],
            [(left + right) / 2, bottom + 0.005],
            [left - 0.005, (top + bottom) / 2],
            [right + 0.005, (top + bottom) / 2],
          ]) {
            const result = await g.call<GeometryPickResultEvent>(
              "cameraQuery",
              {
                type: "GeometryPickQuery",
                x,
                y,
                ...projected[0]!.viewport,
              },
            );
            assert.ok(result.ok);
            assert.notEqual(
              result.hit?.entity,
              entity(frame.inspection, object.id).id,
              "the marker cannot be picked outside its projected bounds",
            );
          }
        }
      }
      await g.click("lighting-cube");
      await g.capture("cube-before-edit");
      await g.page.locator("#object-roughness").fill("0.9");
      await g.page.locator("#object-metallic").fill("0.7");
      await g.page.locator("#object-color").fill("#ab4eff");
      await g.page.locator("#object-param-width").fill("1.8");
      const cube = await g.capture("cube-after-edit");
      const mesh = entity(cube.inspection, "lighting-cube").effective.find(
        (entry) => String(entry.fields.source).startsWith("ipp://mesh/cube"),
      )!;
      assert.equal(
        new URL(String(mesh.fields.source)).searchParams.get("width"),
        "1.8",
      );
      assert.ok(
        (await g.difference("cube-before-edit", "cube-after-edit"))
          .changedPixels > 200,
      );
      await g.click("lighting-sphere");
      assert.equal(
        await g.page.locator("#object-roughness").inputValue(),
        "0.38",
      );
      await g.page.locator("#object-param-radius").fill("0.6");
      await g.capture("sphere-resized");
      await g.click("lighting-spot");
      await g.page.locator("#object-intensity").fill("40");
      await g.page.locator("#object-outer-cone").fill("0.3");
      assert.ok(
        Number(await g.page.locator("#object-inner-cone").inputValue()) < 0.3,
      );
      await g.capture("spot-edited");
      await g.page.locator("#object-select").selectOption("lighting-cube");
      assert.equal(
        await g.page.locator("#object-color").inputValue(),
        "#ab4eff",
      );
      assert.equal(
        await g.page.locator("#object-param-width").inputValue(),
        "1.8",
      );
      await g.navigate("shapes");
      const geometry = await g.capture("geometry-return");
      assert.equal(geometry.frame.drawCalls, 12);
      await g.navigate("lighting");
      await g.seek(0);
      assert.equal(
        await g.page.locator("#object-param-width").inputValue(),
        "1.8",
      );
      assert.deepEqual(
        selected((await g.capture("selection-retained")).inspection),
        ["lighting-cube"],
      );
      await g.page.locator("#object-select").selectOption("");
      assert.deepEqual(
        selected((await g.capture("selection-cleared")).inspection),
        [],
      );
      assert.deepEqual(g.errors, []);
    },
  );
});

test("skinned gallery picking and its selected pills follow both bone segments", {
  timeout: 60_000,
}, async (context) => {
  await runBrowserEnvironment(
    "skinned selection geometry",
    galleryEnvironment,
    context.signal,
    async (scenario) => {
      const g = await openGallery(scenario);
      const symbol = "lighting-skinning";
      await g.navigate("lighting");
      await g.seek(0);
      await g.click(symbol);
      // Bring the whole beam in front of the other objects so both bones are exposed.
      await g.page.locator("#object-position-x").fill("-2.5");
      await g.page.locator("#object-position-y").fill("1.5");
      await g.page.locator("#object-position-z").fill("2.5");
      const rest = await g.capture("pills-rest");
      const beam = entity(rest.inspection, symbol).id;
      assert.equal(rest.frame.drawCalls, 10, "both selected pills render");
      assert.ok(
        rest.inspection.resources.some(
          (resource) =>
            resource.source.endsWith("/assets/6/920013#immutable") &&
            resource.status === "loaded",
        ),
      );

      const probe = async (local: number[], part: number | undefined) => {
        const [point] = await g.call<
          Awaited<ReturnType<typeof projectGalleryPoints>>
        >("projectGalleryPoints", symbol, [local]);
        assert.ok(point);
        const result = await g.call<GeometryPickResultEvent>("cameraQuery", {
          type: "GeometryPickQuery",
          x: point.x,
          y: point.y,
          ...point.viewport,
        });
        await scenario.evidence.record("beam-probe", { local, part, result });
        assert.ok(result.ok);
        if (part === undefined) {
          assert.notEqual(
            result.hit?.entity,
            beam,
            "empty space is not pickable",
          );
        } else {
          assert.equal(result.hit?.entity, beam);
          assert.equal(result.hit.part, part);
        }
      };
      const yellow = (label: string) =>
        g.call<{ count: number; x: number; y: number }>(
          "captureColorRegion",
          label,
          [255, 231, 0],
        );
      await probe([0, -0.75, 0], 0);
      await probe([0, 0.85, 0], 1);
      const restOutline = await yellow("pills-rest");
      assert.ok(restOutline.count > 20);

      await g.seek(4, symbol);
      const bent = await g.capture("pills-bent");
      assert.equal(bent.frame.drawCalls, 10);
      const angle = (72 * Math.PI) / 180;
      const upper = [-0.85 * Math.sin(angle), 0.85 * Math.cos(angle), 0];
      await probe([0, -0.75, 0], 0);
      await probe(upper, 1);
      await probe([0, 0.9, 0], undefined);
      // Inside the old broad mesh enclosure, between the bent segments.
      await probe([-0.75, -0.65, 0], undefined);
      const bentOutline = await yellow("pills-bent");
      assert.ok(bentOutline.count > 20);
      assert.ok(
        restOutline.x - bentOutline.x > 5,
        "the yellow contour moves left with the upper bone",
      );

      await g.page.locator("#object-scale").fill("1.25");
      await g.page.locator("#object-param-width").fill("0.7");
      await g.page.locator("#object-position-z").fill("2");
      const edited = await g.capture("pills-edited");
      assert.equal(edited.frame.drawCalls, 10);
      await probe([0, -0.75, 0], 0);
      await probe(upper, 1);
      assert.ok((await yellow("pills-edited")).count > 20);
      assert.ok(
        (await g.difference("pills-bent", "pills-edited")).changedPixels > 100,
      );
      assert.deepEqual(selected(edited.inspection), [symbol]);
      assert.deepEqual(g.errors, []);
    },
  );
});
