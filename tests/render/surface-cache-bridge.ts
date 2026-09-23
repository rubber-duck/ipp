/**
 * Device-level WebGL oracle for whole-Surface cache targets, run in the page
 * against a distribution's shipped `webgl.js` bridge. It mirrors the native
 * GLES probe (`crates/ipp-render-gl/examples/smoke/surface_cache_target.rs`):
 * bounded creation, resizing, nesting rules, a translucent box repainted into
 * the target (with glyph atlas population nested inside in GUI builds), front
 * and mirrored premultiplied composites over an opaque background, then handle
 * lifetimes across deletion, context loss and restoration. GUI builds also
 * check that a retained box repainted into a small target sizes its
 * antialiasing geometry from the target, not the drawing buffer. Expected
 * pixels follow from linear premultiplied blending, independently of the bridge.
 */

type BridgeImport = (...values: number[]) => number;

interface WebGlBridge {
  imports: Record<string, BridgeImport | undefined>;
  setMemory(memory: WebAssembly.Memory): void;
  capture(): Uint8Array<ArrayBuffer>;
  isContextLost(): boolean;
  dispose(): void;
  loseContext(): void;
  restoreContext(): void;
  info(): Record<string, unknown>;
}

const WIDTH = 320;
const HEIGHT = 240;
const SHADERS = "/crates/ipp-render-gl/src/services/render/shaders";

/** Content [0, 2] x [0, 1] metres to clip space, texture row zero at the top. */
const CONTENT = [1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1, 0, -1, -1, 0, 1];
/** The content quad over pixels x 32..288 and rows 72..168, and its mirror. */
const FRONT = [0.8, 0, 0, 0, 0, -0.8, 0, 0, 0, 0, 1, 0, -0.8, 0.4, 0, 1];
const MIRRORED = [-0.8, 0, 0, 0, 0, -0.8, 0, 0, 0, 0, 1, 0, 0.8, 0.4, 0, 1];
/** The content quad over pixels x 80..240 and rows 80..160: 5 pixels per texel of a 32 x 16 target. */
const MAGNIFIED = [
  0.5,
  0,
  0,
  0,
  0,
  -2 / 3,
  0,
  0,
  0,
  0,
  1,
  0,
  -0.5,
  1 / 3,
  0,
  1,
];

/**
 * `GuiBoxVertex` words: position, placement, shape, fill start and end,
 * border, gradient, material and glow lanes, as `retained_vertices.rs` lays
 * them out for the bridge.
 */
const GUI_BOX_LANES = [2, 4, 4, 4, 4, 4, 4, 4, 4];
/** `GUI_BOX_ANTIALIAS_PAD`: exterior margin generated boxes already carry. */
const GUI_BOX_PAD = 0.002;

/** Six vertices of one opaque box over `[x0, x1] x [y0, y1]` Surface metres. */
function guiBox(
  [x0, y0, x1, y1]: readonly [number, number, number, number],
  color: readonly [number, number, number, number],
) {
  const placement = [x0, y0, x1 - x0, y1 - y0];
  const vertex = (x: number, y: number) => [
    x,
    y,
    ...placement,
    ...[0, 0, 0, 0],
    ...color,
    ...color,
    ...[0, 0, 0, 0],
    ...[0, 0, 0, 0],
    ...[0, 0, 0, 0],
    ...[0, 0, 0, 0],
  ];
  const [left, top, right, bottom] = [
    x0 - GUI_BOX_PAD,
    y0 - GUI_BOX_PAD,
    x1 + GUI_BOX_PAD,
    y1 + GUI_BOX_PAD,
  ];
  return [
    vertex(left, top),
    vertex(left, bottom),
    vertex(right, bottom),
    vertex(left, top),
    vertex(right, bottom),
    vertex(right, top),
  ].flat();
}

async function frames(count: number) {
  for (let frame = 0; frame < count; frame++)
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
}

export async function probeSurfaceCacheBridge(bridgeUrl: string, gui: boolean) {
  const { createWebGlDevice } = (await import(bridgeUrl)) as {
    createWebGlDevice(canvas: OffscreenCanvas): WebGlBridge;
  };
  const device = createWebGlDevice(new OffscreenCanvas(WIDTH, HEIGHT));
  const memory = new WebAssembly.Memory({ initial: 16 });
  device.setMemory(memory);
  let top = 1024;
  const allocate = (bytes: number) => {
    const at = (top + 15) & ~15;
    top = at + bytes;
    return at;
  };
  const floats = (values: readonly number[]) => {
    const at = allocate(values.length * 4);
    new Float32Array(memory.buffer, at, values.length).set(values);
    return at;
  };
  const bytes = (values: Uint8Array) => {
    const at = allocate(values.length);
    new Uint8Array(memory.buffer, at, values.length).set(values);
    return [at, values.length] as const;
  };
  const call = (name: string, ...values: number[]) => {
    const operation = device.imports[name];
    if (typeof operation !== "function")
      throw new Error(`Bridge lacks Surface cache import ${name}`);
    return operation(...values);
  };
  const message = () => {
    const at = allocate(512);
    const length = call("error_message", at, 512);
    return new TextDecoder().decode(new Uint8Array(memory.buffer, at, length));
  };
  const ok = (name: string, ...values: number[]) => {
    const result = call(name, ...values);
    if (result === 0) throw new Error(`${name} failed: ${message()}`);
    return result;
  };
  const rejected = (label: string, name: string, ...values: number[]) => {
    if (call(name, ...values) !== 0)
      throw new Error(`${label} was accepted by the bridge`);
    return message();
  };
  const live = () => device.info().surfaceCacheTargetsLive as number;
  const source = async (name: string) => {
    const response = await fetch(`${SHADERS}/${name}`);
    if (!response.ok) throw new Error(`Missing shader ${name}`);
    return bytes(new TextEncoder().encode(await response.text()));
  };
  const [bitmapVertex, bitmapFragment, cacheFragment, presentVertex, present] =
    await Promise.all(
      [
        "surface_bitmap.vert",
        "surface_bitmap.frag",
        "surface_cache.frag",
        "present.vert",
        "present.frag",
      ].map(source),
    );
  const words = (values: readonly number[]) => {
    const at = allocate(values.length * 4);
    new Uint32Array(memory.buffer, at, values.length).set(values);
    return at;
  };
  const pixel = (pixels: Uint8Array, x: number, y: number) => [
    ...pixels.subarray((y * WIDTH + x) * 4, (y * WIDTH + x) * 4 + 4),
  ];
  const report: Record<string, unknown> = {};

  try {
    const limit = call("surface_cache_limit");
    if (!(limit >= 64)) throw new Error(`Surface cache limit ${limit}`);
    report.limit = limit;
    report.rejectedSizes = [
      rejected("a zero-width target", "create_surface_cache_target", 0, 8),
      rejected(
        "a target beyond the limit",
        "create_surface_cache_target",
        limit + 1,
        8,
      ),
    ];
    if (live() !== 0) throw new Error("Rejected targets stayed live");

    const program = ok("create_program", ...bitmapVertex!, ...cacheFragment!);
    const bitmapProgram = ok(
      "create_program",
      ...bitmapVertex!,
      ...bitmapFragment!,
    );
    const white = ok(
      "create_texture",
      1,
      1,
      ...bytes(new Uint8Array([255, 255, 255, 255])),
    );
    const target = ok("create_surface_cache_target", 16, 8);
    ok("resize_surface_cache_target", target, 64, 32);
    if (live() !== 1) throw new Error(`Live targets ${live()} after create`);

    const content = floats(CONTENT);
    const size = floats([2, 1]);
    ok("begin_surface_cache_target", target);
    report.nested = rejected(
      "a nested cache target",
      "begin_surface_cache_target",
      target,
    );
    report.boundSample = rejected(
      "sampling the bound target",
      "draw_surface_cache",
      program,
      target,
      content,
      size,
    );
    report.boundResize = rejected(
      "resizing the bound target",
      "resize_surface_cache_target",
      target,
      32,
      16,
    );
    if (gui) {
      // Atlas population nested inside a repaint returns to the cache target.
      const page = ok("create_glyph_atlas_page", 256, 256);
      ok("begin_glyph_atlas_page", page);
      report.insideAtlas = rejected(
        "a cache target inside atlas population",
        "begin_surface_cache_target",
        target,
      );
      ok("end_glyph_atlas_page");
      call("delete_glyph_atlas_page", page);
    }
    ok("set_surface_double_sided", 1);
    ok(
      "draw_surface_bitmap",
      bitmapProgram,
      white,
      content,
      floats([0.25, 0.25, 0.75, 0.5]),
      floats([0, 0, 2, 1]),
      floats([1, 0, 0, 0.5]),
    );
    ok("set_surface_double_sided", 0);
    ok("end_surface_cache_target");

    const captures: Uint8Array[] = [];
    for (const mvp of [FRONT, MIRRORED]) {
      ok(
        "begin_frame",
        WIDTH,
        HEIGHT,
        floats([0, 0, 1, 1]),
        ...presentVertex!,
        ...present!,
      );
      ok("set_surface_double_sided", 1);
      ok("draw_surface_cache", program, target, floats(mvp), size);
      ok("set_surface_double_sided", 0);
      ok("end_frame", 1);
      captures.push(device.capture());
    }
    // Half-opaque red over blue composes to the linear midpoint, sRGB 188;
    // applying opacity twice would leave red near 137.
    const blended = ([r, g, b]: number[]) =>
      r! >= 180 && r! <= 196 && g! <= 8 && b! >= 180 && b! <= 196;
    const background = ([r, g, b]: number[]) => r! <= 8 && g! <= 8 && b! >= 247;
    const samples: Record<string, number[][]> = {};
    for (const [name, pixels, painted, clear] of [
      ["front", captures[0]!, 112, 208],
      ["mirrored", captures[1]!, 208, 112],
    ] as const) {
      const values = [
        pixel(pixels, painted, 120),
        pixel(pixels, clear, 120),
        pixel(pixels, 10, 120),
      ];
      samples[name] = values;
      if (
        !blended(values[0]!) ||
        !background(values[1]!) ||
        !background(values[2]!)
      )
        throw new Error(
          `${name} Surface cache composite mismatch: ${JSON.stringify(values)}`,
        );
    }
    // Filtering across the box edge moves monotonically toward the background.
    const edge = Array.from({ length: 21 }, (_, index) =>
      pixel(captures[0]!, 150 + index, 120),
    );
    if (
      edge.some(
        (value, index) =>
          index > 0 &&
          (value[0]! > edge[index - 1]![0]! ||
            value[2]! < edge[index - 1]![2]!),
      )
    )
      throw new Error(
        `Surface cache edge is not monotonic: ${JSON.stringify(edge)}`,
      );
    report.samples = samples;
    report.edge = edge;
    if (gui) report.antialiasViewport = await antialiasViewport();

    // Deleting releases the handle; it is never reused.
    call("delete_surface_cache_target", target);
    if (live() !== 0) throw new Error("Deleted target stayed live");
    report.staleBegin = rejected(
      "a deleted target",
      "begin_surface_cache_target",
      target,
    );

    // Context loss clears every target and the bound state; restoration starts
    // from an empty map and later handles never repeat an earlier one.
    const lost = ok("create_surface_cache_target", 32, 16);
    ok("begin_surface_cache_target", lost);
    device.loseContext();
    for (let attempt = 0; attempt < 120 && !device.isContextLost(); attempt++)
      await frames(1);
    if (!device.isContextLost()) throw new Error("Context was not lost");
    // Restoration is valid only after the loss event has been dispatched.
    await frames(2);
    // info() requires a live context; the lost bridge refuses cache work.
    report.whileLost = {
      limit: call("surface_cache_limit"),
      create: call("create_surface_cache_target", 16, 16),
    };
    if (
      call("surface_cache_limit") !== 0 ||
      call("create_surface_cache_target", 16, 16) !== 0
    )
      throw new Error("A lost context accepted cache work");
    device.restoreContext();
    for (let attempt = 0; attempt < 120 && device.isContextLost(); attempt++)
      await frames(1);
    if (device.isContextLost()) throw new Error("Context was not restored");
    if (live() !== 0) throw new Error("Restoration kept cache targets");
    const restored = ok("create_surface_cache_target", 16, 16);
    if (restored <= lost)
      throw new Error(`Restored handle ${restored} reused ${lost}`);
    ok("begin_surface_cache_target", restored);
    ok("end_surface_cache_target");
    call("delete_surface_cache_target", restored);
    call("delete_surface_cache_target", lost);
    report.handles = { first: target, lost, restored, live: live() };
    if (live() !== 0) throw new Error("Restored target stayed live");
    return report;
  } finally {
    device.dispose();
  }

  /**
   * Repaint an opaque red GUI box into a 32 x 16 target spanning 2 x 1 metres
   * (16 texels per metre) and composite it at 5 pixels per texel. The box's
   * right contour lies 0.3 texel before the centre of texel column 24. With
   * the target viewport, the vertex shader extends coverage geometry 1.5
   * texels past the contour, so that texel keeps its 0.2 antialiased
   * coverage. Sized from the 320 x 240 drawing buffer, the extension would be
   * 0.15 texel and the texel would stay empty.
   */
  async function antialiasViewport() {
    const [boxVertex, boxFragment] = await Promise.all(
      ["surface_box.vert", "surface_box.frag"].map(source),
    );
    const boxProgram = ok("create_program", ...boxVertex!, ...boxFragment!);
    const compositor = ok(
      "create_program",
      ...bitmapVertex!,
      ...cacheFragment!,
    );
    const vertices = guiBox([0.5, 0.25, 24.2 / 16, 0.75], [1, 0, 0, 1]);
    let offset = 0;
    const layout = [136, GUI_BOX_LANES.length];
    GUI_BOX_LANES.forEach((components, location) => {
      layout.push(location, components, offset);
      offset += components * 4;
    });
    const batch = ok(
      "create_gui_batch",
      floats(vertices),
      vertices.length * 4,
      words(layout),
    );
    const small = ok("create_surface_cache_target", 32, 16);
    ok("begin_surface_cache_target", small);
    ok("set_surface_double_sided", 1);
    ok(
      "draw_gui_batch",
      boxProgram,
      batch,
      floats(CONTENT),
      floats([0, 0, 2, 1]),
    );
    ok("set_surface_double_sided", 0);
    ok("end_surface_cache_target");
    ok(
      "begin_frame",
      WIDTH,
      HEIGHT,
      floats([0, 0, 1, 1]),
      ...presentVertex!,
      ...present!,
    );
    ok("set_surface_double_sided", 1);
    ok(
      "draw_surface_cache",
      compositor,
      small,
      floats(MAGNIFIED),
      floats([2, 1]),
    );
    ok("set_surface_double_sided", 0);
    ok("end_frame", 1);
    const pixels = device.capture();
    // Texel column c is centred on pixel 82 + 5c; row 8 on pixel row 122.
    const columns = [23, 24, 25].map((column) =>
      pixel(pixels, 82 + 5 * column, 122),
    );
    call("delete_gui_batch", batch);
    call("delete_surface_cache_target", small);
    const [inside, edge, outside] = columns as [number[], number[], number[]];
    // Linear coverage 0.2 of red over blue encodes near sRGB 124.
    if (
      inside[0]! < 247 ||
      inside[2]! > 8 ||
      edge[0]! < 90 ||
      edge[0]! > 160 ||
      outside[0]! > 8
    )
      throw new Error(
        `Cache repaint antialiasing does not follow the target viewport: ${JSON.stringify(columns)}`,
      );
    return { columns };
  }
}
