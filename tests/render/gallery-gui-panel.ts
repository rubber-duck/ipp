/**
 * Independent restatement of the gallery GUI dashboard for its browser
 * suites: the authored layout in canvas units (origin at the top left, +Y
 * down, 140 units per Surface metre), the design language's sizes it is
 * built from, the bundled font's metrics and the default scroll bar
 * geometry. Control fields name controls and report their state; this model
 * locates them, so a layout regression moves the painted ink and input away
 * from the expected rectangles.
 */
import { GUI_KIT_LAYERS } from "@ipp/react/gui-kit";
import assert from "node:assert/strict";
import type {
  GalleryGuiControl,
  GalleryGuiSelector,
  GalleryGuiState,
} from "./viewer-browser-helper.js";
import type { openGallery } from "./gallery-driver.js";

type Gallery = Awaited<ReturnType<typeof openGallery>>;

/** `[x, y, width, height]` in canvas units. */
export type ContentRect = readonly [number, number, number, number];

/** `[minX, minY, maxX, maxY]` in canvas units. */
export type LogicalRect = readonly [number, number, number, number];

export interface ProjectedPoint {
  readonly x: number;
  readonly y: number;
  readonly clientX: number;
  readonly clientY: number;
}

/**
 * Shure Tech Mono Nerd Font Mono metrics per unit font size, from its `head`
 * (1000 units per em), `hhea` (ascender 885, descender -242, no line gap)
 * and the shared 540-unit monospaced advance.
 */
export const FONT_METRICS = { advance: 0.54, lineHeight: 1.127 } as const;

/** The design language's sizes, in units at its 16-unit body text. */
export const LANGUAGE = {
  line: 1.25,
  inset: 16,
  control: 40,
  small: 32,
  dockedHeight: 24,
  dockedWidth: 32,
  row: 36,
  dense: 24,
  bar: 8,
  textSmall: 13,
  body: 16,
} as const;

const L = LANGUAGE;

/** The Surface in metres, and the canvas units the panel World maps onto it. */
export const SURFACE_SIZE = [7.4, 4.8] as const;
export const UNITS_PER_METRE = 140;
export const CANVAS_SIZE = [1036, 672] as const;

/**
 * The exploded view: Surface metres per plane id, and the plane of each kind
 * of entity. Panels and everything they hold stay whole on the base plane;
 * the kit's overlays float on its planes. A plane's depth is its id times
 * the spacing, whichever other planes are in use.
 */
export const LAYERS = {
  spacing: 0.2,
  panel: 0,
  anchored: GUI_KIT_LAYERS.anchored,
  dialog: GUI_KIT_LAYERS.dialog,
  toast: GUI_KIT_LAYERS.toast,
} as const;

/** Symbolic ids of the dashboard's overlays. */
export const OVERLAYS = {
  dialog: "gui-purge-dialog",
  menu: "gui-node-menu",
  menuSurface: "gui-node-menu/surface",
  toasts: "gui-toasts",
  pulseTip: "gui-pulse/tip",
  scope: "gui-scope-options/popover",
  rate: "gui-rate/list",
  find: "gui-node-find/list",
} as const;

/** One wheel notch of the gallery's GUI input: two dense rows. */
export const WHEEL_STEP = 2 * L.dense;

/** The width of `text` at font size `size`. */
export function textWidth(text: string, size: number): number {
  return [...text].length * FONT_METRICS.advance * size;
}

/** A small secondary button hugs its label with the inset on either side. */
function secondaryWidth(label: string): number {
  return textWidth(label, L.textSmall) + 2 * L.inset;
}

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

/** Columns of the dashboard: 16 apart and 16 from the canvas edges. */
const LEFT = L.inset;
const CENTRE = LEFT + 300 + L.inset;
const RIGHT = CENTRE + 384 + L.inset;
const TOP = L.inset;

/** A panel's content box inside its frame line. */
function inner([x, y, width, height]: ContentRect): ContentRect {
  return [x + L.line, y, width - 2 * L.line, height];
}

/** A header strip's two docked window controls at its end. */
function windowControls(panel: ContentRect) {
  const [x, y, width] = inner(panel);
  const end = x + width - (L.control - L.dockedHeight) / 2;
  const top = y + (L.control - L.dockedHeight) / 2;
  const close: ContentRect = [
    end - L.dockedWidth,
    top,
    L.dockedWidth,
    L.dockedHeight,
  ];
  return {
    minimize: [
      close[0] - L.inset / 2 - L.dockedWidth,
      top,
      L.dockedWidth,
      L.dockedHeight,
    ] as ContentRect,
    close,
  };
}

/** TELEMETRY: header, the scrolling body and the footer. */
const TELEMETRY_PANEL: ContentRect = [LEFT, TOP, 300, 640];
const TELEMETRY_BODY = ((): ContentRect => {
  const [x, y, width, height] = inner(TELEMETRY_PANEL);
  const top = y + L.control + L.line;
  const footer = L.small + L.inset;
  return [x, top, width, y + height - footer - L.line - top];
})();
/** The body's content column: the inset, and the scroll bar's column. */
const BODY_X = TELEMETRY_BODY[0] + L.inset;
const BAR_COLUMN = 3 * L.bar;
const BODY_WIDTH = TELEMETRY_BODY[2] - L.inset - BAR_COLUMN;
const READOUT_TOP = TELEMETRY_BODY[1] + L.inset;
const READOUTS = 6;
const READOUT_NAME_WIDTH = 96;

export const NOTES_TEXT =
  "GAIN DRIVES THE PROJECTOR LIGHT AND THE WAVE AMPLITUDE. " +
  "SCAN HOLDS THE UPLINK. THE AMBER SHIELD GUARDS PURGE.";

/** Wrapped operator notes: small text as wide as the body column. */
export const NOTES = (() => {
  const glyph = FONT_METRICS.advance * L.textSmall;
  const line = FONT_METRICS.lineHeight * L.textSmall;
  const lines = wrapColumns(NOTES_TEXT, Math.floor(BODY_WIDTH / glyph + 1e-4));
  const top = READOUT_TOP + READOUTS * L.dense + L.inset;
  return {
    glyph,
    line,
    lines,
    rect: [BODY_X, top, BODY_WIDTH, lines.length * line] as ContentRect,
  };
})();

/** The EVENTS and SCENE choice under the notes, then the chosen view. */
const VIEW_TOP = NOTES.rect[1] + NOTES.rect[3] + L.inset;
const EVENT_LOG_TOP = VIEW_TOP + L.control + L.inset / 2;

/** Event log entries: small text inset from the list frame and its bar. */
export const EVENT_LOG = {
  textSize: L.textSmall,
  textInset: L.inset,
  /** The log's width less the inset on either side; the right inset holds
   * the list's own scroll bar. */
  textWidth: BODY_WIDTH - 2 * L.inset,
  /** Margin above and below an entry, and its least text height. */
  margin: 4,
  minText: L.inset,
  estimate: 38,
} as const;

/** SIGNAL MONITOR: header, scope, division and three control rows. */
const SCOPE = { width: 231, height: 119 } as const;
const MONITOR_HEIGHT =
  L.control +
  L.line +
  SCOPE.height +
  2 * L.inset +
  L.line +
  2 * L.inset +
  3 * L.control +
  2 * (L.inset / 2);
const MONITOR_PANEL: ContentRect = [CENTRE, TOP, 384, MONITOR_HEIGHT];
const MONITOR_INNER = inner(MONITOR_PANEL);
const SCOPE_TOP = TOP + L.control + L.line;
const CONTROLS_X = MONITOR_INNER[0] + L.inset;
const CONTROLS_WIDTH = MONITOR_INNER[2] - 2 * L.inset;
const CONTROLS_TOP = SCOPE_TOP + SCOPE.height + 2 * L.inset + L.line + L.inset;
const LABEL_WIDTH = 72;
const ACTION_WIDTH = 112;
const controlRow = (index: number) =>
  CONTROLS_TOP + index * (L.control + L.inset / 2);
const ACTION_X = CONTROLS_X + CONTROLS_WIDTH - ACTION_WIDTH;

/** STATUS: at the bottom of the centre column. */
const STATUS_HEIGHT =
  L.control +
  L.line +
  L.inset +
  L.small +
  L.inset / 2 +
  L.control +
  L.inset / 2 +
  96 +
  L.inset;
const STATUS_PANEL: ContentRect = [
  CENTRE,
  TOP + 640 - STATUS_HEIGHT,
  384,
  STATUS_HEIGHT,
];
const STATUS_X = inner(STATUS_PANEL)[0] + L.inset;
const STATUS_WIDTH = inner(STATUS_PANEL)[2] - 2 * L.inset;
const BADGES_TOP = STATUS_PANEL[1] + L.control + L.line + L.inset;
const ALERT_TOP = BADGES_TOP + L.small + L.inset / 2;
const OPERATIONS_TOP = ALERT_TOP + L.control + L.inset / 2;
const PROGRESS_HEIGHT = L.dense + 4 + L.small;

/**
 * The workbench: a panel at the top of the right column whose tab strip,
 * NODES, CONTROLS and COLOUR, hugs its labels, over the selected tab's
 * content at the inset and the footer under every tab.
 */
const NODES_PANEL: ContentRect = [RIGHT, TOP, 288, 448];
const NODES_INNER = inner(NODES_PANEL);
const TAB_LABELS = ["NODES", "CONTROLS", "COLOUR"] as const;
const TABS = (() => {
  let x = NODES_INNER[0];
  return TAB_LABELS.map((label) => {
    // A one-line label's box: its advances and a hundredth of the size.
    const width = textWidth(label, L.body) + L.body / 100 + 2 * L.inset;
    const rect: ContentRect = [x, TOP, width, L.control];
    x += width;
    return rect;
  });
})();
const TAB_CONTENT: ContentRect = [
  NODES_INNER[0] + L.inset,
  TOP + L.control + L.line + L.inset,
  NODES_INNER[2] - 2 * L.inset,
  0,
];
/** NODES: FIND, then the scrolling grid of a header and six rows. */
const FIND_RECT: ContentRect = [
  TAB_CONTENT[0],
  TAB_CONTENT[1],
  TAB_CONTENT[2],
  L.control,
];
const GRID_TOP = FIND_RECT[1] + L.control + L.inset / 2;
const GRID_HEIGHT = 7 * L.row;
const GRID_COLUMNS = (() => {
  const gutter = 4;
  const signal = 71;
  const status = 77;
  const x = TAB_CONTENT[0] + gutter;
  const node = TAB_CONTENT[2] - gutter - signal - status - BAR_COLUMN;
  return {
    node: [x, node] as const,
    signal: [x + node, signal] as const,
    status: [x + node + signal, status] as const,
  };
})();
const FOOTER_BUTTON_Y =
  TOP + 448 - (L.small + L.inset) + (L.small + L.inset - L.small) / 2;
const PURGE_RECT: ContentRect = [
  NODES_INNER[0] + NODES_INNER[2] - L.inset - secondaryWidth("PURGE"),
  FOOTER_BUTTON_Y,
  secondaryWidth("PURGE"),
  L.small,
];

/** ADVANCED: the expander under NODE STATUS and its three setting rows. */
const ADVANCED_TOP = TOP + 448 + L.inset;
const settingRow = (index: number) =>
  ADVANCED_TOP + L.control + L.inset / 2 + index * (L.control + 4);
const SETTING_END = RIGHT + 288 - L.inset;

/** The dashboard at zero scroll. */
export const PANEL = {
  telemetryPanel: TELEMETRY_PANEL,
  telemetryTitle: [
    inner(TELEMETRY_PANEL)[0] + L.inset,
    TOP,
    textWidth("TELEMETRY", L.body),
    L.control,
  ] as ContentRect,
  telemetry: TELEMETRY_BODY,
  /** Readout `index`: its name and value cells. */
  readout: (index: number) => ({
    name: [
      BODY_X,
      READOUT_TOP + index * L.dense,
      READOUT_NAME_WIDTH,
      L.dense,
    ] as ContentRect,
    value: [
      BODY_X + READOUT_NAME_WIDTH + L.line + L.inset,
      READOUT_TOP + index * L.dense,
      BODY_WIDTH - READOUT_NAME_WIDTH - L.line - L.inset,
      L.dense,
    ] as ContentRect,
  }),
  /** The EVENTS and SCENE segments, sharing the body column's width. */
  telemetryView: (segment: 0 | 1): ContentRect => [
    BODY_X + (segment * BODY_WIDTH) / 2,
    VIEW_TOP,
    BODY_WIDTH / 2,
    L.control,
  ],
  /** The event log inside the telemetry content, at zero telemetry scroll. */
  eventLog: [BODY_X, EVENT_LOG_TOP, BODY_WIDTH, 15 * L.dense] as ContentRect,
  clear: [
    inner(TELEMETRY_PANEL)[0] +
      inner(TELEMETRY_PANEL)[2] -
      L.inset -
      secondaryWidth("CLEAR"),
    TOP + 640 - (L.small + L.inset) + L.inset / 2,
    secondaryWidth("CLEAR"),
    L.small,
  ] as ContentRect,

  monitorPanel: MONITOR_PANEL,
  monitorTitle: [
    MONITOR_INNER[0] + L.inset,
    TOP,
    textWidth("SIGNAL MONITOR", L.body),
    L.control,
  ] as ContentRect,
  ...(() => {
    const { minimize, close } = windowControls(MONITOR_PANEL);
    // The SCOPE popover's trigger: small text and its caret in the inset on
    // either side, docked high, half an inset before the window controls.
    const width = textWidth("SCOPE \u{f1a09}", L.textSmall) + 2 * L.inset;
    const scope: ContentRect = [
      minimize[0] - L.inset / 2 - width,
      minimize[1],
      width,
      L.dockedHeight,
    ];
    return { minimize, close, scope };
  })(),
  waveform: [
    MONITOR_INNER[0] + L.inset,
    SCOPE_TOP + L.inset,
    SCOPE.width,
    SCOPE.height,
  ] as ContentRect,
  /** The gain readout's display value: the scope's last column. */
  gainReadout: [
    MONITOR_INNER[0] + L.inset + SCOPE.width + L.inset + L.line + L.inset,
    SCOPE_TOP + L.inset + L.dense,
    MONITOR_INNER[0] +
      MONITOR_INNER[2] -
      L.inset -
      (MONITOR_INNER[0] + L.inset + SCOPE.width + L.inset + L.line + L.inset),
    24 * FONT_METRICS.lineHeight,
  ] as ContentRect,
  gain: [
    CONTROLS_X + LABEL_WIDTH,
    controlRow(0),
    CONTROLS_WIDTH - LABEL_WIDTH,
    L.control,
  ] as ContentRect,
  scan: [
    CONTROLS_X + LABEL_WIDTH,
    controlRow(1) + (L.control - L.small) / 2,
    72,
    L.small,
  ] as ContentRect,
  pulse: [ACTION_X, controlRow(1), ACTION_WIDTH, L.control] as ContentRect,
  callsign: [
    CONTROLS_X + LABEL_WIDTH,
    controlRow(2),
    CONTROLS_WIDTH - LABEL_WIDTH - L.inset - ACTION_WIDTH,
    L.control,
  ] as ContentRect,
  uplink: [ACTION_X, controlRow(2), ACTION_WIDTH, L.control] as ContentRect,

  statusPanel: STATUS_PANEL,
  badges: [STATUS_X, BADGES_TOP, STATUS_WIDTH, L.small] as ContentRect,
  alert: [STATUS_X, ALERT_TOP, STATUS_WIDTH, L.control] as ContentRect,
  /** The alert's action, a secondary button half an inset from its end. */
  alertAction: (label: string): ContentRect => [
    STATUS_X + STATUS_WIDTH - L.inset / 2 - secondaryWidth(label),
    ALERT_TOP + (L.control - L.small) / 2,
    secondaryWidth(label),
    L.small,
  ],
  pulseRing: [STATUS_X, OPERATIONS_TOP, 96, 96] as ContentRect,
  operation: [
    STATUS_X + 96 + L.inset,
    OPERATIONS_TOP + (96 - PROGRESS_HEIGHT) / 2,
    STATUS_WIDTH - 96 - L.inset,
    PROGRESS_HEIGHT,
  ] as ContentRect,

  nodesPanel: NODES_PANEL,
  /** The workbench's tabs: NODES, CONTROLS and COLOUR. */
  tab: (index: 0 | 1 | 2): ContentRect => TABS[index]!,
  find: FIND_RECT,
  gridHeader: [TAB_CONTENT[0], GRID_TOP, TAB_CONTENT[2], L.row] as ContentRect,
  /** The grid's scrolling body: six rows, the bar in a column after them. */
  gridBody: [
    TAB_CONTENT[0],
    GRID_TOP + L.row,
    TAB_CONTENT[2],
    GRID_HEIGHT - L.row,
  ] as ContentRect,
  /** Data row `index` of the node grid at zero scroll. */
  gridRow: (index: number): ContentRect => [
    TAB_CONTENT[0],
    GRID_TOP + L.row + index * L.row,
    TAB_CONTENT[2] - BAR_COLUMN,
    L.row,
  ],
  /** Cell `column` of data row `index`, between its lines. */
  gridCell: (index: number, column: keyof typeof GRID_COLUMNS): ContentRect => [
    GRID_COLUMNS[column][0],
    GRID_TOP + L.row + index * L.row,
    GRID_COLUMNS[column][1],
    L.row,
  ],
  sync: [
    PURGE_RECT[0] - L.inset / 2 - secondaryWidth("SYNC"),
    FOOTER_BUTTON_Y,
    secondaryWidth("SYNC"),
    L.small,
  ] as ContentRect,
  purge: PURGE_RECT,

  advanced: [RIGHT, ADVANCED_TOP, 288, L.control] as ContentRect,
  cyan: [SETTING_END - 160, settingRow(0), 80, L.control] as ContentRect,
  amber: [SETTING_END - 80, settingRow(0), 80, L.control] as ContentRect,
  explode: [
    SETTING_END - 72,
    settingRow(1) + (L.control - L.small) / 2,
    72,
    L.small,
  ] as ContentRect,
  reducedMotion: [
    SETTING_END - L.small,
    settingRow(2) + (L.control - L.small) / 2,
    L.small,
    L.small,
  ] as ContentRect,

  /**
   * PURGE's confirmation dialog, centred on the canvas: the frame's line
   * round a header strip of the control height with its division line, and
   * a body of the inset, two dense lines, the inset, the buttons and the
   * inset. Cancel and the amber action share the row equally, an inset
   * apart.
   */
  dialog: (() => {
    const width = 368;
    const header = L.control + L.line;
    const body = 3 * L.inset + 2 * L.dense + L.control;
    const height = 2 * L.line + header + body;
    const x = (CANVAS_SIZE[0] - width) / 2;
    const y = (CANVAS_SIZE[1] - height) / 2;
    const button = (width - 2 * L.line - 3 * L.inset) / 2;
    const buttonY = y + L.line + header + 2 * L.inset + 2 * L.dense;
    return {
      frame: [x, y, width, height] as ContentRect,
      title: [x + L.line + L.inset, y + L.line, 200, L.control] as ContentRect,
      cancel: [x + L.line + L.inset, buttonY, button, L.control] as ContentRect,
      action: [
        x + L.line + 2 * L.inset + button,
        buttonY,
        button,
        L.control,
      ] as ContentRect,
    };
  })(),

  /**
   * A one-line tooltip above the control at `rect`: small text in a half-inset
   * margin on a small-control-high surface, centred on the control a quarter
   * inset above it.
   */
  tooltip: (rect: ContentRect, text: string): ContentRect => {
    const width = textWidth(text, L.textSmall) + L.inset;
    return [
      rect[0] + rect[2] / 2 - width / 2,
      rect[1] - L.inset / 4 - L.small,
      width,
      L.small,
    ];
  },

  /** Toast `index` of `count` in the stack at the canvas's bottom right. */
  toast: (index: number, count: number): ContentRect => {
    const height = L.control + L.inset;
    const bottom = CANVAS_SIZE[1] - L.inset;
    return [
      CANVAS_SIZE[0] - L.inset - (288 + L.inset),
      bottom - (count - index) * height - (count - 1 - index) * L.inset,
      288 + L.inset,
      height,
    ];
  },
} as const;

/**
 * The fraction of the GAIN slider's width at which its thumb centre sits for
 * `value`: the thumb, three quarters of the control's height square, travels
 * the width less its own edge.
 */
export function gainFraction(value: number): number {
  const [, , width, height] = PANEL.gain;
  const edge = 0.75 * height;
  return (edge / 2 + value * (width - edge)) / width;
}

/** The expected rectangle of one semantic control. */
export function controlRect(selector: GalleryGuiSelector): ContentRect {
  switch (selector.role) {
    case "button":
      switch (selector.name) {
        case "PULSE":
          return PANEL.pulse;
        case "UPLINK":
          return PANEL.uplink;
        case "CLEAR":
          return PANEL.clear;
        case "SYNC":
          return PANEL.sync;
        case "PURGE":
          return PANEL.purge;
        case "Minimize":
        case "Restore":
          return PANEL.minimize;
        case "Close":
          return PANEL.close;
        case "ADVANCED":
          return PANEL.advanced;
        case "CYAN":
          return PANEL.cyan;
        case "AMBER":
          return PANEL.amber;
        case "STOP":
          return PANEL.alertAction("STOP");
        case "EVENTS":
          return PANEL.telemetryView(0);
        case "SCENE":
          return PANEL.telemetryView(1);
        case "SCOPE":
          return PANEL.scope;
        case "NODES":
          return PANEL.tab(0);
        case "CONTROLS":
          return PANEL.tab(1);
        case "COLOUR":
          return PANEL.tab(2);
      }
      break;
    case "checkbox":
      switch (selector.name) {
        case "SCAN":
          return PANEL.scan;
        case "EXPLODE LAYERS":
          return PANEL.explode;
        case "REDUCED MOTION":
          return PANEL.reducedMotion;
      }
      break;
    case "slider":
      return PANEL.gain;
    case "text":
      return selector.name === "FIND" ? PANEL.find : PANEL.callsign;
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

/**
 * Project canvas points through the actual panel and camera: on the Surface
 * plane, or `depth` Surface metres in front of it along its normal, where a
 * layer plane of an exploded panel lies.
 */
export function projectContent(
  g: Gallery,
  points: readonly (readonly [number, number])[],
  depth = 0,
) {
  return g.call<readonly ProjectedPoint[]>(
    "projectGalleryGuiContent",
    points,
    depth,
  );
}

/** Project a point at fractions of one control's expected rectangle. */
export async function controlPoint(
  g: Gallery,
  selector: GalleryGuiSelector,
  fractionX = 0.5,
  fractionY = 0.5,
  depth = 0,
): Promise<ProjectedPoint> {
  const [x, y, width, height] = controlRect(selector);
  await g.page.locator("#ipp-world-canvas").scrollIntoViewIfNeeded();
  const [point] = await projectContent(
    g,
    [[x + width * fractionX, y + height * fractionY]],
    depth,
  );
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
  // An overlay's controls are named by their symbol, since its labels may
  // repeat the dashboard's.
  const matches = state.controls.filter(
    (candidate) =>
      candidate.kind === selector.role &&
      (selector.name === undefined || candidate.label === selector.name) &&
      (selector.symbol === undefined
        ? !overlayControl(candidate)
        : candidate.symbol === selector.symbol),
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
  only: (control: GalleryGuiControl) => boolean = () => true,
): void {
  for (const previous of before.controls.filter(only)) {
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

/**
 * The dashboard's own controls, whose identities persist: not the toasts and
 * alert actions that come and go with what the station has to say.
 */
export function dashboardControl(control: GalleryGuiControl): boolean {
  const symbol = control.symbol ?? "";
  return !overlayControl(control) && !symbol.startsWith("gui-alert");
}

/** A control inside an overlay: a list, popover, menu, dialog or toast. */
export function overlayControl(control: GalleryGuiControl): boolean {
  return control.overlay !== undefined;
}

/**
 * The expected default vertical scroll bar of a scrolling view at `rect` on
 * screen, from the language's default geometry at body text: a bar half the
 * font thick, one thickness in from the right side and half a thickness in
 * from the ends, whose pointed tips the thumb stays off; the thumb is the
 * visible fraction of that rail, never shorter than two thicknesses, placed
 * by offset over capacity.
 */
export function expectedScrollBar(
  view: {
    readonly offset: readonly number[];
    readonly capacity: readonly number[];
  },
  [x, y, width, height]: ContentRect,
) {
  const thickness = L.body / 2;
  const capacity = view.capacity[1]!;
  const content = height + capacity;
  const track: LogicalRect = [
    x + width - 2 * thickness,
    y + thickness / 2,
    x + width - thickness,
    y + height - thickness / 2,
  ];
  const rail = [track[1] + thickness / 2, track[3] - track[1] - thickness];
  const length = Math.min(
    rail[1]!,
    Math.max(rail[1]! * (height / content), 2 * thickness),
  );
  const fraction = capacity > 0 ? view.offset[1]! / capacity : 0;
  const top = rail[0]! + (rail[1]! - length) * fraction;
  return {
    thickness,
    track,
    thumb: [track[0], top, track[2], top + length] as LogicalRect,
  };
}

/** Scroll state of the telemetry ScrollView and the event log VirtualList
 * nested inside it, from their fields. */
export function telemetryScrollViews(state: GalleryGuiState) {
  const outer = control(state, { role: "scrollView", name: "TELEMETRY" });
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
