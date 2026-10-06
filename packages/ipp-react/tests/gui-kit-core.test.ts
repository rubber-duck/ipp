/**
 * Declarations of the GUI kit's root (the `GuiKit` declaration with its themes
 * and reduced motion, rows, text, icons and the secondary button): which
 * entities, components, links, themes and animation each composition writes,
 * through the real reconciler against the recording World in
 * `gui-kit-support.ts`. Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, type ReactNode } from "react";
import type { SystemCommand } from "@ipp/client";
import { Children, Entity, createRoot } from "../src/index.js";
import {
  GUI_KIT_ICONS,
  GuiKit,
  Icon,
  Row,
  SecondaryButton,
  Separator,
  TextLine,
  Spinner,
} from "../src/gui-kit.js";
import {
  TOKENS,
  PART,
  PRESSED,
  CHECK_MARK,
  contract,
  KitWorld,
  FONT,
  render,
  themeRows,
  skinRows,
  ring,
} from "./gui-kit-support.js";

test("GuiKit declares the kit's themes once per World as top-level theme entities", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  await root.render(
    h(
      GuiKit,
      { contract: contract(), font: FONT, fontSize: 16 },
      h(
        GuiKit,
        { fontSize: 32 },
        h(
          Entity,
          { id: "panel" },
          h(Children, null, h(Separator, { id: "line" })),
        ),
      ),
    ),
  );
  const themes = [...world.entities.keys()].filter((name) =>
    name.startsWith("ipp-kit/theme/"),
  );
  assert.ok(themes.includes("ipp-kit/theme/container"));
  assert.ok(themes.includes("ipp-kit/theme/alertWarning"));
  // Looks drawn at another type size scale their em with it.
  const ems: Record<string, number> = {
    secondarySmall: TOKENS.textSmall,
    secondarySmallAmber: TOKENS.textSmall,
    dockedIcon: TOKENS.icon,
    dockedIconAmber: TOKENS.icon,
    secondaryIcon: TOKENS.icon,
    secondaryIconAmber: TOKENS.icon,
  };
  for (const theme of themes) {
    assert.equal(world.entity(theme).parent, null);
    assert.equal(
      world.fields(theme, "GuiTheme").get("em"),
      ems[theme.replace("ipp-kit/theme/", "")] ?? TOKENS.em,
      theme,
    );
  }

  // Rows are tokens, in the contract's encoding.
  assert.deepEqual(themeRows(world, "alertWarning").rows, [
    [
      0,
      {
        part: PART.background,
        color: [...TOKENS.amber.slice(0, 3), 0.01],
        border_width: TOKENS.lineWidth,
        border_color: TOKENS.amber,
        corner_cut: [TOKENS.cut, 0, TOKENS.cut, 0],
      },
    ],
  ]);
  // The check mark is the checkbox look's own, drawn as a Background.
  assert.deepEqual(themeRows(world, "check").rows, [
    [0, { ...CHECK_MARK, part: PART.background }],
  ]);
  // A small-label secondary button keeps the look's lengths against body text.
  assert.equal(
    world.fields("ipp-kit/theme/secondarySmall", "GuiTheme").get("em"),
    13,
  );
  assert.deepEqual(themeRows(world, "expanderHeader").rows, [
    [0, { part: PRESSED, color: TOKENS.surface }],
  ]);

  // The nested kit inherits and changes only the size.
  assert.equal(world.fields("line", "GuiLayout").get("height"), 2.5);
  await root.unmount();
});

test("kit components need a GuiKit in their own World", async () => {
  const root = createRoot(new KitWorld(), { onError: () => {} });
  await assert.rejects(
    root.render(
      h(
        Entity,
        { id: "panel" },
        h(Children, null, h(Separator, { id: "line" })),
      ),
    ),
    /enclosing GuiKit/,
  );
  const bare = createRoot(new KitWorld(), { onError: () => {} });
  await assert.rejects(bare.render(h(GuiKit)), /contract, font and fontSize/);
});

/** The GUI preferences update that sets reduced motion to `reducedMotion`. */
const preference = (reducedMotion: boolean): SystemCommand => ({
  type: "GuiPreferencesUpdateCommand",
  reducedMotion,
});

test("the World's GuiKit sends its reduced motion once per change; nested kits send nothing", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  const kit = (reducedMotion?: boolean, content?: ReactNode) =>
    h(
      GuiKit,
      {
        contract: contract(),
        font: FONT,
        fontSize: 16,
        ...(reducedMotion === undefined ? {} : { reducedMotion }),
      },
      h(Entity, { id: "panel" }, h(Children, null, content)),
    );
  const sent: SystemCommand[] = [];

  // Omitted, the kit leaves the World's preference alone.
  await root.render(kit());
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true));
  sent.push(preference(true));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true, h(Separator, { id: "line" })));
  assert.deepEqual(world.commands, sent);

  // A nested kit changes only the kit animations beneath it.
  await root.render(
    kit(true, h(GuiKit, { reducedMotion: false }, h(Separator, { id: "in" }))),
  );
  await root.render(
    kit(false, h(GuiKit, { reducedMotion: true }, h(Separator, { id: "in" }))),
  );
  sent.push(preference(false));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(false));
  assert.deepEqual(world.commands, sent);

  // Like the themes, a preference the kit turned on goes with the setting or
  // the kit, while unmounting the root leaves both.
  await root.render(kit(true));
  await root.render(kit());
  sent.push(preference(true), preference(false));
  assert.deepEqual(world.commands, sent);
  await root.render(kit(true));
  await root.render(h(Entity, { id: "bare" }));
  sent.push(preference(true), preference(false));
  assert.deepEqual(world.commands, sent);
  assert.equal(world.entities.has("ipp-kit/theme/container"), false);
  await root.render(kit(true));
  sent.push(preference(true));
  await root.unmount();
  assert.deepEqual(world.commands, sent);
  assert.ok(world.entities.has("ipp-kit/theme/container"));
});

test("kit animations follow the nearest GuiKit, so a nested kit holds part of a World still", async () => {
  const spinners = (nested: boolean) => [
    h(Spinner, { key: "running", id: "running", label: "Running" }),
    h(
      GuiKit,
      { key: "nested", reducedMotion: nested },
      h(Spinner, { id: "nested", label: "Nested" }),
    ),
  ];
  const turning = (world: KitWorld) =>
    [...world.controllers.values()].map((controller) =>
      world.symbol(controller.drivers[0]!.target),
    );
  const { world, draw } = await render(spinners(true));
  assert.deepEqual(turning(world), ["running/arc"]);
  assert.deepEqual(skinRows(world, "nested/arc"), ring(3, 0.25));
  assert.deepEqual(world.commands, []);

  // Under the World's setting, only the kit that turns motion back on moves.
  await draw(spinners(false), { reducedMotion: true });
  assert.deepEqual(turning(world), ["nested/arc"]);
  assert.deepEqual(world.commands, [preference(true)]);
  await draw(spinners(true), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(world.commands, [preference(true)]);
});

test("Row centres its children on its own height; text and icons take the type scale and palette", async () => {
  const { world } = await render(
    h(
      Row,
      { id: "row", height: 24, layout: { padding_top: 4, padding_bottom: 4 } },
      h(TextLine, { id: "name", text: "Gain", tone: "accent", size: "small" }),
      h(Icon, { id: "mark", icon: "sync", tone: "amber" }),
    ),
    { fontSize: 32 },
  );
  assert.equal(world.fields("row", "GuiLayout").get("height"), 48);
  // The strut is as tall as the row's content box, so children centre on it.
  assert.deepEqual(world.children("row"), ["row/strut", "name", "mark"]);
  assert.equal(world.fields("row/strut", "GuiLayout").get("height"), 40);
  assert.equal(world.fields("name", "GuiLayout").get("align_y"), 0);
  assert.equal(world.fields("name", "CanvasText").get("font_size"), 26);
  assert.deepEqual(world.tone("name"), TOKENS.accent);
  assert.equal(
    world.fields("mark", "CanvasText").get("text"),
    GUI_KIT_ICONS.sync,
  );
  assert.equal(world.fields("mark", "CanvasText").get("font_size"), 48);
  assert.deepEqual(world.tone("mark"), TOKENS.amber);
});

test("SecondaryButton takes the amber variant at the small type size", async () => {
  const { world } = await render([
    h(SecondaryButton, { key: 1, id: "clear", label: "CLEAR" }),
    h(SecondaryButton, { key: 2, id: "purge", label: "PURGE", amber: true }),
  ]);
  assert.equal(world.skin("clear"), "secondarySmall");
  assert.equal(world.skin("purge"), "secondarySmallAmber");
  assert.equal(
    world.fields("ipp-kit/theme/secondarySmallAmber", "GuiTheme").get("em"),
    TOKENS.textSmall,
  );
  // The amber look keeps the secondary geometry and swaps its colours.
  assert.deepEqual(themeRows(world, "secondarySmallAmber").rows, [
    [
      0,
      {
        part: PART.background,
        corner_cut: [4, 0, 4, 0],
        border_color: TOKENS.amber,
      },
    ],
  ]);
  assert.equal(
    world.fields("purge", "GuiLayout").get("width"),
    world.fields("clear", "GuiLayout").get("width"),
  );
});
