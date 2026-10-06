/**
 * Custom paints in the default skin through a real Host: the panel-paints
 * specimen with its band held still, captured, then captured again after a
 * property write moves the band. Region probes, restated independently of the
 * renderer, check that each pattern is present inside its panel and absent
 * on the page around it, that frames, corner accents and glow are the plain
 * panel's, that a paint which does not compile alone fails its asset and
 * leaves its panel the plain panel's colour, that the property write changes
 * the band and nothing else, and that an ordinary clip on the property sweeps
 * the band from where the write left it.
 */
import type { Client, HostClientBase } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { createElement } from "react";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import {
  CAPTURE_SCALE,
  defineSpecimen,
  type Rect,
} from "../../skin-lab/specimen.js";
import specimen, {
  EXTENT,
  PANELS,
  PanelPaints,
  SWEEP_REST,
  SWEEP_CHANGE,
  panelRect,
  type PanelName,
} from "../../skin-lab/specimens/p01-panel-paints.js";
import { LINE } from "../../skin-lab/themes/geometry.js";
import { themes } from "../../skin-lab/themes/paint.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import type { ProbeResult } from "../default-skin-oracle.js";

/** Where the property write moves the band. */
const SWEEP_MOVED = 0.75;

export interface PanelPaintCapture {
  readonly name: string;
  readonly image: RgbaImage;
}

/** The specimen with its band resting at `sweep`, so its paint settles. */
function still(sweep: number, animated = false) {
  return defineSpecimen({
    ...specimen,
    render: (lab) => createElement(PanelPaints, { lab, sweep, animated }),
  });
}

type Rgb = readonly [number, number, number];

function pixel(image: RgbaImage, x: number, y: number): Rgb {
  const column = Math.min(Math.floor(x * CAPTURE_SCALE), image.width - 1);
  const row = Math.min(Math.floor(y * CAPTURE_SCALE), image.height - 1);
  const index = (row * image.width + column) * 4;
  return [
    image.pixels[index]!,
    image.pixels[index + 1]!,
    image.pixels[index + 2]!,
  ];
}

/** Mean of the three sRGB channels of every capture row inside `rect`. */
function rows(image: RgbaImage, rect: Rect): number[] {
  const [x, y, width, height] = rect.map((value) =>
    Math.round(value * CAPTURE_SCALE),
  ) as [number, number, number, number];
  const means: number[] = [];
  for (let row = y; row < y + height; row++) {
    let sum = 0;
    for (let column = x; column < x + width; column++) {
      const index = (row * image.width + column) * 4;
      sum +=
        image.pixels[index]! +
        image.pixels[index + 1]! +
        image.pixels[index + 2]!;
    }
    means.push(sum / (3 * width));
  }
  return means;
}

/** Mean of the three sRGB channels of every capture column inside `rect`. */
function columns(image: RgbaImage, rect: Rect): number[] {
  const [x, y, width, height] = rect;
  return rows(transpose(image), [y, x, height, width]);
}

function transpose(image: RgbaImage): RgbaImage {
  const pixels = new Uint8Array(image.pixels.length);
  for (let row = 0; row < image.height; row++)
    for (let column = 0; column < image.width; column++)
      pixels.set(
        image.pixels.subarray(
          (row * image.width + column) * 4,
          (row * image.width + column) * 4 + 4,
        ),
        (column * image.height + row) * 4,
      );
  return { width: image.height, height: image.width, pixels } as RgbaImage;
}

const spread = (values: readonly number[]) =>
  Math.max(...values) - Math.min(...values);

const mean = (values: readonly number[]) =>
  values.reduce((sum, value) => sum + value, 0) / values.length;

/** A panel's interior clear of its frame and corner accents. */
function interior(name: PanelName): Rect {
  const [x, y, width, height] = panelRect(name);
  return [x + 12, y + 12, width - 24, height - 24];
}

/** Page strips beside a panel, clear of its frame and glow. */
function besides(name: PanelName): Rect[] {
  const [x, y, width, height] = panelRect(name);
  return [
    [x - 7, y + 12, 3, height - 24],
    [x + width + 4, y + 12, 3, height - 24],
  ];
}

/** Rising crossings of the midpoint between a profile's extremes. */
function periods(profile: readonly number[]): number {
  const middle = (Math.max(...profile) + Math.min(...profile)) / 2;
  let crossings = 0;
  for (let index = 1; index < profile.length; index++)
    if (profile[index - 1]! < middle && profile[index]! >= middle) crossings++;
  return crossings;
}

/** Fraction down the panel of its interior's brightest row. */
function band(image: RgbaImage, name: PanelName): number {
  const profile = rows(image, interior(name));
  const brightest = profile.indexOf(Math.max(...profile));
  const [, y, , height] = panelRect(name);
  const [, top] = interior(name);
  return (top - y + (brightest + 0.5) / CAPTURE_SCALE) / height;
}

/** The colour on a panel's frame: mid-edge and in a corner accent. */
function frame(image: RgbaImage, name: PanelName): Rgb[] {
  const [x, y, width, height] = panelRect(name);
  const inside = LINE / 2;
  return [
    pixel(image, x + width / 2, y + inside),
    pixel(image, x + inside, y + height / 2),
    pixel(image, x + width - inside, y + height - 4),
  ];
}

function near(a: Rgb, b: Rgb, tolerance: number): boolean {
  return a.every(
    (value, channel) => Math.abs(value - b[channel]!) <= tolerance,
  );
}

function probe(
  results: ProbeResult[],
  name: string,
  passed: boolean,
  detail: string,
) {
  results.push({ name, passed, detail });
}

function firstCapture(image: RgbaImage, results: ProbeResult[]) {
  const plain = rows(image, interior("plain"));
  probe(
    results,
    "plain panel is flat",
    spread(plain) <= 2,
    `spread ${spread(plain).toFixed(1)}`,
  );

  const scanlines = rows(image, interior("scanlines"));
  // Four-unit scanlines over the 72-unit interior.
  const lines = periods(scanlines);
  probe(
    results,
    "scanlines cross the panel",
    spread(scanlines) >= 20 && lines >= 15 && lines <= 21,
    `spread ${spread(scanlines).toFixed(1)}, ${lines} periods`,
  );
  probe(
    results,
    "scanlines run along rows",
    spread(columns(image, interior("scanlines"))) <= 3,
    `column spread ${spread(columns(image, interior("scanlines"))).toFixed(1)}`,
  );

  const gridRows = rows(image, interior("grid"));
  const gridColumns = columns(image, interior("grid"));
  probe(
    results,
    "grid lines cross both axes",
    spread(gridRows) >= 10 &&
      spread(gridColumns) >= 10 &&
      periods(gridRows) >= 4 &&
      periods(gridColumns) >= 4,
    `rows ${spread(gridRows).toFixed(1)}/${periods(gridRows)}, columns ${spread(gridColumns).toFixed(1)}/${periods(gridColumns)}`,
  );

  // The page beside each panel is flat; away from the lit grid panel's glow
  // it is the page itself.
  const page = mean(rows(image, besides("plain")[0]!));
  for (const name of PANELS)
    for (const [index, strip] of besides(name).entries()) {
      const values = rows(image, strip);
      const lit =
        name === "grid" ||
        (name === "scanlines" && index === 1) ||
        (name === "sweep" && index === 0);
      probe(
        results,
        `no paint beside ${name} ${index}`,
        spread(values) <= 2 && (lit || Math.abs(mean(values) - page) <= 2),
        `spread ${spread(values).toFixed(1)}, mean ${mean(values).toFixed(1)}`,
      );
    }

  const reference = frame(image, "plain");
  for (const name of ["scanlines", "grid", "sweep", "fallback"] as const) {
    const lines = frame(image, name);
    probe(
      results,
      `${name} keeps the plain frame`,
      lines.every((colour, index) => near(colour, reference[index]!, 3)),
      `${JSON.stringify(lines)} against ${JSON.stringify(reference)}`,
    );
  }

  // The grid panel's own row lights its frame: glow above it, none above the
  // plain panel.
  const glowAt = (name: PanelName) => {
    const [x, y, width] = panelRect(name);
    return pixel(image, x + width / 2, y - 2);
  };
  const glow = mean([...glowAt("grid")]);
  const unlit = mean([...glowAt("plain")]);
  probe(
    results,
    "glow surrounds the painted grid panel",
    glow >= unlit + 10,
    `grid ${glow.toFixed(1)}, plain ${unlit.toFixed(1)}`,
  );

  const fallback = rows(image, interior("fallback"));
  probe(
    results,
    "a broken paint draws the panel colour",
    spread(fallback) <= 2 && Math.abs(mean(fallback) - mean(plain)) <= 2,
    `spread ${spread(fallback).toFixed(1)}, mean ${mean(fallback).toFixed(1)} against ${mean(plain).toFixed(1)}`,
  );

  const rest = band(image, "sweep");
  probe(
    results,
    "the band rests at its property",
    Math.abs(rest - SWEEP_REST) <= 0.04,
    `band at ${rest.toFixed(3)}`,
  );
}

function thirdCapture(
  before: RgbaImage,
  after: RgbaImage,
  results: ProbeResult[],
) {
  const swept = band(after, "sweep");
  probe(
    results,
    "a property clip sweeps the band from its rest",
    Math.abs(swept - (SWEEP_MOVED + SWEEP_CHANGE)) <= 0.04,
    `band at ${swept.toFixed(3)}`,
  );
  for (const name of ["plain", "scanlines", "grid", "fallback"] as const) {
    const unchanged = rows(before, interior(name)).every(
      (value, index) =>
        Math.abs(value - rows(after, interior(name))[index]!) <= 0.5,
    );
    probe(results, `${name} is unchanged by the clip`, unchanged, "");
  }
}

function secondCapture(
  before: RgbaImage,
  after: RgbaImage,
  commands: Readonly<Record<string, number>>,
  results: ProbeResult[],
) {
  const moved = band(after, "sweep");
  probe(
    results,
    "a property write moves the band",
    Math.abs(moved - SWEEP_MOVED) <= 0.04,
    `band at ${moved.toFixed(3)}`,
  );
  const writes = Object.entries(commands)
    .filter(([, count]) => count > 0)
    .map(([kind]) => kind);
  probe(
    results,
    "the write is a property write alone",
    writes.length === 1 && writes[0] === "setDynamicProperty",
    JSON.stringify(commands),
  );
  for (const name of ["plain", "scanlines", "grid", "fallback"] as const) {
    const unchanged = rows(before, interior(name)).every(
      (value, index) =>
        Math.abs(value - rows(after, interior(name))[index]!) <= 0.5,
    );
    probe(results, `${name} is unchanged by the write`, unchanged, "");
  }
}

/**
 * Declare the panel-paints specimen, probe its paints, move the band through
 * a property write and probe again. Returns every result; the caller fails
 * the scenario on any that did not pass.
 */
export async function panelPaints(
  host: HostClientBase<Client>,
  contract: ThemeContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  capture: (image: PanelPaintCapture) => Promise<void>,
): Promise<{ results: ProbeResult[]; assets: Record<string, unknown> }> {
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    specimen: still(SWEEP_REST),
    themes,
    world: "gui-default-skin/p01-panel-paints",
  });
  const results: ProbeResult[] = [];
  try {
    const assets: Record<string, unknown> = {};
    for (const id of ["scanlines", "grid", "sweep", "broken"])
      assets[id] = await session.assetSettled(`paint/${id}`);
    probe(
      results,
      "working paints load and the broken one fails to compile",
      ["scanlines", "grid", "sweep"].every(
        (id) => (assets[id] as { status: string }).status === "loaded",
      ) && (assets.broken as { status: string }).status === "failed",
      JSON.stringify(assets),
    );

    const first = (await session.capture(present)).image;
    await capture({ name: "p01-panel-paints", image: first });
    firstCapture(first, results);

    const commands = await session.update({ specimen: still(SWEEP_MOVED) });
    const second = (await session.capture(present)).image;
    await capture({ name: "p01-panel-paints-moved", image: second });
    secondCapture(first, second, commands, results);

    // An ordinary clip on the property sweeps the band once from its rest and
    // holds where it ends.
    await session.update({ specimen: still(SWEEP_MOVED, true) });
    const clip = await session.assetSettled("paint/sweep-clip");
    probe(
      results,
      "the sweep clip loads",
      clip.status === "loaded",
      JSON.stringify(clip),
    );
    const third = (await session.capture(present)).image;
    await capture({ name: "p01-panel-paints-swept", image: third });
    thirdCapture(second, third, results);
    if (first.width !== EXTENT[0] * CAPTURE_SCALE)
      probe(results, "capture extent", false, `${first.width}`);
    return { results, assets };
  } finally {
    await session.close();
  }
}
