/** Reusable IPPM v1 cube fixture shared by the browser harness and viewer. */

export const CUBE_VERTEX_COUNT = 24;
export const CUBE_INDEX_COUNT = 36;
export const CUBE_SOURCE_BYTES =
  16 +
  CUBE_VERTEX_COUNT * 6 * Float32Array.BYTES_PER_ELEMENT +
  CUBE_INDEX_COUNT * Uint16Array.BYTES_PER_ELEMENT;

type Position = readonly [number, number, number];
type Color = readonly [number, number, number];

interface Face {
  readonly color: Color;
  readonly corners: readonly [Position, Position, Position, Position];
}

// Corners wind counter-clockwise when viewed from outside. Per-face vertices
// keep the small fixture useful for later normal/material expansion.
const faces: readonly Face[] = [
  {
    color: [1, 0.22, 0.08],
    corners: [
      [-1, -1, 1],
      [1, -1, 1],
      [1, 1, 1],
      [-1, 1, 1],
    ],
  },
  {
    color: [0.12, 0.42, 1],
    corners: [
      [1, -1, -1],
      [-1, -1, -1],
      [-1, 1, -1],
      [1, 1, -1],
    ],
  },
  {
    color: [0.12, 1, 0.28],
    corners: [
      [1, -1, 1],
      [1, -1, -1],
      [1, 1, -1],
      [1, 1, 1],
    ],
  },
  {
    color: [0.8, 0.1, 1],
    corners: [
      [-1, -1, -1],
      [-1, -1, 1],
      [-1, 1, 1],
      [-1, 1, -1],
    ],
  },
  {
    color: [1, 0.86, 0.12],
    corners: [
      [-1, 1, 1],
      [1, 1, 1],
      [1, 1, -1],
      [-1, 1, -1],
    ],
  },
  {
    color: [0.12, 0.9, 1],
    corners: [
      [-1, -1, -1],
      [1, -1, -1],
      [1, -1, 1],
      [-1, -1, 1],
    ],
  },
];

/** Create a fresh transferable buffer. The caller gives up ownership on upload. */
export function createCubeMesh(): ArrayBuffer {
  const buffer = new ArrayBuffer(CUBE_SOURCE_BYTES);
  const view = new DataView(buffer);
  let offset = 0;

  for (const byte of [0x49, 0x50, 0x50, 0x4d]) {
    view.setUint8(offset, byte);
    offset += 1;
  }
  for (const value of [1, CUBE_VERTEX_COUNT, CUBE_INDEX_COUNT]) {
    view.setUint32(offset, value, true);
    offset += Uint32Array.BYTES_PER_ELEMENT;
  }

  for (const face of faces) {
    for (const position of face.corners) {
      for (const value of [...position, ...face.color]) {
        view.setFloat32(offset, value, true);
        offset += Float32Array.BYTES_PER_ELEMENT;
      }
    }
  }

  for (let face = 0; face < faces.length; face += 1) {
    const base = face * 4;
    for (const index of [base, base + 1, base + 2, base, base + 2, base + 3]) {
      view.setUint16(offset, index, true);
      offset += Uint16Array.BYTES_PER_ELEMENT;
    }
  }

  if (offset !== buffer.byteLength) {
    throw new Error(
      `cube fixture wrote ${offset} of ${buffer.byteLength} bytes`,
    );
  }
  return buffer;
}
