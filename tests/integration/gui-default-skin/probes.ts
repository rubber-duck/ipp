/**
 * Image-region expectations of the default skin, one probe set per skin-lab
 * specimen. Each probe reads a small logical region of the specimen's
 * composite capture (every state's cell taken from its own capture) and
 * compares it with the design language's colours and geometry rules
 * (tests/skin-lab/README.md), restated here independently of the runtime's
 * look tables: fills, lines, the paired corner
 * cuts, the check mark, the switch block, the slider value and thumb, the
 * dial's rings and pointer, the caret and selection, scroll tracks and
 * thumbs, and the glow that lit states add around a control.
 */
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { CAPTURE_SCALE, type Rect } from "../../skin-lab/specimen.js";
import { BUTTONS } from "../../skin-lab/specimens/a01-button.js";
import { BOXES } from "../../skin-lab/specimens/a02-checkbox.js";
import { RAILS } from "../../skin-lab/specimens/a03-switch.js";
import { SLIDERS, VALUE } from "../../skin-lab/specimens/a04-slider.js";
import { INPUTS } from "../../skin-lab/specimens/a05-text-input.js";
import { FIELDS as STEPPERS } from "../../skin-lab/specimens/d02-numeric-stepper.js";
import { VIEWS } from "../../skin-lab/specimens/a06-scroll-view.js";
import { LISTS } from "../../skin-lab/specimens/a07-virtual-list.js";
import { ITEMS } from "../../skin-lab/specimens/a01-button-items.js";
import {
  SLIDERS as VERTICAL,
  DRAGGED as VERTICAL_DRAGGED,
  VALUE as VERTICAL_VALUE,
  SCALE as E01_SCALE,
} from "../../skin-lab/specimens/e01-vertical-slider.js";
import {
  BIPOLAR,
  RANGE as BIPOLAR_RANGE,
  SCALE as E02_SCALE,
} from "../../skin-lab/specimens/e02-bipolar-slider.js";
import {
  BIPOLAR as BIPOLAR_DIAL,
  DIALS,
  DRAGGED as DIAL_DRAGGED,
  SCALE as D01_SCALE,
  VALUE as DIAL_VALUE,
} from "../../skin-lab/specimens/d01-rotary-knob.js";

type Rgb = readonly [number, number, number];

/** One evaluated expectation, kept for the evidence whether it passed or not. */
export interface ProbeResult {
  readonly name: string;
  readonly passed: boolean;
  readonly detail: string;
}

/** sRGB `#rrggbb`. */
function hex(value: string): Rgb {
  const n = Number.parseInt(value.slice(1), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

const toLinear = (c: number) => {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};

const toSrgb = (v: number) =>
  Math.round(
    255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055),
  );

/** `top` over `bottom` at `alpha`, blended in linear light as the renderer does. */
function over(top: Rgb, bottom: Rgb, alpha: number): Rgb {
  return [0, 1, 2].map((channel) =>
    toSrgb(
      alpha * toLinear(top[channel]!) +
        (1 - alpha) * toLinear(bottom[channel]!),
    ),
  ) as unknown as Rgb;
}

/** The design language's colours (tests/skin-lab/README.md). */
const LANGUAGE = {
  page: hex("#011722"),
  surface: hex("#00131c"),
  accent: hex("#00f4fb"),
  text: hex("#e5f5f7"),
  neutral: hex("#90b0c4"),
  line: hex("#355c70"),
  selection: hex("#0b6fc0"),
  amber: hex("#fbdc6c"),
};

/** Rail interiors: the quiet line at 15% over what lies beneath. */
const railOver = (beneath: Rgb) => over(LANGUAGE.line, beneath, 0.15);

class Probes {
  readonly results: ProbeResult[] = [];

  constructor(private readonly image: RgbaImage) {}

  /** Pixels of a logical rectangle, at least one. */
  private pixels([x, y, width, height]: Rect): Rgb[] {
    const left = Math.round(x * CAPTURE_SCALE);
    const top = Math.round(y * CAPTURE_SCALE);
    const right = Math.max(left + 1, Math.round((x + width) * CAPTURE_SCALE));
    const bottom = Math.max(top + 1, Math.round((y + height) * CAPTURE_SCALE));
    const found: Rgb[] = [];
    for (let row = top; row < bottom; row++)
      for (let column = left; column < right; column++) {
        if (column < 0 || row < 0) continue;
        if (column >= this.image.width || row >= this.image.height) continue;
        const index = (row * this.image.width + column) * 4;
        const { pixels } = this.image;
        found.push([pixels[index]!, pixels[index + 1]!, pixels[index + 2]!]);
      }
    if (!found.length)
      throw new Error(
        `Probe region ${[x, y, width, height]} is outside the capture`,
      );
    return found;
  }

  mean(rect: Rect): Rgb {
    const pixels = this.pixels(rect);
    return [0, 1, 2].map((channel) =>
      Math.round(
        pixels.reduce((sum, pixel) => sum + pixel[channel]!, 0) / pixels.length,
      ),
    ) as unknown as Rgb;
  }

  /** The brightest pixel: where a line or glyph is fully covered. */
  brightest(rect: Rect): Rgb {
    return this.pixels(rect).reduce((best, pixel) =>
      pixel[0] + pixel[1] + pixel[2] > best[0] + best[1] + best[2]
        ? pixel
        : best,
    );
  }

  darkest(rect: Rect): Rgb {
    return this.pixels(rect).reduce((best, pixel) =>
      pixel[0] + pixel[1] + pixel[2] < best[0] + best[1] + best[2]
        ? pixel
        : best,
    );
  }

  /**
   * Logical `[left, top, right, bottom]` of the pixels within `tolerance` of
   * `colour`, such as a label's ink, or none.
   */
  bounds(
    [x, y, width, height]: Rect,
    colour: Rgb,
    tolerance: number,
  ): readonly [number, number, number, number] | undefined {
    const left = Math.round(x * CAPTURE_SCALE);
    const top = Math.round(y * CAPTURE_SCALE);
    let found: [number, number, number, number] | undefined;
    for (let row = top; row < Math.round((y + height) * CAPTURE_SCALE); row++)
      for (
        let column = left;
        column < Math.round((x + width) * CAPTURE_SCALE);
        column++
      ) {
        const index = (row * this.image.width + column) * 4;
        const pixel = this.image.pixels.subarray(index, index + 3);
        if (
          !colour.every(
            (value, channel) => Math.abs(pixel[channel]! - value) <= tolerance,
          )
        )
          continue;
        found = found
          ? [
              Math.min(found[0], column),
              Math.min(found[1], row),
              Math.max(found[2], column + 1),
              Math.max(found[3], row + 1),
            ]
          : [column, row, column + 1, row + 1];
      }
    return found?.map((value) => value / CAPTURE_SCALE) as
      | readonly [number, number, number, number]
      | undefined;
  }

  /** Pixels within `tolerance` of `colour` in every channel. */
  count(rect: Rect, colour: Rgb, tolerance: number): number {
    return this.pixels(rect).filter((pixel) =>
      pixel.every(
        (value, channel) => Math.abs(value - colour[channel]!) <= tolerance,
      ),
    ).length;
  }

  near(name: string, actual: Rgb, expected: Rgb, tolerance: number) {
    const passed = actual.every(
      (value, channel) => Math.abs(value - expected[channel]!) <= tolerance,
    );
    this.results.push({
      name,
      passed,
      detail: `${JSON.stringify(actual)} vs ${JSON.stringify(expected)} ±${tolerance}`,
    });
  }

  /** `actual` is lighter than `than` by at least `margin` in its green and blue channels. */
  lighter(name: string, actual: Rgb, than: Rgb, margin: number) {
    const passed =
      actual[1] - than[1] >= margin && actual[2] - than[2] >= margin;
    this.results.push({
      name,
      passed,
      detail: `${JSON.stringify(actual)} over ${JSON.stringify(than)} by >= ${margin}`,
    });
  }

  within(name: string, actual: number | undefined, low: number, high: number) {
    this.results.push({
      name,
      passed: actual !== undefined && actual >= low && actual <= high,
      detail: `${actual} in ${low}..${high}`,
    });
  }

  at_least(name: string, actual: number, minimum: number) {
    this.results.push({
      name,
      passed: actual >= minimum,
      detail: `${actual} >= ${minimum}`,
    });
  }

  at_most(name: string, actual: number, maximum: number) {
    this.results.push({
      name,
      passed: actual <= maximum,
      detail: `${actual} <= ${maximum}`,
    });
  }
}

/** Line colours: the brightest pixel across a frame's left edge at mid-height. */
const leftEdge = ([x, y, , height]: Rect): Rect => [
  x - 0.5,
  y + height / 2 - 3,
  2.5,
  6,
];

/** The region just outside a frame's left edge, where an edge glow shows. */
const outsideLeft = ([x, y, , height]: Rect): Rect => [
  x - 5,
  y + height / 2 - 3,
  3,
  6,
];

/** The first logical unit of a frame's top-left corner, outside any cut of 3 or more. */
const topLeftCorner = ([x, y]: Rect): Rect => [x, y, 1, 1];

/** The last logical unit of a frame's bottom-right corner. */
const bottomRightCorner = ([x, y, width, height]: Rect): Rect => [
  x + width - 1,
  y + height - 1,
  1,
  1,
];

/** Where a frame's line passes its top-right corner, which no cut removes. */
const topRightLine = ([x, y, width]: Rect): Rect => [
  x + width - 2.5,
  y - 0.5,
  2.5,
  2.5,
];

/**
 * Every cut element cuts its paired top-left and bottom-right corners and
 * keeps the other two.
 */
function pairedCuts(probes: Probes, name: string, frame: Rect, line: Rgb) {
  probes.near(
    `${name} cut top-left`,
    probes.mean(topLeftCorner(frame)),
    LANGUAGE.page,
    6,
  );
  probes.near(
    `${name} cut bottom-right`,
    probes.mean(bottomRightCorner(frame)),
    LANGUAGE.page,
    6,
  );
  probes.near(
    `${name} uncut top-right`,
    probes.brightest(topRightLine(frame)),
    line,
    24,
  );
  probes.near(
    `${name} uncut bottom-left`,
    probes.brightest([frame[0] - 0.5, frame[1] + frame[3] - 2, 2.5, 2.5]),
    line,
    24,
  );
}

function buttons(probes: Probes) {
  const fill = ([x, y]: Rect): Rect => [x + 6, y + 12, 12, 16];
  const label = ([x, y, width]: Rect): Rect => [
    x + width / 2 - 24,
    y + 12,
    48,
    16,
  ];
  const idle = BUTTONS.idle;
  probes.near("idle fill", probes.mean(fill(idle)), LANGUAGE.surface, 4);
  probes.near(
    "idle line",
    probes.brightest(leftEdge(idle)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "idle", idle, LANGUAGE.neutral);
  probes.near("idle label", probes.brightest(label(idle)), LANGUAGE.accent, 30);
  // The label line is centred in the box on both axes: its caps' ink centre
  // lies within a unit of the box centre across and, because the line box
  // includes the descender, about a unit above it.
  const ink = probes.bounds(
    [idle[0] + 2, idle[1] + 2, idle[2] - 4, idle[3] - 4],
    LANGUAGE.accent,
    40,
  );
  probes.within(
    "idle label centred across",
    ink && (ink[0] + ink[2]) / 2 - (idle[0] + idle[2] / 2),
    -1,
    1,
  );
  probes.within(
    "idle label centred down",
    ink && (ink[1] + ink[3]) / 2 - (idle[1] + idle[3] / 2),
    -2,
    0.5,
  );
  const page = probes.mean(outsideLeft(idle));
  probes.near("idle has no glow", page, LANGUAGE.page, 4);

  const hover = BUTTONS.hover;
  probes.near(
    "hover line",
    probes.brightest(leftEdge(hover)),
    LANGUAGE.accent,
    30,
  );
  probes.lighter("hover glow", probes.mean(outsideLeft(hover)), page, 2);

  // Pressed: the accent fill under the hover edge, the label in the surface.
  const pressed = BUTTONS.pressed;
  probes.near("pressed fill", probes.mean(fill(pressed)), LANGUAGE.accent, 8);
  probes.near(
    "pressed label",
    probes.darkest(label(pressed)),
    LANGUAGE.surface,
    30,
  );

  // Focus: the lit line with the full glow, brighter than hover's half.
  const focus = BUTTONS.focus;
  probes.near(
    "focus line",
    probes.brightest(leftEdge(focus)),
    LANGUAGE.accent,
    30,
  );
  probes.lighter(
    "focus glow brighter than hover",
    probes.mean(outsideLeft(focus)),
    probes.mean(outsideLeft(hover)),
    1,
  );
  probes.near("focus fill", probes.mean(fill(focus)), LANGUAGE.surface, 12);

  // Disabled: what would be lit draws in neutral.
  const disabled = BUTTONS.disabled;
  probes.near(
    "disabled line",
    probes.brightest(leftEdge(disabled)),
    LANGUAGE.neutral,
    24,
  );
  probes.near(
    "disabled label",
    probes.brightest(label(disabled)),
    LANGUAGE.neutral,
    30,
  );

  const amber = BUTTONS.amber;
  probes.near(
    "amber line",
    probes.brightest(leftEdge(amber)),
    LANGUAGE.amber,
    30,
  );
  probes.near(
    "amber label",
    probes.brightest(label(amber)),
    LANGUAGE.amber,
    30,
  );
  probes.near("amber fill", probes.mean(fill(amber)), LANGUAGE.surface, 4);
}

/**
 * Buttons as items: selected is the lit fill with the label in the surface
 * colour, kept under hover and focus, on every hierarchy and on a square
 * row; a field keeps focus while a row that does not take focus is pressed;
 * Tab to a row below a scroll view's viewport scrolls it to the bottom.
 */
function items(probes: Probes) {
  /** Inside a box near its leading edge, clear of the centred label. */
  const fill = ([x, y]: Rect): Rect => [x + 6, y + 10, 12, 16];
  const label = ([x, y, width]: Rect): Rect => [
    x + width / 2 - 24,
    y + 12,
    48,
    16,
  ];
  for (const name of [
    "selected",
    "selected-hover",
    "selected-focus",
    "secondary",
  ] as const) {
    probes.near(
      `${name} fill`,
      probes.mean(fill(ITEMS[name])),
      LANGUAGE.accent,
      8,
    );
    probes.near(
      `${name} label`,
      probes.darkest(label(ITEMS[name])),
      LANGUAGE.surface,
      30,
    );
  }
  probes.near(
    "selected line",
    probes.brightest(leftEdge(ITEMS.selected)),
    LANGUAGE.accent,
    30,
  );
  pairedCuts(probes, "selected", ITEMS.selected, LANGUAGE.accent);
  pairedCuts(probes, "secondary", ITEMS.secondary, LANGUAGE.accent);
  const page = probes.mean(outsideLeft(ITEMS.selected));
  probes.near("selected has no glow", page, LANGUAGE.page, 4);
  const hover = probes.mean(outsideLeft(ITEMS["selected-hover"]));
  probes.lighter("selected hover glow", hover, page, 2);
  probes.lighter(
    "selected focus glow brighter than hover",
    probes.mean(outsideLeft(ITEMS["selected-focus"])),
    hover,
    1,
  );
  probes.near(
    "selected disabled fill",
    probes.mean(fill(ITEMS.disabled)),
    LANGUAGE.neutral,
    8,
  );
  probes.near(
    "selected disabled label",
    probes.darkest(label(ITEMS.disabled)),
    LANGUAGE.surface,
    30,
  );

  // Rows: square, the idle surface, the selected fill, and the row that
  // does not take focus looks like any idle row.
  probes.near(
    "idle row fill",
    probes.mean(fill(ITEMS["row-idle"])),
    LANGUAGE.surface,
    4,
  );
  probes.near(
    "selected row fill",
    probes.mean(fill(ITEMS["row-selected"])),
    LANGUAGE.accent,
    8,
  );
  probes.near(
    "selected row square corner",
    probes.mean(topLeftCorner(ITEMS["row-selected"])),
    LANGUAGE.accent,
    12,
  );
  probes.near(
    "no-focus row fill",
    probes.mean(fill(ITEMS["row-option"])),
    LANGUAGE.surface,
    4,
  );

  // The field keeps its lit line and full glow while the row that does not
  // take focus shows its press.
  probes.near(
    "kept focus line",
    probes.brightest(leftEdge(ITEMS.field)),
    LANGUAGE.accent,
    30,
  );
  probes.lighter(
    "kept focus glow",
    probes.mean(outsideLeft(ITEMS.field)),
    page,
    2,
  );
  probes.near(
    "pressed no-focus row fill",
    probes.mean(fill(ITEMS.option)),
    LANGUAGE.accent,
    8,
  );

  // Tab left row 3, selected, at the viewport's bottom, with idle row 2
  // whole above it beyond the reach of its focus glow.
  const [x, y, width] = ITEMS.revealed;
  probes.near(
    "revealed row fill",
    probes.mean(fill(ITEMS.revealed)),
    LANGUAGE.accent,
    8,
  );
  probes.near(
    "row above the revealed row",
    probes.mean([x + 6, y - 34, 12, 8]),
    LANGUAGE.surface,
    4,
  );
  probes.near(
    "revealed row end",
    probes.mean([x + width - 18, y + 10, 12, 16]),
    LANGUAGE.accent,
    8,
  );
}

function checkboxes(probes: Probes) {
  /** Inside the box, clear of its cut and of the centred half-size mark. */
  const corner = ([x, y]: Rect): Rect => [x + 4.5, y + 4.5, 3, 3];
  const mark = ([x, y, width]: Rect): Rect => [
    x + width / 4,
    y + width / 4,
    width / 2,
    width / 2,
  ];
  const unchecked = BOXES.unchecked;
  probes.near(
    "unchecked fill",
    probes.mean([unchecked[0] + 6, unchecked[1] + 6, 20, 20]),
    LANGUAGE.surface,
    4,
  );
  probes.near(
    "unchecked line",
    probes.brightest(leftEdge(unchecked)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "unchecked", unchecked, LANGUAGE.neutral);

  // Checked: the accent fill and line, the mark in the surface colour.
  const checked = BOXES.checked;
  probes.near("checked fill", probes.mean(corner(checked)), LANGUAGE.accent, 8);
  probes.near(
    "checked mark",
    probes.darkest(mark(checked)),
    LANGUAGE.surface,
    24,
  );
  probes.near(
    "checked line",
    probes.brightest(leftEdge(checked)),
    LANGUAGE.accent,
    30,
  );

  // Hover and focus light the edge without changing the fill.
  const hover = BOXES.hover;
  probes.near("hover fill", probes.mean(corner(hover)), LANGUAGE.accent, 8);
  probes.lighter(
    "hover glow",
    probes.mean(outsideLeft(hover)),
    probes.mean(outsideLeft(checked)),
    2,
  );
  const focus = BOXES.focus;
  probes.near("focus fill", probes.mean(corner(focus)), LANGUAGE.accent, 8);
  probes.lighter(
    "focus glow",
    probes.mean(outsideLeft(focus)),
    probes.mean(outsideLeft(checked)),
    2,
  );

  const disabled = BOXES.disabled;
  probes.near(
    "disabled fill",
    probes.mean(corner(disabled)),
    LANGUAGE.neutral,
    8,
  );
}

function switches(probes: Probes) {
  /** Block centres half a rail height in from either end. */
  const block = ([x, y, width, height]: Rect, end: -1 | 1): Rect => [
    (end < 0 ? x + height / 2 : x + width - height / 2) - 5,
    y + height / 2 - 5,
    10,
    10,
  ];
  const off = RAILS.off;
  probes.near("off block", probes.mean(block(off, -1)), LANGUAGE.neutral, 6);
  probes.near("off rail", probes.mean(block(off, 1)), LANGUAGE.surface, 4);
  probes.near(
    "off line",
    probes.brightest(leftEdge(off)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "off", off, LANGUAGE.neutral);

  // On: the accent block on the rail's surface.
  const on = RAILS.on;
  probes.near("on block", probes.mean(block(on, 1)), LANGUAGE.accent, 6);
  probes.near("on rail", probes.mean(block(on, -1)), LANGUAGE.surface, 4);

  const hover = RAILS.hover;
  probes.near(
    "hover line",
    probes.brightest(leftEdge(hover)),
    LANGUAGE.accent,
    30,
  );
  probes.lighter(
    "hover glow",
    probes.mean(outsideLeft(hover)),
    probes.mean(outsideLeft(on)),
    2,
  );

  probes.near(
    "focus line",
    probes.brightest(leftEdge(RAILS.focus)),
    LANGUAGE.accent,
    30,
  );
  probes.near(
    "disabled block",
    probes.mean(block(RAILS.disabled, 1)),
    LANGUAGE.neutral,
    6,
  );
}

function sliders(probes: Probes) {
  /** The thumb, 0.75 of the slider's height, centred on its travel at the value. */
  const thumb = ([x, y, width, height]: Rect): Rect => {
    const edge = 0.75 * height;
    const centre = x + edge / 2 + VALUE * (width - edge);
    return [centre - edge / 2, y + height / 2 - edge / 2, edge, edge];
  };
  const value = ([x, y, , height]: Rect): Rect => [
    x + 10,
    y + height / 2 - 2,
    30,
    4,
  ];
  const rail = ([x, y, width, height]: Rect): Rect => [
    x + width - 22,
    y + height / 2 - 2,
    16,
    4,
  ];
  const above = (rect: Rect): Rect => {
    const [x, y, edge] = thumb(rect);
    return [x + edge / 2 - 4, y - 6, 8, 4];
  };

  const idle = SLIDERS.idle;
  const idleThumb = thumb(idle);
  probes.near("idle value", probes.mean(value(idle)), LANGUAGE.accent, 8);
  probes.near("idle rail", probes.mean(rail(idle)), railOver(LANGUAGE.page), 6);
  probes.near(
    "idle thumb",
    probes.mean([idleThumb[0] + 5, idleThumb[1] + 5, 6, 6]),
    LANGUAGE.surface,
    8,
  );
  probes.near(
    "idle thumb line",
    probes.brightest([idleThumb[0] - 0.5, idleThumb[1] + 6, 2.5, 4]),
    LANGUAGE.accent,
    30,
  );
  // The rail is one bar, 8 units: its fill reaches 2.5 units above its
  // centre, inside its 1.25-unit line, and 5.5 above it is page.
  const railTop = (offset: number): Rect => [
    idle[0] + idle[2] - 20,
    idle[1] + idle[3] / 2 - offset,
    12,
    0.6,
  ];
  probes.near(
    "idle rail thickness",
    probes.mean(railTop(2.4)),
    railOver(LANGUAGE.page),
    6,
  );
  probes.near("idle rail edge", probes.mean(railTop(5.6)), LANGUAGE.page, 6);

  // Dragged: the thumb fills with the accent; the value stays lit.
  const dragging = SLIDERS.dragging;
  const draggedThumb = thumb(dragging);
  probes.near(
    "dragged thumb",
    probes.mean([draggedThumb[0] + 5, draggedThumb[1] + 5, 6, 6]),
    LANGUAGE.accent,
    8,
  );
  probes.near(
    "dragged value",
    probes.mean(value(dragging)),
    LANGUAGE.accent,
    8,
  );
  probes.lighter(
    "focus lights the thumb",
    probes.mean(above(SLIDERS.focus)),
    probes.mean(above(idle)),
    2,
  );
  probes.near(
    "focus leaves the rail ends dark",
    probes.mean([SLIDERS.focus[0] - 5, SLIDERS.focus[1] - 4, 3, 3]),
    probes.mean([idle[0] - 5, idle[1] - 4, 3, 3]),
    2,
  );
  probes.near(
    "disabled value",
    probes.mean(value(SLIDERS.disabled)),
    LANGUAGE.neutral,
    8,
  );
}

/**
 * A vertical slider's parts at `k` times the language's sizes: the square
 * thumb is three quarters of the control's width and travels by its centre
 * from half a thumb above the bottom at the minimum to half a thumb below the
 * top at the maximum; the rail is one bar (8) wide and the fill one bar wide.
 */
function verticalParts([x, y, width, height]: Rect, k: number) {
  const edge = 0.75 * width;
  const centre = x + width / 2;
  const at = (fraction: number) =>
    y + height - edge / 2 - fraction * (height - edge);
  return {
    centre,
    edge,
    at,
    /** A strip down the rail's middle between two heights. */
    rail: (top: number, bottom: number): Rect => [
      centre - 2 * k,
      top,
      4 * k,
      bottom - top,
    ],
    /** The thumb's interior around its centre at a fraction. */
    thumb: (fraction: number): Rect => [
      centre - 3 * k,
      at(fraction) - 3 * k,
      6 * k,
      6 * k,
    ],
    /** Across the thumb's left edge at a fraction. */
    thumbLine: (fraction: number): Rect => [
      centre - edge / 2 - 0.5,
      at(fraction) - 2 * k,
      2.5,
      4 * k,
    ],
    /** Just left of the thumb, where its edge glow shows. */
    beside: (fraction: number): Rect => [
      centre - edge / 2 - 4 * k,
      at(fraction) - 2 * k,
      3 * k,
      4 * k,
    ],
  };
}

function verticalSliders(probes: Probes) {
  const k = E01_SCALE;
  const idle = verticalParts(VERTICAL.idle, k);
  const thumbTop = idle.at(VERTICAL_VALUE) - idle.edge / 2;
  const thumbBottom = idle.at(VERTICAL_VALUE) + idle.edge / 2;
  const [, top, , height] = VERTICAL.idle;
  const bottom = top + height;

  // Idle: the fill rises from the bottom to the thumb, the rail above it.
  probes.near(
    "idle value below the thumb",
    probes.mean(idle.rail(thumbBottom + 4 * k, bottom - 4 * k)),
    LANGUAGE.accent,
    8,
  );
  probes.near(
    "idle rail above the thumb",
    probes.mean(idle.rail(top + 4 * k, thumbTop - 4 * k)),
    railOver(LANGUAGE.page),
    6,
  );
  // The rail is one bar wide: inside its line it is rail 1.8 units right of
  // its centre, and 5.4 to the right it is page.
  const across = (offset: number): Rect => [
    idle.centre + offset * k,
    top + 6 * k,
    0.6 * k,
    thumbTop - top - 12 * k,
  ];
  probes.near(
    "idle rail thickness",
    probes.mean(across(1.8)),
    railOver(LANGUAGE.page),
    6,
  );
  probes.near("idle rail edge", probes.mean(across(5.4)), LANGUAGE.page, 6);
  probes.near(
    "idle thumb",
    probes.mean(idle.thumb(VERTICAL_VALUE)),
    LANGUAGE.surface,
    8,
  );
  probes.near(
    "idle thumb line",
    probes.brightest(idle.thumbLine(VERTICAL_VALUE)),
    LANGUAGE.accent,
    30,
  );

  // Dragged up from 65% to 80% by real input: the solid thumb at 80% and
  // the fill reaching past where it started.
  const dragging = verticalParts(VERTICAL.dragging, k);
  probes.near(
    "dragged thumb at 80%",
    probes.mean(dragging.thumb(VERTICAL_DRAGGED)),
    LANGUAGE.accent,
    8,
  );
  probes.near(
    "dragged value above 65%",
    probes.mean(
      dragging.rail(
        dragging.at(VERTICAL_VALUE + 0.06) - k,
        dragging.at(VERTICAL_VALUE + 0.06) + k,
      ),
    ),
    LANGUAGE.accent,
    8,
  );

  const focus = verticalParts(VERTICAL.focus, k);
  probes.lighter(
    "focus lights the thumb",
    probes.mean(focus.beside(VERTICAL_VALUE)),
    probes.mean(idle.beside(VERTICAL_VALUE)),
    2,
  );
  const hover = verticalParts(VERTICAL.hover, k);
  probes.lighter(
    "hover lights the thumb",
    probes.mean(hover.beside(VERTICAL_VALUE)),
    probes.mean(idle.beside(VERTICAL_VALUE)),
    2,
  );

  const disabled = verticalParts(VERTICAL.disabled, k);
  probes.near(
    "disabled value",
    probes.mean(
      disabled.rail(
        disabled.at(VERTICAL_VALUE) + disabled.edge / 2 + 4 * k,
        bottom - 4 * k,
      ),
    ),
    LANGUAGE.neutral,
    8,
  );
  probes.near(
    "disabled thumb line",
    probes.brightest(disabled.thumbLine(VERTICAL_VALUE)),
    LANGUAGE.neutral,
    30,
  );
}

function bipolarSliders(probes: Probes) {
  const k = E02_SCALE;
  const fraction = (value: number) =>
    (value - BIPOLAR_RANGE.min) / (BIPOLAR_RANGE.max - BIPOLAR_RANGE.min);
  const zeroAt = fraction(0);
  for (const [name, { rect, value }] of Object.entries(BIPOLAR)) {
    const parts = verticalParts(rect, k);
    const zero = parts.at(zeroAt);
    const thumb = parts.at(fraction(value));
    const [, top, , height] = rect;
    const bottom = top + height;
    // The fill lies only between zero and the thumb, on the value's side.
    const [near, far] =
      value < 0
        ? [zero, thumb - parts.edge / 2]
        : [thumb + parts.edge / 2, zero];
    probes.near(
      `${name} fill between zero and the thumb`,
      probes.mean(parts.rail(near + 3 * k, far - 3 * k)),
      LANGUAGE.accent,
      8,
    );
    const [otherNear, otherFar] =
      value < 0 ? [top + 4 * k, zero - 3 * k] : [zero + 3 * k, bottom - 4 * k];
    probes.near(
      `${name} rail beyond zero`,
      probes.mean(parts.rail(otherNear, otherFar)),
      railOver(LANGUAGE.page),
      6,
    );
    const [beyondNear, beyondFar] =
      value < 0
        ? [thumb + parts.edge / 2 + 4 * k, bottom - 4 * k]
        : [top + 4 * k, thumb - parts.edge / 2 - 4 * k];
    probes.near(
      `${name} rail beyond the thumb`,
      probes.mean(parts.rail(beyondNear, beyondFar)),
      railOver(LANGUAGE.page),
      6,
    );
    probes.near(
      `${name} thumb`,
      probes.mean(parts.thumb(fraction(value))),
      LANGUAGE.surface,
      8,
    );
    // The zero mark, client composition, reaches the rail; other marks
    // start past the thumb.
    const gap = (at: number): Rect => [
      parts.centre + 6 * k,
      at - k,
      3 * k,
      2 * k,
    ];
    probes.near(
      `${name} zero mark`,
      probes.brightest(gap(zero)),
      LANGUAGE.neutral,
      30,
    );
    probes.near(
      `${name} -50 mark gap`,
      probes.mean(gap(parts.at(fraction(-50)))),
      LANGUAGE.page,
      6,
    );
  }
}

/**
 * A dial's geometry at `k` times the language's sizes, in the square at the
 * top of its housing: a tick ring half an em inside it, ticks 4 long, a
 * quarter-em gap, then a value arc 4 thick whose centre line the track and
 * the pointer's outer end share. Values run clockwise over 270 degrees from
 * half past seven.
 */
function dialParts(rect: Rect, k: number) {
  const em = 16 * k;
  const centre = [rect[0] + rect[2] / 2, rect[1] + rect[2] / 2] as const;
  const ticks = rect[2] / 2 - em / 2;
  const ring = ticks - 4 * k - em / 4 - 2 * k;
  /** The point at a value fraction and a radius. */
  const at = (fraction: number, radius: number) => {
    const turn = 2 * Math.PI * (0.625 + 0.75 * fraction);
    return [
      centre[0] + radius * Math.sin(turn),
      centre[1] - radius * Math.cos(turn),
    ] as const;
  };
  /** A small square around a point. */
  const spot = ([x, y]: readonly [number, number], half = 1): Rect => [
    x - half,
    y - half,
    2 * half,
    2 * half,
  ];
  return {
    /** On the value ring's centre line, where the track and value arc lie. */
    ring: (fraction: number) => spot(at(fraction, ring)),
    /** In the middle of the tick ring. */
    tick: (fraction: number) => spot(at(fraction, ticks - 2 * k)),
    /** On the pointer, at 0.7 of the ring's radius. */
    pointer: (fraction: number) => spot(at(fraction, 0.7 * ring), 0.75),
    /** The housing's interior around the dial's centre. */
    middle: spot(centre, 2),
  };
}

function dials(probes: Probes) {
  const k = D01_SCALE;
  const idle = dialParts(DIALS.idle, k);

  // Idle: a frame-cut housing in the idle line round the surface, the lit
  // value arc from the minimum to 65% over the track in the quiet line, a
  // quiet tick on each tenth and the lit pointer at the value.
  probes.near("idle housing", probes.mean(idle.middle), LANGUAGE.surface, 4);
  probes.near(
    "idle housing line",
    probes.brightest(leftEdge(DIALS.idle)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "idle housing", DIALS.idle, LANGUAGE.neutral);
  probes.near(
    "idle value arc",
    probes.mean(idle.ring(0.3)),
    LANGUAGE.accent,
    10,
  );
  probes.near(
    "idle track beyond the value",
    probes.brightest(idle.ring(0.85)),
    LANGUAGE.line,
    24,
  );
  probes.near(
    "idle tick at nine tenths",
    probes.brightest(idle.tick(0.9)),
    LANGUAGE.line,
    30,
  );
  probes.near(
    "idle gap between ticks",
    probes.mean(idle.tick(0.95)),
    LANGUAGE.surface,
    6,
  );
  probes.near(
    "idle pointer at the value",
    probes.brightest(idle.pointer(DIAL_VALUE)),
    LANGUAGE.accent,
    30,
  );
  probes.near(
    "idle no pointer elsewhere",
    probes.mean(idle.pointer(0.2)),
    LANGUAGE.surface,
    6,
  );

  // Focus and hover light the housing's own line and glow around it;
  // focus more.
  for (const name of ["focus", "hover"] as const) {
    probes.near(
      `${name} housing line`,
      probes.brightest(leftEdge(DIALS[name])),
      LANGUAGE.accent,
      24,
    );
    probes.lighter(
      `${name} glows round the housing`,
      probes.mean(outsideLeft(DIALS[name])),
      probes.mean(outsideLeft(DIALS.idle)),
      2,
    );
  }

  // Dragged up from 65% to 80% by real input: the housing lit but not
  // filled, the arc reaching past 65% and the pointer at 80%.
  const dragging = dialParts(DIALS.dragging, k);
  probes.near(
    "dragging housing line",
    probes.brightest(leftEdge(DIALS.dragging)),
    LANGUAGE.accent,
    24,
  );
  probes.near(
    "dragging housing stays unfilled",
    probes.mean(dragging.middle),
    LANGUAGE.surface,
    8,
  );
  probes.near(
    "dragging value arc past 65%",
    probes.mean(dragging.ring(0.75)),
    LANGUAGE.accent,
    10,
  );
  probes.near(
    "dragging pointer at 80%",
    probes.brightest(dragging.pointer(DIAL_DRAGGED)),
    LANGUAGE.accent,
    30,
  );

  // Disabled keeps the value and arc, in `neutral`.
  const disabled = dialParts(DIALS.disabled, k);
  probes.near(
    "disabled value arc",
    probes.mean(disabled.ring(0.3)),
    LANGUAGE.neutral,
    10,
  );
  probes.near(
    "disabled pointer",
    probes.brightest(disabled.pointer(DIAL_VALUE)),
    LANGUAGE.neutral,
    30,
  );

  // A bipolar dial fills only between zero, at twelve, and the value.
  const bipolar = dialParts(DIALS.bipolar, k);
  const fraction = (value: number) =>
    (value - BIPOLAR_DIAL.min) / (BIPOLAR_DIAL.max - BIPOLAR_DIAL.min);
  const [value, zero] = [fraction(BIPOLAR_DIAL.value), fraction(0)];
  probes.near(
    "bipolar arc between the value and zero",
    probes.mean(bipolar.ring((value + zero) / 2)),
    LANGUAGE.accent,
    10,
  );
  for (const [where, at] of [
    ["beyond the value", value - 0.15],
    ["beyond zero", zero + 0.15],
  ] as const)
    probes.near(
      `bipolar track ${where}`,
      probes.brightest(bipolar.ring(at)),
      LANGUAGE.line,
      24,
    );
  probes.near(
    "bipolar pointer at the value",
    probes.brightest(bipolar.pointer(value)),
    LANGUAGE.accent,
    30,
  );
}

function textInputs(probes: Probes) {
  /** The line: one em (16) in from the left, centred vertically. */
  const line = ([x, y, , height]: Rect): Rect => [
    x + 16,
    y + height / 2 - 8,
    120,
    16,
  ];
  const idle = INPUTS.idle;
  probes.near(
    "idle fill",
    probes.mean([idle[0] + 130, idle[1] + 10, 24, 18]),
    LANGUAGE.surface,
    4,
  );
  probes.near("idle text", probes.brightest(line(idle)), LANGUAGE.text, 30);
  probes.near(
    "idle line",
    probes.brightest(leftEdge(idle)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "idle", idle, LANGUAGE.neutral);
  const text = probes.bounds(
    [idle[0] + 2, idle[1] + 2, 160, idle[3] - 4],
    LANGUAGE.text,
    40,
  );
  probes.within(
    "idle text starts one em in",
    text && text[0] - idle[0],
    16,
    19,
  );
  probes.within(
    "idle text centred down",
    text && (text[1] + text[3]) / 2 - (idle[1] + idle[3] / 2),
    -2,
    0.5,
  );

  // Focus lights the field and shows the accent caret after the text.
  const focused = INPUTS.focused;
  probes.near(
    "focused line",
    probes.brightest(leftEdge(focused)),
    LANGUAGE.accent,
    30,
  );
  probes.at_least(
    "focused caret",
    probes.count(line(focused), LANGUAGE.accent, 40),
    4,
  );
  probes.at_most(
    "idle has no caret",
    probes.count(line(idle), LANGUAGE.accent, 40),
    0,
  );

  const selection = INPUTS.selection;
  probes.at_least(
    "selection highlight",
    probes.count(line(selection), LANGUAGE.selection, 12),
    40,
  );
  probes.at_most(
    "idle has no highlight",
    probes.count(line(idle), LANGUAGE.selection, 12),
    0,
  );

  probes.near(
    "disabled text",
    probes.brightest(line(INPUTS.disabled)),
    LANGUAGE.neutral,
    30,
  );
  probes.near(
    "hover line",
    probes.brightest(leftEdge(INPUTS.hover)),
    LANGUAGE.accent,
    30,
  );
}

/**
 * A numeric field with step parts: its decrement and increment parts are the
 * squares of its height at its ends, divided from the number by the idle
 * line, with a minus and a plus in content text centred in them.
 */
function steppers(probes: Probes) {
  /** A step part's centre region, where its mark's strokes cross. */
  const mark = ([x, y, width, height]: Rect, index: 0 | 1): Rect => [
    x + (index ? width - height : 0) + height / 2 - 2,
    y + height / 2 - 2,
    4,
    4,
  ];
  /** The separator on a step part's inner side, at mid-height. */
  const separator = ([x, y, width, height]: Rect, index: 0 | 1): Rect => [
    x + (index ? width - height : height) - 1.5,
    y + height / 2 - 3,
    3,
    6,
  ];
  /** The number's area between the parts. */
  const number = ([x, y, width, height]: Rect): Rect => [
    x + height + 2,
    y + 2,
    width - 2 * height - 4,
    height - 4,
  ];
  const idle = STEPPERS.idle;
  probes.near(
    "idle line",
    probes.brightest(leftEdge(idle)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, "idle", idle, LANGUAGE.neutral);
  for (const index of [0, 1] as const) {
    probes.near(
      `idle separator ${index}`,
      probes.brightest(separator(idle, index)),
      LANGUAGE.neutral,
      24,
    );
    probes.near(
      `idle mark ${index}`,
      probes.brightest(mark(idle, index)),
      LANGUAGE.text,
      24,
    );
  }
  const text = probes.bounds(number(idle), LANGUAGE.text, 40);
  probes.within(
    "idle number centred between the parts",
    text && (text[0] + text[2]) / 2 - (idle[0] + idle[2] / 2),
    -2,
    2,
  );

  // Focus lights the one outer border and shows the caret, and leaves the
  // separators unlit.
  const focus = STEPPERS.focus;
  probes.near(
    "focus line",
    probes.brightest(leftEdge(focus)),
    LANGUAGE.accent,
    30,
  );
  probes.at_least(
    "focus caret",
    probes.count(number(focus), LANGUAGE.accent, 40),
    4,
  );
  probes.near(
    "focus separator",
    probes.brightest(separator(focus, 1)),
    LANGUAGE.neutral,
    24,
  );

  // At the upper bound only the increment's mark is muted.
  const limit = STEPPERS.limit;
  probes.near(
    "limit decrement mark",
    probes.brightest(mark(limit, 0)),
    LANGUAGE.text,
    24,
  );
  probes.near(
    "limit increment mark muted",
    probes.brightest(mark(limit, 1)),
    LANGUAGE.neutral,
    24,
  );

  const disabled = STEPPERS.disabled;
  for (const index of [0, 1] as const)
    probes.near(
      `disabled mark ${index}`,
      probes.brightest(mark(disabled, index)),
      LANGUAGE.neutral,
      24,
    );
  probes.near(
    "disabled number",
    probes.brightest(number(disabled)),
    LANGUAGE.neutral,
    30,
  );

  // Hover over the increment part lights that part alone.
  const hover = STEPPERS.hover;
  probes.near(
    "hovered part's separator",
    probes.brightest(separator(hover, 1)),
    LANGUAGE.accent,
    30,
  );
  probes.near(
    "hover leaves the other part",
    probes.brightest(separator(hover, 0)),
    LANGUAGE.neutral,
    24,
  );
  probes.near(
    "hover leaves the field",
    probes.brightest(leftEdge(hover)),
    LANGUAGE.neutral,
    24,
  );
}

/**
 * Default bars of a 228 x 80 frame at the 16-unit body type: half an em (8)
 * thick, one thickness in from the right side and half of one from the ends;
 * the thumb travels the track without its pointed ends and is at least two
 * thicknesses long.
 */
const BAR = { thickness: 8, inset: 8, end: 4 } as const;
const FRAME_SIZE = [228, 80] as const;
const TRACK_LEFT = FRAME_SIZE[0] - BAR.inset - BAR.thickness;
const TRAVEL = FRAME_SIZE[1] - 2 * BAR.end - BAR.thickness;
const VIEWPORT = 70;
const ROW_HEIGHT = 24;

/** A strip down the middle of the bar between `from` and `to`. */
function bar([x, y]: Rect, from: number, to: number): Rect {
  return [x + TRACK_LEFT + BAR.thickness / 2 - 2, y + from, 4, to - from];
}

function thumbSpan(content: number, offset: number): [number, number] {
  const length = Math.max((TRAVEL * VIEWPORT) / content, 2 * BAR.thickness);
  const start =
    BAR.end +
    BAR.thickness / 2 +
    ((TRAVEL - length) * offset) / (content - VIEWPORT);
  return [start, start + length];
}

/** The track interior: the rail fill over the frame's surface. */
const TRACK = railOver(LANGUAGE.surface);

function scrollFrame(probes: Probes, name: string, frame: Rect) {
  const [x, y] = frame;
  probes.near(
    `${name} frame fill`,
    probes.mean([x + TRACK_LEFT - 8, y + 34, 6, 12]),
    LANGUAGE.surface,
    4,
  );
  probes.near(
    `${name} frame line`,
    probes.brightest(leftEdge(frame)),
    LANGUAGE.neutral,
    24,
  );
  pairedCuts(probes, `${name} frame`, frame, LANGUAGE.neutral);
  // The track's top tip: its centre line is lit, its corner is cut away.
  probes.near(
    `${name} track tip cut`,
    probes.mean([x + TRACK_LEFT + 0.5, y + BAR.end + 0.75, 1, 1]),
    LANGUAGE.surface,
    6,
  );
}

function scrollViews(probes: Probes) {
  const content = 8 * ROW_HEIGHT;
  for (const [name, offset] of [
    ["idle", 0],
    ["at-end", content - VIEWPORT],
  ] as const) {
    const frame = VIEWS[name];
    scrollFrame(probes, name, frame);
    const [start, end] = thumbSpan(content, offset);
    probes.near(
      `${name} thumb`,
      probes.mean(bar(frame, start + 2, end - 2)),
      LANGUAGE.accent,
      10,
    );
    const free =
      offset === 0
        ? bar(frame, end + 4, FRAME_SIZE[1] - 10)
        : bar(frame, 10, start - 4);
    probes.near(`${name} track`, probes.mean(free), TRACK, 8);
  }
  const [start, end] = thumbSpan(content, 2 * ROW_HEIGHT);
  const dragged = VIEWS["thumb-drag"];
  probes.near(
    "dragged thumb",
    probes.mean(bar(dragged, start + 2, end - 2)),
    LANGUAGE.accent,
    12,
  );
  probes.near(
    "dragged track",
    probes.mean(bar(dragged, 10, start - 6)),
    TRACK,
    12,
  );
}

function virtualLists(probes: Probes) {
  const content = 12 * ROW_HEIGHT;
  const populated = LISTS.populated;
  scrollFrame(probes, "populated", populated);
  const [start, end] = thumbSpan(content, 0);
  probes.near(
    "populated thumb",
    probes.mean(bar(populated, start + 2, end - 2)),
    LANGUAGE.accent,
    10,
  );
  const [scrolledStart, scrolledEnd] = thumbSpan(content, 4 * ROW_HEIGHT);
  probes.near(
    "scrolled thumb",
    probes.mean(bar(LISTS.scrolled, scrolledStart + 2, scrolledEnd - 2)),
    LANGUAGE.accent,
    10,
  );
  // An empty list keeps its track and shows no thumb.
  const empty = LISTS.empty;
  scrollFrame(probes, "empty", empty);
  probes.near(
    "empty track",
    probes.mean(bar(empty, 10, FRAME_SIZE[1] - 10)),
    TRACK,
    8,
  );
}

/** Probe sets by specimen name. */
export const PROBES: Readonly<Record<string, (probes: Probes) => void>> = {
  "a01-button": buttons,
  "a01-button-items": items,
  "a02-checkbox": checkboxes,
  "a03-switch": switches,
  "a04-slider": sliders,
  "a05-text-input": textInputs,
  "a06-scroll-view": scrollViews,
  "a07-virtual-list": virtualLists,
  "d01-rotary-knob": dials,
  "d02-numeric-stepper": steppers,
  "e01-vertical-slider": verticalSliders,
  "e02-bipolar-slider": bipolarSliders,
};

/** Evaluate one specimen's probes over its composite capture. */
export function probe(specimen: string, image: RgbaImage): ProbeResult[] {
  const set = PROBES[specimen];
  if (!set) throw new Error(`No probes for ${specimen}`);
  const probes = new Probes(image);
  set(probes);
  return probes.results;
}
