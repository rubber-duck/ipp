/**
 * Independent restatement of the gallery GUI demo panel for its browser
 * suites: the authored layout in Surface content units (Canvas units at one
 * per metre, origin at the top left, +Y down), the bundled font's metrics
 * and the verified icon code points. Control fields name controls and
 * report their state; this model locates them, so a layout regression moves
 * the painted ink and input away from the expected rectangles.
 */
import assert from "node:assert/strict";
import type {
  GalleryGuiControl,
  GalleryGuiSelector,
  GalleryGuiState,
} from "./viewer-browser-helper.js";
import type { openGallery } from "./gallery-driver.js";

type Gallery = Awaited<ReturnType<typeof openGallery>>;

/** `[x, y, width, height]` in Surface content units. */
export type ContentRect = readonly [number, number, number, number];

/** `[minX, minY, maxX, maxY]` in Surface content units. */
export type LogicalRect = readonly [number, number, number, number];

export interface ProjectedPoint {
  readonly x: number;
  readonly y: number;
  readonly clientX: number;
  readonly clientY: number;
}

/** Verified Nerd Font code points, restated independently of the fixture. */
export const ICON_CODE_POINTS = {
  dashboard: "",
  signal: "",
  pulse: "",
  aurora: "",
  ember: "",
  neon: "",
} as const;

/**
 * Shure Tech Mono Nerd Font Mono metrics per unit font size, from its `head`
 * (1000 units per em), `hhea` (ascender 885, descender -242, no line gap)
 * and the shared 540-unit monospaced advance.
 */
export const FONT_METRICS = { advance: 0.54, lineHeight: 1.127 } as const;

export const PANEL_SIZE = [7.4, 4.8] as const;

const LEFT = 0.3;
const TOP = 0.25;
const CONTENT = 6.8;
const RIGHT = LEFT + CONTENT;

/** Rows of the panel column: header, gaps, gain, scan, lower row, commands. */
const HEADER_Y = TOP;
const GAIN_Y = HEADER_Y + 0.57 + 0.12;
const SCAN_Y = GAIN_Y + 0.9 + 0.12;
const LOWER_Y = SCAN_Y + 0.66 + 0.12;
const COMMAND_Y = LOWER_Y + 1.32 + 0.1;

/** Vertically centre a height in a row cell. */
function centred(y: number, cell: number, height: number) {
  return y + (cell - height) / 2;
}

const SPAN_Y = centred(HEADER_Y, 0.57, 0.38);
const SIGNAL_X = RIGHT - 0.55;
const SPAN_BUTTON_X = SIGNAL_X - 0.12 - 0.8;
const SKIN_Y = LOWER_Y + 0.84 + 0.1;
const TELEMETRY_X = LEFT + 3.24 + 0.16;
const TELEMETRY_COLUMN_X = TELEMETRY_X + 0.16;
const TELEMETRY_LABEL_Y = LOWER_Y + 0.04;
const TELEMETRY_VIEW_Y = TELEMETRY_LABEL_Y + 0.3 + 0.1;
const NOTES_Y = TELEMETRY_VIEW_Y + 3 * 0.25 + 0.1;
const NOTES_FONT_SIZE = 0.13;

export const NOTES_TEXT =
  "GAIN DRIVES THE PROJECTOR LIGHT, CUBE ENERGY AND WAVE AMPLITUDE. " +
  "HOLD SCAN TO ARM UPLINK. THE AMBER SHIELD GUARDS PURGE FROM STRAY CLICKS.";

/** Greedy word wrap of monospaced text into lines of at most `columns`. */
export function wrapColumns(text: string, columns: number): string[] {
  const lines: string[] = [];
  let line = "";
  for (const word of text.split(" ")) {
    const candidate = line === "" ? word : `${line} ${word}`;
    if (candidate.length <= columns || line === "") line = candidate;
    else {
      lines.push(line);
      line = word;
    }
  }
  if (line !== "") lines.push(line);
  return lines;
}

/** Wrapped operator notes: a fixed 2.86-wide Text at 0.13 type. */
export const NOTES = (() => {
  const glyph = FONT_METRICS.advance * NOTES_FONT_SIZE;
  const line = FONT_METRICS.lineHeight * NOTES_FONT_SIZE;
  const lines = wrapColumns(NOTES_TEXT, Math.floor(2.86 / glyph + 1e-4));
  return {
    glyph,
    line,
    lines,
    rect: [
      TELEMETRY_COLUMN_X,
      NOTES_Y,
      2.86,
      lines.length * line,
    ] as ContentRect,
  };
})();

/** The panel layout at zero telemetry scroll. */
export const PANEL = {
  shell: [0.03, 0.03, 7.34, 4.74] as ContentRect,
  dashboardCell: [LEFT, HEADER_Y, 0.48, 0.57] as ContentRect,
  title: [LEFT + 0.48, HEADER_Y, 2.2, 0.57] as ContentRect,
  spanFrame: (wide: boolean): ContentRect => [
    LEFT + 0.48 + 2.2,
    SPAN_Y,
    wide ? 2 : 1.2,
    0.38,
  ],
  span: [SPAN_BUTTON_X, SPAN_Y, 0.8, 0.38] as ContentRect,
  signalCell: [SIGNAL_X, HEADER_Y, 0.55, 0.57] as ContentRect,
  gainFrame: [LEFT, GAIN_Y, CONTENT, 0.9] as ContentRect,
  gain: [LEFT + 0.2, GAIN_Y + 0.08 + 0.3, 5.1, 0.36] as ContentRect,
  scanFrame: [LEFT, SCAN_Y, CONTENT, 0.66] as ContentRect,
  scan: [
    LEFT + 0.2 + 1.28,
    centred(SCAN_Y, 0.66, 0.48),
    1.04,
    0.48,
  ] as ContentRect,
  waveform: [
    LEFT + 0.2 + 1.28 + 1.28 + 0.3,
    SCAN_Y + 0.1,
    3.54,
    0.46,
  ] as ContentRect,
  pulse: [LEFT, LOWER_Y, 3.24, 0.84] as ContentRect,
  pulseCell: [LEFT, LOWER_Y, 0.9, 0.84] as ContentRect,
  skin: (skin: "aurora" | "ember" | "neon"): ContentRect => [
    LEFT + 0.48 + 0.06 + ["aurora", "ember", "neon"].indexOf(skin) * 0.92,
    SKIN_Y,
    0.86,
    0.38,
  ],
  telemetryFrame: [TELEMETRY_X, LOWER_Y, 3.4, 1.32] as ContentRect,
  telemetryLabel: [
    TELEMETRY_COLUMN_X,
    TELEMETRY_LABEL_Y,
    3.08,
    0.3,
  ] as ContentRect,
  telemetry: [TELEMETRY_COLUMN_X, TELEMETRY_VIEW_Y, 3.05, 0.78] as ContentRect,
  /** The event log inside the telemetry content, at zero telemetry scroll. */
  eventLog: [
    TELEMETRY_COLUMN_X,
    NOTES_Y + NOTES.rect[3] + 0.12,
    2.86,
    0.56,
  ] as ContentRect,
  callsign: [LEFT + 1.12, COMMAND_Y, 2.12, 0.38] as ContentRect,
  uplink: [LEFT + 1.12 + 2.12 + 0.16, COMMAND_Y, 1, 0.38] as ContentRect,
  purge: [RIGHT - 0.84, COMMAND_Y, 0.84, 0.38] as ContentRect,
} as const;

/** The glyph box of an icon: its advance wide and one line tall, placed in
 * its cell by the cell's horizontal alignment and centred vertically. */
export function iconBox(
  cell: ContentRect,
  fontSize: number,
  alignX: -1 | 0 | 1 = 0,
): ContentRect {
  const [x, y, width, height] = cell;
  const box = [
    FONT_METRICS.advance * fontSize,
    FONT_METRICS.lineHeight * fontSize,
  ] as const;
  return [
    x + ((alignX + 1) / 2) * (width - box[0]),
    y + (height - box[1]) / 2,
    box[0],
    box[1],
  ];
}

/** A skin button's leading 0.25-wide icon cell. */
function skinIconCell(skin: "aurora" | "ember" | "neon"): ContentRect {
  const [x, y, , height] = PANEL.skin(skin);
  return [x, y, 0.25, height];
}

/** Every icon's glyph box, with the font sizes and alignments it is
 * authored with. */
export const ICON_BOXES = {
  dashboard: iconBox(PANEL.dashboardCell, 0.48, -1),
  signal: iconBox(PANEL.signalCell, 0.44, 1),
  pulse: iconBox(PANEL.pulseCell, 0.56),
  aurora: iconBox(skinIconCell("aurora"), 0.22),
  ember: iconBox(skinIconCell("ember"), 0.22),
  neon: iconBox(skinIconCell("neon"), 0.22),
} as const;

/** The expected rectangle of one semantic control. */
export function controlRect(selector: GalleryGuiSelector): ContentRect {
  switch (selector.role) {
    case "button":
      switch (selector.name) {
        case "SPAN":
          return PANEL.span;
        case "PULSE":
          return PANEL.pulse;
        case "UPLINK":
          return PANEL.uplink;
        case "PURGE":
          return PANEL.purge;
        case "AURORA":
        case "EMBER":
        case "NEON":
          return PANEL.skin(
            selector.name.toLowerCase() as "aurora" | "ember" | "neon",
          );
      }
      break;
    case "checkbox":
      return PANEL.scan;
    case "slider":
      return PANEL.gain;
    case "text":
      return PANEL.callsign;
    case "scrollView":
      return PANEL.telemetry;
    case "virtualList":
      return PANEL.eventLog;
  }
  throw new Error(`No modelled ${selector.role} '${selector.name ?? ""}'`);
}

/** `[minX, minY, maxX, maxY]` of a content rectangle. */
export function logical([x, y, width, height]: ContentRect): LogicalRect {
  return [x, y, x + width, y + height];
}

/** Project Surface content points through the actual panel and camera. */
export function projectContent(
  g: Gallery,
  points: readonly (readonly [number, number])[],
) {
  return g.call<readonly ProjectedPoint[]>("projectGalleryGuiContent", points);
}

/** Project a point at fractions of one control's expected rectangle. */
export async function controlPoint(
  g: Gallery,
  selector: GalleryGuiSelector,
  fractionX = 0.5,
  fractionY = 0.5,
): Promise<ProjectedPoint> {
  const [x, y, width, height] = controlRect(selector);
  await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
  const [point] = await projectContent(g, [
    [x + width * fractionX, y + height * fractionY],
  ]);
  assert.ok(point, "control point did not project");
  return point;
}

/** Normalized completed-frame bounds of a control, inset by fractions. */
export function controlRegion(
  g: Gallery,
  selector: GalleryGuiSelector,
  insetX = 0.05,
  insetY = 0.1,
) {
  const [x, y, width, height] = controlRect(selector);
  return g.call<readonly [number, number, number, number]>(
    "galleryGuiContentRegion",
    [
      x + width * insetX,
      y + height * insetY,
      x + width * (1 - insetX),
      y + height * (1 - insetY),
    ],
  );
}

/** The one control a selector names. */
export function control(
  state: GalleryGuiState,
  selector: GalleryGuiSelector,
): GalleryGuiControl {
  const matches = state.controls.filter(
    (candidate) =>
      candidate.kind === selector.role &&
      (selector.name === undefined || candidate.label === selector.name),
  );
  assert.equal(
    matches.length,
    1,
    `expected one ${selector.role} ${selector.name ?? "control"}, found ${matches.length}`,
  );
  return matches[0]!;
}

export function controlValue(
  state: GalleryGuiState,
  role: GalleryGuiSelector["role"],
  name?: string,
) {
  return control(state, { role, ...(name === undefined ? {} : { name }) })
    .value;
}

/** Controls keep their exact World, entity and component incarnation, kind
 * and value across a change. */
export function assertRetainedControls(
  before: GalleryGuiState,
  after: GalleryGuiState,
  valuesToo = true,
): void {
  for (const previous of before.controls) {
    const retained = after.controls.find(
      ({ target }) =>
        target.world.id === previous.target.world.id &&
        target.world.incarnation === previous.target.world.incarnation &&
        target.entity === previous.target.entity &&
        target.component === previous.target.component &&
        target.incarnation === previous.target.incarnation,
    );
    assert.ok(
      retained,
      `removed ${previous.kind} ${previous.label || previous.symbol}`,
    );
    assert.equal(retained.kind, previous.kind);
    if (!valuesToo) continue;
    assert.deepEqual(retained.value, previous.value);
  }
}

/** Scroll state of the telemetry ScrollView and the event log VirtualList
 * nested inside it, from their fields. */
export function telemetryScrollViews(state: GalleryGuiState) {
  const outer = control(state, { role: "scrollView" });
  const inner = control(state, { role: "virtualList" });
  assert.ok(
    inner.ancestry.includes(outer.target.entity),
    "the event log is not nested in the telemetry view",
  );
  const view = (snapshot: GalleryGuiControl) => {
    assert.ok(snapshot.scroll, `${snapshot.kind} has no scroll state`);
    assert.equal(snapshot.value.kind, "scroll");
    const value = snapshot.value as Extract<
      GalleryGuiControl["value"],
      { kind: "scroll" }
    >;
    return {
      control: snapshot,
      offset: value.offset,
      anchorIndex: value.anchorIndex,
      anchorOffset: value.anchorOffset,
      capacity: snapshot.scroll.capacity,
      viewport: snapshot.scroll.viewport,
      itemCount: snapshot.scroll.itemCount,
      first: snapshot.scroll.first,
      last: snapshot.scroll.last,
    };
  };
  return { outer: view(outer), inner: view(inner) };
}

export type ScrollViews = ReturnType<typeof telemetryScrollViews>;
