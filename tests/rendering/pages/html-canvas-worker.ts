import type { HtmlCanvasFrame } from "../support/html-canvas-producer.js";

let gl: WebGL2RenderingContext;
let texture: WebGLTexture | null = null;
let program: WebGLProgram | null = null;
let current: HtmlCanvasFrame | undefined;
let generation = 1;
const statistics = { received: 0, rejected: 0, closed: 0, replayed: 0 };
const send = (value: unknown) => globalThis.postMessage(value);

function close(image: ImageBitmap): void {
  image.close();
  if (image.width !== 0 || image.height !== 0)
    throw new Error("ImageBitmap did not close");
  statistics.closed++;
}

function shader(kind: number, source: string): WebGLShader {
  const value = gl.createShader(kind)!;
  gl.shaderSource(value, source);
  gl.compileShader(value);
  if (!gl.getShaderParameter(value, gl.COMPILE_STATUS))
    throw new Error(gl.getShaderInfoLog(value)!);
  return value;
}

function prepare(): void {
  program = gl.createProgram()!;
  const vertex = shader(
    gl.VERTEX_SHADER,
    "#version 300 es\nout vec2 uv; void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);uv=vec2(p.x,1.-p.y);gl_Position=vec4(p*2.-1.,0,1);}",
  );
  const fragment = shader(
    gl.FRAGMENT_SHADER,
    "#version 300 es\nprecision highp float;uniform sampler2D image;in vec2 uv;out vec4 color;void main(){vec4 c=texture(image,uv);vec3 linear=c.rgb*c.a;vec3 srgb=mix(12.92*linear,1.055*pow(linear,vec3(1./2.4))-.055,step(vec3(.0031308),linear));color=vec4(srgb,1);}",
  );
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  gl.deleteShader(vertex);
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS))
    throw new Error(gl.getProgramInfoLog(program)!);
}

function upload(frame: HtmlCanvasFrame): void {
  if (texture) gl.deleteTexture(texture);
  texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
  gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
  gl.texImage2D(
    gl.TEXTURE_2D,
    0,
    gl.SRGB8_ALPHA8,
    frame.width,
    frame.height,
    0,
    gl.RGBA,
    gl.UNSIGNED_BYTE,
    null,
  );
  gl.texSubImage2D(
    gl.TEXTURE_2D,
    0,
    0,
    0,
    gl.RGBA,
    gl.UNSIGNED_BYTE,
    frame.image,
  );
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  if (gl.getError() !== gl.NO_ERROR)
    throw new Error("ImageBitmap texture import failed");
}

function draw(): void {
  if (!current || !texture) throw new Error("No admitted snapshot");
  gl.canvas.width = current.width;
  gl.canvas.height = current.height;
  gl.viewport(0, 0, current.width, current.height);
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  gl.useProgram(program);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  if (gl.getError() !== gl.NO_ERROR) throw new Error("Snapshot drawing failed");
}

/** Test-only capture: pixels never enter the producer or its frame delivery. */
function capture() {
  draw();
  const { width, height } = current!;
  const raw = new Uint8Array(width * height * 4);
  gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, raw);
  const pixels = new Uint8Array(raw.length);
  for (let y = 0; y < height; y++)
    pixels.set(
      raw.subarray(y * width * 4, (y + 1) * width * 4),
      (height - y - 1) * width * 4,
    );
  return {
    width,
    height,
    pixels: Array.from(pixels),
    sequence: current!.sequence,
    statistics: { ...statistics },
  };
}

globalThis.onmessage = ({ data }) => {
  try {
    if (data.canvas) {
      const context = (data.canvas as OffscreenCanvas).getContext("webgl2", {
        alpha: false,
        antialias: false,
        premultipliedAlpha: false,
      });
      if (!context) throw new Error("Worker WebGL2 unavailable");
      gl = context;
      prepare();
      const debug = gl.getExtension("WEBGL_debug_renderer_info");
      send({
        type: "ready",
        renderer: debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL),
      });
    } else if (data.frame) {
      const frame = data.frame as HtmlCanvasFrame;
      statistics.received++;
      if (
        data.generation !== generation ||
        (current && frame.sequence <= current.sequence)
      ) {
        statistics.rejected++;
        close(frame.image);
        send({
          type: "rejected",
          sequence: frame.sequence,
          statistics: { ...statistics },
        });
        return;
      }
      upload(frame);
      if (current) close(current.image);
      current = frame;
      draw();
      send({
        type: "frame",
        sequence: frame.sequence,
        latencyMs:
          performance.timeOrigin + performance.now() - frame.capturedAt,
        statistics: { ...statistics },
      });
    } else if (data.operation === "capture") {
      send({ type: "capture", ...capture() });
    } else if (data.operation === "recover") {
      const extension = gl.getExtension("WEBGL_lose_context");
      if (!extension) throw new Error("Context loss control is unavailable");
      gl.canvas.addEventListener(
        "webglcontextlost",
        (event) => {
          event.preventDefault();
          setTimeout(() => extension.restoreContext(), 0);
        },
        { once: true },
      );
      gl.canvas.addEventListener(
        "webglcontextrestored",
        () => {
          try {
            texture = null;
            prepare();
            upload(current!);
            statistics.replayed++;
            send({ type: "recovered", ...capture() });
          } catch (error) {
            send({ type: "error", error: String(error) });
          }
        },
        { once: true },
      );
      extension.loseContext();
    } else if (data.operation === "invalidate") {
      generation++;
      send({ type: "invalidated", generation });
    } else if (data.operation === "dispose") {
      if (current) close(current.image);
      current = undefined;
      gl.deleteTexture(texture);
      gl.deleteProgram(program);
      send({ type: "disposed", statistics: { ...statistics } });
    }
  } catch (error) {
    send({ type: "error", error: String(error) });
  }
};
