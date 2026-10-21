/**
 * Device-level WebGL oracle for GL error checks, run in the page against a
 * distribution's shipped `webgl.js` bridge. It mirrors the native GLES probe
 * (`crates/ipp-render-gl/examples/smoke/error_checks.rs`). An error is raised
 * mid-frame through the canvas's own context, as a failed driver call would.
 * Outside exhaustive mode later draws, Surface cache composites and an
 * unpolled frame end do not report it, a polled frame end does, and Surface
 * cache passes force one shared frame-end check. Exhaustive mode
 * reports it at the next draw. Context loss fails even
 * an unpolled frame end, so recovery never waits for a sampled check.
 */

type BridgeImport = (...values: number[]) => number;

interface WebGlBridge {
  imports: Record<string, BridgeImport | undefined>;
  setMemory(memory: WebAssembly.Memory): void;
  isContextLost(): boolean;
  dispose(): void;
  loseContext(): void;
}

const SIZE = 64;
const SHADERS = "/crates/ipp-render-gl/src/services/render/shaders";
/** Content [0, 2] x [0, 1] metres to clip space. */
const CONTENT = [1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1, 0, -1, -1, 0, 1];

export async function probeErrorCheckBridge(bridgeUrl: string) {
  const { createWebGlDevice } = (await import(bridgeUrl)) as {
    createWebGlDevice(canvas: OffscreenCanvas): WebGlBridge;
  };
  const canvas = new OffscreenCanvas(SIZE, SIZE);
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: false });
  if (!gl) throw new Error("The bridge context is unavailable");
  const getError = gl.getError.bind(gl);
  const framebufferStatus = gl.checkFramebufferStatus.bind(gl);
  let errorPolls = 0;
  let framebufferPolls = 0;
  gl.getError = () => {
    errorPolls++;
    return getError();
  };
  gl.checkFramebufferStatus = (target) => {
    framebufferPolls++;
    return framebufferStatus(target);
  };
  // Instrument the actual context before the bridge binds its methods.
  const device = createWebGlDevice(canvas);
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
      throw new Error(`Bridge lacks import ${name}`);
    return operation(...values);
  };
  const message = () => {
    const at = allocate(512);
    const length = call("error_message", at, 512);
    return new TextDecoder().decode(new Uint8Array(memory.buffer, at, length));
  };
  const ok = (label: string, name: string, ...values: number[]) => {
    if (call(name, ...values) === 0)
      throw new Error(`${label}: ${name} failed: ${message()}`);
  };
  /** The call fails with the raised INVALID_ENUM, not another error. */
  const reports = (label: string, name: string, ...values: number[]) => {
    if (call(name, ...values) !== 0)
      throw new Error(`${label}: ${name} did not report the error`);
    const text = message();
    if (!text.includes("0x500"))
      throw new Error(`${label}: ${name} reported ${text}`);
    return text;
  };
  const raise = () => gl.enable(0xffff);
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
  const report: Record<string, unknown> = {};

  try {
    const bitmap = call("create_program", ...bitmapVertex!, ...bitmapFragment!);
    if (bitmap === 0) throw new Error(`Program failed: ${message()}`);
    const white = call(
      "create_texture",
      1,
      1,
      ...bytes(new Uint8Array([255, 255, 255, 255])),
    );
    if (white === 0) throw new Error(`Texture failed: ${message()}`);
    const content = floats(CONTENT);
    const draw = [
      "draw_surface_bitmap",
      bitmap,
      white,
      content,
      floats([0, 0, 1, 0.5]),
      floats([0, 0, 2, 1]),
      floats([1, 1, 1, 1]),
    ] as const;
    const clear = floats([0, 0, 0, 1]);
    const begin = (label: string) =>
      ok(
        label,
        "begin_frame",
        SIZE,
        SIZE,
        clear,
        ...presentVertex!,
        ...present!,
      );
    ok("setup", "set_surface_double_sided", 1);

    begin("clean");
    ok("clean", ...draw);
    ok("clean", "end_frame", 1);

    // Later draws and an unpolled frame end leave the error for a polled end.
    begin("sampled");
    ok("sampled", ...draw);
    raise();
    ok("sampled", ...draw);
    ok("sampled", "end_frame", 0);
    begin("polled");
    ok("polled", ...draw);
    report.polledFrameEnd = reports("polled", "end_frame", 1);

    // A polled end reports an error raised earlier in the same frame.
    begin("same frame");
    raise();
    ok("same frame", ...draw);
    report.sameFrameEnd = reports("same frame", "end_frame", 1);

    // Kept passes require a check even when the Rust frame schedule is not due.
    const target = call("create_surface_cache_target", 32, 16);
    if (target === 0) throw new Error(`Cache target failed: ${message()}`);
    raise();
    ok("repaint", "begin_surface_cache_target", target);
    ok("repaint", ...draw);
    ok("repaint", "end_surface_cache_target");
    report.repaintFrameEnd = reports("repaint", "end_frame", 0);

    // Warm the Camera depth attachment, then verify that retained targets do
    // not synchronously query framebuffer status or errors between passes.
    ok("camera allocation", "begin_camera_target", target, clear);
    ok("camera allocation", "end_surface_cache_target");
    ok("camera allocation", "end_frame", 0);
    const errorsBefore = errorPolls;
    const framebuffersBefore = framebufferPolls;
    begin("multiple passes");
    for (let pass = 0; pass < 3; pass++) {
      ok("multiple passes", "begin_camera_target", target, clear);
      ok("multiple passes", ...draw);
      ok("multiple passes", "end_surface_cache_target");
    }
    if (errorPolls !== errorsBefore || framebufferPolls !== framebuffersBefore)
      throw new Error("Retained Camera passes performed synchronous queries");
    ok("multiple passes", "end_frame", 0);
    if (errorPolls !== errorsBefore + 1)
      throw new Error(
        "Kept passes must share exactly one frame-end error check",
      );
    report.retainedPassErrorPolls = errorPolls - errorsBefore;
    report.retainedPassFramebufferPolls = framebufferPolls - framebuffersBefore;

    // A checked resource operation may consume the GL error before frame end.
    // The frame still fails, so its earlier images cannot be marked reusable.
    ok("intervening check", "begin_surface_cache_target", target);
    ok("intervening check", ...draw);
    raise();
    ok("intervening check", "end_surface_cache_target");
    reports(
      "intervening check",
      "create_texture",
      1,
      1,
      ...bytes(new Uint8Array([255, 255, 255, 255])),
    );
    report.consumedPassError = reports("intervening check", "end_frame", 0);

    // Composites are routine draws; the frame end reports their errors.
    const compositor = call(
      "create_program",
      ...bitmapVertex!,
      ...cacheFragment!,
    );
    if (compositor === 0) throw new Error(`Program failed: ${message()}`);
    ok("repainted", "begin_surface_cache_target", target);
    ok("repainted", "end_surface_cache_target");
    begin("composite");
    raise();
    ok(
      "composite",
      "draw_surface_cache",
      compositor,
      target,
      content,
      floats([2, 1]),
    );
    report.compositeFrameEnd = reports("composite", "end_frame", 1);
    call("delete_surface_cache_target", target);

    // Exhaustive mode attributes the error to the next routine call.
    call("set_draw_checks", 1);
    begin("exhaustive");
    ok("exhaustive", ...draw);
    raise();
    report.exhaustiveDraw = reports("exhaustive", ...draw);
    ok("exhaustive", "end_frame", 1);
    call("set_draw_checks", 0);

    // Loss fails an unpolled frame end and the device reports it as loss.
    begin("loss");
    ok("loss", ...draw);
    device.loseContext();
    for (let attempt = 0; attempt < 120 && !device.isContextLost(); attempt++)
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    if (!device.isContextLost()) throw new Error("Context was not lost");
    if (call("end_frame", 0) !== 0)
      throw new Error("An unpolled frame end ignored context loss");
    if (call("is_context_lost") !== 1)
      throw new Error("The bridge did not report context loss");
    report.lossFrameEnd = message();
    return report;
  } finally {
    gl.getError = getError;
    gl.checkFramebufferStatus = framebufferStatus;
    device.dispose();
  }
}
