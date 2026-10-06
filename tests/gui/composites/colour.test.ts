import { check } from "../../harness/page/checks.js";
import {
  compositeTests,
  json,
  type CompositeImage,
} from "./support/composite-tests.js";
import type { prepare } from "./pages/colour.js";

/**
 * The HSV model on sRGB-encoded values from the textbook sector formula, as
 * 0..255 levels before rounding.
 */
function hsvLevels(hue: number, saturation: number, value: number): number[] {
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

/** The sRGB transfer function and its inverse, on 0..255 levels. */
const decode = (level: number) => {
  const encoded = level / 255;
  return encoded <= 0.04045
    ? encoded / 12.92
    : ((encoded + 0.055) / 1.055) ** 2.4;
};
const encode = (linear: number) =>
  255 *
  (linear <= 0.0031308 ? 12.92 * linear : 1.055 * linear ** (1 / 2.4) - 0.055);

/** The RGB levels of one canvas pixel of a capture. */
const pixel = (image: CompositeImage, [x, y]: readonly [number, number]) => {
  const offset = (y * image.width + x) * 4;
  return [...image.pixels.subarray(offset, offset + 3)];
};

/** The largest channel difference between a pixel and expected levels. */
const levelError = (actual: readonly number[], expected: readonly number[]) =>
  Math.max(...actual.map((level, index) => Math.abs(level - expected[index]!)));

/**
 * A colour control's surfaces in canvas pixels, `[x, y, width, height]`, from
 * its box and font size by the control's arrangement, restated: half an em
 * inside the box, one em apart, rails 1.5 em wide beside the field and the
 * swatch 1.5 em tall beneath them.
 */
function colourSurfaces([x, y, width, height]: readonly number[], em: number) {
  const [inset, gap, rail] = [em / 2, em, 1.5 * em];
  const tall = height! - 2 * inset - gap - rail;
  const fieldWidth = width! - 2 * inset - 2 * (gap + rail);
  const left = x! + inset;
  const top = y! + inset;
  return {
    field: [left, top, fieldWidth, tall],
    hue: [left + fieldWidth + gap, top, rail, tall],
    alpha: [left + fieldWidth + 2 * gap + rail, top, rail, tall],
    swatch: [left, top + tall + gap, width! - 2 * inset, rail],
  };
}

/** Check that every named pixel lies within one level of its colour. */
const withinALevel = (pixels: Record<string, number>) => {
  for (const [name, error] of Object.entries(pixels))
    check(
      error <= 1,
      `The colour's ${name} pixel is ${error} levels from the reported colour`,
    );
};

type Hsva = {
  hue: number;
  saturation: number;
  value: number;
  alpha: number;
};

compositeTests<typeof prepare>("colour", { budget: 60 }, async (run) => {
  const { page, step } = run;
  const surfaces = colourSurfaces(await step("colourBounds"), 8);
  const start = await step("colourValue");
  const [fx, fy, fieldWidth, fieldHeight] = surfaces.field as [
    number,
    number,
    number,
    number,
  ];
  const fieldPixel = (dx: number, dy: number) => [fx + dx, fy + dy] as const;
  const centre = ([x, y]: readonly [number, number]) =>
    run.pagePoint([x + 0.5, y + 0.5]);
  // Pointer offsets are pixel centres across and down the field, whose height
  // the rails share; a pointer is placed within a pixel, so channels it sets
  // are compared within one.
  const across = (offset: number) => (offset + 0.5) / fieldWidth;
  const down = (offset: number) => (offset + 0.5) / fieldHeight;
  const pixelWide = 0.75 / fieldHeight;
  const swatchCentre = [
    Math.floor(surfaces.swatch[0]! + surfaces.swatch[2]! / 2),
    Math.floor(surfaces.swatch[1]! + surfaces.swatch[3]! / 2),
  ] as const;
  const hue = start.hue + 0.009;
  let fieldKeys: Hsva = start;
  let picked: Hsva = start;
  let turned: Hsva = start;

  // The colour control holds one HSVA value whose field, hue rail and alpha
  // rail are Tab stops of one control with their own keys. The hue rail's
  // arrows step the hue, finely with Shift; the field's step its saturation
  // and value.
  await run.case("colour parts as Tab stops with their keys", async () => {
    await step("colourAction", { kind: "focus", part: 0 });
    const tabs = [await step("expectColourFocus", 0, true)];
    for (const [key, part] of [
      ["Tab", 1],
      ["Tab", 2],
      ["Shift+Tab", 1],
    ] as const) {
      await page.keyboard.press(key);
      tabs.push(await step("expectColourFocus", part, true));
    }
    await page.keyboard.press("ArrowUp");
    await page.keyboard.press("Shift+ArrowDown");
    const hueKeys = await step("expectColour", { ...start, hue });
    await page.keyboard.press("Shift+Tab");
    await step("expectColourFocus", 0, true);
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowDown");
    fieldKeys = await step("expectColour", {
      ...start,
      hue,
      saturation: start.saturation - 0.01,
      value: 0.99,
    });
    return { tabs, hueKeys, fieldKeys };
  });

  // A drag on the field sets saturation and value at the pointer, pixel
  // centres here, and only them, reporting all four channels once per
  // changed frame; the field under the marker and the swatch show the
  // reported colour.
  await run.case("colour field drag", async () => {
    const records = (await step("colourRecords")).length;
    await page.mouse.move(...centre(fieldPixel(17, 53)));
    await page.mouse.down();
    await page.mouse.move(...centre(fieldPixel(40, 30)));
    await step(
      "expectColour",
      { hue, saturation: across(40), value: 1 - down(30), alpha: 1 },
      pixelWide,
    );
    await page.mouse.move(...centre(fieldPixel(53, 17)));
    picked = await step(
      "expectColour",
      { hue, saturation: across(53), value: 1 - down(17), alpha: 1 },
      pixelWide,
    );
    const image = await run.capture("colour-field-drag");
    await page.mouse.up();
    await step("expectColourRecorded", picked);
    const fieldRecords = await step("colourRecords", records);
    check(
      fieldRecords.length >= 2 &&
        fieldRecords.every(
          ({ value }, index) =>
            value.hue === fieldKeys.hue &&
            value.alpha === 1 &&
            (index === 0 ||
              json(value) !== json(fieldRecords[index - 1]!.value)),
        ) &&
        json(fieldRecords.at(-1)!.value) === json(picked),
      `Field drag records ${json(fieldRecords)}`,
    );
    const pixels = {
      "field marker": levelError(
        pixel(image, fieldPixel(53, 17)),
        hsvLevels(hue, across(53), 1 - down(17)),
      ),
      "field swatch": levelError(
        pixel(image, swatchCentre),
        hsvLevels(picked.hue, picked.saturation, picked.value),
      ),
    };
    withinALevel(pixels);
    return { picked, records: fieldRecords.length, pixels };
  });

  // A drag on the hue rail sets the hue alone, and the frame that shows its
  // thumb there shows the field and swatch in the new hue.
  await run.case("colour hue drag", async () => {
    const [hx, hy, hw] = surfaces.hue as [number, number, number];
    const railPoint = (dy: number) =>
      run.pagePoint([hx + hw / 2, hy + dy + 0.5]);
    const records = (await step("colourRecords")).length;
    await page.mouse.move(...railPoint(40));
    await page.mouse.down();
    await step("expectColour", { ...picked, hue: 1 - down(40) }, pixelWide);
    await page.mouse.move(...railPoint(60));
    turned = await step(
      "expectColour",
      { ...picked, hue: 1 - down(60) },
      pixelWide,
    );
    const image = await run.capture("colour-hue-drag");
    await page.mouse.up();
    await step("expectColourRecorded", turned);
    const hueRecords = await step("colourRecords", records);
    check(
      hueRecords.length >= 2 &&
        hueRecords.every(
          ({ value }) =>
            value.saturation === picked.saturation &&
            value.value === picked.value,
        ),
      `Hue drag records ${json(hueRecords)} after ${json(picked)}`,
    );
    const pixels = {
      "hue corner": levelError(
        pixel(image, fieldPixel(68, 3)),
        hsvLevels(turned.hue, across(68), 1 - down(3)),
      ),
      "hue marker": levelError(
        pixel(image, fieldPixel(53, 17)),
        hsvLevels(turned.hue, across(53), 1 - down(17)),
      ),
      "hue swatch": levelError(
        pixel(image, swatchCentre),
        hsvLevels(turned.hue, turned.saturation, turned.value),
      ),
    };
    withinALevel(pixels);
    return { turned, records: hueRecords.length, pixels };
  });

  // A client sets a translucent colour: the swatch composites it over each
  // checker cell in linear light.
  await run.case("translucent colour over the checker", async () => {
    await step("colourAction", {
      kind: "color",
      value: [turned.hue, turned.saturation, turned.value, 0.5],
    });
    const translucent = await step("expectColour", { ...turned, alpha: 0.5 });
    const image = await run.capture("colour-translucent");
    const [sx, sy] = surfaces.swatch as [number, number];
    const levels = hsvLevels(turned.hue, turned.saturation, turned.value);
    const composited = (checker: number) =>
      levels.map((level) =>
        encode(0.5 * decode(level) + 0.5 * decode(checker)),
      );
    const pixels = {
      "translucent light": levelError(
        pixel(image, [sx + 1, sy + 1]),
        composited(0xcc),
      ),
      "translucent dark": levelError(
        pixel(image, [sx + 5, sy + 1]),
        composited(0x99),
      ),
    };
    withinALevel(pixels);
    return { translucent, pixels };
  });
});
