/** Lossless 8-bit RGBA PNG output and 8-bit PNG input for captures and references. */
import { deflateSync, inflateSync } from "node:zlib";
import type { RgbaImage } from "./images.js";

const SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

const CRC_TABLE = Uint32Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit++)
    value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});

function chunk(type: string, data: Uint8Array): Buffer {
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  let crc = 0xffffffff;
  for (const byte of body) crc = CRC_TABLE[(crc ^ byte) & 0xff]! ^ (crc >>> 8);
  const frame = Buffer.alloc(body.length + 8);
  frame.writeUInt32BE(data.length, 0);
  body.copy(frame, 4);
  frame.writeUInt32BE((crc ^ 0xffffffff) >>> 0, body.length + 4);
  return frame;
}

/** An RGBA PNG with up-filtered scanlines, which compress flat GUI paint well. */
export function encodePng(image: RgbaImage): Buffer {
  const { width, height, pixels } = image;
  if (pixels.length !== width * height * 4)
    throw new Error(
      `RGBA image has ${pixels.length} bytes for ${width}x${height}`,
    );
  const stride = width * 4;
  const scanlines = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    const row = y * (stride + 1);
    scanlines[row] = y ? 2 : 0;
    for (let x = 0; x < stride; x++) {
      const value = pixels[y * stride + x]!;
      scanlines[row + 1 + x] = y
        ? (value - pixels[(y - 1) * stride + x]!) & 255
        : value;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 6, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from(SIGNATURE),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(scanlines, { level: 6 })),
    chunk("IEND", new Uint8Array()),
  ]);
}

/** Decode an 8-bit, non-interlaced greyscale, RGB, palette or RGBA PNG. */
export function decodePng(bytes: Uint8Array): RgbaImage {
  if (SIGNATURE.some((value, index) => bytes[index] !== value))
    throw new Error("Not a PNG file");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let offset = 8;
  let width = 0;
  let height = 0;
  let colorType = -1;
  let palette: Uint8Array | undefined;
  let transparency: Uint8Array | undefined;
  const data: Uint8Array[] = [];
  while (offset < bytes.length) {
    const length = view.getUint32(offset);
    const type = String.fromCharCode(...bytes.subarray(offset + 4, offset + 8));
    const body = bytes.subarray(offset + 8, offset + 8 + length);
    offset += 12 + length;
    if (type === "IHDR") {
      width = view.getUint32(body.byteOffset - bytes.byteOffset);
      height = view.getUint32(body.byteOffset - bytes.byteOffset + 4);
      const [depth, color, , , interlace] = body.subarray(8, 13);
      if (depth !== 8 || interlace !== 0)
        throw new Error(
          `Unsupported PNG: bit depth ${depth}, interlace ${interlace}`,
        );
      colorType = color!;
    } else if (type === "PLTE") palette = body;
    else if (type === "tRNS") transparency = body;
    else if (type === "IDAT") data.push(body);
    else if (type === "IEND") break;
  }
  const channels = { 0: 1, 2: 3, 3: 1, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`Unsupported PNG colour type ${colorType}`);
  const raw = inflateSync(Buffer.concat(data));
  const stride = width * channels;
  if (raw.length !== (stride + 1) * height)
    throw new Error("Truncated PNG image data");
  const rows = new Uint8Array(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)]!;
    const source = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const row = rows.subarray(y * stride, (y + 1) * stride);
    const above = y ? rows.subarray((y - 1) * stride, y * stride) : undefined;
    for (let x = 0; x < stride; x++) {
      const left = x >= channels ? row[x - channels]! : 0;
      const up = above ? above[x]! : 0;
      const corner = above && x >= channels ? above[x - channels]! : 0;
      let predictor = 0;
      if (filter === 1) predictor = left;
      else if (filter === 2) predictor = up;
      else if (filter === 3) predictor = (left + up) >> 1;
      else if (filter === 4) {
        const estimate = left + up - corner;
        const toLeft = Math.abs(estimate - left);
        const toUp = Math.abs(estimate - up);
        const toCorner = Math.abs(estimate - corner);
        predictor =
          toLeft <= toUp && toLeft <= toCorner
            ? left
            : toUp <= toCorner
              ? up
              : corner;
      } else if (filter !== 0) throw new Error(`Invalid PNG filter ${filter}`);
      row[x] = (source[x]! + predictor) & 255;
    }
  }
  const pixels = new Uint8Array(width * height * 4);
  for (let index = 0; index < width * height; index++) {
    const at = index * channels;
    let rgba: [number, number, number, number];
    if (colorType === 0) rgba = [rows[at]!, rows[at]!, rows[at]!, 255];
    else if (colorType === 4)
      rgba = [rows[at]!, rows[at]!, rows[at]!, rows[at + 1]!];
    else if (colorType === 2)
      rgba = [rows[at]!, rows[at + 1]!, rows[at + 2]!, 255];
    else if (colorType === 6)
      rgba = [rows[at]!, rows[at + 1]!, rows[at + 2]!, rows[at + 3]!];
    else {
      const entry = rows[at]!;
      if (!palette || entry * 3 + 2 >= palette.length)
        throw new Error("PNG palette index out of range");
      rgba = [
        palette[entry * 3]!,
        palette[entry * 3 + 1]!,
        palette[entry * 3 + 2]!,
        transparency?.[entry] ?? 255,
      ];
    }
    pixels.set(rgba, index * 4);
  }
  return { width, height, pixels };
}
