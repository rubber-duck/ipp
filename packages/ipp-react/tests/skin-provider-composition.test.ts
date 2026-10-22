import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h } from "react";
import { Children, Entity, createRoot } from "../src/index.js";
import { Button, Font, Layout, SkinProvider } from "../src/gui.js";
import type { ReactWorldContract } from "../src/scope/world.js";
import { contract, FONT, KitWorld, skinFixture } from "./gui-kit-support.js";

test("a provider inside native Children keeps its themes outside layout and resolves the root contract", async () => {
  const world = new KitWorld();
  const inputs = contract();
  const root = createRoot(world, {
    contract: {
      ...inputs,
      SCHEMA_HASH: world.schemaHash,
      components: world.components,
      encodeRowsTable: () => {
        throw new Error("unused row encoder");
      },
    } as unknown as ReactWorldContract,
  });
  const skin = skinFixture(inputs);
  const scene = (size: number) =>
    h(
      Entity,
      { id: "layout" },
      h(Layout, { width: 400, height: 100 }),
      h(
        Children,
        null,
        h(
          SkinProvider,
          { id: "nested", skin, font: FONT, fontSize: size },
          h(Entity, { id: "button" }, h(Button, { label: "Save" })),
        ),
      ),
    );
  try {
    await root.render(scene(16));
    const button = world.entity("button");
    assert.deepEqual(world.children("layout"), ["button"]);
    const themes = [...world.entities.keys()].filter((id) =>
      id.startsWith("nested/theme/"),
    );
    assert.ok(themes.length > 0);
    for (const id of themes) assert.equal(world.entity(id).parent, null);
    assert.equal(world.fields("button", "GuiFont").get("source"), FONT);
    assert.equal(world.fields("button", "GuiFont").get("font_size"), 16);
    await root.render(scene(24));
    assert.equal(world.entity("button"), button);
    assert.equal(world.fields("button", "GuiFont").get("font_size"), 24);
    assert.deepEqual(world.children("layout"), ["button"]);
  } finally {
    await root.unmount();
  }
});

test("explicit local and native ancestor fonts take precedence over provider fallback", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  const inputs = contract();
  const skin = skinFixture(inputs);
  const scene = (order: "before" | "after" | "none", ancestor: boolean) =>
    h(
      SkinProvider,
      { id: "skin", skin, contract: inputs, font: FONT, fontSize: 16 },
      h(
        Entity,
        { id: "parent" },
        ancestor && h(Font, { source: "memory:ancestor", font_size: 22 }),
        h(
          Children,
          null,
          h(
            Entity,
            { id: "button" },
            order === "before" &&
              h(Font, { source: "memory:explicit", font_size: 18 }),
            h(Button, { label: "Go" }),
            order === "after" &&
              h(Font, { source: "memory:explicit", font_size: 18 }),
          ),
        ),
        // JSX nesting alone does not make a native child.
        h(Entity, { id: "unlinked" }, h(Button, { label: "Elsewhere" })),
      ),
    );
  try {
    for (const order of ["before", "after"] as const) {
      await root.render(scene(order, true));
      assert.equal(
        world.fields("button", "GuiFont").get("source"),
        "memory:explicit",
      );
      assert.equal(world.fields("button", "GuiFont").get("font_size"), 18);
    }
    await root.render(scene("none", true));
    assert.equal(world.entity("button").components.has("GuiFont"), false);
    assert.equal(world.fields("unlinked", "GuiFont").get("source"), FONT);
    await root.render(scene("none", false));
    assert.equal(world.fields("button", "GuiFont").get("source"), FONT);
  } finally {
    await root.unmount();
  }
});

test("body font changes inherit the independent icon font", async () => {
  const { Icon, TextLine } = await import("../src/gui-kit.js");
  const world = new KitWorld();
  const root = createRoot(world);
  const inputs = contract();
  const scene = (font: string) =>
    h(
      SkinProvider,
      {
        contract: inputs,
        skin: skinFixture(inputs),
        font: FONT,
        iconFont: "memory:icons",
        fontSize: 16,
      },
      h(
        SkinProvider,
        { font },
        h(Icon, { id: "icon", icon: "search" }),
        h(TextLine, { id: "text", text: "Search" }),
      ),
    );
  try {
    await root.render(scene("memory:noto"));
    assert.equal(
      world.fields("text", "CanvasText").get("source"),
      "memory:noto",
    );
    assert.equal(
      world.fields("icon", "CanvasText").get("source"),
      "memory:icons",
    );
    const icon = world.entity("icon");
    await root.render(scene("memory:another-body-font"));
    assert.equal(world.entity("icon"), icon);
    assert.equal(
      world.fields("icon", "CanvasText").get("source"),
      "memory:icons",
    );
  } finally {
    await root.unmount();
  }
});
