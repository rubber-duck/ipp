/** Skinnable control and theme authoring unit tests (ipp-9nx.14).
 *
 * Headless and node-runnable: pure builders, validators and runtime-property
 * compilation with no transport or reconciler. Callback/path invariants live
 * in `gui-effects.test.ts`; runtime evidence lives in the GUI suite.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { equalGuiNode } from "../src/gui/description.js";
import {
  Button,
  Checkbox,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  Slider,
  TextInput,
  buttonNode,
  checkboxNode,
  controlDeclarationSignature,
  describeButton,
  describeCheckbox,
  describeSlider,
  describeTextInput,
  isGuiControlHostType,
  sliderNode,
  textInputNode,
} from "../src/gui/controls.js";
import {
  GUI_THEME_PARTS,
  compileGuiTheme,
  defaultGuiTheme,
  guiThemeKey,
  validateGuiTheme,
  type GuiControlTheme,
} from "../src/gui/theme.js";
import { guiPartFromIndex, type GuiPartValues } from "@ipp/client";

/** Compiled theme rows keyed by part name: `background`,
 * `background_pressed` or `background_idle_checked`. */
function themeRows(theme: GuiControlTheme): Record<string, GuiPartValues> {
  return Object.fromEntries(
    [...compileGuiTheme(theme)].map(([index, values]) => {
      const id = guiPartFromIndex(index);
      return [
        [id.part, id.state, id.variant].filter(Boolean).join("_"),
        values,
      ];
    }),
  );
}
import {
  actionsForControlKind,
  controlKindForData,
  nameForData,
} from "../src/gui/callbacks.js";

test("control host types are distinct and guarded", () => {
  assert.equal(
    new Set([
      GUI_BUTTON_HOST_TYPE,
      GUI_CHECKBOX_HOST_TYPE,
      GUI_SLIDER_HOST_TYPE,
      GUI_TEXT_INPUT_HOST_TYPE,
    ]).size,
    4,
  );
  assert.equal(isGuiControlHostType(GUI_BUTTON_HOST_TYPE), true);
  assert.equal(isGuiControlHostType(GUI_TEXT_INPUT_HOST_TYPE), true);
  assert.equal(isGuiControlHostType("ipp-gui-text"), false);
  assert.equal(isGuiControlHostType(undefined), false);
});

test("control builders emit node data and kind-specific values", () => {
  assert.deepEqual(buttonNode("Go"), {
    data: { kind: "button", label: "Go" },
    values: {},
  });
  assert.deepEqual(checkboxNode(undefined), {
    data: { kind: "checkbox" },
    values: { checked: false },
  });
  assert.deepEqual(checkboxNode(true), {
    data: { kind: "checkbox" },
    values: { checked: true },
  });
  assert.deepEqual(sliderNode({}), {
    data: { kind: "slider" },
    values: { value: 0, min: 0, max: 1, step: 0 },
  });
  assert.deepEqual(sliderNode({ value: 1.5, min: 0, max: 2, step: 0.5 }), {
    data: { kind: "slider" },
    values: { value: 1.5, min: 0, max: 2, step: 0.5 },
  });
  assert.deepEqual(textInputNode({}), {
    data: { kind: "textInput", text: "", placeholder: "" },
    values: {},
  });
  assert.deepEqual(textInputNode({ text: "ada", placeholder: "name" }), {
    data: { kind: "textInput", text: "ada", placeholder: "name" },
    values: {},
  });
  assert.equal(equalGuiNode(buttonNode("Go"), buttonNode("Go")), true);
  assert.equal(equalGuiNode(buttonNode("Go"), buttonNode("Stop")), false);
  assert.equal(equalGuiNode(buttonNode("Go"), checkboxNode(false)), false);
  // Runtime-owned values never make declarations differ; ranges do.
  assert.equal(equalGuiNode(checkboxNode(false), checkboxNode(true)), true);
  assert.equal(
    equalGuiNode(sliderNode({ value: 0.1 }), sliderNode({ value: 0.9 })),
    true,
  );
  assert.equal(
    equalGuiNode(sliderNode({ max: 1 }), sliderNode({ max: 2 })),
    false,
  );
});

test("control validators reject invalid declarations loudly", () => {
  assert.throws(
    () => buttonNode(7 as unknown as string),
    /GUI Button label must be a string/,
  );
  assert.throws(
    () => checkboxNode("yes" as unknown as boolean),
    /GUI Checkbox checked must be a boolean/,
  );
  assert.throws(
    () => sliderNode({ value: Number.NaN }),
    /GUI Slider value must be a finite number/,
  );
  assert.throws(
    () => sliderNode({ min: 2, max: 1 }),
    /GUI Slider min must not exceed max/,
  );
  assert.throws(
    () => sliderNode({ step: -1 }),
    /GUI Slider step must be a finite number >= 0/,
  );
  assert.throws(
    () => textInputNode({ text: 3 as unknown as string }),
    /GUI TextInput text must be a string/,
  );
  assert.throws(
    () => textInputNode({ placeholder: null as unknown as string }),
    /GUI TextInput placeholder must be a string/,
  );
  assert.throws(
    () =>
      describeButton({
        label: "Go",
        onPress: "now" as unknown as never,
      }),
    /GUI Button onPress must be a function/,
  );
  assert.throws(
    () =>
      describeSlider({
        onScalarCommit: 1 as unknown as never,
      }),
    /GUI Slider onScalarCommit must be a function/,
  );
});

test("control components are pure element factories without transport", () => {
  const button = Button({ label: "Go" });
  assert.equal(button.type, GUI_BUTTON_HOST_TYPE);
  assert.equal((button.props as { label: string }).label, "Go");
  assert.equal(Checkbox({}).type, GUI_CHECKBOX_HOST_TYPE);
  assert.equal(Slider({}).type, GUI_SLIDER_HOST_TYPE);
  assert.equal(TextInput({}).type, GUI_TEXT_INPUT_HOST_TYPE);
});

test("control descriptions carry node data and style only, never value writes", () => {
  const decl = describeCheckbox({
    checked: true,
    color: [1, 0, 0, 1],
    onToggle: () => {},
  });
  assert.equal(decl.hostType, GUI_CHECKBOX_HOST_TYPE);
  assert.deepEqual(decl.data, { kind: "checkbox" });
  assert.deepEqual(decl.values, { checked: true });
  assert.deepEqual(decl.style.color, [1, 0, 0, 1]);
  assert.equal(decl.style.opacity, 1);
  assert.equal(decl.nodeRef, null);
  assert.deepEqual(Object.keys(decl).sort(), [
    "data",
    "hostType",
    "nodeRef",
    "onAction",
    "onActionCapture",
    "onPress",
    "onScalarCommit",
    "onSubmit",
    "onTextCommit",
    "onToggle",
    "style",
    "values",
  ]);
  // A changed `checked` prop is explicit replacement structure, not a write:
  // the declaration just names different initial values.
  assert.deepEqual(describeCheckbox({}).values, { checked: false });
  assert.deepEqual(describeSlider({ value: 2 }).values, {
    value: 2,
    min: 0,
    max: 1,
    step: 0,
  });
  assert.deepEqual(describeTextInput({ text: "v2" }).data, {
    kind: "textInput",
    text: "v2",
    placeholder: "",
  });
  assert.deepEqual(describeButton({ label: "Go" }).data, {
    kind: "button",
    label: "Go",
  });
});

test("callbacks, refs and initial values resubmit nothing", () => {
  const first = describeSlider({ value: 1 });
  const second = describeSlider({ value: 1, onScalarCommit: () => {} });
  assert.equal(
    controlDeclarationSignature(first),
    controlDeclarationSignature(second),
  );
  const ref = { current: null };
  assert.equal(
    controlDeclarationSignature(describeSlider({ value: 1, nodeRef: ref })),
    controlDeclarationSignature(first),
  );
  assert.equal(
    controlDeclarationSignature(describeSlider({ value: 2 })),
    controlDeclarationSignature(first),
  );
  assert.notEqual(
    controlDeclarationSignature(
      describeSlider({ value: 1, color: [1, 0, 0, 1] }),
    ),
    controlDeclarationSignature(first),
  );
  assert.notEqual(
    controlDeclarationSignature(
      describeSlider({
        value: 1,
        theme: { parts: { background: { base: { opacity: 0.5 } } } },
      }),
    ),
    controlDeclarationSignature(first),
  );
});

test("themes compile to root theme part rows without resolving interaction", () => {
  const theme: GuiControlTheme = {
    parts: {
      background: {
        base: { color: [1, 1, 1, 1], opacity: 0.5 },
        pressed: { color: [0, 0, 0, 1] },
        checked: { color: [0, 1, 0, 1], opacity: 0.9 },
      },
      focusRing: { base: { color: [1, 0, 0, 1] } },
    },
  };
  const properties = themeRows(theme);
  assert.deepEqual(properties["background"]?.color, [1, 1, 1, 1]);
  assert.deepEqual(properties["background_pressed"]?.color, [0, 0, 0, 1]);
  assert.deepEqual(properties["background_idle_checked"]?.color, [0, 1, 0, 1]);
  assert.deepEqual(properties["focusRing"]?.color, [1, 0, 0, 1]);
  assert.deepEqual(GUI_THEME_PARTS, [
    "background",
    "fill",
    "label",
    "icon",
    "focusRing",
  ]);
});

test("theme validation fails closed on names, keys and properties", () => {
  validateGuiTheme(defaultGuiTheme);
  assert.throws(
    () => validateGuiTheme({ parts: { "bad-name": {} } } as never),
    /not a runtime base part/,
  );
  assert.throws(
    () =>
      validateGuiTheme({
        parts: {
          background: { hover: { color: [1, 1, 1, 1] } } as never,
        },
      }),
    /unknown key "hover"/,
  );
  assert.throws(
    () =>
      validateGuiTheme({
        parts: { background: { base: { color: [2, 0, 0, 1] } } },
      }),
    /color must be four finite numbers/,
  );
  assert.throws(
    () =>
      validateGuiTheme({
        parts: { background: { base: { opacity: 2 } } },
      }),
    /opacity must be a finite number/,
  );
});

test("transition declarations author the rendering contract", () => {
  const properties = themeRows({
    parts: {
      background: {
        base: {
          color: [0, 0, 0, 1],
          opacity: 1,
          scale: [1, 1],
        },
        pressed: {
          color: [1, 0, 0, 1],
          transition: {
            motion: { kind: 10, source: "asset://motion", variant: 2 },
            duration: 0.25,
            easing: "smoothstep",
            track: 3,
            time: 0.5,
          },
        },
      },
    },
  });
  assert.deepEqual(properties["background_pressed"]?.motion, {
    kind: 10,
    source: "asset://motion",
    variant: 2,
  });
  assert.deepEqual(properties["background_pressed"]?.duration, 0.25);
  assert.deepEqual(properties["background_pressed"]?.easing, 1);
  assert.deepEqual(properties["background_pressed"]?.track, 3);
  assert.deepEqual(properties["background_pressed"]?.time, 0.5);
  assert.throws(
    () =>
      compileGuiTheme({
        parts: {
          background: {
            base: { color: [0, 0, 0, 1] },
            pressed: {
              transition: {
                motion: { kind: 10, source: "asset://motion" },
                duration: 0.25,
              },
            },
          },
        },
      }),
    /requires base color, opacity, and scale/,
  );
  const transition = (track: number): GuiControlTheme => ({
    parts: {
      background: {
        base: { color: [0, 0, 0, 1], opacity: 1, scale: [1, 1] },
        pressed: {
          transition: {
            motion: { kind: 10, source: "asset://motion" },
            duration: 0.25,
            track,
          },
        },
      },
    },
  });
  assert.throws(
    () => validateGuiTheme(transition(16_777_217)),
    /represented exactly as f32/,
  );
  assert.throws(() => validateGuiTheme(transition(0xffff_fffe)), /u32::MAX-2/);
});

test("switch knob alignment authors an animated align_x property", () => {
  const knob = (alignX: number, time: number) => ({
    alignX,
    transition: {
      motion: { kind: 10, source: "asset://motion" },
      duration: 0.2,
      time,
    },
  });
  const properties = themeRows({
    parts: {
      icon: {
        base: { color: [1, 1, 1, 1], opacity: 1, scale: [1, 1], alignX: -1 },
        checked: knob(1, 0.5),
        unchecked: knob(-1, 0),
      },
    },
  });
  assert.deepEqual(properties["icon"]?.alignX, -1);
  assert.deepEqual(properties["icon_pressed_checked"]?.alignX, 1);
  assert.deepEqual(properties["icon_idle_unchecked"]?.alignX, -1);
  assert.throws(
    () =>
      validateGuiTheme({
        parts: { icon: { checked: { alignX: Number.NaN } } },
      }),
    /alignX must be a finite number/,
  );
});

test("theme fonts enter measured node style and label assets reject", () => {
  const font = { kind: 11, source: "asset://font", variant: 2 };
  assert.deepEqual(
    describeButton({ label: "Measured", theme: { font, parts: {} } }).style
      .asset,
    font,
  );
  assert.equal(
    describeButton({
      label: "Override",
      asset: null,
      theme: { font, parts: {} },
    }).style.asset,
    null,
  );
  assert.throws(
    () =>
      validateGuiTheme({
        parts: {
          label: {
            base: { asset: { kind: 11, source: "asset://font" } },
          },
        },
      }),
    /label\.asset is unsupported/,
  );
});

test("theme-only control paint stays in theme rows instead of node style", () => {
  const button = describeButton({
    label: "Painted",
    theme: defaultGuiTheme,
  });
  assert.equal(button.style.backgroundColor, undefined);
  const properties = themeRows(defaultGuiTheme);
  assert.deepEqual(properties["background"]?.color, [0.16, 0.34, 0.72, 1]);
  assert.deepEqual(properties["icon_idle_checked"]?.opacity, 1);
  assert.deepEqual(properties["icon_idle_unchecked"]?.opacity, 0);
});

test("colour-only state styles select a solid fill over an inherited gradient", () => {
  const glow = { color: [0, 1, 1, 1] as const, intensity: 0.2, radius: 0.03 };
  const properties = themeRows({
    parts: {
      background: {
        base: {
          color: [0.1, 0.1, 0.1, 1],
          gradient: { kind: "linear", color0: [1, 0, 0, 1] },
          glow,
        },
        hovered: {
          color: [0, 1, 0, 1],
          gradient: { kind: "radial", color0: [0, 0, 1, 1] },
        },
        pressed: { opacity: 0.8 },
        disabled: { color: [0.4, 0.4, 0.4, 0.5], glow: { intensity: 0 } },
        checked: { color: [1, 1, 0, 1] },
      },
      icon: { base: { color: [1, 1, 1, 1] }, hovered: { color: [0, 0, 0, 1] } },
    },
  });

  assert.deepEqual(properties["background"]?.fillMode, 1);
  assert.deepEqual(properties["background_hovered"]?.fillMode, 2);
  assert.deepEqual(properties["background_disabled"]?.fillMode, 0);
  assert.deepEqual(properties["background_hovered_checked"]?.fillMode, 0);
  // Styles without a colour keep inheriting the fill; glow properties inherit
  // independently and are removed only by an explicit zero intensity.
  assert.equal(properties["background_pressed"]?.fillMode, undefined);
  assert.equal(properties["background_disabled"]?.glowColor, undefined);
  assert.deepEqual(properties["background_disabled"]?.glowIntensity, 0);
  // Parts without any gradient need no explicit mode.
  assert.equal(properties["icon_hovered"]?.fillMode, undefined);
  assert.equal(
    Object.values(themeRows(defaultGuiTheme)).some(
      (values) => values.fillMode !== undefined,
    ),
    false,
  );
});

test("node kinds map to control roles, names and actions", () => {
  assert.equal(controlKindForData(buttonNode("Go").data), "button");
  assert.equal(controlKindForData(checkboxNode(true).data), "checkbox");
  assert.equal(controlKindForData(sliderNode({}).data), "slider");
  assert.equal(controlKindForData(textInputNode({}).data), "textInput");
  assert.equal(
    controlKindForData({ kind: "container", containerKind: "row" }),
    null,
  );
  assert.equal(controlKindForData({ kind: "text", text: "hi" }), null);
  assert.equal(controlKindForData({ kind: "drawing" }), null);
  assert.equal(controlKindForData({ kind: "image" }), null);
  assert.deepEqual(actionsForControlKind("button"), ["press"]);
  assert.deepEqual(actionsForControlKind("checkbox"), ["toggle", "focus"]);
  assert.deepEqual(actionsForControlKind("slider"), ["setScalar", "focus"]);
  assert.deepEqual(actionsForControlKind("textInput"), ["setText", "focus"]);
  assert.equal(nameForData(buttonNode("Go").data), "Go");
  assert.equal(
    nameForData(textInputNode({ placeholder: "name" }).data),
    "name",
  );
  assert.equal(nameForData(textInputNode({}).data), undefined);
  assert.equal(nameForData(checkboxNode(true).data), undefined);
});

test("shape material declarations author the rendering contract and validate closed", () => {
  const properties = themeRows({
    parts: {
      background: {
        base: {
          cornerRadius: [0.05, 0.05],
          borderWidth: 0.01,
          borderColor: [0.2, 0.4, 0.8, 1],
          gradient: {
            kind: "linear",
            start: [0, 0],
            end: [1, 1],
            color0: [1, 0, 0, 1],
            color1: [0, 0, 1, 1],
          },
          glow: {
            color: [1, 0.5, 0, 0.8],
            intensity: 1.5,
            radius: 0.02,
            falloff: 2.0,
          },
        },
        hovered: {
          gradient: {
            kind: "radial",
            start: [0.5, 0.5],
            radius: 0.75,
            color0: [1, 1, 0, 1],
            color1: [0, 1, 0, 1],
          },
        },
      },
    },
  });

  assert.deepEqual(properties["background"]?.cornerRadius, [0.05, 0.05]);
  assert.deepEqual(properties["background"]?.borderWidth, 0.01);
  assert.deepEqual(properties["background"]?.borderColor, [0.2, 0.4, 0.8, 1]);
  assert.deepEqual(properties["background"]?.fillMode, 1);
  assert.deepEqual(properties["background"]?.gradientStart, [0, 0]);
  assert.deepEqual(properties["background"]?.gradientEnd, [1, 1]);
  assert.deepEqual(properties["background"]?.gradientColor0, [1, 0, 0, 1]);
  assert.deepEqual(properties["background"]?.gradientColor1, [0, 0, 1, 1]);
  assert.deepEqual(properties["background"]?.glowColor, [1, 0.5, 0, 0.8]);
  assert.deepEqual(properties["background"]?.glowIntensity, 1.5);
  assert.deepEqual(properties["background"]?.glowRadius, 0.02);
  assert.deepEqual(properties["background"]?.glowFalloff, 2.0);

  assert.deepEqual(properties["background_hovered"]?.fillMode, 2);
  assert.deepEqual(properties["background_hovered"]?.gradientStart, [0.5, 0.5]);
  assert.deepEqual(properties["background_hovered"]?.gradientRadius, 0.75);

  assert.throws(
    () =>
      compileGuiTheme({
        parts: { background: { base: { cornerRadius: [-1, 0] } } },
      }),
    /cornerRadius must be two non-negative finite numbers/,
  );
  assert.throws(
    () =>
      compileGuiTheme({
        parts: { background: { base: { borderWidth: -0.5 } } },
      }),
    /borderWidth must be a non-negative finite number/,
  );
  assert.throws(
    () =>
      compileGuiTheme({
        parts: { background: { base: { borderColor: [1, 1, 2, 1] } } },
      }),
    /borderColor must be four finite numbers in 0..1/,
  );
  assert.throws(
    () =>
      compileGuiTheme({
        parts: {
          background: { base: { gradient: { kind: "invalid" as never } } },
        },
      }),
    /kind must be "linear" or "radial"/,
  );
  assert.throws(
    () =>
      compileGuiTheme({
        parts: {
          background: { base: { gradient: { kind: "radial", radius: -1 } } },
        },
      }),
    /radius must be a non-negative finite number/,
  );
  assert.throws(
    () =>
      compileGuiTheme({
        parts: { background: { base: { glow: { intensity: -1 } } } },
      }),
    /intensity must be a non-negative finite number/,
  );
});

test("themes are identified once per root by name or content", () => {
  const red: GuiControlTheme = {
    parts: { background: { base: { color: [1, 0, 0, 1] } } },
  };
  const green: GuiControlTheme = {
    parts: { background: { base: { color: [0, 1, 0, 1] } } },
  };
  // Equal content shares one runtime theme; different content does not.
  assert.equal(guiThemeKey(red), guiThemeKey({ ...red }));
  assert.notEqual(guiThemeKey(red), guiThemeKey(green));
  // A name keeps identity across content edits, so the runtime theme updates
  // in place.
  assert.equal(
    guiThemeKey({ name: "panel", ...red }),
    guiThemeKey({ name: "panel", ...green }),
  );
  assert.throws(
    () => validateGuiTheme({ name: "", parts: {} }),
    /name must be a nonempty string/,
  );
});
