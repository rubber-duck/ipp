import type {
  SurfaceCollection,
  SurfaceContent,
  SurfaceEdit,
  SurfaceStyle,
} from "./surface-types.js";
export * from "./surface-types.js";

function surfaceVector(
  w: Writer,
  values: readonly number[],
  length: number,
  unit = false,
): void {
  if (values.length !== length) fail("Surface vector length");
  for (const value of values) {
    if (unit && (value < 0 || value > 1)) fail("Surface colour range");
    w.f32(value);
  }
}

function writeSurfaceContent(w: Writer, content: SurfaceContent): void {
  switch (content.kind) {
    case "label":
      exactFields(content, ["kind", "text"]);
      w.u8(1);
      w.string(content.text);
      break;
    case "glyphRun":
      exactFields(content, ["kind", "glyphs"]);
      w.u8(2);
      w.count(content.glyphs.length, 65536);
      for (const glyph of content.glyphs) {
        exactFields(glyph, ["glyphId", "position", "color"]);
        w.u32(glyph.glyphId);
        surfaceVector(w, glyph.position, 2);
        w.boolean(glyph.color !== undefined);
        if (glyph.color !== undefined) surfaceVector(w, glyph.color, 4, true);
      }
      break;
    case "drawing":
      exactFields(content, ["kind"]);
      w.u8(3);
      break;
    case "bitmap":
      exactFields(content, ["kind", "size"]);
      if (content.size.some((v) => v <= 0)) fail("Surface bitmap size");
      w.u8(4);
      surfaceVector(w, content.size, 2);
      break;
    default:
      fail("Surface content kind");
  }
}

function readSurfaceContent(r: Reader): SurfaceContent {
  switch (r.u8()) {
    case 1:
      return { kind: "label", text: r.string() };
    case 2: {
      const count = r.count(65536);
      const glyphs: import("./surface-types.js").PositionedGlyph[] = [];
      for (let i = 0; i < count; i++) {
        const glyphId = r.u32();
        const position: [number, number] = [r.f32(), r.f32()];
        const color: [number, number, number, number] | undefined = r.boolean()
          ? [r.f32(), r.f32(), r.f32(), r.f32()]
          : undefined;
        glyphs.push({ glyphId, position, ...(color ? { color } : {}) });
      }
      return { kind: "glyphRun", glyphs };
    }
    case 3:
      return { kind: "drawing" };
    case 4:
      return { kind: "bitmap", size: [r.f32(), r.f32()] };
    default:
      return fail("Surface content kind");
  }
}

/** Encode typed item structure; style stays in named component properties. */
export function encodeSurfaceItems(
  collection: SurfaceCollection,
): Uint8Array<ArrayBuffer> {
  const w = new Writer(65536);
  w.u8(1);
  if (collection.nextId <= 0) fail("Surface next identity");
  w.u32(collection.nextId);
  w.count(collection.items.length, 65536);
  const seen = new Set<number>();
  for (const item of collection.items) {
    if (item.id <= 0 || item.id >= collection.nextId || seen.has(item.id))
      fail("Surface item identity");
    seen.add(item.id);
    w.u32(item.id);
    writeSurfaceContent(w, item.content);
  }
  return w.finish();
}

export function decodeSurfaceItems(bytes: Uint8Array): SurfaceCollection {
  const r = new Reader(bytes);
  if (r.u8() !== 1) fail("Surface items version");
  const nextId = r.u32();
  const count = r.count(65536);
  const items: import("./surface-types.js").SurfaceItem[] = [];
  for (let i = 0; i < count; i++)
    items.push({ id: r.u32(), content: readSurfaceContent(r) });
  r.done();
  const collection = { nextId, items };
  // Validate the same identity and content constraints used for authoring.
  encodeSurfaceItems(collection);
  return collection;
}

function writeSurfaceAsset(w: Writer, asset: SurfaceStyle["asset"]): void {
  w.boolean(asset != null);
  if (asset == null) return;
  if (asset.kind <= 0) fail("Surface asset type");
  w.u16(asset.kind);
  w.u32(asset.variant ?? 0);
  w.string(asset.source);
}

function writeSurfaceScale(w: Writer, scale: readonly number[]): void {
  if (scale.some((value) => value < 0)) fail("Surface scale");
  surfaceVector(w, scale, 2);
}

function writeSurfaceOpacity(w: Writer, opacity: number): void {
  if (opacity < 0 || opacity > 1) fail("Surface opacity");
  w.f32(opacity);
}

function writeSurfaceFontSize(w: Writer, fontSize: number): void {
  if (fontSize <= 0) fail("Surface font size");
  w.f32(fontSize);
}

function writeSurfaceStyle(w: Writer, style: SurfaceStyle): void {
  surfaceVector(w, style.position ?? [0, 0], 2);
  writeSurfaceScale(w, style.scale ?? [1, 1]);
  surfaceVector(w, style.color ?? [1, 1, 1, 1], 4, true);
  writeSurfaceOpacity(w, style.opacity ?? 1);
  writeSurfaceFontSize(w, style.fontSize ?? 0.1);
  writeSurfaceAsset(w, style.asset);
}

/** Versioned payload inside the correlated Surface request. */
export function encodeSurfaceEdit(edit: SurfaceEdit): Uint8Array<ArrayBuffer> {
  const w = new Writer(65536);
  w.u8(1);
  const actions = { insert: 1, update: 2, remove: 3, move: 4 };
  if (!Object.hasOwn(actions, edit.action)) fail("Surface edit action");
  w.u8(actions[edit.action]);
  if (edit.entity === 0n || edit.id <= 0) fail("Surface edit identity");
  w.u64(edit.entity);
  w.u32(edit.id);
  switch (edit.action) {
    case "insert":
      exactFields(edit, [
        "action",
        "entity",
        "id",
        "index",
        "content",
        "style",
      ]);
      w.u32(edit.index);
      writeSurfaceContent(w, edit.content);
      writeSurfaceStyle(w, edit.style ?? {});
      break;
    case "update": {
      exactFields(edit, ["action", "entity", "id", "patch"]);
      const p = edit.patch;
      const fields = [
        "content",
        "position",
        "scale",
        "color",
        "opacity",
        "fontSize",
        "asset",
      ] as const;
      exactFields(p, [...fields]);
      w.u8(
        fields.reduce(
          (mask, name, index) =>
            p[name] === undefined ? mask : mask | (1 << index),
          0,
        ),
      );
      if (p.content !== undefined) writeSurfaceContent(w, p.content);
      if (p.position !== undefined) surfaceVector(w, p.position, 2);
      if (p.scale !== undefined) writeSurfaceScale(w, p.scale);
      if (p.color !== undefined) surfaceVector(w, p.color, 4, true);
      if (p.opacity !== undefined) writeSurfaceOpacity(w, p.opacity);
      if (p.fontSize !== undefined) writeSurfaceFontSize(w, p.fontSize);
      if (p.asset !== undefined) writeSurfaceAsset(w, p.asset);
      break;
    }
    case "remove":
      exactFields(edit, ["action", "entity", "id"]);
      break;
    case "move":
      exactFields(edit, ["action", "entity", "id", "index"]);
      w.u32(edit.index);
      break;
  }
  return w.finish();
}
