import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h } from "react";
import {
  Entity,
  PlotBars2d,
  PlotGridBars3d,
  PlotFrame2d,
  PlotFrame3d,
  createRoot,
  CustomMaterial,
  PlotCalloutStyle,
  PlotCalloutPaint,
} from "../src/index.js";
import { Paint, SkinProvider } from "../src/gui.js";
import { contract, FONT, KitWorld, skinFixture } from "./gui-kit-support.js";

test("GUI skin context supplies chart finishes while explicit fields and opt-outs win", async () => {
  const world = new KitWorld(),
    root = createRoot(world),
    inputs = contract();
  const native = skinFixture(inputs);
  const glass = {
    ...native,
    plots: () => ({
      paint: { source: "memory:glass-paint" },
      material: { source: "memory:glass-mesh", alpha_mode: 2 },
      frame2d: { grid_alpha: 0.2 },
      frame3d: { mark_outlines: false },
    }),
  };
  const plots = {
    components: world.components,
    encodeRowsTable: () => {
      throw new Error("No authored rows");
    },
  };
  const scene = (skin: typeof native) =>
    h(
      SkinProvider,
      { id: "skin", contract: inputs, skin, font: FONT, fontSize: 16 },
      h(
        Entity,
        { id: "planar" },
        h(PlotFrame2d, { grid_alpha: 0.7 }),
        h(PlotBars2d, { contract: plots }),
      ),
      h(
        Entity,
        { id: "spatial" },
        h(PlotFrame3d),
        h(PlotGridBars3d, { contract: plots }),
      ),
      h(
        Entity,
        { id: "custom" },
        h(PlotFrame2d, { skin: false }),
        h(PlotBars2d, { contract: plots, skin: false }),
        h(Paint, { source: "memory:authored" }),
      ),
    );
  try {
    await root.render(scene(glass));
    const planar = world.entity("planar"),
      spatial = world.entity("spatial");
    assert.equal(
      world.fields("planar", "CanvasPaint").get("source"),
      "memory:glass-paint",
    );
    assert.equal(world.fields("planar", "PlotFrame2d").get("grid_alpha"), 0.7);
    assert.equal(
      world.fields("spatial", "CustomMaterial").get("alpha_mode"),
      2,
    );
    assert.equal(
      world.fields("spatial", "PlotFrame3d").get("mark_outlines"),
      false,
    );
    assert.equal(
      world.fields("custom", "CanvasPaint").get("source"),
      "memory:authored",
    );
    await root.render(scene(native));
    assert.equal(world.entity("planar"), planar);
    assert.equal(world.entity("spatial"), spatial);
    assert.equal(planar.components.has("CanvasPaint"), false);
    assert.equal(spatial.components.has("CustomMaterial"), false);
    assert.equal(
      world.fields("custom", "CanvasPaint").get("source"),
      "memory:authored",
    );
  } finally {
    await root.unmount();
  }
});

test("explicit chart appearance wins in either declaration order and restores inheritance on removal", async () => {
  for (const explicitFirst of [true, false]) {
    const world = new KitWorld(),
      root = createRoot(world),
      inputs = contract();
    const skin = (source: string) => ({
      ...skinFixture(inputs),
      plots: () => ({
        paint: { source },
        material: { source, alpha_mode: 2 },
        callout: { padding_x: 12 },
        calloutPaint: { source },
      }),
    });
    const plots = {
      components: world.components,
      encodeRowsTable: () => {
        throw new Error("No rows");
      },
    };
    const scene = (explicit: boolean, source = "memory:inherited") =>
      h(
        SkinProvider,
        {
          id: "skin",
          contract: inputs,
          skin: skin(source),
          font: FONT,
          fontSize: 16,
        },
        ...[false, true].map((spatial) => {
          const chart = h(spatial ? PlotGridBars3d : PlotBars2d, {
            key: "chart",
            contract: plots,
          });
          const overrides = explicit
            ? [
                h(spatial ? CustomMaterial : Paint, {
                  key: "finish",
                  source: "memory:explicit",
                }),
                h(PlotCalloutStyle, { key: "callout", padding_x: 7 }),
                h(PlotCalloutPaint, {
                  key: "callout-paint",
                  source: "memory:explicit-callout",
                }),
              ]
            : [];
          return h(
            Entity,
            { key: String(spatial), id: spatial ? "spatial" : "planar" },
            ...(explicitFirst ? [...overrides, chart] : [chart, ...overrides]),
          );
        }),
      );
    try {
      await root.render(scene(true));
      for (const [id, component] of [
        ["planar", "CanvasPaint"],
        ["spatial", "CustomMaterial"],
      ]) {
        assert.equal(
          world.fields(id!, component!).get("source"),
          "memory:explicit",
        );
        assert.equal(world.fields(id!, "PlotCalloutStyle").get("padding_x"), 7);
        assert.equal(
          world.fields(id!, "PlotCalloutPaint").get("source"),
          "memory:explicit-callout",
        );
      }
      const entity = world.entity("planar");
      await root.render(scene(true, "memory:changed"));
      assert.equal(
        world.fields("planar", "CanvasPaint").get("source"),
        "memory:explicit",
      );
      await root.render(scene(false, "memory:changed"));
      assert.equal(world.entity("planar"), entity);
      assert.equal(
        world.fields("planar", "CanvasPaint").get("source"),
        "memory:changed",
      );
      assert.equal(
        world.fields("spatial", "CustomMaterial").get("source"),
        "memory:changed",
      );
      assert.equal(
        world.fields("planar", "PlotCalloutStyle").get("padding_x"),
        12,
      );
      await root.render(scene(true));
      assert.equal(
        world.fields("planar", "CanvasPaint").get("source"),
        "memory:explicit",
      );
    } finally {
      await root.unmount();
    }
  }
});
