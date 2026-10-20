import {
  createWebGlDevice,
  type WebGlHostExports,
} from "../../../crates/ipp-render-gl/src/services/render/device/webgl/webgl.js";
import type { HtmlCanvasFrame } from "../support/html-canvas-producer.js";

let device: WebGlHostExports;
let gl: WebGL2RenderingContext;
let canvas: OffscreenCanvas;
let texture = 0;
let program = 0;
let current: Omit<HtmlCanvasFrame, "image"> | undefined;
let generation = 1;
let memory: WebAssembly.Memory;
let top = 1024;
const sources: Record<string, readonly [number, number]> = {};
const statistics = { received: 0, rejected: 0, closed: 0, replayed: 0 };
const send = (value: unknown) => globalThis.postMessage(value);
const require = (condition: unknown, message: string): void => {
  if (!condition) throw new Error(message);
};
function call(name: string, ...args: number[]): number {
  return (device.imports[name] as (...args: number[]) => number)(...args);
}
function ok(name: string, ...args: number[]): number {
  const result = call(name, ...args);
  if (!result) {
    const count = call("error_message", 0, 512);
    throw new Error(
      `${name}: ${new TextDecoder().decode(new Uint8Array(memory.buffer, 0, count))}`,
    );
  }
  return result;
}
function floats(values: readonly number[]): number {
  const at = top;
  top += values.length * 4;
  new Float32Array(memory.buffer, at, values.length).set(values);
  return at;
}
async function prepare(): Promise<void> {
  memory = new WebAssembly.Memory({ initial: 2 });
  device.setMemory(memory);
  top = 1024;
  for (const name of [
    "surface_bitmap.vert",
    "surface_bitmap.frag",
    "present.vert",
    "present.frag",
  ]) {
    const response = await fetch(`/${name}`);
    require(response.ok, `Shader unavailable: ${name}`);
    const bytes = new TextEncoder().encode(await response.text());
    const at = top;
    top = (top + bytes.length + 3) & ~3;
    new Uint8Array(memory.buffer, at, bytes.length).set(bytes);
    sources[name] = [at, bytes.length];
  }
  program = ok(
    "create_program",
    ...sources["surface_bitmap.vert"]!,
    ...sources["surface_bitmap.frag"]!,
  );
}
function draw(): void {
  require(current && texture, "No imported image");
  const mark = top;
  try {
    ok(
      "begin_frame",
      current!.width,
      current!.height,
      floats([0, 0, 0, 1]),
      ...sources["present.vert"]!,
      ...sources["present.frag"]!,
    );
    ok("set_surface_double_sided", 1);
    ok(
      "draw_surface_bitmap",
      program,
      texture,
      floats([2, 0, 0, 0, 0, -2, 0, 0, 0, 0, 1, 0, -1, 1, 0, 1]),
      floats([0, 0, 1, 1]),
      floats([0, 0, 1, 1]),
      floats([1, 1, 1, 1]),
    );
    ok("end_frame", 1);
  } finally {
    top = mark;
  }
}
function capture() {
  draw();
  return {
    width: current!.width,
    height: current!.height,
    pixels: Array.from(device.capture()),
    sequence: current!.sequence,
    statistics: { ...statistics },
  };
}
function closed(image: ImageBitmap): void {
  require(image.width === 0 &&
    image.height === 0, "Transferred bitmap remains open");
}
function upload(frame: HtmlCanvasFrame): void {
  const replacement = ok("create_texture", frame.width, frame.height, 0, 0);
  const stage = device.stageWebImage(frame.image);
  let imported = false;
  // Exercise hostile but legal upload state; import must restore it exactly.
  const active = gl.getParameter(gl.ACTIVE_TEXTURE) as number;
  gl.activeTexture(gl.TEXTURE3);
  const binding = gl.getParameter(gl.TEXTURE_BINDING_2D) as WebGLTexture | null;
  const buffer = gl.getParameter(
    gl.PIXEL_UNPACK_BUFFER_BINDING,
  ) as WebGLBuffer | null;
  const sentinel = gl.createTexture();
  const pbo = gl.createBuffer();
  const names = [
    gl.UNPACK_ALIGNMENT,
    gl.UNPACK_ROW_LENGTH,
    gl.UNPACK_IMAGE_HEIGHT,
    gl.UNPACK_SKIP_PIXELS,
    gl.UNPACK_SKIP_ROWS,
    gl.UNPACK_SKIP_IMAGES,
    gl.UNPACK_FLIP_Y_WEBGL,
    gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL,
    gl.UNPACK_COLORSPACE_CONVERSION_WEBGL,
  ];
  const old = names.map((name) => gl.getParameter(name) as number);
  const hostile = [8, 23, 29, 3, 5, 7, true, true, gl.BROWSER_DEFAULT_WEBGL];
  try {
    gl.bindTexture(gl.TEXTURE_2D, sentinel);
    gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, pbo);
    names.forEach((name, index) => gl.pixelStorei(name, hostile[index]!));
    ok("web_image_copy", stage, replacement, frame.width, frame.height);
    require(gl.getParameter(gl.ACTIVE_TEXTURE) ===
      gl.TEXTURE3, "Import changed active unit");
    require(gl.getParameter(gl.TEXTURE_BINDING_2D) ===
      sentinel, "Import changed texture binding");
    require(gl.getParameter(gl.PIXEL_UNPACK_BUFFER_BINDING) ===
      pbo, "Import changed PBO binding");
    names.forEach((name, index) =>
      require(gl.getParameter(name) ===
        hostile[index], `Import changed unpack ${name}`),
    );
    closed(frame.image);
    statistics.closed++;
    require(call(
      "web_image_copy",
      stage,
      replacement,
      frame.width,
      frame.height,
    ) === 0, "Image stage was reusable");
    imported = true;
  } finally {
    device.releaseWebImage(stage);
    names.forEach((name, index) => gl.pixelStorei(name, old[index]!));
    gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, buffer);
    gl.bindTexture(gl.TEXTURE_2D, binding);
    gl.activeTexture(active);
    gl.deleteTexture(sentinel);
    gl.deleteBuffer(pbo);
    if (!imported) call("delete_texture", replacement);
  }
  if (texture) call("delete_texture", texture);
  texture = replacement;
}
function snapshot(): ImageBitmap {
  const source = new OffscreenCanvas(8, 8);
  const context = source.getContext("webgl2", { antialias: false })!;
  context.clearColor(0, 1, 0, 1);
  context.clear(context.COLOR_BUFFER_BIT);
  const image = source.transferToImageBitmap();
  context.getExtension("WEBGL_lose_context")?.loseContext();
  return image;
}
async function event(name: string, action: () => void): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const listener = () => {
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(() => {
      canvas.removeEventListener(name, listener);
      reject(new Error(`Timed out ${name}`));
    }, 5000);
    canvas.addEventListener(name, listener, { once: true });
    action();
  });
}
async function checks() {
  const target = ok("create_texture", 8, 8, 0, 0);
  const first = snapshot();
  const firstId = device.stageWebImage(first);
  const excess = snapshot();
  let refused = false;
  try {
    device.stageWebImage(excess);
  } catch {
    refused = true;
  }
  require(refused, "Unbounded bitmap staging");
  closed(excess);
  require(first.width === 8, "Capacity rejection closed the retained image");
  device.releaseWebImage(firstId);
  closed(first);
  const invalid = snapshot();
  const invalidId = device.stageWebImage(invalid);
  require(call("web_image_copy", invalidId, target, 9, 8) ===
    0, "Mismatched import accepted");
  closed(invalid);
  const failedUpload = snapshot();
  const failedId = device.stageWebImage(failedUpload);
  const sentinel = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, sentinel);
  gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 5);
  gl.enable(0xffff); // Actual GL error, observed after the import touched state.
  require(call("web_image_copy", failedId, target, 8, 8) ===
    0, "Failed GL import accepted");
  closed(failedUpload);
  require(gl.getParameter(gl.TEXTURE_BINDING_2D) ===
    sentinel, "Failure changed texture binding");
  require(gl.getParameter(gl.UNPACK_SKIP_ROWS) ===
    5, "Failure changed unpack state");
  gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
  gl.bindTexture(gl.TEXTURE_2D, null);
  gl.deleteTexture(sentinel);
  const next = snapshot();
  const nextId = device.stageWebImage(next);
  require(nextId > firstId, "Staging name reused");
  device.releaseWebImage(firstId);
  require(next.width === 8, "Stale release closed a replacement");
  ok("web_image_copy", nextId, target, 8, 8);
  closed(next);
  const fence = ok("completion_fence_create");
  const deadline = performance.now() + 5000;
  let polls = 0;
  while (ok("completion_fence_poll", fence) !== 2) {
    require(performance.now() < deadline, "GPU fence never completed");
    polls++;
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  call("completion_fence_delete", fence);
  require(call("completion_fence_poll", fence) === 0, "Deleted fence survived");
  const lostFence = ok("completion_fence_create");
  const lostImage = snapshot();
  const lostImageId = device.stageWebImage(lostImage);
  await event("webglcontextlost", () => device.loseContext());
  closed(lostImage);
  require(call("web_image_copy", lostImageId, target, 8, 8) ===
    0, "Lost image remained available");
  require(call("completion_fence_poll", lostFence) ===
    0, "Lost fence survived");
  const duringLoss = snapshot();
  refused = false;
  try {
    device.stageWebImage(duringLoss);
  } catch {
    refused = true;
  }
  require(refused, "Lost context accepted staging");
  closed(duringLoss);
  await event("webglcontextrestored", () => {
    // Restoration is permitted after the loss event has finished dispatching.
    setTimeout(() => device.restoreContext(), 0);
  });
  require(call("completion_fence_poll", lostFence) ===
    0, "Restoration revived old fence");
  const newFence = ok("completion_fence_create");
  require(newFence > lostFence, "Restoration reused fence name");
  call("completion_fence_delete", newFence);
  const disposedImage = snapshot();
  const disposedId = device.stageWebImage(disposedImage);
  device.dispose();
  closed(disposedImage);
  require(call("web_image_copy", disposedId, target, 8, 8) ===
    0, "Disposed bridge accepted image");
  const afterDispose = snapshot();
  refused = false;
  try {
    device.stageWebImage(afterDispose);
  } catch {
    refused = true;
  }
  require(refused, "Disposed bridge accepted staging");
  closed(afterDispose);
  return {
    capacityRejected: true,
    uploadErrorClosed: true,
    extentRejected: true,
    staleReleaseInert: true,
    fenceCompleted: true,
    fencePolls: polls,
    lossInvalidated: true,
    disposalClosed: true,
    unpackStatePreserved: true,
  };
}

globalThis.onmessage = async ({ data }) => {
  try {
    if (data.canvas) {
      canvas = data.canvas;
      device = createWebGlDevice(canvas);
      gl = canvas.getContext("webgl2")!;
      await prepare();
      send({ type: "ready", renderer: device.info().unmaskedRenderer });
    } else if (data.frame) {
      const frame = data.frame as HtmlCanvasFrame;
      statistics.received++;
      if (
        data.generation !== generation ||
        (current && frame.sequence <= current.sequence)
      ) {
        statistics.rejected++;
        frame.image.close();
        closed(frame.image);
        statistics.closed++;
        send({
          type: "rejected",
          sequence: frame.sequence,
          statistics: { ...statistics },
        });
        return;
      }
      upload(frame);
      const { image: _, ...metadata } = frame;
      current = metadata;
      draw();
      send({
        type: "frame",
        sequence: frame.sequence,
        latencyMs:
          performance.timeOrigin + performance.now() - frame.capturedAt,
        statistics: { ...statistics },
      });
    } else if (data.operation === "capture")
      send({ type: "capture", ...capture() });
    else if (data.operation === "deviceChecks")
      send({ type: "deviceChecked", checks: await checks() });
    else if (data.operation === "invalidate") {
      generation++;
      send({ type: "invalidated", generation });
    } else if (data.operation === "dispose") {
      device.dispose();
      send({ type: "disposed", statistics: { ...statistics } });
    }
  } catch (error) {
    send({ type: "error", error: String(error) });
  }
};
