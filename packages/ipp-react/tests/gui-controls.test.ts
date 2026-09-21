/** Skinnable control and theme authoring unit tests (ipp-9nx.14).
 *
 * Headless and node-runnable: pure builders, validators and runtime-property
 * compilation with no transport or reconciler. Callback/path invariants live
 * in `gui-effects.test.ts`; runtime evidence lives in the GUI suite.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { equalGuiContent } from "../src/gui/description.js";
import {
  Button,
  Checkbox,
  GUI_BUTTON_HOST_TYPE,
  GUI_CHECKBOX_HOST_TYPE,
  GUI_SLIDER_HOST_TYPE,
  GUI_TEXT_INPUT_HOST_TYPE,
  Slider,
  TextInput,
  buttonContent,
  checkboxContent,
  controlDeclarationSignature,
  describeButton,
  describeCheckbox,
  describeSlider,
  describeTextInput,
  isGuiControlHostType,
  sliderContent,
  textInputContent,
} from "../src/gui/controls.js";
import {
  GUI_THEME_PARTS,
  defaultGuiTheme,
  guiThemeProperties,
  validateGuiTheme,
  type GuiControlTheme,
} from "../src/gui/theme.js";
import {
  actionsForControlKind,
  controlKindForContent,
  nameForContent,
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

test("control content builders emit the frozen .10 content contract", () => {
  assert.deepEqual(buttonContent("Go"), { kind: "button", label: "Go" });
  assert.deepEqual(checkboxContent(undefined), {
    kind: "checkbox",
    checked: false,
  });
  assert.deepEqual(checkboxContent(true), {
    kind: "checkbox",
    checked: true,
  });
  assert.deepEqual(sliderContent({}), {
    kind: "slider",
    value: 0,
    min: 0,
    max: 1,
    step: 0,
  });
  assert.deepEqual(sliderContent({ value: 1.5, min: 0, max: 2, step: 0.5 }), {
    kind: "slider",
    value: 1.5,
    min: 0,
    max: 2,
    step: 0.5,
  });
  assert.deepEqual(textInputContent({}), {
    kind: "textInput",
    text: "",
    placeholder: "",
  });
  assert.deepEqual(textInputContent({ text: "ada", placeholder: "name" }), {
    kind: "textInput",
    text: "ada",
    placeholder: "name",
  });
  assert.equal(equalGuiContent(buttonContent("Go"), buttonContent("Go")), true);
  assert.equal(
    equalGuiContent(buttonContent("Go"), buttonContent("Stop")),
    false,
  );
  assert.equal(
    equalGuiContent(buttonContent("Go"), checkboxContent(false)),
    false,
  );
});

test("control validators reject invalid declarations loudly", () => {
  assert.throws(
    () => buttonContent(7 as unknown as string),
    /GUI Button label must be a string/,
  );
  assert.throws(
    () => checkboxContent("yes" as unknown as boolean),
    /GUI Checkbox checked must be a boolean/,
  );
  assert.throws(
    () => sliderContent({ value: Number.NaN }),
    /GUI Slider value must be a finite number/,
  );
  assert.throws(
    () => sliderContent({ min: 2, max: 1 }),
    /GUI Slider min must not exceed max/,
  );
  assert.throws(
    () => sliderContent({ step: -1 }),
    /GUI Slider step must be a finite number >= 0/,
  );
  assert.throws(
    () => textInputContent({ text: 3 as unknown as string }),
    /GUI TextInput text must be a string/,
  );
  assert.throws(
    () => textInputContent({ placeholder: null as unknown as string }),
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

test("control descriptions carry content and style only, never value writes", () => {
  const decl = describeCheckbox({
    checked: true,
    color: [1, 0, 0, 1],
    onToggle: () => {},
  });
  assert.equal(decl.hostType, GUI_CHECKBOX_HOST_TYPE);
  assert.deepEqual(decl.content, { kind: "checkbox", checked: true });
  assert.deepEqual(decl.style.color, [1, 0, 0, 1]);
  assert.equal(decl.style.opacity, 1);
  assert.equal(decl.nodeRef, null);
  assert.deepEqual(Object.keys(decl).sort(), [
    "content",
    "hostType",
    "nodeRef",
    "onAction",
    "onActionCapture",
    "onPress",
    "onScalarCommit",
    "onTextCommit",
    "onToggle",
    "style",
  ]);
  // A changed `checked` prop is explicit replacement structure, not a write:
  // the declaration just names different content.
  assert.deepEqual(describeCheckbox({}).content, {
    kind: "checkbox",
    checked: false,
  });
  assert.deepEqual(describeSlider({ value: 2 }).content, {
    kind: "slider",
    value: 2,
    min: 0,
    max: 1,
    step: 0,
  });
  assert.deepEqual(describeTextInput({ text: "v2" }).content, {
    kind: "textInput",
    text: "v2",
    placeholder: "",
  });
  assert.deepEqual(describeButton({ label: "Go" }).content, {
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

test("themes compile to runtime named parts without resolving interaction", () => {
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
  const properties = guiThemeProperties(9, theme);
  assert.deepEqual(properties.node_9_part_background_color, {
    kind: "vec4",
    value: [1, 1, 1, 1],
  });
  assert.deepEqual(properties.node_9_part_background_pressed_color, {
    kind: "vec4",
    value: [0, 0, 0, 1],
  });
  assert.deepEqual(properties.node_9_part_background_idle_checked_color, {
    kind: "vec4",
    value: [0, 1, 0, 1],
  });
  assert.deepEqual(properties.node_9_part_focusRing_color, {
    kind: "vec4",
    value: [1, 0, 0, 1],
  });
  assert.deepEqual(GUI_THEME_PARTS, [
    "background",
    "label",
    "icon",
    "focusRing",
  ]);
});

test("theme validation fails closed on names, keys and lanes", () => {
  validateGuiTheme(defaultGuiTheme);
  assert.throws(
    () => validateGuiTheme({ parts: { "bad-name": {} } } as never),
    /not a runtime named part/,
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
  const properties = guiThemeProperties(4, {
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
  assert.deepEqual(properties.node_4_part_background_pressed_motion, {
    kind: "asset",
    value: { kind: 10, source: "asset://motion", variant: 2 },
  });
  assert.deepEqual(properties.node_4_part_background_pressed_duration, {
    kind: "f32",
    value: 0.25,
  });
  assert.deepEqual(properties.node_4_part_background_pressed_easing, {
    kind: "f32",
    value: 1,
  });
  assert.deepEqual(properties.node_4_part_background_pressed_track, {
    kind: "f32",
    value: 3,
  });
  assert.deepEqual(properties.node_4_part_background_pressed_time, {
    kind: "f32",
    value: 0.5,
  });
  assert.throws(
    () =>
      guiThemeProperties(4, {
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
    /requires base color, opacity, and scale lanes/,
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

test("theme-only control paint stays in named parts instead of node style", () => {
  const button = describeButton({
    label: "Painted",
    theme: defaultGuiTheme,
  });
  assert.equal(button.style.backgroundColor, undefined);
  const properties = guiThemeProperties(3, defaultGuiTheme);
  assert.deepEqual(properties.node_3_part_background_color, {
    kind: "vec4",
    value: [0.16, 0.34, 0.72, 1],
  });
  assert.deepEqual(properties.node_3_part_icon_idle_checked_opacity, {
    kind: "f32",
    value: 1,
  });
  assert.deepEqual(properties.node_3_part_icon_idle_unchecked_opacity, {
    kind: "f32",
    value: 0,
  });
});

test("content kinds map to control roles, names and actions", () => {
  assert.equal(controlKindForContent(buttonContent("Go")), "button");
  assert.equal(controlKindForContent(checkboxContent(true)), "checkbox");
  assert.equal(controlKindForContent(sliderContent({})), "slider");
  assert.equal(controlKindForContent(textInputContent({})), "textInput");
  assert.equal(
    controlKindForContent({ kind: "container", containerKind: "row" }),
    null,
  );
  assert.equal(controlKindForContent({ kind: "text", text: "hi" }), null);
  assert.equal(controlKindForContent({ kind: "drawing" }), null);
  assert.equal(controlKindForContent({ kind: "image", size: [1, 1] }), null);
  assert.deepEqual(actionsForControlKind("button"), ["press"]);
  assert.deepEqual(actionsForControlKind("checkbox"), ["toggle", "focus"]);
  assert.deepEqual(actionsForControlKind("slider"), ["setScalar", "focus"]);
  assert.deepEqual(actionsForControlKind("textInput"), ["setText", "focus"]);
  assert.equal(nameForContent(buttonContent("Go")), "Go");
  assert.equal(
    nameForContent(textInputContent({ placeholder: "name" })),
    "name",
  );
  assert.equal(nameForContent(textInputContent({})), undefined);
  assert.equal(nameForContent(checkboxContent(true)), undefined);
});
