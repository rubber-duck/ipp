/** Image probes independent of the glass shader and native theme row values. */
import type { RgbaImage } from "../../../tools/shared-host/images.js";

/** A control's bounds in capture pixels, without its outer halo. */
export type ActionRect = readonly [number, number, number, number];

export interface ActionImageProbe {
  readonly name: string;
  readonly passed: boolean;
  readonly detail: string;
}

function luminance(image: RgbaImage, rect: ActionRect): number {
  const [x, y, width, height] = rect;
  const left = Math.max(0, Math.ceil(x));
  const top = Math.max(0, Math.ceil(y));
  const right = Math.min(image.width, Math.ceil(x + width));
  const bottom = Math.min(image.height, Math.ceil(y + height));
  if (right <= left || bottom <= top)
    throw new Error(`Empty action probe region ${rect}`);
  let total = 0;
  for (let row = top; row < bottom; row++) {
    for (let column = left; column < right; column++) {
      const offset = (row * image.width + column) * 4;
      total +=
        image.pixels[offset]! * 0.2126 +
        image.pixels[offset + 1]! * 0.7152 +
        image.pixels[offset + 2]! * 0.0722;
    }
  }
  return total / ((right - left) * (bottom - top));
}

/** Avoid the centred label and polished edges. */
function face([x, y, width, height]: ActionRect): ActionRect {
  return [x + width * 0.35, y + height * 0.2, width * 0.3, height * 0.12];
}

/** Middle of the horizontal rim, clear of the rounded corners. */
function rim([x, y, width, height]: ActionRect): ActionRect {
  return [x + width * 0.35, y, width * 0.3, Math.max(1, height * 0.04)];
}

function probe(
  name: string,
  passed: boolean,
  values: Readonly<Record<string, number>>,
): ActionImageProbe {
  return { name, passed, detail: JSON.stringify(values) };
}

function brightestLabelPixel(
  image: RgbaImage,
  [x, y, width, height]: ActionRect,
): number {
  let brightest = 0;
  for (let py = Math.floor(y + height * 0.3); py < y + height * 0.7; py++)
    for (let px = Math.floor(x + width * 0.35); px < x + width * 0.65; px++) {
      const i = (py * image.width + px) * 4;
      brightest = Math.max(
        brightest,
        image.pixels[i]! * 0.2126 +
          image.pixels[i + 1]! * 0.7152 +
          image.pixels[i + 2]! * 0.0722,
      );
    }
  return brightest;
}

/**
 * Compare the same actual control across native states. Regions derive from
 * captured bounds, so resizing or device scale cannot silently move probes.
 * The broad tolerances allow GLES/WebGL antialiasing differences while
 * rejecting flat bright hover fills and missing native state response.
 */
export function probeGlassButtonStates(
  images: {
    readonly idle: RgbaImage;
    readonly hover: RgbaImage;
    readonly pressed: RgbaImage;
    readonly disabled: RgbaImage;
    readonly selectedDisabled: RgbaImage;
  },
  bounds: ActionRect,
): readonly ActionImageProbe[] {
  const hoverFace = luminance(images.hover, face(bounds));
  const hoverRim = luminance(images.hover, rim(bounds));
  const idleRim = luminance(images.idle, rim(bounds));
  const pressedFace = luminance(images.pressed, face(bounds));
  const disabledRim = luminance(images.disabled, rim(bounds));
  const pressedLabel = brightestLabelPixel(images.pressed, bounds);
  const disabledFace = luminance(images.disabled, face(bounds));
  const selectedDisabledFace = luminance(images.selectedDisabled, face(bounds));
  return [
    probe(
      "disabled button retains its selected value",
      selectedDisabledFace > disabledFace + 8,
      { selectedDisabledFace, disabledFace, pressedFace },
    ),
    probe("pressed label retains contrast", pressedLabel > pressedFace + 40, {
      pressedLabel,
      pressedFace,
    }),
    probe("hover retains a dark centre", hoverRim > hoverFace + 12, {
      hoverFace,
      hoverRim,
    }),
    probe("native hover lights the rim", hoverRim > idleRim + 3, {
      hoverRim,
      idleRim,
    }),
    probe(
      "native press subdues the face reflection",
      pressedFace < hoverFace - 1,
      {
        pressedFace,
        hoverFace,
      },
    ),
    probe("disabled rim is restrained", disabledRim < hoverRim - 3, {
      disabledRim,
      hoverRim,
    }),
  ];
}

/** Actual selected text adds visible coverage inside the text band. */
export function probeGlassSelection(
  collapsed: RgbaImage,
  selected: RgbaImage,
  band: ActionRect,
): ActionImageProbe {
  const before = luminance(collapsed, band);
  const after = luminance(selected, band);
  return probe("native selection paints the text band", after > before + 2, {
    before,
    after,
  });
}

/** The approved liquid finish has a narrow bevel and a restrained directional
 * reflection inside a quiet translucent body. Independent pixel regions avoid the
 * label and reject both the old flat material and an indiscriminate glow fill.
 */
export function probeGlassOpticalFinish(
  image: RgbaImage,
  [x, y, width, height]: ActionRect,
): readonly ActionImageProbe[] {
  const sample = (u: number, v: number, w = 0.025, h = 0.05) =>
    luminance(image, [x + width * u, y + height * v, width * w, height * h]);
  const faceSamples = Array.from({ length: 16 }, (_, i) =>
    sample(0.1 + i * 0.05, 0.18),
  );
  const reflection = Math.max(...faceSamples);
  const quietFace = Math.min(...faceSamples);
  // The filtered bevel occupies the first interior pixel row. A proportional
  // inset can skip that row and measure the darker shoulder instead.
  const bevel = sample(0.35, 0, 0.3, 1 / height);
  const outside = sample(0.35, -0.1, 0.3, 0.04);
  return [
    probe(
      "directional reflection stays subtle inside glass",
      reflection > quietFace + 2 && reflection < quietFace + 12,
      { reflection, quietFace },
    ),
    probe("bevel stays distinct from the smoked face", bevel > quietFace + 40, {
      bevel,
      quietFace,
    }),
    probe("glass face remains dark enough for light labels", quietFace < 85, {
      quietFace,
    }),
    probe(
      "edge halo stays close to the contour",
      outside < 45 && outside < bevel * 0.35,
      { outside, bevel },
    ),
  ];
}
