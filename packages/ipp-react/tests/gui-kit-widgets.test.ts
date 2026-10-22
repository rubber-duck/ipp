import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, Fragment, useState } from "react";
import {
  Button,
  Checkbox,
  Switch,
  TextField,
  Column,
  OverlayStack,
  Row,
  TextLine,
  Autocomplete,
  Dropdown,
  NumericStepper,
  Popover,
  Dialog,
  LabelledSlider,
  ColorPicker,
  RadioGroup,
  GuiKit,
  type GuiKitControlName,
} from "../src/gui-kit.js";
import type { GuiControlHandle } from "../src/gui/control-ref.js";
import { cybertronSkin } from "../src/gui-cybertron/skin.js";
import { glassSkin } from "../src/gui-glass/kit.js";
import { compactSizing } from "../src/gui-kit/sizing.js";
import { createRoot } from "../src/index.js";
import { contract, FONT, KitWorld, render, settle } from "./gui-kit-support.js";

test("Switch defaults keep a wide low track at body scale and explicit sizes remain available", async () => {
  const { world, root } = await render(
    h(
      Column,
      null,
      h(Switch, { id: "track", semanticLabel: "Smooth trace" }),
      h(Switch, { id: "explicit", width: 80, height: 20 }),
    ),
    { fontSize: 20 },
  );
  try {
    assert.equal(world.fields("track", "GuiLayout").get("fit_width"), true);
    assert.equal(world.fields("track", "GuiLayout").get("min_width"), 50);
    assert.equal(world.fields("track", "GuiLayout").get("height"), 27.5);
    assert.equal(world.fields("track", "GuiCheckbox").get("label"), "");
    assert.equal(
      world.fields("track", "GuiBehavior").get("semantic_label"),
      "Smooth trace",
    );
    assert.equal(world.fields("explicit", "GuiLayout").get("width"), 80);
    assert.equal(world.fields("explicit", "GuiLayout").get("height"), 20);
  } finally {
    await root.unmount();
  }
});

test("peer skins preserve compact control targets, choice marks and form layout", async () => {
  const inputs = contract();
  const skins = [cybertronSkin(inputs), glassSkin(inputs)];
  for (const key of Object.keys(
    compactSizing,
  ) as (keyof typeof compactSizing)[])
    assert.equal(skins[0]!.tokens[key], skins[1]!.tokens[key], key);
  for (const role of Object.keys(skins[0]!.controls) as GuiKitControlName[])
    assert.deepEqual(
      skins[0]!.themes[skins[0]!.controls[role]]!.metrics,
      skins[1]!.themes[skins[1]!.controls[role]]!.metrics,
      role,
    );
  for (const skin of skins) {
    for (const [role, visible] of [
      ["checkbox", [18, 18]],
      ["switch", [40, 22]],
    ] as const) {
      const theme = skin.themes[skin.controls[role]]!;
      const metrics = theme.metrics!;
      assert.equal(metrics.min_height, role === "switch" ? 22 : 28);
      if (skin === skins[0]) {
        const row = theme.rows.find(
          (row) =>
            row.part === inputs.guiPaintPartIndex({ part: "background" }),
        );
        const outset = row!.outset as readonly number[];
        assert.equal(metrics.min_width! + 2 * outset[0]!, visible[0]);
        assert.equal(metrics.min_height! + 2 * outset[1]!, visible[1]);
      }
    }
  }
  const world = new KitWorld();
  const root = createRoot(world);
  const layouts: unknown[] = [];
  const identities: unknown[] = [];
  try {
    for (const skin of skins) {
      // This recording World verifies declarations; native captures exercise the real materials.
      const declarations = {
        tokens: skin.tokens,
        controls: skin.controls,
        kit: skin.kit,
        themes: Object.fromEntries(
          Object.entries(skin.themes).map(([name, theme]) => [
            name,
            {
              em: theme.em,
              rows: theme.rows,
              metrics: theme.metrics,
              motion: theme.motion,
            },
          ]),
        ),
      };
      await root.render(
        h(
          GuiKit,
          {
            id: "peer",
            skin: declarations,
            contract: inputs,
            font: FONT,
            fontSize: 16,
          },
          h(
            Column,
            { id: "form" },
            h(Checkbox, { id: "check", label: "Enabled" }),
            h(Switch, { id: "motion", label: "Motion" }),
            h(RadioGroup, {
              id: "axis",
              options: [{ value: "x", label: "X" }],
            }),
          ),
        ),
      );
      identities.push(
        world.entity("check"),
        world.entity("motion"),
        world.entity("axis/x/mark"),
      );
      layouts.push(
        ["form", "check", "motion", "axis", "axis/x/mark", "axis/x/label"].map(
          (id) => [...world.fields(id, "GuiLayout")],
        ),
      );
      assert.equal(world.fields("check", "GuiLayout").get("height"), 28);
      assert.equal(world.fields("motion", "GuiLayout").get("height"), 22);
      assert.equal(world.fields("axis/x/mark", "GuiLayout").get("width"), 28);
    }
    assert.deepEqual(layouts[0], layouts[1]);
    for (let index = 0; index < 3; index++)
      assert.equal(identities[index], identities[index + 3]);
  } finally {
    await root.unmount();
  }
});

test("complete widgets own their entities, fonts, layout and native handles", async () => {
  const button = { current: null as GuiControlHandle | null };
  const { world, root } = await render(
    h(
      Column,
      { id: "form", width: 280, padding: 12 },
      h(
        Row,
        { id: "actions", height: 40 },
        h(Button, { id: "save", ref: button }, "Save"),
      ),
      h(
        Checkbox,
        { id: "enabled", defaultChecked: true, disabled: true },
        "Enable",
      ),
      h(Switch, { id: "motion", label: "Motion" }),
      h(TextField, {
        id: "name",
        label: "Name",
        defaultValue: "Alice",
        error: "Required",
      }),
      h(
        OverlayStack,
        { id: "stack", height: 30 },
        h(TextLine, { id: "line", text: "Ready" }),
      ),
    ),
  );
  assert.equal(world.fields("form", "GuiLayout").get("width"), 280);
  assert.equal(world.fields("form", "GuiLayout").get("padding_left"), 12);
  assert.equal(world.fields("save", "GuiButton").get("label"), "Save");
  assert.equal(world.fields("save", "GuiFont").get("font_size"), 16);
  assert.ok(button.current);
  assert.equal(world.fields("enabled", "GuiCheckbox").get("checked"), true);
  assert.equal(world.fields("enabled", "GuiBehavior").get("enabled"), false);
  assert.equal(world.fields("name/input", "GuiTextInput").get("text"), "Alice");
  assert.equal(
    world.fields("name/input", "GuiBehavior").get("semantic_label"),
    "Name",
  );
  assert.deepEqual(world.children("name"), ["name/field"]);
  assert.deepEqual(world.children("name/field"), [
    "name/field/label",
    "name/row",
    "name/field/error",
  ]);
  assert.equal(world.fields("stack", "GuiLayout").get("kind"), 3);
  await root.unmount();
  assert.equal(button.current, null);
});

test("uncontrolled fields observe runtime changes without writing them back", async () => {
  const checked: boolean[] = [];
  const text: string[] = [];
  const scene = (seed: string) =>
    h(
      Fragment,
      null,
      h(Checkbox, {
        id: "check",
        defaultChecked: true,
        onCheckedChange: (value) => checked.push(value),
      }),
      h(TextField, {
        id: "text",
        defaultValue: seed,
        onChange: (value) => text.push(value),
      }),
    );
  const { world, root, draw } = await render(scene("initial"));
  world.value("check", "GuiCheckbox", { checked: true });
  world.value("text/input", "GuiTextInput", { text: "initial" });
  await settle(root);
  assert.deepEqual(checked, []);
  assert.deepEqual(text, []);
  world.value("check", "GuiCheckbox", { checked: false });
  world.value("text/input", "GuiTextInput", { text: "typed" });
  await settle(root);
  assert.deepEqual(checked, [false]);
  assert.deepEqual(text, ["typed"]);
  await draw(scene("changed seed"));
  assert.equal(world.fields("text/input", "GuiTextInput").get("text"), "typed");
  assert.deepEqual(world.compared, []);
  await root.unmount();
});

test("text and checked application updates synchronize and suppress their echoes", async () => {
  const text: string[] = [];
  const checked: boolean[] = [];
  const scene = (value: string, valueChecked: boolean) =>
    h(
      Fragment,
      null,
      h(TextField, {
        id: "text",
        value,
        onChange: (value) => text.push(value),
      }),
      h(Checkbox, {
        id: "check",
        checked: valueChecked,
        onCheckedChange: (value) => checked.push(value),
      }),
    );
  const { world, root, draw } = await render(scene("first", false));
  await draw(scene("reset", true));
  await settle(root);
  assert.deepEqual(world.compared, [
    ["text/input", "text", "reset"],
    ["check", "checked", true],
  ]);
  world.value("text/input", "GuiTextInput", { text: "reset" });
  world.value("check", "GuiCheckbox", { checked: true });
  await settle(root);
  assert.deepEqual(text, []);
  assert.deepEqual(checked, []);
  await root.unmount();
});

test("Autocomplete synchronizes inputValue without reopening its list on a reset echo", async () => {
  const changes: string[] = [];
  const scene = (inputValue: string) =>
    h(Autocomplete, {
      id: "auto",
      inputValue,
      suggestions: [{ key: "al", label: "Alpine" }],
      onInputChange: (value) => changes.push(value),
    });
  const { world, root, draw } = await render(scene("Alpha"));
  await draw(scene(""));
  await settle(root);
  assert.deepEqual(world.compared, [["auto", "text", ""]]);
  world.value("auto", "GuiTextInput", { text: "" });
  await settle(root);
  assert.equal(world.fields("auto/list", "GuiBehavior").get("visible"), false);
  assert.deepEqual(changes, []);
  world.value("auto", "GuiTextInput", { text: "Al" });
  await settle(root);
  assert.equal(world.fields("auto/list", "GuiBehavior").get("visible"), true);
  assert.deepEqual(changes, ["Al"]);
  await root.unmount();
});

test("generated kit IDs preserve independent keyed component instances through reorder", async () => {
  const refs = new Map<string, GuiControlHandle>();
  const callbacks = new Map(
    ["a", "b"].map((key) => [
      key,
      (handle: GuiControlHandle | null) => {
        if (handle) refs.set(key, handle);
        else refs.delete(key);
      },
    ]),
  );
  const scene = (keys: string[]) =>
    h(
      Column,
      null,
      ...keys.map((key) => h(Button, { key, ref: callbacks.get(key)! }, key)),
    );
  const { world, root, draw } = await render(scene(["a", "b"]));
  await settle(root);
  const first = new Map(refs);
  assert.equal(first.size, 2);
  assert.notEqual(first.get("a")?.target.entity, first.get("b")?.target.entity);
  const entities = [...world.entities.keys()].filter((id) =>
    id.startsWith("ipp-react/"),
  );
  assert.ok(entities.length >= 3);
  await draw(scene(["b", "a"]));
  await settle(root);
  assert.equal(refs.get("a"), first.get("a"));
  assert.equal(refs.get("b"), first.get("b"));
  await root.unmount();
});

test("compound popover keeps the custom trigger handle and bubbles its press once", async () => {
  let presses = 0;
  const trigger = { current: null as GuiControlHandle | null };
  const { world, root } = await render(
    h(
      Popover.Root,
      { id: "pop" },
      h(
        Popover.Trigger,
        null,
        h(
          Button,
          { id: "trigger", ref: trigger, onPress: () => presses++ },
          "Options",
        ),
      ),
      h(
        Popover.Content,
        null,
        h(TextLine, { id: "content", text: "Custom body" }),
      ),
    ),
  );
  await settle(root);
  assert.ok(trigger.current);
  assert.deepEqual(world.children("pop"), ["pop/trigger", "pop/popover"]);
  world.effect("trigger", { kind: "pressed" }, "GuiButton", 1n, [
    "panel",
    "pop",
    "pop/trigger",
    "trigger",
  ]);
  await settle(root);
  assert.equal(presses, 1);
  assert.equal(world.fields("pop/popover", "GuiBehavior").get("visible"), true);
  await root.unmount();
});

test("compound dialog composes authored header, body and actions with native modal behavior", async () => {
  const { world, root } = await render(
    h(
      Dialog.Root,
      { id: "dialog", defaultOpen: true },
      h(
        Dialog.Content,
        null,
        h(Dialog.Header, { title: "Settings" }),
        h(
          Dialog.Body,
          null,
          h(TextField, { id: "field", defaultValue: "draft" }),
        ),
        h(Dialog.Actions, null, h(Dialog.Close, { id: "close" }, "Done")),
      ),
    ),
  );
  assert.equal(world.fields("dialog/dialog", "GuiOverlay").get("mode"), 2);
  assert.equal(world.fields("dialog/dialog", "GuiOverlay").get("band"), 2);
  world.effect("close", { kind: "pressed" }, "GuiButton");
  await settle(root);
  assert.equal(
    world.fields("dialog/dialog", "GuiBehavior").get("visible"),
    false,
  );
  await root.unmount();
});

test("controlled fields restore rejected edits after React commits and omit restore echoes", async () => {
  const changes: unknown[] = [];
  const { world, root } = await render(
    h(
      Fragment,
      null,
      h(TextField, {
        id: "name",
        value: "allowed",
        onChange: (value) => changes.push(value),
      }),
      h(Checkbox, {
        id: "check",
        checked: true,
        onCheckedChange: (value) => changes.push(value),
      }),
      h(LabelledSlider, {
        id: "gain",
        min: 0,
        max: 100,
        value: 20,
        onChange: (value) => changes.push(value),
      }),
      h(ColorPicker, {
        id: "color",
        value: { hue: 0, saturation: 1, value: 1, alpha: 1 },
        onChange: (value) => changes.push(value),
      }),
    ),
  );
  assert.deepEqual(changes, [], "subscription seeds do not report edits");
  world.value("name/input", "GuiTextInput", { text: "rejected" });
  world.value("check", "GuiCheckbox", { checked: false });
  world.value("gain/slider", "GuiSlider", {
    value: 70,
    upper: 0,
    range: false,
  });
  world.value("color/control", "GuiColor", {
    hue: 0.5,
    saturation: 0.5,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.equal(
    world.fields("name/input", "GuiTextInput").get("text"),
    "allowed",
  );
  assert.equal(world.fields("check", "GuiCheckbox").get("checked"), true);
  assert.equal(world.fields("gain/slider", "GuiSlider").get("value"), 20);
  assert.equal(world.fields("color/control", "GuiColor").get("hue"), 0);
  assert.equal(changes.length, 4);
  const writes = world.compared.length;
  world.value("name/input", "GuiTextInput", { text: "allowed" });
  world.value("check", "GuiCheckbox", { checked: true });
  world.value("gain/slider", "GuiSlider", {
    value: 20,
    upper: 0,
    range: false,
  });
  world.value("color/control", "GuiColor", {
    hue: 0,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.equal(changes.length, 4);
  assert.equal(
    world.compared.length,
    writes,
    "restoring echoes never produce a feedback loop",
  );
  await root.unmount();
});

test("controlled callback updates commit before convergence and accept without stale writes", async () => {
  const changes: unknown[] = [];
  function Form() {
    const [text, setText] = useState("initial");
    const [checked, setChecked] = useState(false);
    const [gain, setGain] = useState(20);
    return h(
      Fragment,
      null,
      h(TextField, {
        id: "name",
        value: text,
        onChange: (value) => {
          changes.push(value);
          setText(value);
        },
      }),
      h(Checkbox, {
        id: "check",
        checked,
        onCheckedChange: (value) => {
          changes.push(value);
          setChecked(value);
        },
      }),
      h(LabelledSlider, {
        id: "gain",
        min: 0,
        max: 100,
        value: gain,
        onChange: (value) => {
          changes.push(value);
          setGain(value);
        },
      }),
    );
  }
  const { world, root } = await render(h(Form));
  world.value("name/input", "GuiTextInput", { text: "accepted" });
  world.value("check", "GuiCheckbox", { checked: true });
  world.value("gain/slider", "GuiSlider", {
    value: 35,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, ["accepted", true, 35]);
  assert.deepEqual(
    world.compared,
    [],
    "accepted changes are never written back a round trip later",
  );
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "35");
  await root.unmount();
});

test("owned optional widget configuration resets when props disappear", async () => {
  const { world, root, draw } = await render(
    h(TextField, {
      id: "text",
      placeholder: "Search",
      masked: true,
      padding: 9,
      margin: 4,
      disabled: true,
    }),
  );
  await draw(h(TextField, { id: "text" }));
  assert.equal(
    world.fields("text/input", "GuiTextInput").get("placeholder"),
    "",
  );
  assert.equal(world.fields("text/input", "GuiTextInput").get("masked"), false);
  assert.equal(world.fields("text", "GuiLayout").get("padding_left"), 0);
  assert.equal(world.fields("text", "GuiLayout").get("margin_top"), 0);
  assert.equal(world.fields("text/input", "GuiBehavior").get("enabled"), true);
  await root.unmount();
});

test("intrinsic kit containers author fit flags and reset named omission defaults", async () => {
  const scene = (configured: boolean) =>
    h(
      Column,
      {
        id: "fit-column",
        ...(configured ? { fitHeight: false, fitWidth: true } : {}),
      },
      h(TextField, { id: "fit-field", label: "Name" }),
      h(
        Popover.Root,
        { id: "fit-popover" },
        h(Popover.Trigger, null, h(Button, null, "Open")),
      ),
    );
  const { world, draw } = await render(scene(true));
  assert.equal(
    world.fields("fit-column", "GuiLayout").get("fit_height"),
    false,
  );
  assert.equal(world.fields("fit-column", "GuiLayout").get("fit_width"), true);
  await draw(scene(false));
  assert.equal(world.fields("fit-column", "GuiLayout").get("fit_height"), true);
  assert.equal(world.fields("fit-column", "GuiLayout").get("fit_width"), false);
  for (const id of ["fit-field", "fit-popover", "fit-popover/trigger"])
    assert.equal(world.fields(id, "GuiLayout").get("fit_height"), true);
});

test("named layout props share logical units, skin spacing, raw precedence and omission resets", async () => {
  const scene = (configured: boolean) =>
    h(
      Column,
      { gap: "section" },
      h(Dropdown, {
        id: "layout-select",
        options: [{ key: "a", label: "A" }],
        ...(configured
          ? {
              width: 180,
              minWidth: 100,
              padding: "compact" as const,
              layout: { width: 160 },
            }
          : {}),
      }),
      h(NumericStepper, {
        id: "layout-number",
        min: 0,
        max: 10,
        ...(configured ? { width: 180, margin: "label" as const } : {}),
      }),
      h(TextLine, {
        id: "layout-text",
        text: "Caption",
        ...(configured ? { width: 180, maxWidth: 200 } : {}),
      }),
      h(Popover, {
        id: "layout-popup",
        label: "Open",
        title: "Options",
        width: 100,
        contentWidth: 260,
      }),
    );
  const { world, root, draw } = await render(scene(true), { fontSize: 32 });
  const select = world.entity("layout-select");
  try {
    assert.equal(world.fields("layout-select", "GuiLayout").get("width"), 160);
    assert.equal(
      world.fields("layout-select", "GuiLayout").get("min_width"),
      100,
    );
    assert.equal(
      world.fields("layout-select", "GuiLayout").get("padding_left"),
      8,
    );
    assert.equal(world.fields("layout-number", "GuiLayout").get("width"), 180);
    assert.equal(
      world.fields("layout-number", "GuiLayout").get("margin_left"),
      16,
    );
    assert.equal(world.fields("layout-text", "GuiLayout").get("width"), 180);
    assert.equal(world.fields("layout-popup", "GuiLayout").get("width"), 100);
    assert.equal(
      world.fields("layout-popup/popover", "GuiLayout").get("width"),
      260,
    );
    await draw(scene(false));
    assert.equal(world.entity("layout-select"), select);
    assert.equal(world.fields("layout-select", "GuiLayout").get("width"), 480);
    assert.equal(
      world.fields("layout-select", "GuiLayout").get("min_width"),
      0,
    );
    assert.equal(
      world.fields("layout-select", "GuiLayout").get("padding_left"),
      32,
    );
    assert.equal(
      world.fields("layout-number", "GuiLayout").get("margin_left"),
      0,
    );
    assert.equal(world.fields("layout-text", "GuiLayout").get("max_width"), -1);
  } finally {
    await root.unmount();
  }
});
