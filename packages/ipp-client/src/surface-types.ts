import type { ClientAssetSource } from "./types.js";

/** Explicit glyph positions are metres relative to the item's baseline origin. */
export interface PositionedGlyph {
  glyphId: number;
  position: readonly [number, number];
  color?: readonly [number, number, number, number];
}

export type SurfaceContent =
  | { kind: "label"; text: string }
  | { kind: "glyphRun"; glyphs: readonly PositionedGlyph[] }
  | { kind: "drawing" }
  | { kind: "bitmap"; size: readonly [number, number] };

export interface SurfaceStyle {
  position?: readonly [number, number];
  scale?: readonly [number, number];
  /** Linear RGB and straight coverage alpha. */
  color?: readonly [number, number, number, number];
  opacity?: number;
  /** Metres per em; labels start on the item's baseline. */
  fontSize?: number;
  asset?: ClientAssetSource | null;
}

export interface SurfaceItem {
  id: number;
  content: SurfaceContent;
}

export interface SurfaceCollection {
  nextId: number;
  items: readonly SurfaceItem[];
}

export type SurfaceEdit =
  | {
      action: "insert";
      entity: bigint;
      id: number;
      index: number;
      content: SurfaceContent;
      style?: SurfaceStyle;
    }
  | {
      action: "update";
      entity: bigint;
      id: number;
      patch: SurfaceStyle & { content?: SurfaceContent };
    }
  | { action: "remove"; entity: bigint; id: number }
  | { action: "move"; entity: bigint; id: number; index: number };

export type SurfaceProperty =
  | "position"
  | "scale"
  | "color"
  | "opacity"
  | "font_size"
  | "asset";

/** Stable target name for ordinary property animation and StateOverlay commands. */
export function surfaceProperty(id: number, property: SurfaceProperty): string {
  if (!Number.isInteger(id) || id <= 0 || id > 0xffffffff)
    throw new RangeError("Surface item identity must be a nonzero u32");
  if (
    !["position", "scale", "color", "opacity", "font_size", "asset"].includes(
      property,
    )
  )
    throw new RangeError("Unknown Surface item property");
  return `item_${id}_${property}`;
}

/**
 * Map a 2D point in Surface content coordinates ([0, width] x [0, height], +X right, +Y down)
 * to centred entity-local 3D coordinates (+X right, +Y up, front +Z).
 */
export function surfaceContentToEntityLocal(
  contentPoint: readonly [number, number],
  surfaceSize: readonly [number, number],
): [number, number, number] {
  return [
    contentPoint[0] - surfaceSize[0] * 0.5,
    surfaceSize[1] * 0.5 - contentPoint[1],
    0.0,
  ];
}

/**
 * Map a 2D point in centred entity-local coordinates to 2D Surface content coordinates.
 */
export function entityLocalToSurfaceContent(
  entityPoint: readonly [number, number],
  surfaceSize: readonly [number, number],
): [number, number] {
  return [
    entityPoint[0] + surfaceSize[0] * 0.5,
    surfaceSize[1] * 0.5 - entityPoint[1],
  ];
}
