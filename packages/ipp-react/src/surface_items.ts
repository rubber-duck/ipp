import {
  surfaceProperty,
  type DynamicValue,
  type SurfaceCollection,
} from "@ipp/client";
import type { SurfaceItemProps } from "./components.js";

type SurfaceContent = SurfaceItemProps["content"];

function equalNumberTuple(a: readonly number[], b: readonly number[]): boolean {
  return (
    a.length === b.length &&
    a.every((value, index) => Object.is(value, b[index]))
  );
}

function equalContent(a: SurfaceContent, b: SurfaceContent): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "drawing" && b.kind === "drawing") return true;
  if (a.kind === "label" && b.kind === "label") return a.text === b.text;
  if (a.kind === "bitmap" && b.kind === "bitmap")
    return equalNumberTuple(a.size, b.size);
  if (a.kind !== "glyphRun" || b.kind !== "glyphRun") return false;
  return (
    a.glyphs.length === b.glyphs.length &&
    a.glyphs.every((glyph, index) => {
      const other = b.glyphs[index]!;
      return (
        glyph.glyphId === other.glyphId &&
        equalNumberTuple(glyph.position, other.position) &&
        (glyph.color === other.color ||
          (glyph.color !== undefined &&
            other.color !== undefined &&
            equalNumberTuple(glyph.color, other.color)))
      );
    })
  );
}

function snapshotContent(content: SurfaceContent): SurfaceContent {
  switch (content.kind) {
    case "drawing":
      return { kind: "drawing" };
    case "label":
      return { kind: "label", text: content.text };
    case "bitmap":
      return { kind: "bitmap", size: [...content.size] as [number, number] };
    case "glyphRun":
      return {
        kind: "glyphRun",
        glyphs: content.glyphs.map((glyph) => ({
          glyphId: glyph.glyphId,
          position: [...glyph.position] as [number, number],
          ...(glyph.color
            ? { color: [...glyph.color] as [number, number, number, number] }
            : {}),
        })),
      };
  }
}

/** Authoring identities belong to one mounted declaration, independently of order. */
export class SurfaceItemDeclarations {
  private nextId = 1;
  private ids = new Map<string, number>();
  private encoded:
    | {
        items: readonly { id: number; content: SurfaceContent }[];
        bytes: Uint8Array<ArrayBuffer>;
      }
    | undefined;

  describe(
    items: readonly SurfaceItemProps[],
    encode: (collection: SurfaceCollection) => Uint8Array<ArrayBuffer>,
  ) {
    const ids = new Map<string, number>();
    let nextId = this.nextId;
    const properties: Record<string, DynamicValue> = {};
    const collection = items.map((item) => {
      if (typeof item.key !== "string" || !item.key || ids.has(item.key))
        throw new Error("Surface items require unique nonempty keys");
      const id = this.ids.get(item.key) ?? nextId++;
      ids.set(item.key, id);
      properties[surfaceProperty(id, "position")] = {
        kind: "vec2",
        value: item.position ?? [0, 0],
      };
      properties[surfaceProperty(id, "scale")] = {
        kind: "vec2",
        value: item.scale ?? [1, 1],
      };
      properties[surfaceProperty(id, "color")] = {
        kind: "vec4",
        value: item.color ?? [1, 1, 1, 1],
      };
      properties[surfaceProperty(id, "opacity")] = {
        kind: "f32",
        value: item.opacity ?? 1,
      };
      properties[surfaceProperty(id, "font_size")] = {
        kind: "f32",
        value: item.fontSize ?? 0.1,
      };
      if (item.asset != null)
        properties[surfaceProperty(id, "asset")] = {
          kind: "asset",
          value: item.asset,
        };
      return { id, content: item.content };
    });
    const unchanged =
      this.encoded?.items.length === collection.length &&
      collection.every((item, index) => {
        const previous = this.encoded!.items[index]!;
        return (
          item.id === previous.id &&
          equalContent(item.content, previous.content)
        );
      });
    const bytes = unchanged
      ? this.encoded!.bytes
      : encode({ nextId, items: collection }).slice();
    this.ids = ids;
    this.nextId = nextId;
    if (!unchanged)
      this.encoded = {
        items: collection.map(({ id, content }) => ({
          id,
          content: snapshotContent(content),
        })),
        bytes,
      };
    return { items: bytes, ...properties };
  }
}
