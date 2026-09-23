/**
 * Device-level WebGL oracle for GL error checks, run in the page against a
 * distribution's shipped `webgl.js` bridge. It mirrors the native GLES probe
 * (`crates/ipp-render-gl/examples/smoke/error_checks.rs`). An error is raised
 * mid-frame through the canvas's own context, as a failed driver call would.
 * Outside exhaustive mode later draws and an unpolled frame end do not report
 * it, a polled frame end does, and a Surface cache repaint's end rejects the
 * image. Exhaustive mode reports it at the next draw. Context loss fails even
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
  const device = createWebGlDevice(canvas);
  // A canvas returns its existing context for the same context type.
  const gl = canvas.getContext("webgl2");
  if (!gl) throw new Error("The bridge context is unavailable");
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
  const [bitmapVertex, bitmapFragment, presentVertex, present] =
    await Promise.all(
      [
        "surface_bitmap.vert",
        "surface_bitmap.frag",
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

    // A repaint whose draws raised an error is never kept as an image.
    const target = call("create_surface_cache_target", 32, 16);
    if (target === 0) throw new Error(`Cache target failed: ${message()}`);
    ok("repaint", "begin_surface_cache_target", target);
    raise();
    ok("repaint", ...draw);
    report.repaintEnd = reports("repaint", "end_surface_cache_target");
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
    device.dispose();
  }
}
