/**
 * Helpers the gallery GUI suites share: opening a settled dashboard, reading
 * its state and the exploded planes, and matching a
 * feature's plane by its projected depth in completed frames.
 */
import type { Inspection } from "@ipp/client";
import assert from "node:assert/strict";
import { openGallery } from "./gallery-driver.js";
import {
  LAYERS,
  projectContent,
  type ProjectedPoint,
} from "./gallery-gui-panel.js";
import type { RgbaFrame } from "./retained-gui-images.js";
import type { GalleryGuiState } from "./viewer-browser-helper.js";

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
  await g.page.waitForFunction(
    () =>
      document.querySelector("#gui-operation")?.textContent ===
      "Node sync: complete",
  );
  const deadline = performance.now() + 15_000;
  for (;;) {
    const state = await g.call<GalleryGuiState>("galleryGuiState");
    const closes = state.controls.filter(({ symbol }) =>
      /^gui-toasts\/[^/]+\/close$/.test(symbol ?? ""),
    );
    if (!dismissToasts || closes.length === 0) return state;
    if (closes.length === 1)
      try {
        await g.call(
          "galleryGuiAction",
          { role: "button", name: "Dismiss" },
          { kind: "press" },
        );
      } catch (failure) {
        // The toast may dismiss itself at the end of its time first.
        if (
          !(failure instanceof Error && failure.message.includes("StaleTarget"))
        )
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
    await g.page.locator("#gui-shield-toggle").click();
    await g.page.waitForFunction((old) => {
      const current = document.querySelector(
        'textarea[data-ipp-native-text="true"]',
      );
      return (
        document.querySelector("#gui-shield")?.textContent === "lifted" &&
        current !== old &&
        current?.isConnected
      );
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

/** Play the panel's layer spacing to `spacing` from the sidebar. */
export async function toggleLayers(g: Gallery, exploded: boolean) {
  await g.page.locator("#gui-explode-toggle").click();
  await g.page.waitForFunction(
    (word) => document.querySelector("#gui-layers")?.textContent === word,
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
): Promise<GalleryGuiState> {
  const deadline = performance.now() + 15_000;
  let lastError: unknown;
  while (performance.now() < deadline) {
    try {
      const state = await g.call<GalleryGuiState>("galleryGuiState");
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
