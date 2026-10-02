/** Top-left RGBA8 images: cropping, enlargement and side-by-side composition. */

/** A mutable top-left RGBA8 image. */
export interface RgbaImage {
  readonly width: number;
  readonly height: number;
  readonly pixels: Uint8Array<ArrayBuffer>;
}

/** A pixel rectangle: left, top, width, height. */
export type PixelRect = readonly [number, number, number, number];

export function blank(
  width: number,
  height: number,
  color: readonly [number, number, number, number] = [0, 0, 0, 255],
): RgbaImage {
  const pixels = new Uint8Array(width * height * 4);
  for (let index = 0; index < width * height; index++)
    pixels.set(color, index * 4);
  return { width, height, pixels };
}

/** Copy `rect` of `source` (clipped to it) into `target` at `[x, y]`. */
export function blit(
  target: RgbaImage,
  source: RgbaImage,
  rect: PixelRect,
  x: number,
  y: number,
): void {
  const [left, top, width, height] = rect;
  for (let row = 0; row < height; row++) {
    const sourceY = top + row;
    const targetY = y + row;
    if (sourceY < 0 || sourceY >= source.height) continue;
    if (targetY < 0 || targetY >= target.height) continue;
    for (let column = 0; column < width; column++) {
      const sourceX = left + column;
      const targetX = x + column;
      if (sourceX < 0 || sourceX >= source.width) continue;
      if (targetX < 0 || targetX >= target.width) continue;
      const from = (sourceY * source.width + sourceX) * 4;
      target.pixels.set(
        source.pixels.subarray(from, from + 4),
        (targetY * target.width + targetX) * 4,
      );
    }
  }
}

/** `rect` of `source`; area outside the source stays transparent black. */
export function crop(source: RgbaImage, rect: PixelRect): RgbaImage {
  const image = blank(rect[2], rect[3], [0, 0, 0, 0]);
  blit(image, source, rect, 0, 0);
  return image;
}

/** Nearest-neighbour enlargement by a whole factor, so pixels stay visible. */
export function enlarge(source: RgbaImage, factor: number): RgbaImage {
  if (!Number.isInteger(factor) || factor < 1)
    throw new Error("Enlargement factor must be a positive integer");
  const image = blank(source.width * factor, source.height * factor);
  for (let y = 0; y < image.height; y++)
    for (let x = 0; x < image.width; x++) {
      const from =
        (Math.floor(y / factor) * source.width + Math.floor(x / factor)) * 4;
      image.pixels.set(
        source.pixels.subarray(from, from + 4),
        (y * image.width + x) * 4,
      );
    }
  return image;
}

/** Equal-scale images left to right, separated by a gutter and top-aligned. */
export function sideBySide(
  images: readonly RgbaImage[],
  gutter = 8,
): RgbaImage {
  const width =
    images.reduce((total, image) => total + image.width, 0) +
    gutter * Math.max(0, images.length - 1);
  const height = Math.max(...images.map((image) => image.height));
  const composed = blank(width, height, [128, 128, 128, 255]);
  let x = 0;
  for (const image of images) {
    blit(composed, opaque(image), [0, 0, image.width, image.height], x, 0);
    x += image.width + gutter;
  }
  return composed;
}

/** Composite over black so transparent capture regions read as empty. */
function opaque(image: RgbaImage): RgbaImage {
  const pixels = Uint8Array.from(image.pixels);
  for (let index = 3; index < pixels.length; index += 4) {
    const alpha = pixels[index]! / 255;
    for (let channel = 1; channel <= 3; channel++)
      pixels[index - channel] = Math.round(pixels[index - channel]! * alpha);
    pixels[index] = 255;
  }
  return { width: image.width, height: image.height, pixels };
}
