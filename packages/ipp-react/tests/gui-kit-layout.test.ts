import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, type ReactNode } from "react";
import * as Kit from "../src/gui-kit.js";
import { Arc } from "../src/gui-kit/arc.js";
import { Floating } from "../src/gui-kit/overlays/overlay.js";
import { SelectTrigger } from "../src/gui-kit/selection/select-trigger.js";
import {
  render,
  TOKENS,
  contract,
  FONT,
  skinFixture,
} from "./gui-kit-support.js";

type Props = Kit.GuiKitLayoutProps;
type Specimen = readonly [string, (props: Props) => ReactNode, string?];
const id = "subject";

function ContextSpecimen(props: Props) {
  const menu = Kit.useContextMenu<string>();
  return h(Kit.ContextMenu, {
    id,
    menu: { ...menu, request: { target: "target", point: [0, 0] } },
    items: [],
    ...props,
  });
}

// Each public root must keep authoring props out of its native control, and
// restore its own defaults without replacing the root when those props leave.
const specimens: readonly Specimen[] = [
  ["Row", (p) => h(Kit.Row, { id, ...p })],
  ["Column", (p) => h(Kit.Column, { id, ...p })],
  ["Toolbar", (p) => h(Kit.Toolbar, { id, ...p })],
  ["ActionRow", (p) => h(Kit.ActionRow, { id, ...p })],
  ["OverlayStack", (p) => h(Kit.OverlayStack, { id, ...p })],
  ["Panel", (p) => h(Kit.Panel, { id, ...p })],
  ["PanelBody", (p) => h(Kit.PanelBody, { id, ...p })],
  ["PanelHeader", (p) => h(Kit.PanelHeader, { id, title: "Title", ...p })],
  ["PanelFooter", (p) => h(Kit.PanelFooter, { id, ...p })],
  ["ScrollArea", (p) => h(Kit.ScrollArea, { id, ...p })],
  ["Form", (p) => h(Kit.Form, { id, ...p })],
  ["Field", (p) => h(Kit.Field, { id, label: "Label", ...p })],
  ["FormSection", (p) => h(Kit.FormSection, { id, ...p })],
  ["Button", (p) => h(Kit.Button, { id, label: "Apply", ...p })],
  [
    "SecondaryButton",
    (p) => h(Kit.SecondaryButton, { id, label: "Cancel", ...p }),
  ],
  ["WindowControl", (p) => h(Kit.WindowControl, { id, kind: "close", ...p })],
  ["WindowControls", (p) => h(Kit.WindowControls, { id, onClose() {}, ...p })],
  ["Checkbox", (p) => h(Kit.Checkbox, { id, label: "Enabled", ...p })],
  ["Switch", (p) => h(Kit.Switch, { id, ...p })],
  ["TextField", (p) => h(Kit.TextField, { id, label: "Name", ...p })],
  ["TextLine", (p) => h(Kit.TextLine, { id, text: "Text", ...p })],
  ["Icon", (p) => h(Kit.Icon, { id, icon: "close", ...p })],
  ["CheckMark", (p) => h(Kit.CheckMark, { id, ...p })],
  ["Separator", (p) => h(Kit.Separator, { id, ...p })],
  [
    "LabelledSeparator",
    (p) => h(Kit.LabelledSeparator, { id, label: "Section", ...p }),
  ],
  [
    "Arc",
    (p) => h(Arc, { id, theme: "container", size: 20, thickness: 2, ...p }),
  ],
  ["Dropdown", (p) => h(Kit.Dropdown, { id, options: [], ...p })],
  [
    "SearchableDropdown",
    (p) => h(Kit.SearchableDropdown, { id, options: [], ...p }),
  ],
  ["MultiSelect", (p) => h(Kit.MultiSelect, { id, options: [], ...p })],
  ["Autocomplete", (p) => h(Kit.Autocomplete, { id, suggestions: [], ...p })],
  [
    "SelectTrigger",
    (p) => h(SelectTrigger, { id, text: "Choice", open: false, ...p }),
  ],
  ["OptionList", (p) => h(Kit.OptionList, { id, options: [], ...p })],
  ["RadioGroup", (p) => h(Kit.RadioGroup, { id, options: [], ...p })],
  [
    "SegmentedControl",
    (p) => h(Kit.SegmentedControl, { id, options: [], ...p }),
  ],
  ["Tabs", (p) => h(Kit.Tabs, { id, tabs: [], ...p })],
  ["TreeView", (p) => h(Kit.TreeView, { id, nodes: [], ...p })],
  ["NumericStepper", (p) => h(Kit.NumericStepper, { id, ...p })],
  [
    "LabelledSlider",
    (p) => h(Kit.LabelledSlider, { id, min: 0, max: 1, ...p }),
  ],
  ["RangeSlider", (p) => h(Kit.RangeSlider, { id, min: 0, max: 1, ...p })],
  ["SliderScale", (p) => h(Kit.SliderScale, { id, min: 0, max: 1, ...p })],
  ["Knob", (p) => h(Kit.Knob, { id, min: 0, max: 1, ...p })],
  ["ColorPicker", (p) => h(Kit.ColorPicker, { id, ...p })],
  ["DataGrid", (p) => h(Kit.DataGrid, { id, columns: [], rows: [], ...p })],
  [
    "Expander",
    (p) =>
      h(Kit.Expander, { id, label: "Details", defaultExpanded: true, ...p }),
  ],
  ["ProgressBar", (p) => h(Kit.ProgressBar, { id, label: "Loading", ...p })],
  [
    "CircularProgress",
    (p) => h(Kit.CircularProgress, { id, label: "Loading", ...p }),
  ],
  ["Spinner", (p) => h(Kit.Spinner, { id, label: "Loading", ...p })],
  [
    "StatusBadge",
    (p) => h(Kit.StatusBadge, { id, status: "active", label: "Online", ...p }),
  ],
  [
    "InlineAlert",
    (p) => h(Kit.InlineAlert, { id, severity: "error", text: "Error", ...p }),
  ],
  ["EmptyState", (p) => h(Kit.EmptyState, { id, text: "No results", ...p })],
  ["Menu", (p) => h(Kit.Menu, { id, items: [], ...p })],
  ["ContextMenu", (p) => h(ContextSpecimen, p), `${id}/items`],
  ["Toast", (p) => h(Kit.Toast, { id, text: "Saved", persistent: true, ...p })],
  ["ToastStack", (p) => h(Kit.ToastStack, { id, toasts: [], ...p })],
  ["Floating", (p) => h(Floating, { id, mode: "light", open: true, ...p })],
  [
    "ConfirmationDialog",
    (p) =>
      h(Kit.ConfirmationDialog, {
        id,
        open: true,
        title: "Confirm",
        body: "Continue?",
        action: "Apply",
        ...p,
      }),
  ],
  [
    "Popover",
    (p) => h(Kit.Popover, { id, label: "Open", title: "Options", ...p }),
  ],
  ["Popover.Root", (p) => h(Kit.Popover.Root, { id, ...p })],
  [
    "Popover.Trigger",
    (p) => h(Kit.Popover.Root, { id }, h(Kit.Popover.Trigger, p)),
    `${id}/trigger`,
  ],
  [
    "Popover.Content",
    (p) => h(Kit.Popover.Root, { id }, h(Kit.Popover.Content, p)),
    `${id}/popover`,
  ],
  [
    "Dialog.Content",
    (p) => h(Kit.Dialog.Root, { id }, h(Kit.Dialog.Content, p)),
    `${id}/dialog`,
  ],
  [
    "Dialog.Header",
    (p) =>
      h(
        Kit.Dialog.Root,
        { id },
        h(Kit.Dialog.Header, { title: "Title", ...p }),
      ),
    `${id}/header`,
  ],
  [
    "Dialog.Body",
    (p) => h(Kit.Dialog.Root, { id }, h(Kit.Dialog.Body, p)),
    `${id}/body`,
  ],
  [
    "Dialog.Actions",
    (p) => h(Kit.Dialog.Root, { id }, h(Kit.Dialog.Actions, p)),
    `${id}/actions`,
  ],
  [
    "Dialog.Trigger",
    (p) =>
      h(
        Kit.Dialog.Root,
        { id: "scope" },
        h(Kit.Dialog.Trigger, { id, label: "Open", ...p }),
      ),
  ],
  [
    "Dialog.Close",
    (p) =>
      h(
        Kit.Dialog.Root,
        { id: "scope" },
        h(Kit.Dialog.Close, { id, label: "Close", ...p }),
      ),
  ],
];

for (const [name, element, target = id] of specimens) {
  test(`${name} accepts common layout props and restores defaults on omission`, async () => {
    const { world, root, draw } = await render(element({}), { fontSize: 32 });
    const fields = () => Object.fromEntries(world.fields(target, "GuiLayout"));
    const before = fields();
    const identity = world.entity(target);
    try {
      await draw(
        element({
          width: 123,
          height: 43,
          minWidth: 0,
          minHeight: 0,
          maxWidth: 200,
          maxHeight: 100,
          flex: 1,
          padding: "compact",
          margin: "label",
          alignX: "start",
          alignY: "end",
          fitWidth: true,
          fitHeight: false,
          clip: true,
          layout: { width: 127 },
        }),
      );
      const actual = fields();
      for (const [field, value] of Object.entries({
        width: 127,
        height: 43,
        min_width: 0,
        min_height: 0,
        max_width: 200,
        max_height: 100,
        flex: 1,
        margin_left: 16,
        align_x: -1,
        align_y: 1,
        fit_width: true,
        fit_height: false,
        clip: true,
      }))
        assert.equal(actual[field], value, `${name}.${field}`);
      // ScrollArea's padding belongs to its scroll content rather than its frame.
      const padded = name === "ScrollArea" ? `${target}/body` : target;
      assert.equal(world.fields(padded, "GuiLayout").get("padding_left"), 8);
      await draw(
        element({ width: undefined, padding: undefined, alignY: undefined }),
      );
      assert.deepEqual(
        fields(),
        before,
        `${name} restores its original defaults`,
      );
      assert.equal(
        world.entity(target),
        identity,
        `${name} retains its root identity`,
      );
      await draw(element({ width: 0, height: 0, padding: 0, margin: 0 }));
      assert.equal(fields().width, 0);
      assert.equal(fields().height, 0);
      assert.equal(world.fields(padded, "GuiLayout").get("padding_left"), 0);
    } finally {
      await root.unmount();
    }
  });
}

test("text field gap overrides reach labels, help and errors and restore the role", async () => {
  const field = (props: Kit.TextFieldProps) =>
    h(Kit.TextField, {
      id,
      label: "Name",
      description: "Help",
      error: "Error",
      ...props,
    });
  const { world, root, draw } = await render(
    field({ gap: "field", layout: { gap: 0 }, disabled: true }),
    { fontSize: 20 },
  );
  try {
    assert.equal(world.fields(`${id}/field`, "GuiLayout").get("gap"), 0);
    assert.deepEqual(world.tone(`${id}/field/label`), TOKENS.neutral);
    assert.deepEqual(world.tone(`${id}/field/error`), TOKENS.error);
    assert.deepEqual(world.tone(`${id}/field/description`), TOKENS.neutral);
    const input = world.entity(`${id}/input`);
    await draw(field({ gap: "field" }));
    assert.equal(world.fields(`${id}/field`, "GuiLayout").get("gap"), 20);
    await draw(field({}));
    assert.equal(world.fields(`${id}/field`, "GuiLayout").get("gap"), 10);
    assert.equal(world.entity(`${id}/input`), input);
    assert.deepEqual(world.tone(`${id}/field/label`), TOKENS.accent);
  } finally {
    await root.unmount();
  }
});

test("empty and loading option rows preserve the option height at text scale", async () => {
  const { world, root, draw } = await render(
    h(Kit.OptionList, { id, options: [] }),
    { fontSize: 20 },
  );
  try {
    assert.equal(world.fields(`${id}/empty`, "GuiLayout").get("height"), 45);
    await draw(h(Kit.OptionList, { id, options: [], loading: "Loading" }));
    assert.equal(world.fields(`${id}/loading`, "GuiLayout").get("height"), 45);
  } finally {
    await root.unmount();
  }
});

test("toolbar control minimum yields to explicit height and returns on omission", async () => {
  const { world, root, draw } = await render(
    h(Kit.Toolbar, { id, height: 20 }),
  );
  try {
    assert.equal(world.fields(id, "GuiLayout").get("min_height"), 0);
    await draw(h(Kit.Toolbar, { id, layout: { height: 18 } }));
    assert.equal(world.fields(id, "GuiLayout").get("min_height"), 0);
    await draw(h(Kit.Toolbar, { id, height: undefined, minHeight: undefined }));
    assert.equal(world.fields(id, "GuiLayout").get("min_height"), TOKENS.row);
  } finally {
    await root.unmount();
  }
});

test("container roles and explicit dimensions remain distinct at non-default text scale", async () => {
  const skin = skinFixture();
  const content = (width?: number) =>
    h(
      Kit.GuiKit,
      {
        id: "roles",
        contract: contract(),
        font: FONT,
        fontSize: 20,
        skin: {
          ...skin,
          tokens: {
            ...skin.tokens,
            spacing: {
              inset: 24,
              section: 24,
              field: 16,
              label: 8,
              compact: 4,
            },
          },
        },
      },
      h(Kit.PanelHeader, { id: "header", title: "Title" }),
      h(Kit.PanelBody, { id: "body" }),
      h(Kit.PanelFooter, { id: "footer" }),
      h(Kit.Popover, {
        id: "popover",
        label: "Open",
        title: "Details",
        width: 100,
      }),
      h(Kit.ConfirmationDialog, {
        id: "confirm",
        open: true,
        title: "Confirm",
        body: "Proceed?",
        action: "Apply",
        width,
      }),
      h(
        Kit.Dialog.Root,
        { id: "dialog" },
        h(
          Kit.Dialog.Content,
          { width },
          h(Kit.Dialog.Body, null),
          h(Kit.Dialog.Actions, null),
        ),
      ),
      h(Kit.DataGrid, {
        id: "grid",
        columns: [{ key: "name", title: "Name", width: 96 }],
        rows: [],
      }),
    );
  const { world, root, draw } = await render(content(250));
  const field = (id: string, key: string) =>
    world.fields(id, "GuiLayout").get(key);
  try {
    for (const id of [
      "header/row",
      "body",
      "footer/row",
      "popover/content",
      "confirm/body",
      "dialog/body",
      "dialog/actions",
    ])
      assert.equal(
        field(id, "padding_left"),
        30,
        `${id} uses container role, not control inset`,
      );
    assert.equal(field("footer/row", "padding_top"), 10);
    assert.equal(field("popover", "width"), 100);
    assert.equal(field("popover/popover", "width"), 300);
    assert.equal(field("confirm", "width"), 250);
    assert.equal(field("dialog/dialog", "width"), 250);
    assert.equal(field("grid/header/name", "width"), 96);
    await draw(content());
    assert.equal(field("confirm", "width"), 460);
    assert.equal(field("dialog/dialog", "width"), 460);
  } finally {
    await root.unmount();
  }
});
