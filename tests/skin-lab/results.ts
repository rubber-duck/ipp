/** Capture images of a specimen: the composite, its reference side-by-side and state zooms. */
import {
  crop,
  enlarge,
  sideBySide,
  type PixelRect,
  type RgbaImage,
} from "../../tools/shared-host/images.js";
import type { SpecimenCapture } from "./specimen-session.js";
import { CAPTURE_SCALE, type SkinSpecimen } from "./specimen.js";

export interface ImageOptions {
  /** The decoded reference crop, when it exists locally. */
  readonly reference?: RgbaImage;
  /** States to compose as enlarged reference/capture pairs; `all` selects every captured state. */
  readonly zoom?: readonly string[];
  readonly zoomFactor?: number;
  /** Also return each state's whole capture. */
  readonly raw?: boolean;
}

/** The reference rectangle matching a rectangle of capture pixels. */
function referenceRect(
  specimen: SkinSpecimen,
  capture: PixelRect,
): PixelRect | undefined {
  const reference = specimen.reference;
  if (!reference) return undefined;
  const ratio = (reference.scale ?? CAPTURE_SCALE) / CAPTURE_SCALE;
  return [
    Math.round(reference.origin[0] + capture[0] * ratio),
    Math.round(reference.origin[1] + capture[1] * ratio),
    Math.round(capture[2] * ratio),
    Math.round(capture[3] * ratio),
  ];
}

/**
 * Images keyed by output name: `<name>` (composite), `<name>.compare`
 * (reference region left, capture right, same scale), `<name>.<state>.zoom`
 * and `<name>.<state>.raw`.
 */
export function captureImages(
  name: string,
  specimen: SkinSpecimen,
  capture: SpecimenCapture,
  options: ImageOptions,
): Record<string, RgbaImage> {
  const images: Record<string, RgbaImage> = { [name]: capture.image };
  const { reference } = options;
  if (reference) {
    const whole = referenceRect(specimen, [
      0,
      0,
      capture.image.width,
      capture.image.height,
    ])!;
    images[`${name}.compare`] = sideBySide([
      crop(reference, whole),
      capture.image,
    ]);
  }
  const zoom = options.zoom?.includes("all")
    ? capture.states.map((state) => state.name)
    : (options.zoom ?? []);
  for (const state of zoom) {
    const captured = capture.states.find((entry) => entry.name === state);
    if (!captured) throw new Error(`State ${state} was not captured`);
    const pair = [crop(capture.image, captured.cell)];
    const rect = referenceRect(specimen, captured.cell);
    if (reference && rect) pair.unshift(crop(reference, rect));
    images[`${name}.${state}.zoom`] = sideBySide(
      pair.map((picture) => enlarge(picture, options.zoomFactor ?? 4)),
    );
  }
  if (options.raw)
    for (const state of capture.states)
      images[`${name}.${state.name}.raw`] = state.image;
  return images;
}
