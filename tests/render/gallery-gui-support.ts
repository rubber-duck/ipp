/**
 * Helpers the gallery GUI suites share: opening a settled dashboard, reading
 * its state and the exploded planes, and matching a
 * feature's plane by its projected depth in completed frames.
 */
import type { GuiAction, Inspection } from "@ipp/client";
import assert from "node:assert/strict";
import { openGallery } from "./gallery-driver.js";
import {
  LAYERS,
  controlRect,
  projectContent,
  type ProjectedPoint,
} from "./gallery-gui-panel.js";
import type { RgbaFrame } from "./retained-gui-images.js";
import type {
  GalleryGuiSelector,
  GalleryGuiState,
} from "./viewer-browser-helper.js";

export interface RegionStats {
  readonly pixels: number;
  readonly mean: readonly [number, number, number];
  readonly min: readonly [number, number, number];
  readonly max: readonly [number, number, number];
}

/** Symbolic IDs the gallery names, restated independently of the fixture. */
export const PANEL_ENTITY = "gui-demo";

/** Hover, press and selection transitions of the default looks, plus
 * host-frame and capture latency. */
export const SKIN_SETTLE_MS = 550;

/**
 * These scenarios keep the gallery's full canvas: their image assertions
 * measure the panel's on-screen pixel coverage, which half the canvas does
 * not give them: body text, scroll bars and window controls would be a few
 * pixels across.
 */
export const PIXEL_COVERAGE_CANVAS_SHARE = 1;

export function sceneEntity(inspection: Inspection, symbolicId: string) {
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === symbolicId,
  );
  assert.ok(entity, `missing scene entity ${symbolicId}`);
  return entity;
}

export function fieldsWith(
  inspection: Inspection,
  symbolicId: string,
  field: string,
) {
  const fields = sceneEntity(inspection, symbolicId).components.find(
    (entry) => field in entry.fields,
  )?.fields;
  assert.ok(fields, `${symbolicId} has no ${field} field`);
  return fields;
}

/**
 * Wait until the station has finished its first node sync: eight rows in
 * signal order, the operation complete, and its toast dismissed so frames
 * are static.
 */
export async function awaitStationIdle(g: Gallery, dismissToasts = true) {
  await waitForGuiReadouts(
    g,
    (readout) => readout("#gui-operation") === "Node sync: complete",
  );
  if (dismissToasts) await dismissGuiToasts(g);
  const state = await g.call<GalleryGuiState>("galleryGuiState");
  await g.capture("station-idle-ready");
  return state;
}

/** Dismiss observed toast controls, accepting only a confirmed expiry race. */
export async function dismissGuiToasts(g: Gallery) {
  const deadline = performance.now() + 15_000;
  for (;;) {
    const state = await g.call<GalleryGuiState>("galleryGuiState");
    const closes = state.controls.filter(({ symbol }) =>
      /^gui-toasts\/[^/]+\/close$/.test(symbol ?? ""),
    );
    if (closes.length === 0) return state;
    const close = closes.at(-1)!;
    assert.ok(close.symbol);
    try {
      await sceneGuiAction(
        g,
        { role: "button", name: "Dismiss", symbol: close.symbol },
        { kind: "press" },
      );
    } catch (failure) {
      if (
        !(failure instanceof Error) ||
        (!failure.message.includes("StaleTarget") &&
          !failure.message.includes("Expected one button 'Dismiss', found 0"))
      )
        throw failure;
      const current = await g.call<GalleryGuiState>("galleryGuiState");
      if (current.controls.some((item) => item.symbol === close.symbol))
        throw failure;
    }
    assert.ok(performance.now() < deadline, "the station's toasts stayed");
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

export type Gallery = Awaited<ReturnType<typeof openGallery>>;

/** Lift the real scene cover and await the replacement physical input owner. */
export async function liftInputShield(g: Gallery): Promise<void> {
  const previous = await g.page
    .locator('textarea[data-ipp-native-text="true"]')
    .elementHandle();
  assert.ok(previous, "the physical input context has no native bridge");
  try {
    await pressPresentationControl(g, "INPUT SHIELD");
    await waitForGuiReadouts(
      g,
      (readout) => readout("#gui-shield") === "lifted",
    );
    await g.page.waitForFunction((old) => {
      const current = document.querySelector(
        'textarea[data-ipp-native-text="true"]',
      );
      return current !== old && current?.isConnected;
    }, previous);
  } finally {
    await previous.dispose();
  }
}

export function decodeRegion(region: {
  width: number;
  height: number;
  pixels: string;
}): RgbaFrame {
  return {
    width: region.width,
    height: region.height,
    pixels: new Uint8Array(Buffer.from(region.pixels, "base64")),
  };
}

/**
 * Brightness (the brightest channel) of one capture's client pixels in a
 * window of `radius` around `centre`, with the window's pixel origin and its
 * count of bright ink pixels.
 */
export async function brightness(
  g: Gallery,
  label: string,
  frame: { readonly width: number; readonly height: number },
  centre: ProjectedPoint,
  radius: number,
) {
  const x = centre.x * frame.width;
  const y = centre.y * frame.height;
  const region = await g.call<{
    left: number;
    top: number;
    width: number;
    height: number;
    pixels: string;
  }>("viewerCaptureRegionPixels", label, [
    (x - radius) / frame.width,
    (y - radius) / frame.height,
    (x + radius) / frame.width,
    (y + radius) / frame.height,
  ]);
  const pixels = decodeRegion(region).pixels;
  const values = new Float64Array(region.width * region.height);
  for (let index = 0; index < values.length; index++)
    values[index] = Math.max(
      pixels[index * 4]!,
      pixels[index * 4 + 1]!,
      pixels[index * 4 + 2]!,
    );
  const { left, top, width, height } = region;
  return {
    left,
    top,
    width,
    height,
    values,
    ink: values.reduce((count, value) => count + (value >= 150 ? 1 : 0), 0),
  };
}

export type BrightnessWindow = Awaited<ReturnType<typeof brightness>>;

/** A feature of the panel for plane matching: a content point and its plane. */
export interface PlaneFeature {
  readonly at: readonly [number, number];
  readonly plane: number;
  /** Window radius in pixels; 18 by default. */
  readonly radius?: number;
}

/**
 * Match each feature's patch of the flat capture against the exploded
 * capture where every candidate plane would put it, at its id times the
 * spacing. The feature's plane must match best, so a feature one plane off
 * fails, and a raised plane must separate visibly. Returns the scores for
 * the evidence.
 */
export async function assertPlanes(
  g: Gallery,
  flat: {
    readonly label: string;
    readonly frame: CaptureSize;
    readonly projection: unknown;
  },
  exploded: { readonly label: string; readonly frame: CaptureSize },
  features: Readonly<Record<string, PlaneFeature>>,
) {
  // Every plane the kit uses, and the one above them.
  const top =
    Math.max(...Object.values(features).map(({ plane }) => plane)) + 1;
  const candidates = Array.from({ length: top + 1 }, (_, plane) => plane);
  const slack = 2;
  const shifts: Record<string, unknown> = {};
  for (const [name, { at, plane, radius = 18 }] of Object.entries(features)) {
    const stencil = [at, [at[0] + 1, at[1]], [at[0], at[1] + 1]] as const;
    const [base, flatX, flatY] = await g.call<readonly ProjectedPoint[]>(
      "projectGalleryGuiContent",
      stencil,
      0,
      flat.projection,
    );
    const patch = await brightness(g, flat.label, flat.frame, base!, radius);
    assert.ok(patch.ink > 0, `the flat ${name} feature has no ink`);
    const scores: { plane: number; shift: object; difference: number }[] = [];
    for (const candidate of candidates) {
      const [raised, raisedX, raisedY] = await projectContent(
        g,
        stencil,
        candidate * LAYERS.spacing,
      );
      const shift = {
        x: (raised!.x - base!.x) * flat.frame.width,
        y: (raised!.y - base!.y) * flat.frame.height,
      };
      const window = await brightness(
        g,
        exploded.label,
        exploded.frame,
        raised!,
        radius * 2 + slack + 1,
      );
      // Locally reproject each reference pixel between the independently
      // observed cameras. A small stencil captures scale, skew and rotation;
      // candidate depth still comes from the separate physical layer model.
      const fx = (flatX!.x - base!.x) * flat.frame.width;
      const fy = (flatX!.y - base!.y) * flat.frame.height;
      const gx = (flatY!.x - base!.x) * flat.frame.width;
      const gy = (flatY!.y - base!.y) * flat.frame.height;
      const determinant = fx * gy - fy * gx;
      let difference = Infinity;
      for (let dy = -slack; dy <= slack; dy++)
        for (let dx = -slack; dx <= slack; dx++) {
          let sum = 0;
          let count = 0;
          for (let y = 0; y < patch.height; y++)
            for (let x = 0; x < patch.width; x++) {
              const px = patch.left + x + 0.5 - base!.x * flat.frame.width;
              const py = patch.top + y + 0.5 - base!.y * flat.frame.height;
              const u = (px * gy - py * gx) / determinant;
              const v = (py * fx - px * fy) / determinant;
              const wx =
                Math.floor(
                  (raised!.x +
                    u * (raisedX!.x - raised!.x) +
                    v * (raisedY!.x - raised!.x)) *
                    exploded.frame.width +
                    dx,
                ) - window.left;
              const wy =
                Math.floor(
                  (raised!.y +
                    u * (raisedX!.y - raised!.y) +
                    v * (raisedY!.y - raised!.y)) *
                    exploded.frame.height +
                    dy,
                ) - window.top;
              if (wx < 0 || wy < 0 || wx >= window.width || wy >= window.height)
                continue;
              sum += Math.abs(
                patch.values[y * patch.width + x]! -
                  window.values[wy * window.width + wx]!,
              );
              count++;
            }
          if (count >= patch.values.length / 2)
            difference = Math.min(difference, sum / count);
        }
      scores.push({
        plane: candidate,
        shift,
        difference,
      });
    }
    shifts[name] = { plane, scores };
    const best = scores.reduce((left, right) =>
      right.difference < left.difference ? right : left,
    );
    assert.equal(
      best.plane,
      plane,
      `${name} matches plane ${best.plane}, not ${plane}: ${JSON.stringify(scores)}`,
    );
    if (plane > 0) {
      const [zero] = await projectContent(g, [at], 0);
      const [separated] = await projectContent(g, [at], plane * LAYERS.spacing);
      const x = (separated!.x - zero!.x) * exploded.frame.width;
      const y = (separated!.y - zero!.y) * exploded.frame.height;
      assert.ok(
        Math.hypot(x, y) > 6 * plane,
        `${name}'s plane barely separates: ${JSON.stringify({ x, y })}`,
      );
    }
  }
  return shifts;
}

export type CaptureSize = { readonly width: number; readonly height: number };

/** Play the panel's layer spacing to `spacing` through its in-scene switch. */
export async function toggleLayers(
  g: Gallery,
  exploded: boolean,
  interaction: "pointer" | "action" = "pointer",
) {
  if (interaction === "action")
    await sceneGuiAction(
      g,
      { role: "checkbox", name: "EXPLODE LAYERS" },
      { kind: "toggle" },
    );
  else await pressPresentationControl(g, "EXPLODE LAYERS");
  await waitForGuiReadouts(
    g,
    (readout, word) => readout("#gui-layers") === word,
    exploded ? "exploded" : "flat",
  );
  await g.waitFor((inspection) => {
    const spacing = Number(
      fieldsWith(inspection, PANEL_ENTITY, "layer_spacing").layer_spacing,
    );
    return Math.abs(spacing - (exploded ? LAYERS.spacing : 0)) < 1e-4;
  });
}

export async function waitForGuiState(
  g: Gallery,
  predicate: (state: GalleryGuiState) => boolean = () => true,
  flush = true,
): Promise<GalleryGuiState> {
  const deadline = performance.now() + 15_000;
  let lastError: unknown;
  while (performance.now() < deadline) {
    try {
      const state = await g.call<GalleryGuiState>("galleryGuiState", flush);
      if (predicate(state)) return state;
    } catch (failure) {
      lastError = failure;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(
    `GUI demo did not settle${lastError instanceof Error ? `: ${lastError.message}` : ""}`,
  );
}

export function dynamicProperty(
  inspection: Inspection,
  symbolicId: string,
  property: string,
) {
  const value = sceneEntity(inspection, symbolicId).components.find(
    (entry) => entry.properties && property in entry.properties,
  )?.properties?.[property];
  assert.ok(value, `${symbolicId} has no ${property} property`);
  return value;
}
/** Application callbacks observed through the mounted production controller. */
interface ApplicationState {
  app: {
    phase: string;
    password: string;
    reveal: boolean;
    progress: number;
    lines: readonly string[];
    settings: boolean;
    settingsPage: string;
    logOpen: boolean;
    strength: number;
    range: number;
    charge: { phase: string; progress: number };
  };
  declarationIssue?: string;
  accent: string;
  exploded: boolean;
  layerStep: number;
  presentationTab: string;
  workbenchTab: string;
  autoscan: boolean;
  gain: number;
  callsign: string;
  shieldArmed: boolean;
  shieldBlocker?: unknown;
  vectorOnly: boolean;
  reducedMotion: boolean;
  surfaceShape: string;
  surfaceFacing: string;
  surfaceCache: string;
  lastCommand: string;
  pulseSequence: number;
  pulseStrength: number;
  pulse: { state: string; value: number };
  selected: string | null;
  nodes: readonly { base: number }[];
  events: readonly string[];
  operation?: { kind: string; phase: string; callsign?: string };
  eventWindow: { first: number; last: number };
  tuning: {
    beam: number;
    light: number;
    offset: number;
    sweep: readonly number[];
    rate: string;
    channels: readonly string[];
    grid: string;
    sweepShown: boolean;
    focus: string | null;
    color: { hue: number; saturation: number; value: number };
  };
}

export async function guiApplication(g: Gallery) {
  return g.call<{ state: ApplicationState; ready: boolean; error?: string }>(
    "galleryGuiApplicationState",
  );
}

function applicationReadout(
  application: Awaited<ReturnType<typeof guiApplication>>,
  selector: string,
): string {
  const s = application.state;
  const t = s.tuning;
  switch (selector.replace(/^#gui-/, "")) {
    case "status":
      return application.error
        ? "error"
        : application.ready
          ? "ready"
          : "loading";
    case "accent":
      return s.accent;
    case "layers":
      return s.exploded ? "exploded" : "flat";
    case "presentation-tab":
      return s.presentationTab;
    case "tab":
      return s.workbenchTab;
    case "focus":
      return t.focus === undefined || t.focus === null
        ? "none"
        : ((
            {
              projector: "PROJECTOR",
              core: "CORE",
              lens: "LENS",
              beam: "BEAM",
              dust: "DUST",
              stage: "STAGE",
              base: "BASE",
              floor: "FLOOR",
              lights: "LIGHTS",
              glow: "PROJECTOR LIGHT",
              key: "KEY LIGHT",
              fill: "FILL LIGHT",
              panel: "PANEL",
              shield: "SHIELD",
            } as Readonly<Record<string, string>>
          )[t.focus] ?? "none");
    case "tuning":
      return `beam ${t.beam}%, light ${t.light}%, offset ${t.offset}%, sweep ${t.sweep.join("-")}%, ${t.rate}`;
    case "channels":
      return t.channels.join(" ") || "none";
    case "scope":
      return `${t.grid}, sweep ${t.sweepShown ? "on" : "off"}`;
    case "colour": {
      const { hue, saturation, value } = t.color;
      const channel = (offset: number) => {
        const turn = (((hue + offset) % 1) + 1) % 1;
        const pure = Math.min(Math.max(Math.abs(turn * 6 - 3) - 1, 0), 1);
        return Math.round(value * (1 - saturation + saturation * pure) * 255)
          .toString(16)
          .padStart(2, "0")
          .toUpperCase();
      };
      return `#${channel(0)}${channel(2 / 3)}${channel(1 / 3)}`;
    }
    case "callsign":
      return s.callsign || "unassigned";
    case "autoscan":
      return s.autoscan ? "enabled" : "standby";
    case "gain":
      return `${Math.round(s.gain * 100)}%`;
    case "nodes":
      return `${s.nodes.filter((n) => Math.round(100 * n.base * (0.35 + 0.65 * s.gain)) >= 40).length} of ${s.nodes.length} online${s.selected ? `, ${s.selected} selected` : ""}`;
    case "operation": {
      const o = s.operation;
      if (!o) return "none";
      const label =
        o.kind === "sync"
          ? o.phase === "pending"
            ? "Connecting to nodes"
            : "Node sync"
          : o.kind === "purge"
            ? "Node purge"
            : o.phase === "pending"
              ? "Opening uplink"
              : `Uplink ${o.callsign ?? ""}`.trim();
      return `${label}: ${o.phase}`;
    }
    case "shield":
      return s.shieldArmed ? "armed" : "lifted";
    case "events":
      return `items ${s.eventWindow.first}-${s.eventWindow.last} of ${s.events.length}`;
    case "command":
      return s.lastCommand;
  }
  throw new Error(`Unknown application readout ${selector}`);
}

export async function guiReadout(
  g: Gallery,
  selector: string,
): Promise<string> {
  return applicationReadout(await guiApplication(g), selector);
}

export async function waitForGuiReadouts<T = undefined>(
  g: Gallery,
  predicate: (readout: (selector: string) => string, argument: T) => unknown,
  argument?: T,
) {
  const deadline = performance.now() + 15_000;
  for (;;) {
    const application = await guiApplication(g);
    if (
      predicate(
        (selector) => applicationReadout(application, selector),
        argument as T,
      )
    )
      return;
    assert.ok(
      performance.now() < deadline,
      "GUI application readouts did not settle",
    );
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

export async function selectPresentationTab(
  g: Gallery,
  tab: "layers" | "shell" | "style",
) {
  const application = await guiApplication(g);
  if (application.state.presentationTab !== tab)
    await sceneGuiAction(
      g,
      { role: "button", name: tab === "shell" ? "SURFACE" : tab.toUpperCase() },
      { kind: "press" },
    );
  await waitForGuiReadouts(
    g,
    (readout) => readout("#gui-presentation-tab") === tab,
  );
  const symbol =
    tab === "layers"
      ? "gui-layer-step/dial"
      : tab === "shell"
        ? "gui-surface-shape"
        : "gui-reduced-motion";
  await waitForGuiState(g, (state) =>
    state.controls.some((item) => item.symbol === symbol && item.visible),
  );
}

function controlTab(name?: string): "layers" | "shell" | "style" | undefined {
  if (["CYAN", "AMBER", "REDUCED MOTION"].includes(name ?? "")) return "style";
  if (
    ["EXPLODE LAYERS", "GUI ONLY", "INPUT SHIELD", "LAYER STEP"].includes(
      name ?? "",
    )
  )
    return "layers";
  if (["PANEL SHAPE", "CURVED FACING", "PRESENTATION"].includes(name ?? ""))
    return "shell";
  return undefined;
}

export async function sceneGuiAction(
  g: Gallery,
  selector: GalleryGuiSelector,
  action: GuiAction,
): Promise<GalleryGuiState> {
  const tab = controlTab(selector.name);
  if (tab) await selectPresentationTab(g, tab);
  return g.call<GalleryGuiState>("galleryGuiAction", selector, action);
}

/** Real pointer input at the independently restated presentation control. */
export async function pressPresentationControl(g: Gallery, name: string) {
  const tab = controlTab(name);
  if (tab) await selectPresentationTab(g, tab);
  const role = [
    "EXPLODE LAYERS",
    "GUI ONLY",
    "INPUT SHIELD",
    "REDUCED MOTION",
  ].includes(name)
    ? "checkbox"
    : "button";
  const rect = controlRect({ role, name });
  const inspection = await g.inspect();
  const spacing = Number(
    fieldsWith(inspection, PANEL_ENTITY, "layer_spacing").layer_spacing,
  );
  const [point] = await projectContent(
    g,
    [[rect[0] + rect[2] / 2, rect[1] + rect[3] / 2]],
    LAYERS.presentation * spacing,
  );
  await g.capture("presentation-control-ready");
  await g.page.mouse.move(point!.clientX, point!.clientY);
  await g.settle();
  const hovered = await g.call<GalleryGuiState>("galleryGuiState");
  assert.ok(
    hovered.controls.some(
      (item) =>
        item.kind === role &&
        item.label.startsWith(name) &&
        item.interaction.hovered,
    ),
    `Pointer missed ${name}; hovered ${hovered.controls
      .filter((item) => item.interaction.hovered)
      .map((item) => item.symbol)
      .join(", ")}`,
  );
  await g.page.mouse.down();
  await g.page.mouse.up();
}

/** A real dropdown trigger press, followed by its evaluated option row. */
export async function selectSceneOption(g: Gallery, name: string, key: string) {
  const option = key === "automatic" ? "AUTO" : key.toUpperCase();
  const application = await guiApplication(g);
  const current =
    name === "PANEL SHAPE"
      ? application.state.surfaceShape
      : name === "CURVED FACING"
        ? application.state.surfaceFacing
        : application.state.surfaceCache;
  if (current === key) return;
  await pressPresentationControl(g, name);
  const state = await waitForGuiState(g, (state) =>
    state.controls.some(
      (item) => item.label === option && item.visible && item.available,
    ),
  );
  const item = state.controls.find(
    (item) => item.label === option && item.visible && item.available,
  )!;
  const spacing = Number(
    fieldsWith(await g.inspect(), PANEL_ENTITY, "layer_spacing").layer_spacing,
  );
  const [x, y, width, height] = item.bounds;
  const [point] = await projectContent(
    g,
    [[x + width / 2, y + height / 2]],
    LAYERS.anchored * spacing,
  );
  await g.page.mouse.click(point!.clientX, point!.clientY);
  await waitForGuiState(
    g,
    (state) =>
      !state.controls.some(
        (item) => item.label === option && item.visible && item.overlay,
      ),
  );
}
