/**
 * Helpers the gallery GUI suites share: opening a settled dashboard, reading
 * its state, the oblique view and the exploded planes, and matching a
 * feature's plane by its projected depth in completed frames.
 */
import type { Inspection } from "@ipp/client";
import assert from "node:assert/strict";
import { GUI_KIT_LAYERS } from "@ipp/react/gui-kit";
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

/**
 * Mean absolute brightness difference between `patch` moved by `shift`
 * pixels and the pixels of `window` beneath it, at the best alignment within
 * `slack` pixels either way, which absorbs rounding of projected positions.
 */
export function patchDifference(
  patch: BrightnessWindow,
  window: BrightnessWindow,
  shift: { readonly x: number; readonly y: number },
  slack: number,
) {
  let best = Number.POSITIVE_INFINITY;
  for (let dy = -slack; dy <= slack; dy++)
    for (let dx = -slack; dx <= slack; dx++) {
      const offsetX = patch.left + Math.round(shift.x) + dx - window.left;
      const offsetY = patch.top + Math.round(shift.y) + dy - window.top;
      let sum = 0;
      let count = 0;
      for (let row = 0; row < patch.height; row++) {
        const y = row + offsetY;
        if (y < 0 || y >= window.height) continue;
        for (let column = 0; column < patch.width; column++) {
          const x = column + offsetX;
          if (x < 0 || x >= window.width) continue;
          sum += Math.abs(
            patch.values[row * patch.width + column]! -
              window.values[y * window.width + x]!,
          );
          count += 1;
        }
      }
      if (count >= (patch.width * patch.height) / 2)
        best = Math.min(best, sum / count);
    }
  return best;
}

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
  flat: { readonly label: string; readonly frame: CaptureSize },
  exploded: { readonly label: string; readonly frame: CaptureSize },
  features: Readonly<Record<string, PlaneFeature>>,
) {
  // Every plane the kit uses, and the one above them.
  const top = Math.max(...Object.values(GUI_KIT_LAYERS)) + 1;
  const candidates = Array.from({ length: top + 1 }, (_, plane) => plane);
  const slack = 2;
  const shifts: Record<string, unknown> = {};
  for (const [name, { at, plane, radius = 18 }] of Object.entries(features)) {
    const [base] = await projectContent(g, [at]);
    const patch = await brightness(g, flat.label, flat.frame, base!, radius);
    assert.ok(patch.ink > 0, `the flat ${name} feature has no ink`);
    const scores: { plane: number; shift: object; difference: number }[] = [];
    for (const candidate of candidates) {
      const [raised] = await projectContent(
        g,
        [at],
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
        radius + slack + 1,
      );
      scores.push({
        plane: candidate,
        shift,
        difference: patchDifference(patch, window, shift, slack),
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
      const { x, y } = scores[plane]!.shift as { x: number; y: number };
      assert.ok(
        Math.hypot(x, y) > 6 * plane,
        `${name}'s plane barely separates: ${JSON.stringify({ x, y })}`,
      );
    }
  }
  return shifts;
}

export type CaptureSize = { readonly width: number; readonly height: number };

/**
 * Turn the panel to an oblique view: facing the camera, then 50 degrees
 * about its vertical axis, so planes along its normal separate on screen.
 * The caller releases the override.
 */
export async function obliquePanel(g: Gallery) {
  await g.page.mouse.move(1, 1);
  await g.call("faceGalleryGuiToCamera", 0.7);
  const faced = fieldsWith(await g.inspect(), PANEL_ENTITY, "qx");
  const turn = (50 * Math.PI) / 180;
  const [qx, qy, qz, qw] = multiply(
    ["qx", "qy", "qz", "qw"].map((key) => Number(faced[key])),
    [0, Math.sin(turn / 2), 0, Math.cos(turn / 2)],
  );
  // A replacing override restores every field it leaves out, so it names
  // the faced position too.
  await g.call(
    "overrideGalleryGuiTransform",
    {
      x: Number(faced.x),
      y: Number(faced.y),
      z: Number(faced.z),
      qx: qx!,
      qy: qy!,
      qz: qz!,
      qw: qw!,
    },
    true,
  );
}

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

/**
 * Quaternion product `left * right`, `[x, y, z, w]`: `right`'s rotation
 * applied in `left`'s frame.
 */
export function multiply(left: readonly number[], right: readonly number[]) {
  const [ax, ay, az, aw] = left as [number, number, number, number];
  const [bx, by, bz, bw] = right as [number, number, number, number];
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
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
