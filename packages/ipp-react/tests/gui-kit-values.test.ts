/**
 * Declarations of the GUI kit's value controls (slider scale, labelled and
 * range sliders, knob, numeric stepper and colour picker): which entities,
 * components, links, themes and animation each composition writes, through the
 * real reconciler against the recording World in `gui-kit-support.ts`.
 * Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h } from "react";
import { Entity } from "../src/index.js";
import {
  Knob,
  LabelledSlider,
  NumericStepper,
  RangeSlider,
  SliderScale,
  ColorPicker,
  formatHex,
  hsvaToRgba,
  parseHex,
  rgbaToHsva,
  type ColorPickerProps,
  type KnobProps,
  type NumericStepperProps,
  type LabelledSliderProps,
  type RangeSliderProps,
} from "../src/gui-kit.js";
import { TOKENS, render, settle, near, skinRows } from "./gui-kit-support.js";

/**
 * The slider composites' rail at kit body size `fontSize`: the runtime's
 * unsized depth, its thumb, and where a scale's ticks, origin mark and labels
 * start across the rail from the slider's edge.
 */
function rail(fontSize: number) {
  const k = fontSize / 16;
  const depth = (fontSize * 4) / 3;
  const thumb = 0.75 * depth;
  const tick = depth / 2 + thumb / 2 + 4 * k;
  return {
    depth,
    thumb,
    tick,
    origin: depth / 2 + 4 * k,
    end: tick + 12 * k,
    row: 24 * k,
    gap: 8 * k,
  };
}

/** Width of a line of `text` at `size`, as the kit gives it. */
const lineOf = (text: string, size: number) =>
  [...text].length * 0.54 * size + size / 100;

test("SliderScale marks values where the thumb's centre is, its origin from the rail", async () => {
  const { world } = await render(
    h(SliderScale, {
      id: "pan",
      min: -100,
      max: 100,
      origin: 0,
      vertical: true,
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  // As long as the unsized slider, reaching across to its widest label.
  const root = world.fields("pan", "GuiLayout");
  assert.equal(root.get("kind"), 3);
  assert.equal(root.get("height"), 256);
  near(root.get("width"), g.end + g.gap + lineOf("+100", 26));
  const marks = [0, 1, 2, 3, 4].map((index) => `pan/mark/${index}`);
  assert.deepEqual(world.children("pan"), marks);
  const labels = ["-100", "-50", "0", "+50", "+100"];
  marks.forEach((mark, index) => {
    // A thumb-sized box aligned by the value's fraction, minimum at the bottom.
    const box = world.fields(mark, "GuiLayout");
    assert.equal(box.get("kind"), 3);
    near(box.get("height"), g.thumb);
    near(box.get("align_y"), 1 - index / 2);
    assert.deepEqual(world.children(mark), [`${mark}/tick`, `${mark}/label`]);
    const origin = index === 2;
    assert.equal(world.skin(`${mark}/tick`), origin ? "division" : "quiet");
    const tick = world.fields(`${mark}/tick`, "GuiLayout");
    near(tick.get("margin_left"), origin ? g.origin : g.tick);
    near(tick.get("width"), g.end - (origin ? g.origin : g.tick));
    assert.equal(tick.get("height"), 2.5);
    assert.equal(tick.get("align_y"), 0);
    // The label past the tick, centred on the mark, small and neutral.
    const label = world.fields(`${mark}/label`, "GuiLayout");
    near(label.get("margin_left"), g.end + g.gap);
    near(label.get("margin_top"), (g.thumb - g.row) / 2);
    near(label.get("margin_bottom"), (g.thumb - g.row) / 2);
    assert.equal(
      world.fields(`${mark}/label/text`, "CanvasText").get("text"),
      labels[index],
    );
    assert.equal(
      world.fields(`${mark}/label/text`, "CanvasText").get("font_size"),
      26,
    );
    assert.deepEqual(world.tone(`${mark}/label/text`), TOKENS.neutral);
  });
});

test("a horizontal SliderScale hangs its labels under the ticks with the marks' decimals", async () => {
  const { world } = await render(
    h(SliderScale, { id: "volts", min: 0, max: 1, count: 3, units: "V" }),
    { fontSize: 32 },
  );
  const g = rail(32);
  // As long as its stack, reaching down to its labels' dense row.
  const root = world.fields("volts", "GuiLayout");
  near(root.get("height"), g.end + g.row);
  assert.equal(root.has("width"), false);
  const marks = [0, 1, 2].map((index) => `volts/mark/${index}`);
  assert.deepEqual(world.children("volts"), marks);
  assert.deepEqual(
    marks.map((mark) => world.fields(mark, "GuiLayout").get("align_x")),
    [-1, 0, 1],
  );
  near(world.fields(marks[0]!, "GuiLayout").get("width"), g.thumb);
  // Units follow every label; a unit of letters after a space.
  assert.deepEqual(
    marks.map((mark) =>
      world.fields(`${mark}/label/text`, "CanvasText").get("text"),
    ),
    ["0.0 V", "0.5 V", "1.0 V"],
  );
  const label = world.fields("volts/mark/1/label", "GuiLayout");
  near(label.get("margin_top"), g.end);
  near(label.get("margin_left"), (g.thumb - lineOf("0.5 V", 26)) / 2);
  near(label.get("margin_right"), (g.thumb - lineOf("0.5 V", 26)) / 2);
  const tick = world.fields("volts/mark/1/tick", "GuiLayout");
  near(tick.get("margin_top"), g.tick);
  near(tick.get("height"), g.end - g.tick);
  assert.equal(tick.get("align_x"), 0);
});

const gain = (props: Partial<LabelledSliderProps> = {}) =>
  h(LabelledSlider, {
    id: "gain",
    label: "GAIN",
    min: 0,
    max: 100,
    step: 1,
    fineStep: 0.1,
    units: "%",
    ...props,
  });

test("LabelledSlider: caption and readout above a rail between its end captions", async () => {
  const changes: number[] = [];
  const { world, root, draw } = await render(
    gain({ defaultValue: 65, onChange: (value) => changes.push(value) }),
    { fontSize: 32 },
  );
  const g = rail(32);
  const layout = world.fields("gain", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  near(layout.get("height"), g.row + g.depth);
  assert.deepEqual(world.children("gain"), ["gain/header", "gain/rail-row"]);
  assert.deepEqual(world.children("gain/header"), [
    "gain/header/strut",
    "gain/label",
    "gain/readout",
  ]);
  assert.deepEqual(world.tone("gain/label"), TOKENS.accent);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "65%");
  assert.deepEqual(world.tone("gain/readout"), TOKENS.text);
  // The rail between the range's ends, centred on it, half an inset away.
  assert.deepEqual(world.children("gain/rail-row"), [
    "gain/min",
    "gain/rail",
    "gain/max",
  ]);
  assert.equal(world.fields("gain/min/text", "CanvasText").get("text"), "0%");
  assert.deepEqual(world.tone("gain/min/text"), TOKENS.neutral);
  near(world.fields("gain/min", "GuiLayout").get("height"), g.depth);
  near(world.fields("gain/min", "GuiLayout").get("margin_right"), g.gap);
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "100%");
  assert.equal(world.fields("gain/rail", "GuiLayout").get("flex"), 1);
  // The runtime's slider at its unsized depth, filling the rail, unskinned.
  assert.deepEqual(world.children("gain/rail"), ["gain/slider"]);
  const slider = world.fields("gain/slider", "GuiSlider");
  assert.deepEqual(
    ["min", "max", "step", "fine_step", "axis", "value"].map((field) =>
      slider.get(field),
    ),
    [0, 100, 1, 0.1, 0, 65],
  );
  assert.equal(slider.has("origin"), false);
  assert.equal(world.fields("gain/slider", "GuiLayout").get("kind"), 3);
  near(world.fields("gain/slider", "GuiLayout").get("height"), g.depth);
  assert.equal(world.entity("gain/slider").components.has("GuiSkin"), false);
  const behavior = world.fields("gain/slider", "GuiBehavior");
  assert.equal(behavior.get("semantic_label"), "GAIN");
  assert.equal(behavior.get("enabled"), true);

  // A committed value is reported and read out at once.
  await settle(root);
  world.value("gain/slider", "GuiSlider", {
    value: 70,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [70]);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "70%");
  assert.equal(world.fields("gain/slider", "GuiSlider").get("value"), 65);
  assert.deepEqual(world.compared, []);

  // Disabled: the value stays, every text turns neutral.
  await draw(gain({ defaultValue: 65, disabled: true }));
  assert.equal(
    world.fields("gain/slider", "GuiBehavior").get("enabled"),
    false,
  );
  for (const text of ["gain/label", "gain/readout", "gain/min/text"])
    assert.deepEqual(world.tone(text), TOKENS.neutral, text);
});

test("a vertical LabelledSlider centres its rail, with a signed scale right of it", async () => {
  const { world } = await render(
    h(LabelledSlider, {
      id: "pan",
      label: "PAN",
      min: -100,
      max: 100,
      step: 1,
      origin: 0,
      defaultValue: -30,
      units: "%",
      vertical: true,
      length: 160,
      scale: { count: 5 },
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  const reach = g.end + g.gap + lineOf("+100", 26);
  // Caption, rail and readout; the scale labels the ends, so no captions.
  near(world.fields("pan", "GuiLayout").get("height"), 2 * g.row + 320);
  assert.deepEqual(world.children("pan"), [
    "pan/label",
    "pan/track",
    "pan/readout",
  ]);
  assert.equal(world.fields("pan/label", "GuiLayout").get("align_x"), 0);
  assert.equal(
    world.fields("pan/readout/text", "CanvasText").get("text"),
    "-30%",
  );
  // The rail centred in the column; the scale reaches past it.
  const box = world.fields("pan/rail", "GuiLayout");
  near(box.get("width"), reach);
  assert.equal(box.get("height"), 320);
  assert.equal(box.get("align_x"), 0);
  near(box.get("margin_right"), g.depth - reach);
  assert.deepEqual(world.children("pan/rail"), ["pan/scale", "pan/slider"]);
  const slider = world.fields("pan/slider", "GuiSlider");
  assert.equal(slider.get("axis"), 1);
  assert.equal(slider.get("origin"), 0);
  assert.equal(slider.get("value"), -30);
  near(world.fields("pan/slider", "GuiLayout").get("width"), g.depth);
  // The scale's zero is the slider's origin, its labels signed, unitless.
  assert.equal(world.fields("pan/scale", "GuiLayout").get("height"), 320);
  assert.equal(world.skin("pan/scale/mark/2/tick"), "division");
  assert.equal(
    world.fields("pan/scale/mark/4/label/text", "CanvasText").get("text"),
    "+100",
  );
});

test("a slider composite reports committed values and writes only the application's own", async () => {
  const changes: number[] = [];
  const onChange = (value: number) => changes.push(value);
  const handle: { current: unknown } = { current: null };
  const { world, root, draw } = await render(
    gain({ value: 20, onChange, ref: handle as never }),
  );
  await settle(root);
  assert.ok(handle.current, "the composite's ref receives the handle");

  // A drag commits 30: reported, and nothing is written back when the
  // application follows.
  world.value("gain/slider", "GuiSlider", {
    value: 30,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
  await draw(gain({ value: 30, onChange, ref: handle as never }));
  await settle(root);
  assert.deepEqual(world.compared, []);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "30%");

  // The application's own value, even the one declared at mount, is written
  // by compare-and-set from the reported one; its echo is not reported.
  await draw(gain({ value: 20, onChange, ref: handle as never }));
  await settle(root);
  assert.deepEqual(world.compared, [["gain/slider", "value", 20]]);
  assert.equal(world.fields("gain/readout", "CanvasText").get("text"), "20%");
  world.value("gain/slider", "GuiSlider", {
    value: 20,
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
  // Single precision: the runtime's 0.1 is the application's 0.1.
  await draw(gain({ value: 0.1, onChange }));
  await settle(root);
  world.value("gain/slider", "GuiSlider", {
    value: Math.fround(0.1),
    upper: 0,
    range: false,
  });
  await settle(root);
  assert.deepEqual(changes, [30]);
});

const distance = (props: Partial<RangeSliderProps> = {}) =>
  h(RangeSlider, {
    id: "dist",
    min: 0,
    max: 100,
    step: 1,
    units: "m",
    ...props,
  });

test("RangeSlider: readouts follow their thumbs and both values report once", async () => {
  const changes: (readonly number[])[] = [];
  const { world, root } = await render(
    distance({
      label: "DISTANCE",
      defaultValue: [20, 80],
      onChange: (value) => changes.push(value),
    }),
    { fontSize: 32 },
  );
  const g = rail(32);
  near(world.fields("dist", "GuiLayout").get("height"), 2 * g.row + g.depth);
  assert.deepEqual(world.children("dist"), ["dist/header", "dist/rail-row"]);
  assert.deepEqual(world.children("dist/rail-row"), [
    "dist/min",
    "dist/rail",
    "dist/max",
  ]);
  assert.equal(
    world.fields("dist/max/text", "CanvasText").get("text"),
    "100 m",
  );
  const slider = world.fields("dist/slider", "GuiSlider");
  assert.deepEqual(
    ["range", "value", "upper", "axis"].map((field) => slider.get(field)),
    [true, 20, 80, 0],
  );
  // Each readout centred under its thumb, a dense row below the slider.
  assert.deepEqual(world.children("dist/rail"), [
    "dist/thumb/0",
    "dist/thumb/1",
    "dist/slider",
  ]);
  const readouts = () =>
    [0, 1].map((index) => [
      world.fields(`dist/thumb/${index}`, "GuiLayout").get("align_x"),
      world.fields(`dist/readout/${index}/text`, "CanvasText").get("text"),
    ]);
  const close = (pairs: unknown[][], expected: [number, string][]) =>
    pairs.forEach(([align, text], index) => {
      near(align, expected[index]![0]);
      assert.equal(text, expected[index]![1]);
    });
  close(readouts(), [
    [-0.6, "20 m"],
    [0.6, "80 m"],
  ]);
  // The lower readout hangs from its thumb's centre toward the minimum.
  const readout = world.fields("dist/readout/0", "GuiLayout");
  near(readout.get("margin_top"), g.depth);
  near(readout.get("margin_left"), g.thumb / 2 - 8 - lineOf("20 m", 26));
  assert.deepEqual(world.tone("dist/readout/0/text"), TOKENS.text);

  // A drag of the lower thumb: one event, one report of both values.
  await settle(root);
  world.value("dist/slider", "GuiSlider", {
    value: 35,
    upper: 80,
    range: true,
  });
  await settle(root);
  assert.deepEqual(changes, [[35, 80]]);
  close(readouts(), [
    [-0.3, "35 m"],
    [0.6, "80 m"],
  ]);
});

test("a range's own interval is written one field at a time, never inverted", async () => {
  const changes: (readonly number[])[] = [];
  const onChange = (value: readonly number[]) => changes.push(value);
  const { world, root, draw } = await render(
    distance({ value: [20, 80], onChange }),
  );
  await settle(root);
  // Up past the upper value: the upper value moves first.
  await draw(distance({ value: [85, 95], onChange }));
  await settle(root);
  assert.deepEqual(world.compared, [
    ["dist/slider", "upper", 95],
    ["dist/slider", "value", 85],
  ]);
  // The state between the writes, and the echo, are not reported.
  world.value("dist/slider", "GuiSlider", {
    value: 20,
    upper: 95,
    range: true,
  });
  world.value("dist/slider", "GuiSlider", {
    value: 85,
    upper: 95,
    range: true,
  });
  await settle(root);
  assert.deepEqual(changes, []);
  // Down below the lower value: the lower value moves first; a value that
  // keeps its upper end writes only the lower one.
  await draw(distance({ value: [5, 10], onChange }));
  await draw(distance({ value: [0, 10], onChange }));
  await settle(root);
  assert.deepEqual(world.compared.slice(2), [
    ["dist/slider", "value", 5],
    ["dist/slider", "upper", 10],
    ["dist/slider", "value", 0],
  ]);
});

test("a vertical RangeSlider puts each readout right of its thumb", async () => {
  const { world } = await render(
    distance({ defaultValue: [20, 80], vertical: true, length: 160 }),
    { fontSize: 32 },
  );
  const g = rail(32);
  near(world.fields("dist", "GuiLayout").get("height"), 2 * g.row + 320);
  assert.deepEqual(world.children("dist"), [
    "dist/max",
    "dist/track",
    "dist/min",
  ]);
  assert.equal(world.fields("dist/slider", "GuiSlider").get("axis"), 1);
  const offset = (g.depth + g.thumb) / 2 + g.gap;
  const box = world.fields("dist/rail", "GuiLayout");
  near(box.get("width"), offset + lineOf("100 m", 26));
  near(world.fields("dist/thumb/1", "GuiLayout").get("align_y"), -0.6);
  near(world.fields("dist/thumb/0", "GuiLayout").get("height"), g.thumb);
  near(world.fields("dist/readout/1", "GuiLayout").get("margin_left"), offset);
});

const knob = (props: Partial<KnobProps> = {}) =>
  h(Knob, {
    id: "gain",
    label: "GAIN",
    min: 0,
    max: 100,
    step: 1,
    units: "%",
    ...props,
  });

test("Knob: the dial's housing holds the readout under the dial and the ends at the sweep's", async () => {
  const changes: number[] = [];
  const { world, root, draw } = await render(
    knob({ defaultValue: 65, onChange: (value) => changes.push(value) }),
  );
  const layout = world.fields("gain", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  assert.equal(layout.get("width"), 80);
  assert.equal(layout.get("height"), 24 + 80 + 24);
  assert.deepEqual(world.children("gain"), ["gain/caption", "gain/dial"]);
  assert.deepEqual(world.tone("gain/label"), TOKENS.accent);
  // The housing is the dial-presented slider, a dense row taller than wide.
  const dial = world.fields("gain/dial", "GuiLayout");
  assert.deepEqual(
    ["kind", "width", "height", "align_x"].map((field) => dial.get(field)),
    [3, 80, 104, 0],
  );
  const slider = world.fields("gain/dial", "GuiSlider");
  assert.equal(slider.get("axis"), 2);
  assert.equal(slider.get("value"), 65);
  assert.equal(world.entity("gain/dial").components.has("GuiSkin"), false);
  assert.deepEqual(world.children("gain/dial"), [
    "gain/min",
    "gain/max",
    "gain/readout",
  ]);
  const readout = world.fields("gain/readout", "GuiLayout");
  assert.equal(readout.get("margin_top"), 80);
  assert.equal(readout.get("align_x"), 0);
  assert.equal(
    world.fields("gain/readout/text", "CanvasText").get("text"),
    "65%",
  );
  // Each end centred a small size below the tick ring's end, but half an
  // inset inside the housing: 100% would reach past it.
  const reach = (40 - 8) / Math.SQRT2;
  const min = world.fields("gain/min", "GuiLayout");
  near(min.get("margin_left"), 40 - reach - lineOf("0%", 13) / 2);
  near(min.get("margin_top"), 40 + reach + 13 - 12);
  const max = world.fields("gain/max", "GuiLayout");
  near(max.get("margin_left"), 80 - 8 - lineOf("100%", 13));
  near(max.get("margin_top"), 40 + reach + 13 - 12);
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "100%");

  await settle(root);
  world.value("gain/dial", "GuiSlider", { value: 80, upper: 0, range: false });
  await settle(root);
  assert.deepEqual(changes, [80]);
  assert.equal(
    world.fields("gain/readout/text", "CanvasText").get("text"),
    "80%",
  );

  // Bipolar from zero, signed ends; the paired input's row under the housing.
  await draw(
    h(Knob, {
      id: "gain",
      label: "PAN",
      min: -100,
      max: 100,
      step: 1,
      origin: 0,
      children: h(Entity, { id: "precise" }),
    }),
  );
  assert.equal(world.fields("gain/dial", "GuiSlider").get("origin"), 0);
  assert.equal(world.fields("gain/min/text", "CanvasText").get("text"), "-100");
  assert.equal(world.fields("gain/max/text", "CanvasText").get("text"), "+100");
  // The input's column: a numeric stepper's field and error line.
  assert.equal(
    world.fields("gain", "GuiLayout").get("height"),
    128 + 8 + 40 + 8 + 40,
  );
  assert.deepEqual(world.children("gain/slot"), ["precise"]);
  const input = world.fields("gain/slot", "GuiLayout");
  assert.equal(input.get("height"), 40 + 8 + 40);
  assert.equal(input.get("margin_top"), 8);

  // A larger dial gives longer end captions room.
  await draw(knob({ size: 112, defaultValue: 65 }));
  assert.equal(world.fields("gain", "GuiLayout").get("width"), 112);
  const larger = world.fields("gain/dial", "GuiLayout");
  assert.equal(larger.get("width"), 112);
  assert.equal(larger.get("height"), 112 + 24);
  near(
    world.fields("gain/max", "GuiLayout").get("margin_top"),
    56 + (56 - 8) / Math.SQRT2 + 13 - 12,
  );
});

test("range readouts hang from their thumbs away from each other, so thumbs together keep them apart", async () => {
  const { world, draw } = await render(distance({ defaultValue: [50, 50] }), {
    fontSize: 32,
  });
  const g = rail(32);
  const clear = 8;
  // Both marks at the same place; the lower text ends a quarter inset before
  // the thumbs' centre and the upper one starts a quarter inset after it.
  near(world.fields("dist/thumb/0", "GuiLayout").get("align_x"), 0);
  near(world.fields("dist/thumb/1", "GuiLayout").get("align_x"), 0);
  const width = lineOf("50 m", 26);
  const lower = world.fields("dist/readout/0", "GuiLayout");
  const upper = world.fields("dist/readout/1", "GuiLayout");
  near(lower.get("margin_left"), g.thumb / 2 - clear - width);
  near(lower.get("margin_right"), g.thumb / 2 + clear);
  near(upper.get("margin_left"), g.thumb / 2 + clear);
  near(upper.get("margin_right"), g.thumb / 2 - clear - width);
  const lowerEnd = (lower.get("margin_left") as number) + width;
  assert.ok(
    lowerEnd + 2 * clear <= (upper.get("margin_left") as number) + 1e-9,
    "the readouts overlap",
  );

  // Vertical: the lower readout's row hangs down from the centre, the upper
  // one's ends there.
  await draw(distance({ defaultValue: [50, 50], vertical: true, length: 160 }));
  const down = world.fields("dist/readout/0", "GuiLayout");
  const up = world.fields("dist/readout/1", "GuiLayout");
  near(down.get("margin_top"), g.thumb / 2);
  near(down.get("margin_bottom"), g.thumb / 2 - g.row);
  near(up.get("margin_top"), g.thumb / 2 - g.row);
  near(up.get("margin_bottom"), g.thumb / 2);
});

const exposure = (props: Partial<NumericStepperProps> = {}) =>
  h(NumericStepper, {
    id: "ev",
    label: "EXPOSURE",
    min: -4,
    max: 4,
    step: 0.25,
    fineStep: 0.05,
    precision: 2,
    units: "EV",
    defaultValue: 1.25,
    ...props,
  });

test("NumericStepper: caption and range, the numeric field with its unit, and the error line's room", async () => {
  const { world, draw } = await render(exposure(), { fontSize: 32 });
  // Caption, field and the error line reserved, at twice the design size.
  const root = world.fields("ev", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("height"), 2 * (24 + 40 + 8 + 40));
  assert.deepEqual(world.children("ev"), ["ev/caption", "ev/row"]);
  assert.deepEqual(world.children("ev/caption"), [
    "ev/caption/strut",
    "ev/label",
    "ev/ends",
  ]);
  assert.deepEqual(world.tone("ev/label"), TOKENS.accent);
  const ends = world.fields("ev/ends", "CanvasText");
  assert.equal(ends.get("text"), "-4.00 – +4.00 EV");
  assert.equal(ends.get("font_size"), 26);
  assert.deepEqual(world.tone("ev/ends"), TOKENS.neutral);
  // The runtime's numeric input fills the row beside its unit.
  assert.deepEqual(world.children("ev/row"), [
    "ev/row/strut",
    "ev/field",
    "ev/units",
  ]);
  const field = world.fields("ev/field", "GuiTextInput");
  assert.deepEqual(
    [
      "numeric",
      "value",
      "min",
      "max",
      "step",
      "fine_step",
      "precision",
      "step_parts",
    ].map((name) => field.get(name)),
    [true, 1.25, -4, 4, 0.25, 0.05, 2, true],
  );
  const fieldLayout = world.fields("ev/field", "GuiLayout");
  assert.equal(fieldLayout.get("flex"), 1);
  assert.equal(fieldLayout.get("height"), 80);
  assert.equal(world.entity("ev/field").components.has("GuiSkin"), false);
  const behavior = world.fields("ev/field", "GuiBehavior");
  assert.equal(behavior.get("semantic_label"), "EXPOSURE");
  assert.equal(behavior.get("enabled"), true);
  assert.equal(world.fields("ev/units", "CanvasText").get("text"), "EV");
  assert.equal(world.fields("ev/units", "GuiLayout").get("margin_left"), 16);
  assert.deepEqual(world.tone("ev/units"), TOKENS.text);

  // Without a caption or range, and without step parts; disabled, neutral.
  await draw(
    h(NumericStepper, {
      id: "ev",
      units: "%",
      stepParts: false,
      disabled: true,
    }),
  );
  assert.equal(
    world.fields("ev", "GuiLayout").get("height"),
    2 * (40 + 8 + 40),
  );
  assert.deepEqual(world.children("ev"), ["ev/row"]);
  assert.equal(
    world.fields("ev/field", "GuiTextInput").get("step_parts"),
    false,
  );
  assert.equal(world.fields("ev/field", "GuiBehavior").get("enabled"), false);
  assert.deepEqual(world.tone("ev/units"), TOKENS.neutral);
});

test("a rejected entry shows its error until a later commit, submission, discard or blur", async () => {
  const changes: number[] = [];
  const { world, root } = await render(
    exposure({
      onChange: (value) => changes.push(value),
      invalidMessage: (text) => `${text} is not a number`,
    }),
  );
  await settle(root);
  const shown = () =>
    world.entities.has("ev/error")
      ? world.fields("ev/error/text", "CanvasText").get("text")
      : undefined;
  const reject = async (text: string, tick: bigint) => {
    world.effect("ev/field", { kind: "rejected", text }, "GuiTextInput", tick);
    await settle(root);
  };

  // A rejection shows the error alert under the field, which stays put.
  await reject("abc", 5n);
  assert.equal(shown(), "abc is not a number");
  assert.equal(world.skin("ev/error"), "alertError");
  assert.equal(world.fields("ev/error", "GuiLayout").get("margin_top"), 8);
  assert.deepEqual(world.children("ev"), ["ev/caption", "ev/row", "ev/error"]);
  assert.equal(world.fields("ev", "GuiLayout").get("height"), 24 + 40 + 8 + 40);

  // A commit, here at tick 10, clears it and is reported.
  world.value("ev/field", "GuiTextInput", { numeric: true, value: 2 });
  await settle(root);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);
  // A rejection from that frame or earlier, delivered after it, stays
  // superseded; a later one shows.
  await reject("x", 10n);
  assert.equal(shown(), undefined);
  await reject("y", 11n);
  assert.equal(shown(), "y is not a number");

  // Blur ends the edit, and with it the error.
  world.effect(
    "ev/field",
    { kind: "focusChanged", focused: false, changed: true, part: 0 },
    "GuiTextInput",
    12n,
  );
  await settle(root);
  assert.equal(shown(), undefined);

  // A submission commits an edit even when the number stays the same.
  await reject("z", 13n);
  assert.equal(shown(), "z is not a number");
  world.effect(
    "ev/field",
    { kind: "submitted", text: "2.00" },
    "GuiTextInput",
    14n,
  );
  await settle(root);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);

  // A discarded edit, as by Escape, clears it without a commit, and a
  // rejection from that frame or earlier stays superseded.
  await reject("w", 15n);
  assert.equal(shown(), "w is not a number");
  world.effect(
    "ev/field",
    { kind: "discarded", text: "w" },
    "GuiTextInput",
    16n,
  );
  await settle(root);
  assert.equal(shown(), undefined);
  await reject("v", 16n);
  assert.equal(shown(), undefined);
  assert.deepEqual(changes, [2]);
});

/** Each of `actual`'s leading numbers within `tolerance` of `expected`'s. */
const nearAll = (
  actual: readonly number[],
  expected: readonly number[],
  tolerance = 1e-9,
) =>
  expected.forEach((value, index) =>
    assert.ok(
      Math.abs(actual[index]! - value) <= tolerance,
      `${actual} is not ${expected}`,
    ),
  );

/** The sheet's #54F4FF as the colour control holds it. */
const CYAN = {
  hue: (4 - 160 / 171) / 6,
  saturation: 171 / 255,
  value: 1,
  alpha: 1,
};

test("hex and channels convert to and from HSVA stably, keeping hue through grey and black", () => {
  // Every byte colour on a grid reads back as itself through the control's
  // single precision, with and without coverage.
  const levels = Array.from({ length: 16 }, (_, index) => index * 17);
  for (const red of levels)
    for (const green of levels)
      for (const blue of levels) {
        const hex = `#${[red, green, blue]
          .map((level) => level.toString(16).padStart(2, "0"))
          .join("")
          .toUpperCase()}`;
        assert.equal(formatHex(parseHex(hex)!), hex);
        assert.equal(formatHex(parseHex(`${hex}80`)!), `${hex}80`);
      }
  assert.deepEqual(hsvaToRgba(CYAN), {
    red: 84,
    green: 244,
    blue: 255,
    alpha: 1,
  });
  assert.equal(formatHex(CYAN), "#54F4FF");
  // Coverage as a byte after the colour, only while not opaque or wanted.
  const half = parseHex("#54f4ff80")!;
  near(half.alpha, 128 / 255);
  assert.equal(formatHex(half), "#54F4FF80");
  assert.equal(formatHex(half, false), "#54F4FF");
  assert.equal(parseHex(" 54F4FF ", { ...CYAN, alpha: 0.25 })?.alpha, 0.25);
  for (const text of ["#54F4F", "#54F4FFF", "54F4FG", "", "#54F4FF8"])
    assert.equal(parseHex(text), undefined, text);

  // Grey, white and black keep the previous hue, black its saturation too.
  const previous = { hue: 0.4, saturation: 0.6, value: 0.8, alpha: 1 };
  const grey = parseHex("#808080", previous)!;
  assert.equal(grey.hue, 0.4);
  assert.equal(grey.saturation, 0);
  near(grey.value, 128 / 255);
  assert.deepEqual(parseHex("#FFFFFF", previous), {
    hue: 0.4,
    saturation: 0,
    value: 1,
    alpha: 1,
  });
  assert.deepEqual(parseHex("#000000", previous), {
    hue: 0.4,
    saturation: 0.6,
    value: 0,
    alpha: 1,
  });
  // Bytes are clamped to their range.
  assert.deepEqual(rgbaToHsva({ red: 300, green: -4, blue: 0, alpha: 2 }), {
    hue: 0,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
});

const MAGENTA_PRESET = parseHex("#F4449F")!;

const picker = (props: Partial<ColorPickerProps> = {}) =>
  h(ColorPicker, {
    id: "pick",
    label: "COLOR",
    defaultValue: CYAN,
    presets: [{ value: MAGENTA_PRESET, label: "Magenta" }],
    ...props,
  });

test("ColorPicker: labels over the control, channels beside it, presets, hex in sRGB and the error line's room", async () => {
  const { world, draw } = await render(picker());
  const root = world.fields("pick", "GuiLayout");
  assert.equal(root.get("kind"), 2);
  assert.equal(root.get("width"), 240 + 16 + 112);
  assert.equal(root.get("height"), 24 + 200 + (8 + 32) + 2 * (8 + 40));
  assert.deepEqual(world.children("pick"), [
    "pick/caption",
    "pick/main",
    "pick/presets",
    "pick/hex",
  ]);
  // The caption over the field, the rails' names centred over the rails.
  assert.equal(world.fields("pick/label", "GuiLayout").get("margin_left"), 8);
  assert.deepEqual(world.tone("pick/label/text"), TOKENS.accent);
  near(
    world.fields("pick/hue", "GuiLayout").get("margin_left"),
    180 - lineOf("Hue", 13) / 2,
  );
  near(
    world.fields("pick/opacity", "GuiLayout").get("margin_left"),
    220 - lineOf("Opacity", 13) / 2,
  );
  // The runtime's control at its own size, holding the colour.
  assert.deepEqual(world.children("pick/main"), [
    "pick/control",
    "pick/channels",
  ]);
  const control = world.fields("pick/control", "GuiColor");
  assert.deepEqual(
    ["hue", "saturation", "value", "alpha", "alpha_rail"].map((field) =>
      control.get(field),
    ),
    [CYAN.hue, CYAN.saturation, 1, 1, true],
  );
  assert.equal(world.fields("pick/control", "GuiLayout").get("width"), 240);
  assert.equal(world.fields("pick/control", "GuiLayout").get("height"), 200);
  // R, G and B in bytes and A in percent, numeric fields without parts.
  assert.deepEqual(world.children("pick/channels"), [
    "pick/red",
    "pick/green",
    "pick/blue",
    "pick/alpha",
  ]);
  for (const [channel, number, max] of [
    ["red", 84, 255],
    ["green", 244, 255],
    ["blue", 255, 255],
    ["alpha", 100, 100],
  ] as const) {
    const field = world.fields(`pick/${channel}/field`, "GuiTextInput");
    assert.deepEqual(
      ["numeric", "value", "min", "max", "precision", "step_parts"].map(
        (name) => field.get(name),
      ),
      [true, number, 0, max, 0, false],
      channel,
    );
  }
  assert.equal(world.fields("pick/alpha/units", "CanvasText").get("text"), "%");
  // The hex entry states its colour space.
  assert.equal(
    world.fields("pick/hex/field", "GuiTextInput").get("text"),
    "#54F4FF",
  );
  assert.equal(
    world.fields("pick/hex/space", "CanvasText").get("text"),
    "sRGB",
  );
  // A preset is a Button painted in its colour over the secondary look.
  assert.equal(world.fields("pick/presets", "GuiGroup").get("selection"), 0);
  assert.equal(world.skin("pick/preset/0"), "secondary");
  assert.equal(
    world.fields("pick/preset/0", "GuiBehavior").get("semantic_label"),
    "Magenta",
  );
  const swatch = skinRows(world, "pick/preset/0")[0]![1].color as number[];
  nearAll(swatch, [((244 / 255 + 0.055) / 1.055) ** 2.4], 1e-6);
  near(swatch[3]!, 1);

  // Without alpha: no alpha rail, A field or Opacity, a narrower control.
  await draw(h(ColorPicker, { id: "pick", alpha: false }));
  assert.equal(
    world.fields("pick/control", "GuiColor").get("alpha_rail"),
    false,
  );
  assert.equal(world.entities.has("pick/alpha"), false);
  assert.equal(world.entities.has("pick/opacity"), false);
  assert.equal(world.fields("pick", "GuiLayout").get("width"), 200 + 16 + 112);
});

test("ColorPicker entries set the one colour and report what the runtime commits", async () => {
  const changes: unknown[] = [];
  const { world, root } = await render(
    picker({ onChange: (value) => changes.push(value) }),
  );
  await settle(root);
  const colors = () =>
    world.actions
      .filter(([symbol]) => symbol === "pick/control")
      .map(([, action]) => (action as { value: number[] }).value);

  // A drag reported by the runtime is the application's, and the channels
  // and hex follow it.
  world.value("pick/control", "GuiColor", {
    hue: 0,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.deepEqual(changes, [{ hue: 0, saturation: 1, value: 1, alpha: 1 }]);
  assert.equal(
    world.fields("pick/red/field", "GuiTextInput").get("value"),
    255,
  );
  assert.equal(
    world.fields("pick/green/field", "GuiTextInput").get("value"),
    0,
  );
  assert.equal(
    world.fields("pick/hex/field", "GuiTextInput").get("text"),
    "#FF0000",
  );
  assert.deepEqual(colors(), []);

  // A submitted hex sets the colour with the control's action; a grey keeps
  // the hue.
  const submit = (text: string, tick: bigint) =>
    world.effect(
      "pick/hex/field",
      { kind: "submitted", text },
      "GuiTextInput",
      tick,
    );
  submit("#00ff00", 20n);
  await settle(root);
  nearAll(colors().at(-1)!, [1 / 3, 1, 1, 1]);
  submit("#808080", 21n);
  await settle(root);
  const grey = colors().at(-1)!;
  assert.equal(grey[0], 0);
  assert.equal(grey[1], 0);

  // Text that is no hex shows the error and sets nothing.
  const before = colors().length;
  submit("#12345", 22n);
  await settle(root);
  assert.equal(colors().length, before);
  assert.equal(
    world.fields("pick/error/text", "CanvasText").get("text"),
    "Use #RRGGBB or #RRGGBBAA",
  );
  assert.equal(world.skin("pick/error"), "alertError");
  // The next committed colour clears it.
  world.value("pick/control", "GuiColor", {
    hue: 0.5,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  await settle(root);
  assert.equal(world.entities.has("pick/error"), false);

  // A channel's number is an entry only while its field holds focus: the
  // field's own update from a drag sets nothing.
  world.value("pick/blue/field", "GuiTextInput", { numeric: true, value: 9 });
  await settle(root);
  assert.equal(colors().length, before);
  world.effect(
    "pick/green/field",
    { kind: "focusChanged", focused: true, changed: true, part: 0 },
    "GuiTextInput",
    1n,
  );
  world.value("pick/green/field", "GuiTextInput", {
    numeric: true,
    value: 128,
  });
  await settle(root);
  const typed = colors().at(-1)!;
  assert.deepEqual(
    hsvaToRgba({
      hue: typed[0]!,
      saturation: typed[1]!,
      value: typed[2]!,
      alpha: typed[3]!,
    }),
    { red: 0, green: 128, blue: 255, alpha: 1 },
  );
  // A percent of alpha changes the alpha alone.
  world.effect(
    "pick/alpha/field",
    { kind: "focusChanged", focused: true, changed: true, part: 0 },
    "GuiTextInput",
    2n,
  );
  world.value("pick/alpha/field", "GuiTextInput", { numeric: true, value: 50 });
  await settle(root);
  nearAll(colors().at(-1)!, [0.5, 1, 1, 0.5]);

  // A channel entry that is no number shows its error until that field
  // discards the edit.
  world.effect(
    "pick/green/field",
    { kind: "rejected", text: "x" },
    "GuiTextInput",
    30n,
  );
  await settle(root);
  assert.equal(
    world.fields("pick/error/text", "CanvasText").get("text"),
    "G: Not a number",
  );
  world.effect(
    "pick/green/field",
    { kind: "discarded", text: "x" },
    "GuiTextInput",
    31n,
  );
  await settle(root);
  assert.equal(world.entities.has("pick/error"), false);

  // Text typed in the hex field is entered on blur; once the next colour
  // has replaced it, a later blur enters nothing, so a drag's colour is
  // never written over by the text the field held before it.
  const focusHex = (focused: boolean, tick: bigint) =>
    world.effect(
      "pick/hex/field",
      { kind: "focusChanged", focused, changed: true, part: 0 },
      "GuiTextInput",
      tick,
    );
  focusHex(true, 3n);
  world.value("pick/hex/field", "GuiTextInput", { text: "#0000ff" });
  focusHex(false, 1000n);
  await settle(root);
  nearAll(colors().at(-1)!, [2 / 3, 1, 1, 1]);
  const entries = colors().length;
  world.value("pick/control", "GuiColor", {
    hue: 0.25,
    saturation: 1,
    value: 1,
    alpha: 1,
  });
  focusHex(true, 1001n);
  focusHex(false, 1002n);
  await settle(root);
  assert.equal(colors().length, entries);

  // A preset is an explicit choice.
  world.effect("pick/preset/0", { kind: "pressed" });
  await settle(root);
  const preset = colors().at(-1)!;
  near(preset[0]!, MAGENTA_PRESET.hue);
  near(preset[2]!, MAGENTA_PRESET.value);

  // An application's own colour is set, and its echo not reported back.
  const own: unknown[] = [];
  const controlled = await render(
    picker({ value: CYAN, onChange: (value) => own.push(value) }),
  );
  await settle(controlled.root);
  await controlled.draw(
    picker({ value: MAGENTA_PRESET, onChange: (value) => own.push(value) }),
  );
  await settle(controlled.root);
  const written = controlled.world.actions.at(-1)!;
  assert.equal(written[0], "pick/control");
  controlled.world.value("pick/control", "GuiColor", { ...MAGENTA_PRESET });
  await settle(controlled.root);
  assert.deepEqual(own, []);
});
