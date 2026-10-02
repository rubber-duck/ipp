import { check, compositeTests, json, type CompositeImage } from "./harness.js";
import type { prepare } from "./kit-colour.js";
import { KIT_COLOUR_FONT } from "./kit-colour-panel.js";

type Hsva = {
  hue: number;
  saturation: number;
  value: number;
  alpha: number;
};

/**
 * The HSV model on sRGB-encoded values from the textbook sector formula, as
 * 0..255 levels before rounding.
 */
function hsvLevels({ hue, saturation, value }: Hsva): number[] {
  const sector = (((hue % 1) + 1) % 1) * 6;
  const f = sector - Math.floor(sector);
  const [p, q, t] = [
    1 - saturation,
    1 - saturation * f,
    1 - saturation * (1 - f),
  ];
  const rgb = [
    [1, t, p],
    [q, 1, p],
    [p, 1, t],
    [p, q, 1],
    [t, p, 1],
    [1, p, q],
  ][Math.floor(sector) % 6]!;
  return rgb.map((channel) => channel * value * 255);
}

/** The bytes, hex and alpha percent a picker shows for a colour. */
function shown(color: Hsva) {
  const bytes = hsvLevels(color).map(Math.round);
  const hex = `#${bytes
    .map((level) => level.toString(16).padStart(2, "0"))
    .join("")
    .toUpperCase()}`;
  return {
    hex,
    channels: [...bytes, Math.round(color.alpha * 100)],
  };
}

/** The RGB levels of one canvas pixel of a capture. */
const pixel = (image: CompositeImage, [x, y]: readonly [number, number]) => {
  const offset = (y * image.width + x) * 4;
  return [...image.pixels.subarray(offset, offset + 3)];
};

/** The largest channel difference between a pixel and expected levels. */
const levelError = (actual: readonly number[], expected: readonly number[]) =>
  Math.max(...actual.map((level, index) => Math.abs(level - expected[index]!)));

compositeTests<typeof prepare>("kit-colour", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const em = KIT_COLOUR_FONT;
  /**
   * The control's hue rail and swatch in canvas pixels, by its arrangement:
   * surfaces half an em inside its box and one em apart, rails 1.5 em wide
   * beside the field, the swatch 1.5 em tall beneath them.
   */
  const control = await step("pickerBounds", "kit-pick/control");
  const [cx, cy, cw, ch] = control;
  const tall = ch - em - em - 1.5 * em;
  const fieldWidth = cw - em - 2 * (em + 1.5 * em);
  const hueRail = [cx + em / 2 + fieldWidth + em, cy + em / 2, 1.5 * em, tall];
  const swatch = [
    Math.floor(cx + cw / 2),
    Math.floor(cy + ch - em / 2 - 0.75 * em),
  ] as const;
  /** Click a control of the picker at its middle. */
  const click = async (symbol: string) => {
    const [x, y, width, height] = await step("pickerBounds", symbol);
    await page.mouse.click(...run.pagePoint([x + width / 2, y + height / 2]));
  };
  /** Replace the focused hex field's text and press Enter. */
  const typeHex = async (from: string, text: string) => {
    await click("kit-pick/hex/field");
    await step("expectPickerEdit", from);
    await page.keyboard.press("Control+A");
    await page.keyboard.insertText(text);
    await step("expectPickerEdit", text);
    await page.keyboard.press("Enter");
  };

  // A typed hex is the colour: the control holds it, the channels show its
  // bytes, the swatch paints it and the application hears it once.
  await run.case("kit colour hex entry", async () => {
    await typeHex("#54F4FF", "#FF8000");
    const state = await step("expectPicker", {
      hex: "#FF8000",
      channels: [255, 128, 0, 100],
      error: "",
    });
    const image = await run.capture("kit-colour-hex");
    const swatchError = levelError(pixel(image, swatch), [255, 128, 0]);
    check(
      json(shown(state.color).channels) === json([255, 128, 0, 100]) &&
        json(state.reports.at(-1)) === json(state.color) &&
        swatchError <= 1,
      `Hex entry ${json(state)}, swatch ${swatchError} levels off`,
    );
    return { state, swatchError };
  });

  // A drag on the hue rail moves the colour; the hex and channels follow it.
  await run.case("kit colour hue drag", async () => {
    const before = await step("expectPickerSettled");
    const [x, y, width, height] = hueRail as [number, number, number, number];
    const at = (fraction: number) =>
      run.pagePoint([x + width / 2, y + fraction * height]);
    await page.mouse.move(...at(0.3));
    await page.mouse.down();
    await page.mouse.move(...at(0.6), { steps: 4 });
    await page.mouse.up();
    const state = await step("expectPickerSettled");
    const expected = shown(state.color);
    check(
      Math.abs(state.color.hue - before.color.hue) > 0.1 &&
        state.hex === expected.hex &&
        json(state.channels) === json(expected.channels) &&
        json(state.reports.at(-1)) === json(state.color),
      `Hue drag ${json(state)}, expected ${json(expected)}`,
    );
    return state;
  });

  // A hex that does not parse keeps the colour and shows the error line.
  await run.case("kit colour invalid hex", async () => {
    const before = await step("expectPickerSettled");
    await typeHex(before.hex, "#12345");
    const state = await step("expectPicker", {
      error: "Use #RRGGBB or #RRGGBBAA",
      hex: "#12345",
    });
    check(
      json(state.color) === json(before.color) &&
        state.reports.length === before.reports.length,
      `Invalid hex changed ${json(before)} to ${json(state)}`,
    );
    await run.capture("kit-colour-invalid");
    return state;
  });

  // A preset is an explicit choice: it sets the colour, and the next colour
  // replaces the invalid text and clears the error.
  await run.case("kit colour preset", async () => {
    await click("kit-pick/preset/0");
    const state = await step("expectPicker", {
      hex: "#F4449F",
      channels: [244, 68, 159, 100],
      error: "",
    });
    check(
      json(state.reports.at(-1)) === json(state.color),
      `Preset reports ${json(state.reports.at(-1))}, colour ${json(state.color)}`,
    );
    await run.capture("kit-colour-preset");
    return state;
  });
});
