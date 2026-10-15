import {
  SkinProvider,
  Skin,
  Button,
  Checkbox,
  Slider,
  TextInput,
  Color,
  ScrollView,
  VirtualList,
} from "../src/gui.js";
import { AttachmentContext } from "../src/composition/attached-world.js";
/**
 * Declarations of the GUI kit's root (the `GuiKit` declaration with its themes
 * and reduced motion, rows, text, icons and the secondary button): which
 * entities, components, links, themes and animation each composition writes,
 * through the real reconciler against the recording World in
 * `gui-kit-support.ts`. Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import { useGuiKit, type GuiKitScope } from "../src/gui-kit/kit.js";
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, type ReactNode } from "react";
import type { SystemCommand } from "@ipp/client";
import { Children, Entity, createRoot } from "../src/index.js";
import {
  GUI_KIT_ICONS,
  GuiKit,
  Form,
  Field,
  FormSection,
  TextField,
  Dropdown,
  Icon,
  Row,
  Column,
  Toolbar,
  Button as KitButton,
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
  skinFixture,
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
      {
        contract: contract(),
        id: "ipp-kit",
        skin: skinFixture(),
        font: FONT,
        fontSize: 16,
      },
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
  await assert.rejects(
    bare.render(h(GuiKit)),
    /SkinProvider needs a skin, font and fontSize/,
  );
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
        id: "ipp-kit",
        skin: skinFixture(),
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
      { id: "row", height: 48, layout: { padding_top: 4, padding_bottom: 4 } },
      h(TextLine, { id: "name", text: "Gain", tone: "accent", size: "small" }),
      h(Icon, { id: "mark", icon: "sync", tone: "amber" }),
    ),
    { fontSize: 32 },
  );
  assert.equal(world.fields("row", "GuiLayout").get("height"), 48);
  assert.deepEqual(world.children("row"), ["name", "mark"]);
  assert.equal(world.fields("row", "GuiLayout").get("align_items"), 0);
  assert.equal(world.fields("row", "GuiLayout").get("min_height"), 0);
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

test("appearance changes keep native entities and propagate through nested kit scopes", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  let scope: GuiKitScope | undefined;
  function Probe() {
    scope = useGuiKit();
    return h(SecondaryButton, { id: "appearance-action", label: "Save" });
  }
  const renderAppearance = async (
    accent: typeof TOKENS.accent,
    theme: string,
  ) => {
    await root.render(
      h(
        GuiKit,
        {
          contract: contract(),
          font: FONT,
          fontSize: 16,
          appearance: {
            tokens: { ...TOKENS, accent },
            themes: { secondary: theme, secondarySmall: theme },
            controls: { textInput: "glass/input" },
          },
        },
        h(Entity, { id: theme }),
        h(GuiKit, { fontSize: 24 }, h(Probe)),
      ),
    );
  };
  await renderAppearance(TOKENS.accent, "glass/secondary");
  const entity = world.entity("appearance-action");
  assert.equal(scope!.theme("secondary"), "glass/secondary");
  assert.throws(
    () => scope!.theme("container"),
    /does not define the container/,
  );
  assert.equal(scope!.controlTheme("textInput"), "glass/input");
  assert.equal(scope!.controlTheme("scroll"), undefined);
  assert.equal(scope!.unit(8), 12);
  await renderAppearance(TOKENS.accent, "other/secondary");
  assert.equal(world.entity("appearance-action"), entity);
  assert.equal(scope!.theme("secondary"), "other/secondary");
  assert.deepEqual(scope!.color("accent"), TOKENS.accent);
  await root.unmount();
});

test("SkinProvider resolves native roles, explicit overrides and opt-out without replacing controls", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  const inputs = contract();
  const skin = skinFixture(inputs);
  const parts = inputs.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([[0, { part: PART.background, opacity: 0.5 }]]),
  });
  const explicit = () => h(Skin, { parts });
  const view = (override: "before" | "after" | false, alternate = false) =>
    h(
      SkinProvider,
      {
        id: "auto",
        skin: alternate
          ? { ...skin, controls: { ...skin.controls, button: "check" } }
          : skin,
        contract: inputs,
        font: FONT,
        fontSize: 16,
      },
      h(
        Entity,
        { id: "button" },
        override === "before" && explicit(),
        h(Button, { label: "Go" }),
        override === "after" && explicit(),
      ),
      h(Entity, { id: "checkbox" }, h(Checkbox)),
      h(Entity, { id: "switch" }, h(Checkbox, { skin: "switch" })),
      h(Entity, { id: "slider" }, h(Slider)),
      h(Entity, { id: "dial" }, h(Slider, { axis: 2 })),
      h(Entity, { id: "text" }, h(TextInput)),
      h(Entity, { id: "color" }, h(Color)),
      h(Entity, { id: "scroll" }, h(ScrollView)),
      h(Entity, { id: "list" }, h(VirtualList, { renderItem: () => null })),
      h(Entity, { id: "bare" }, h(Button, { skin: false })),
      h(GuiKit, { fontSize: 20 }, h(Entity, { id: "inherited" }, h(Button))),
      h(
        SkinProvider,
        { id: "nested", skin },
        h(Entity, { id: "nested-button" }, h(Button)),
      ),
      h(
        AttachmentContext,
        { value: null },
        h(Entity, { id: "other-world" }, h(Button)),
      ),
    );
  try {
    await root.render(view(false));
    const native = world.fields("button", "GuiButton");
    for (const [id, theme] of Object.entries({
      button: "secondary",
      checkbox: "check",
      switch: "check",
      slider: "frame",
      dial: "frame",
      text: "cellEditor",
      color: "frame",
      scroll: "frame",
      list: "frame",
      inherited: "secondary",
    }))
      assert.equal(
        world.fields(id, "GuiSkin").get("theme"),
        `auto/theme/${theme}`,
      );
    assert.equal(
      world.fields("nested-button", "GuiSkin").get("theme"),
      "nested/theme/secondary",
    );
    for (const id of ["bare", "other-world"])
      assert.equal(world.entity(id).components.has("GuiSkin"), false);

    for (const order of ["before", "after"] as const) {
      await root.render(view(order));
      assert.equal(world.fields("button", "GuiSkin").get("theme"), 0n);
      assert.deepEqual(world.fields("button", "GuiSkin").get("parts"), parts);
      // A changed provider must not overwrite a mounted explicit skin.
      await root.render(view(order, true));
      assert.equal(world.fields("button", "GuiSkin").get("theme"), 0n);
      await root.render(view(false, true));
      assert.equal(
        world.fields("button", "GuiSkin").get("theme"),
        "auto/theme/check",
      );
      assert.deepEqual(
        world.fields("button", "GuiSkin").get("parts"),
        inputs.GuiSkin.encodeParts({ nextSlot: 0, rows: new Map() }),
      );
      assert.equal(world.fields("button", "GuiButton"), native);
    }
  } finally {
    await root.unmount();
  }
});

test("native controls outside any skin provider stay unstyled", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  try {
    await root.render(
      h(Entity, { id: "bare" }, h(Button, { label: "No implicit skin" })),
    );
    assert.equal(world.entity("bare").components.has("GuiSkin"), false);
  } finally {
    await root.unmount();
  }
});

test("flow defaults center Row children without adding layout entities and preserve authored overrides", async () => {
  const scene = (explicit: boolean) => [
    h(
      Row,
      {
        key: "row",
        id: "natural",
        ...(explicit
          ? {
              height: 20,
              gap: 4,
              alignItems: "end" as const,
              layout: { gap: 7, align_items: -1 },
            }
          : {}),
      },
      h(KitButton, {
        id: "natural/button",
        label: "Go",
        layout: { align_y: 1 },
      }),
    ),
    h(
      Column,
      {
        key: "column",
        id: "column",
        ...(explicit ? { gap: 9, alignItems: "center" as const } : {}),
      },
      h(TextLine, { id: "column/text", text: "Content" }),
    ),
    h(
      Toolbar,
      {
        key: "toolbar",
        id: "toolbar",
        ...(explicit ? { gap: 0, alignItems: "start" as const } : {}),
      },
      h(KitButton, { id: "toolbar/button", label: "Go" }),
    ),
  ];
  const { world, draw } = await render(scene(false), { fontSize: 32 });
  const row = world.entity("natural");
  const button = world.entity("natural/button");
  const fields = (id: string) => world.fields(id, "GuiLayout");
  assert.deepEqual(world.children("natural"), ["natural/button"]);
  assert.equal(fields("natural").get("height"), -1);
  assert.equal(fields("natural").get("fit_height"), true);
  assert.equal(fields("natural").get("min_height"), TOKENS.row * 2);
  assert.equal(fields("natural").get("align_items"), 0);
  assert.equal(fields("natural").get("gap"), 0);
  assert.equal(
    fields("natural/button").get("align_y"),
    1,
    "parent alignment leaves the child's explicit alignment authored",
  );
  assert.equal(fields("column").get("align_items"), 2);
  assert.equal(fields("column").get("gap"), 0);
  assert.equal(fields("toolbar").get("gap"), TOKENS.inset);
  assert.equal(fields("toolbar").get("align_items"), 0);
  await draw(scene(true));
  assert.equal(world.entity("natural"), row);
  assert.equal(world.entity("natural/button"), button);
  assert.equal(fields("natural").get("height"), 20);
  assert.equal(
    fields("natural").get("min_height"),
    0,
    "an explicit compact row height does not inherit the ordinary minimum",
  );
  assert.equal(
    fields("natural").get("gap"),
    7,
    "raw layout gap wins over named gap",
  );
  assert.equal(
    fields("natural").get("align_items"),
    -1,
    "raw layout cross alignment wins over named alignment",
  );
  assert.equal(fields("column").get("gap"), 9);
  assert.equal(fields("column").get("align_items"), 0);
  assert.equal(
    fields("toolbar").get("gap"),
    0,
    "explicit zero disables semantic toolbar spacing",
  );
  await draw(scene(false));
  assert.equal(world.entity("natural"), row);
  assert.equal(fields("natural").get("height"), -1);
  assert.equal(fields("natural").get("min_height"), TOKENS.row * 2);
  assert.equal(fields("natural").get("align_items"), 0);
  assert.equal(fields("natural").get("gap"), 0);
  assert.equal(fields("column").get("align_items"), 2);
  assert.equal(fields("column").get("gap"), 0);
  assert.equal(fields("toolbar").get("gap"), TOKENS.inset);
});

test("semantic forms share configurable spacing, fill defaults and stable control identities", async () => {
  const world = new KitWorld();
  const root = createRoot(world);
  let scope!: GuiKitScope;
  function Fields() {
    scope = useGuiKit();
    return h(
      Form,
      { id: "form" },
      h(
        FormSection,
        { id: "section", title: "Preferences" },
        h(
          Field,
          { id: "field", label: "Name", description: "Shown to others" },
          h(TextField, { id: "name", defaultValue: "Explorer" }),
        ),
        h(Dropdown, { id: "channel", options: [{ key: "a", label: "Alpha" }] }),
        h(TextField, { id: "narrow", width: 100, layout: { width: 120 } }),
      ),
      h(
        Field,
        { id: "joined", gap: 0, label: "Joined" },
        h(TextField, { id: "joined/input", layout: { gap: 0 } }),
      ),
    );
  }
  const draw = async (custom: boolean) => {
    const inputs = contract();
    await root.render(
      h(
        GuiKit,
        {
          id: "forms-kit",
          contract: inputs,
          skin: skinFixture({
            ...inputs,
            tokens: {
              ...TOKENS,
              ...(custom
                ? { spacing: { label: 0, field: 12, section: 20 } }
                : {}),
            },
          }),
          font: FONT,
          fontSize: 32,
        },
        h(Fields),
      ),
    );
    await root.flush();
  };
  try {
    await draw(false);
    const name = world.entity("name/input");
    const trigger = world.entity("channel");
    assert.deepEqual(scope.spacing, {
      compact: 8,
      label: 16,
      field: 32,
      section: 48,
      inset: 32,
    });
    assert.equal(world.fields("form", "GuiLayout").get("gap"), 32);
    assert.equal(world.fields("section", "GuiLayout").get("gap"), 32);
    assert.equal(world.fields("section/heading", "GuiLayout").get("gap"), 16);
    assert.equal(world.fields("section/body", "GuiLayout").get("gap"), 32);
    assert.equal(world.fields("field", "GuiLayout").get("gap"), 16);
    assert.deepEqual(world.children("field"), [
      "field/label",
      "name",
      "field/description",
    ]);
    assert.equal(world.fields("name", "GuiLayout").get("width"), -1);
    assert.deepEqual(world.children("name"), ["name/row"]);
    assert.equal(world.fields("name/row", "GuiLayout").get("width"), -1);
    assert.equal(world.fields("name/input", "GuiLayout").get("flex"), 1);
    assert.equal(world.fields("channel", "GuiLayout").get("width"), -1);
    assert.equal(
      world.fields("narrow", "GuiLayout").get("width"),
      120,
      "explicit raw dimensions win over named and form defaults",
    );
    assert.equal(world.fields("joined", "GuiLayout").get("gap"), 0);
    assert.equal(world.fields("joined/input", "GuiLayout").get("gap"), 0);
    await draw(true);
    assert.equal(
      scope.spacing.inset,
      32,
      "role overrides never create a second inset owner",
    );
    assert.equal(world.fields("form", "GuiLayout").get("gap"), 24);
    assert.equal(
      world.fields("field", "GuiLayout").get("gap"),
      0,
      "zero is an intentional spacing override",
    );
    assert.equal(world.fields("name", "GuiLayout").get("gap"), 0);
    assert.equal(world.entity("name/input"), name);
    assert.equal(world.entity("channel"), trigger);
  } finally {
    await root.unmount();
  }
});
