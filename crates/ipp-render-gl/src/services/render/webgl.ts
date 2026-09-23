/** Optional shader paths match the selected Rust capabilities. */

declare const IPP_SHADOWS: boolean;
/** Replaced by the host bundler to match Rust skeletal-animation. */
declare const IPP_SKELETAL_ANIMATION: boolean;
declare const IPP_MESH_POSES: boolean;
declare const IPP_PARTICLES: boolean;
declare const IPP_SURFACES: boolean;
declare const IPP_GUI: boolean;

/** Host bindings for the Rust GL device. No scene data or draw preparation lives here. */
export interface WebGlHostExports {
  imports: WebAssembly.Imports[string];
  setMemory(memory: WebAssembly.Memory): void;
  resize(width: number, height: number): void;
  /**
   * Completed drawing buffer RGBA bytes in top-left row order. The drawing
   * buffer is not preserved after presentation, so call this in the same task
   * as the frame's `end_frame`, before the browser composites the canvas.
   */
  capture(): Uint8Array<ArrayBuffer>;
  isContextLost(): boolean;
  dispose(): void;
  loseContext(): void;
  restoreContext(): void;
  info(): Record<string, unknown>;
}

type WebGlRenderProgram = {
  object: WebGLProgram;
  mvp: WebGLUniformLocation | null;
  parameters: number;
  /** Whether the parameter block is bound to uniform buffer binding zero. */
  parametersBound: boolean;
  parameterLocations: Map<string, WebGLUniformLocation | null>;
  /** Per-draw uniform values last uploaded to this program, by location. */
  values: Map<WebGLUniformLocation, number | Float32Array>;
  material: WebGLUniformLocation | null;
  lighting?: Record<string, WebGLUniformLocation | null>;
  texture?: WebGLUniformLocation | null;
  joints?: WebGLUniformLocation | null;
  poseWeight?: WebGLUniformLocation | null;
};

/** Retained GUI or glyph vertices laid out by a Rust-owned attribute table. */
type WebGlRetainedBatch = {
  vao: WebGLVertexArrayObject;
  vbo: WebGLBuffer;
  stride: number;
  count: number;
  bytes: number;
};

type WebGlRenderMesh = {
  vao: WebGLVertexArrayObject;
  vertex: WebGLBuffer;
  color?: WebGLBuffer;
  normal?: WebGLBuffer;
  weight?: WebGLBuffer;
  index: WebGLBuffer;
  count: number;
  uv?: WebGLBuffer;
  skin?: [WebGLBuffer, WebGLBuffer];
};

/** Supply imports as `ipp_gl`; set memory after instantiate, before Rust attach. */
export function createWebGlDevice(canvas: OffscreenCanvas): WebGlHostExports {
  const context = canvas.getContext("webgl2", {
    alpha: false,
    antialias: false,
    depth: true,
    stencil: false,
    premultipliedAlpha: false,
    // Capture reads back in the rendering task, so compositing may swap buffers
    // instead of copying the frame.
    preserveDrawingBuffer: false,
  });
  if (!context) throw new Error("WebGL 2 is unavailable");
  const gl = context;
  function initializeAttributeDefaults(): void {
    // Generic values are context state, retained across VAO switches and draws.
    // This device owns the context and never assigns other constants to these
    // slots. Reapply them only when a restored context resets its state.
    gl.vertexAttrib3f(1, 1, 1, 1);
    gl.vertexAttrib3f(2, 0, 0, 0);
    gl.vertexAttrib3f(3, 1, 0, 0);
    gl.vertexAttrib3f(4, 0, 0, 0);
  }

  initializeAttributeDefaults();
  // The Rust shader emits sRGB-encoded values for this drawing buffer.
  gl.drawingBufferColorSpace = "srgb";
  let maxViewport = gl.getParameter(gl.MAX_VIEWPORT_DIMS) as Int32Array;
  if (
    gl.isContextLost() ||
    !gl.getContextAttributes()?.depth ||
    gl.getParameter(gl.DEPTH_BITS) < 16 ||
    gl.getParameter(gl.MAX_VERTEX_ATTRIBS) <
      (IPP_PARTICLES
        ? 14
        : IPP_MESH_POSES
          ? 9
          : IPP_SKELETAL_ANIMATION
            ? 7
            : 5) ||
    maxViewport[0]! <= 0 ||
    maxViewport[1]! <= 0
  ) {
    throw new Error("WebGL 2 depth/attribute/viewport baseline unavailable");
  }

  let maxTextureSize = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
  if (!(maxTextureSize > 0)) {
    throw new Error("WebGL texture size baseline unavailable");
  }

  let maxParameterBytes = gl.getParameter(gl.MAX_UNIFORM_BLOCK_SIZE) as number;
  let maxParameterTextures = Math.min(
    gl.getParameter(gl.MAX_VERTEX_TEXTURE_IMAGE_UNITS) as number,
    gl.getParameter(gl.MAX_TEXTURE_IMAGE_UNITS) as number,
  );
  let boundProgram: WebGLProgram | null | undefined;
  let boundVertexArray: WebGLVertexArrayObject | null | undefined;
  let blendMode: number | undefined;
  let boundShadowMap: number | undefined;

  /*
   * Framebuffer bindings, viewport and depth writes as this bridge last set
   * them. Passes that interrupt a target (Surface cache repaints, glyph atlas
   * population, shadow maps and the linear frame target) save and restore it
   * without synchronous getParameter queries. Only this bridge changes the
   * worker-owned context; creation and restoration read the defaults.
   */
  let drawFramebuffer: WebGLFramebuffer | null = null;
  let readFramebuffer: WebGLFramebuffer | null = null;
  let viewportState: readonly number[] = Array.from(
    gl.getParameter(gl.VIEWPORT) as Int32Array,
  );
  let depthWrite = gl.getParameter(gl.DEPTH_WRITEMASK) as boolean;

  function bindFramebuffers(
    draw: WebGLFramebuffer | null,
    read: WebGLFramebuffer | null,
  ): void {
    const drawChanged = draw !== drawFramebuffer;
    const readChanged = read !== readFramebuffer;
    if (drawChanged && readChanged && draw === read)
      gl.bindFramebuffer(gl.FRAMEBUFFER, draw);
    else {
      if (drawChanged) gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, draw);
      if (readChanged) gl.bindFramebuffer(gl.READ_FRAMEBUFFER, read);
    }
    drawFramebuffer = draw;
    readFramebuffer = read;
  }

  function setViewport(viewport: readonly number[]): void {
    const [x, y, width, height] = viewport as [number, number, number, number];
    const known = viewportState;
    if (
      known[0] === x &&
      known[1] === y &&
      known[2] === width &&
      known[3] === height
    )
      return;
    gl.viewport(x, y, width, height);
    viewportState = [x, y, width, height];
  }

  function setDepthMask(enabled: boolean): void {
    if (depthWrite === enabled) return;
    gl.depthMask(enabled);
    depthWrite = enabled;
  }

  type WebGlTarget = {
    framebuffer: WebGLFramebuffer | null;
    readFramebuffer: WebGLFramebuffer | null;
    viewport: readonly number[];
    depthMask: boolean;
  };

  /** The bound target; the viewport array is replaced, never mutated. */
  function currentTarget(): WebGlTarget {
    return {
      framebuffer: drawFramebuffer,
      readFramebuffer,
      viewport: viewportState,
      depthMask: depthWrite,
    };
  }

  /** Deleting a bound framebuffer binds the default framebuffer. */
  function forgetFramebuffer(framebuffer: WebGLFramebuffer): void {
    if (drawFramebuffer === framebuffer) drawFramebuffer = null;
    if (readFramebuffer === framebuffer) readFramebuffer = null;
  }

  function invalidateSubmission(): void {
    boundProgram = undefined;
    boundVertexArray = undefined;
    blendMode = undefined;
    boundShadowMap = undefined;
  }

  function useProgram(program: WebGLProgram | null): void {
    if (boundProgram !== program) {
      gl.useProgram(program);
      boundProgram = program;
    }
  }

  function bindVertexArray(vao: WebGLVertexArrayObject | null): void {
    if (boundVertexArray !== vao) {
      gl.bindVertexArray(vao);
      boundVertexArray = vao;
    }
  }

  let lossExtension = gl.getExtension("WEBGL_lose_context");
  const programs = new Map<number, WebGlRenderProgram>();
  const meshes = new Map<number, WebGlRenderMesh>();
  const textures = new Map<number, WebGLTexture>();
  const surfacePaths = new Map<
    number,
    {
      texture: WebGLTexture;
      bandTexture: WebGLTexture;
      vao: WebGLVertexArrayObject;
      count: number;
      width: number;
      bandCount: number;
      bandWidth: number;
    }
  >();
  let surfaceQuadVao: WebGLVertexArrayObject | null = null;
  /** Retained analytic instance streams, each with its own instanced vertex array. */
  const surfaceInstanceStreams = IPP_SURFACES
    ? new Map<
        number,
        { vao: WebGLVertexArrayObject; vbo: WebGLBuffer; count: number }
      >()
    : undefined;
  const guiBatches = IPP_GUI
    ? new Map<number, WebGlRetainedBatch>()
    : undefined;
  const glyphBatches = IPP_GUI
    ? new Map<number, WebGlRetainedBatch>()
    : undefined;
  const glyphAtlasPages = IPP_GUI
    ? new Map<
        number,
        {
          texture: number;
          framebuffer: WebGLFramebuffer;
          width: number;
          height: number;
        }
      >()
    : undefined;
  /** The bound atlas page and the target saved by the first begin. */
  let glyphAtlasTarget:
    | { width: number; height: number; saved: WebGlTarget }
    | undefined;
  /** Whole-Surface cache images keyed by never-reused `id()` handles. */
  const surfaceCacheTargets = IPP_SURFACES
    ? new Map<
        number,
        {
          texture: WebGLTexture;
          framebuffer: WebGLFramebuffer;
          width: number;
          height: number;
        }
      >()
    : undefined;
  /** The bound cache target and the host state saved when it was bound. */
  let surfaceCacheTarget:
    | { handle: number; width: number; height: number; saved: WebGlTarget }
    | undefined;

  /** Rebind the host state saved when the cache target was bound, if any. */
  function restoreSurfaceCacheTarget(): void {
    const saved = surfaceCacheTarget;
    if (!saved) return;
    surfaceCacheTarget = undefined;
    bindFramebuffers(saved.saved.framebuffer, saved.saved.readFramebuffer);
    setViewport(saved.saved.viewport);
    setDepthMask(saved.saved.depthMask);
    // Repaint draws changed blending and depth writes behind the submission
    // cache; the next draw reapplies its state.
    blendMode = undefined;
  }

  /** Bind a path's curve texture to unit 0 and its band texture to unit 1. */
  function bindPathTextures(path: {
    texture: WebGLTexture;
    bandTexture: WebGLTexture;
  }): void {
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, path.texture);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, path.bandTexture);
  }

  /** Unbind unit 1 after a path draw, which also holds the shadow map. */
  function releaseBandTexture(): void {
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.activeTexture(gl.TEXTURE0);
    boundShadowMap = undefined;
  }

  /** Antialiasing viewport of Surface draws: atlas page, then cache target, then drawing buffer. */
  function activeSurfaceViewport(): [number, number] {
    if (IPP_GUI && glyphAtlasTarget)
      return [glyphAtlasTarget.width, glyphAtlasTarget.height];
    if (IPP_SURFACES && surfaceCacheTarget)
      return [surfaceCacheTarget.width, surfaceCacheTarget.height];
    return [gl.drawingBufferWidth, gl.drawingBufferHeight];
  }
  const shadows = IPP_SHADOWS
    ? new Map<
        number,
        { texture: WebGLTexture; framebuffer: WebGLFramebuffer; size: number }
      >()
    : undefined;
  let shadowTarget: WebGlTarget | undefined;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const encoder = new TextEncoder();
  let memory: WebAssembly.Memory | undefined;
  let floatMemory: Float32Array<ArrayBuffer> | undefined;
  let wordMemory: Uint32Array<ArrayBuffer> | undefined;
  let parameterCapacity = 0;
  let instanceCapacity = 0;
  let nextId = 1;
  let parameterBuffer: WebGLBuffer | null = null;
  /** Whether `parameterBuffer` is bound to uniform buffer binding zero. */
  let parameterBufferBound = false;
  let linearTarget:
    | {
        framebuffer: WebGLFramebuffer;
        color: WebGLTexture;
        depth: WebGLRenderbuffer;
        vao: WebGLVertexArrayObject;
        program: WebGLProgram;
        /** Whether the program's sampler selects unit 0; set on first use. */
        samplerSet: boolean;
        width: number;
        height: number;
      }
    | undefined;
  let presentationTarget: WebGlTarget | undefined;

  function releaseLinearTarget() {
    if (!linearTarget) return;
    forgetFramebuffer(linearTarget.framebuffer);
    gl.deleteFramebuffer(linearTarget.framebuffer);
    gl.deleteTexture(linearTarget.color);
    gl.deleteRenderbuffer(linearTarget.depth);
    gl.deleteVertexArray(linearTarget.vao);
    gl.deleteProgram(linearTarget.program);
    linearTarget = undefined;
  }

  function beginLinearTarget(
    width: number,
    height: number,
    vertex: string,
    fragment: string,
  ) {
    if (width > maxTextureSize || height > maxTextureSize)
      throw new Error("Linear target exceeds texture limits");
    const previous = currentTarget();
    if (
      linearTarget &&
      (linearTarget.width !== width || linearTarget.height !== height)
    )
      releaseLinearTarget();
    if (!linearTarget) {
      const framebuffer = gl.createFramebuffer(),
        color = gl.createTexture(),
        depth = gl.createRenderbuffer(),
        vao = gl.createVertexArray(),
        program = gl.createProgram();
      let vs: WebGLShader | undefined, fs: WebGLShader | undefined;
      try {
        if (!framebuffer || !color || !depth || !vao || !program)
          throw new Error("Linear target allocation failed");
        vs = compile(gl.VERTEX_SHADER, vertex);
        fs = compile(gl.FRAGMENT_SHADER, fragment);
        gl.attachShader(program, vs);
        gl.attachShader(program, fs);
        gl.linkProgram(program);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS))
          throw new Error(
            gl.getProgramInfoLog(program) || "Presentation shader link failed",
          );
        gl.bindTexture(gl.TEXTURE_2D, color);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texImage2D(
          gl.TEXTURE_2D,
          0,
          // sRGB storage preserves dark gradients at 8 bits. The framebuffer
          // blends in linear space, and sampling decodes before presentation.
          gl.SRGB8_ALPHA8,
          width,
          height,
          0,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          null,
        );
        gl.bindRenderbuffer(gl.RENDERBUFFER, depth);
        gl.renderbufferStorage(
          gl.RENDERBUFFER,
          gl.DEPTH_COMPONENT24,
          width,
          height,
        );
        bindFramebuffers(framebuffer, framebuffer);
        gl.framebufferTexture2D(
          gl.FRAMEBUFFER,
          gl.COLOR_ATTACHMENT0,
          gl.TEXTURE_2D,
          color,
          0,
        );
        gl.framebufferRenderbuffer(
          gl.FRAMEBUFFER,
          gl.DEPTH_ATTACHMENT,
          gl.RENDERBUFFER,
          depth,
        );
        if (
          gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE
        )
          throw new Error("Incomplete linear target");
        check();
        linearTarget = {
          framebuffer,
          color,
          depth,
          vao,
          program,
          samplerSet: false,
          width,
          height,
        };
      } catch (error) {
        if (framebuffer) forgetFramebuffer(framebuffer);
        gl.deleteFramebuffer(framebuffer);
        gl.deleteTexture(color);
        gl.deleteRenderbuffer(depth);
        gl.deleteVertexArray(vao);
        gl.deleteProgram(program);
        bindFramebuffers(previous.framebuffer, previous.readFramebuffer);
        throw error;
      } finally {
        if (vs) gl.deleteShader(vs);
        if (fs) gl.deleteShader(fs);
      }
    }
    presentationTarget = previous;
    bindFramebuffers(linearTarget.framebuffer, linearTarget.framebuffer);
  }

  function presentLinearTarget() {
    const previous = presentationTarget,
      target = linearTarget;
    if (!previous || !target) return;
    presentationTarget = undefined;
    bindFramebuffers(previous.framebuffer, previous.readFramebuffer);
    setViewport([0, 0, target.width, target.height]);
    gl.disable(gl.DEPTH_TEST);
    gl.disable(gl.CULL_FACE);
    gl.disable(gl.BLEND);
    setDepthMask(false);
    // Draws before the next begin_frame, such as Surface cache repaints,
    // must reapply their blending.
    blendMode = undefined;
    useProgram(target.program);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindSampler(0, null);
    gl.bindTexture(gl.TEXTURE_2D, target.color);
    if (!target.samplerSet) {
      gl.uniform1i(gl.getUniformLocation(target.program, "u_texture"), 0);
      target.samplerSet = true;
    }
    bindVertexArray(target.vao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    setViewport(previous.viewport);
    setDepthMask(true);
  }

  function parameterLocation(program: WebGlRenderProgram, name: string) {
    if (!program.parameterLocations.has(name))
      program.parameterLocations.set(
        name,
        gl.getUniformLocation(program.object, name),
      );
    return program.parameterLocations.get(name)!;
  }

  /*
   * Uniform values belong to their program and persist across draws, frames
   * and targets. Only this bridge sets them and programs are never relinked,
   * so these helpers skip values the current program already holds. Values
   * compare with Object.is, so a signed zero still uploads.
   */
  function programInt(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation | null | undefined,
    value: number,
  ): void {
    if (!location || Object.is(program.values.get(location), value)) return;
    program.values.set(location, value);
    gl.uniform1i(location, value);
  }

  function programFloat(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation | null | undefined,
    value: number,
  ): void {
    if (!location || Object.is(program.values.get(location), value)) return;
    program.values.set(location, value);
    gl.uniform1f(location, value);
  }

  /** Record `count` floats for `location`; true when they must be uploaded. */
  function changedFloats(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation,
    values: ArrayLike<number>,
    offset: number,
    count: number,
  ): boolean {
    let known = program.values.get(location);
    if (!(known instanceof Float32Array) || known.length !== count) {
      known = new Float32Array(count);
      program.values.set(location, known);
      for (let index = 0; index < count; index++)
        known[index] = values[offset + index]!;
      return true;
    }
    let changed = false;
    for (let index = 0; index < count; index++) {
      const value = values[offset + index]!;
      if (!Object.is(known[index], value)) {
        known[index] = value;
        changed = true;
      }
    }
    return changed;
  }

  function programVec4(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation | null | undefined,
    x: number,
    y: number,
    z: number,
    w: number,
  ): void {
    if (!location || !changedFloats(program, location, [x, y, z, w], 0, 4))
      return;
    gl.uniform4f(location, x, y, z, w);
  }

  /** Set a vec4 uniform from WASM memory if it changed. */
  function programVec4At(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation | null | undefined,
    pointer: number,
  ): void {
    if (!location) return;
    const memory = wholeFloats(pointer, 4);
    if (changedFloats(program, location, memory, pointer / 4, 4))
      gl.uniform4fv(location, memory, pointer / 4, 4);
  }

  /** Set a mat4 uniform from WASM memory if it changed. */
  function programMatrixAt(
    program: WebGlRenderProgram,
    location: WebGLUniformLocation | null | undefined,
    pointer: number,
  ): void {
    if (!location) return;
    const memory = wholeFloats(pointer, 16);
    if (changedFloats(program, location, memory, pointer / 4, 16))
      gl.uniformMatrix4fv(location, false, memory, pointer / 4, 16);
  }

  let shaderProgramsCreated = 0;
  let shaderProgramAttempts = 0;
  let disposed = false;
  let lastError = "WebGL operation failed";

  function live(): void {
    if (disposed) throw new Error("WebGL device disposed");
    if (gl.isContextLost()) throw new Error("WebGL context lost");
  }

  /**
   * Draws, uniforms, streams, retained-batch replacement and the frame start
   * poll `getError`, a synchronous round trip, only in exhaustive mode. The
   * Rust device decides which frame ends poll; every import checks loss.
   */
  let exhaustiveDrawChecks = false;
  function checkDraw(): void {
    if (exhaustiveDrawChecks) check();
  }

  function check(): void {
    live();
    const error = gl.getError();
    if (error !== gl.NO_ERROR) {
      throw new Error(`WebGL error 0x${error.toString(16)}`);
    }
  }

  function id(): number {
    if (nextId > 0xffff_ffff)
      throw new Error("WebGL handle capacity exhausted");
    return nextId++;
  }

  function buffer(pointer: number, bytes: number, alignment = 1): ArrayBuffer {
    if (!memory) throw new Error("WebGL WASM memory has not been attached");
    const current = memory.buffer;
    if (
      !Number.isSafeInteger(pointer) ||
      !Number.isSafeInteger(bytes) ||
      pointer < 0 ||
      bytes < 0 ||
      pointer % alignment !== 0 ||
      pointer + bytes > current.byteLength
    ) {
      throw new Error("WebGL import memory range is invalid");
    }
    // Every import checks the current single-owner memory and its own live range.
    // Whole-memory views are refreshed after growth; no component range is cached.
    // GL copies synchronously and the bridge cannot reenter Rust.
    if (!(current instanceof ArrayBuffer)) {
      throw new Error("WebGL device requires unshared WASM memory");
    }
    return current;
  }

  function floats(pointer: number, count: number): Float32Array<ArrayBuffer> {
    return new Float32Array(buffer(pointer, count * 4, 4), pointer, count);
  }

  function words(pointer: number, count: number): Uint32Array<ArrayBuffer> {
    return new Uint32Array(buffer(pointer, count * 4, 4), pointer, count);
  }

  /**
   * Upload retained vertices described by a Rust-owned `#[repr(C)]` layout table:
   * stride, attribute count, then location, components and offset per attribute.
   */
  const createRetainedBatch = IPP_GUI
    ? (
        batches: Map<number, WebGlRetainedBatch>,
        vertexPointer: number,
        byteLength: number,
        layoutPointer: number,
        kind: string,
      ): number => {
        const header = words(layoutPointer, 2);
        const stride = header[0]!;
        const attributeCount = header[1]!;
        const attributes = words(layoutPointer + 8, attributeCount * 3);
        if (stride === 0 || stride % 4 !== 0 || byteLength % stride !== 0)
          throw new Error(`${kind} batch length does not match its layout`);
        const vertexData = floats(vertexPointer, byteLength / 4);
        const vao = gl.createVertexArray();
        const vbo = gl.createBuffer();
        if (!vao || !vbo) {
          if (vao) gl.deleteVertexArray(vao);
          if (vbo) gl.deleteBuffer(vbo);
          throw new Error(`${kind} batch allocation failed`);
        }
        bindVertexArray(vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
        gl.bufferData(gl.ARRAY_BUFFER, vertexData, gl.DYNAMIC_DRAW);
        for (let index = 0; index < attributeCount; index++) {
          const location = attributes[index * 3]!;
          gl.enableVertexAttribArray(location);
          gl.vertexAttribPointer(
            location,
            attributes[index * 3 + 1]!,
            gl.FLOAT,
            false,
            stride,
            attributes[index * 3 + 2]!,
          );
        }
        bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);

        try {
          check();
        } catch (error) {
          gl.deleteVertexArray(vao);
          gl.deleteBuffer(vbo);
          throw error;
        }
        const handle = id();
        batches.set(handle, {
          vao,
          vbo,
          stride,
          count: byteLength / stride,
          bytes: byteLength,
        });
        return handle;
      }
    : undefined;

  /** Replace a retained batch's complete store with vertices of its layout. */
  const updateRetainedBatch = IPP_GUI
    ? (
        batch: WebGlRetainedBatch | undefined,
        vertexPointer: number,
        byteLength: number,
        kind: string,
      ): void => {
        if (!batch) throw new Error(`Stale ${kind} batch handle`);
        if (byteLength % batch.stride !== 0)
          throw new Error(`${kind} batch length does not match its layout`);
        const vertexData = floats(vertexPointer, byteLength / 4);
        // Replace the complete store; GL preserves any previous storage needed
        // by queued draws. The driver may still stall to allocate.
        gl.bindBuffer(gl.ARRAY_BUFFER, batch.vbo);
        gl.bufferData(gl.ARRAY_BUFFER, vertexData, gl.DYNAMIC_DRAW);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        // Outside exhaustive mode the device checks this frame's end instead.
        checkDraw();
        batch.count = byteLength / batch.stride;
        batch.bytes = byteLength;
      }
    : undefined;

  /**
   * View a packed analytic instance stream of `count` sixteen-float instances after
   * validating each descriptor against the path's curve and band ranges.
   */
  const surfaceInstanceValues = IPP_SURFACES
    ? (
        pathHandle: number,
        instancePointer: number,
        count: number,
      ): Float32Array<ArrayBuffer> => {
        const path = surfacePaths.get(pathHandle >>> 0);
        count >>>= 0;
        if (!path) throw new Error("Stale surface path handle");
        if (count === 0) throw new Error("Empty surface instance stream");
        const values = floats(instancePointer >>> 0, count * 16);
        for (let index = 0; index < count; index += 1) {
          const base = index * 16;
          const curveStart = values[base + 12]!;
          const curveCount = values[base + 13]!;
          const bandOffset = values[base + 14]!;
          if (
            curveCount <= 0 ||
            curveStart + curveCount > path.count ||
            bandOffset < 0 ||
            bandOffset + 32 > path.bandCount
          )
            throw new Error("Invalid surface instance range");
        }
        return values;
      }
    : undefined;

  function wholeFloats(
    pointer: number,
    count: number,
  ): Float32Array<ArrayBuffer> {
    const current = buffer(pointer, count * 4, 4);
    if (floatMemory?.buffer !== current)
      floatMemory = new Float32Array(current);
    return floatMemory;
  }

  function wholeWords(
    pointer: number,
    count: number,
  ): Uint32Array<ArrayBuffer> {
    const current = buffer(pointer, count * 4, 4);
    if (wordMemory?.buffer !== current) wordMemory = new Uint32Array(current);
    return wordMemory;
  }

  function matrixUniform(
    location: WebGLUniformLocation | null,
    pointer: number,
    count: number,
  ): void {
    gl.uniformMatrix4fv(
      location,
      false,
      wholeFloats(pointer, count),
      pointer / 4,
      count,
    );
  }

  function vector3Uniform(
    location: WebGLUniformLocation | null,
    pointer: number,
    count: number,
  ): void {
    gl.uniform3fv(location, wholeFloats(pointer, count), pointer / 4, count);
  }

  function vector4Uniform(
    location: WebGLUniformLocation | null,
    pointer: number,
    count: number,
  ): void {
    gl.uniform4fv(location, wholeFloats(pointer, count), pointer / 4, count);
  }

  function source(pointer: number, length: number): string {
    return decoder.decode(
      new Uint8Array(buffer(pointer, length), pointer, length),
    );
  }

  function status(operation: () => number): number {
    try {
      live();
      return operation();
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
      return 0;
    }
  }

  function compile(kind: number, text: string): WebGLShader {
    const shader = gl.createShader(kind);
    if (!shader) throw new Error("WebGL shader allocation failed");
    gl.shaderSource(shader, text);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const message =
        gl.getShaderInfoLog(shader) || "WebGL shader compilation failed";
      gl.deleteShader(shader);
      throw new Error(message);
    }
    return shader;
  }

  function deleteMesh(mesh: WebGlRenderMesh): void {
    gl.deleteVertexArray(mesh.vao);
    gl.deleteBuffer(mesh.vertex);
    gl.deleteBuffer(mesh.index);
    if (mesh.color) gl.deleteBuffer(mesh.color);
    if (mesh.normal) gl.deleteBuffer(mesh.normal);
    if (IPP_SKELETAL_ANIMATION && mesh.skin)
      for (const value of mesh.skin) gl.deleteBuffer(value);
    if (mesh.weight) gl.deleteBuffer(mesh.weight);
    if (mesh.uv) gl.deleteBuffer(mesh.uv);
  }

  function resize(width: number, height: number): void {
    live();
    if (
      !Number.isInteger(width) ||
      !Number.isInteger(height) ||
      width <= 0 ||
      height <= 0 ||
      width > maxViewport[0]! ||
      height > maxViewport[1]!
    ) {
      throw new Error("Invalid WebGL viewport dimensions");
    }
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    if (gl.drawingBufferWidth !== width || gl.drawingBufferHeight !== height) {
      throw new Error("WebGL drawing buffer allocation did not match viewport");
    }
    checkDraw();
  }

  function lost(event: Event): void {
    linearTarget = undefined;
    presentationTarget = undefined;
    parameterBuffer = null;
    parameterBufferBound = false;
    parameterCapacity = 0;
    instanceCapacity = 0;
    if (IPP_PARTICLES) {
      instanceBuffer = null;
      instanceCount = 0;
    }
    if (IPP_SURFACES) {
      surfacePaths.clear();
      surfaceQuadVao = null;
      surfaceInstanceStreams!.clear();
      surfaceCacheTargets!.clear();
      surfaceCacheTarget = undefined;
    }
    if (IPP_GUI) {
      guiBatches!.clear();
      glyphBatches!.clear();
      glyphAtlasPages!.clear();
      glyphAtlasTarget = undefined;
    }

    event.preventDefault();
    invalidateSubmission();
    programs.clear();
    meshes.clear();
    textures.clear();
    if (IPP_SHADOWS) {
      shadows!.clear();
      shadowTarget = undefined;
    }
  }

  function restored(): void {
    initializeAttributeDefaults();
    parameterBufferBound = false;
    // A restored context starts from default bindings and state.
    drawFramebuffer = null;
    readFramebuffer = null;
    viewportState = Array.from(gl.getParameter(gl.VIEWPORT) as Int32Array);
    depthWrite = gl.getParameter(gl.DEPTH_WRITEMASK) as boolean;
    programs.clear();
    meshes.clear();
    textures.clear();
    if (IPP_SURFACES) {
      surfacePaths.clear();
      surfaceQuadVao = null;
      surfaceInstanceStreams!.clear();
      surfaceCacheTargets!.clear();
      surfaceCacheTarget = undefined;
    }
    if (IPP_GUI) {
      guiBatches!.clear();
      glyphBatches!.clear();
      glyphAtlasPages!.clear();
      glyphAtlasTarget = undefined;
    }
    if (IPP_SHADOWS) {
      shadows!.clear();
      shadowTarget = undefined;
    }
    gl.drawingBufferColorSpace = "srgb";
    maxViewport = gl.getParameter(gl.MAX_VIEWPORT_DIMS) as Int32Array;
    maxTextureSize = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
    maxParameterBytes = gl.getParameter(gl.MAX_UNIFORM_BLOCK_SIZE) as number;
    maxParameterTextures = Math.min(
      gl.getParameter(gl.MAX_VERTEX_TEXTURE_IMAGE_UNITS) as number,
      gl.getParameter(gl.MAX_TEXTURE_IMAGE_UNITS) as number,
    );
    invalidateSubmission();
    lossExtension = gl.getExtension("WEBGL_lose_context");
  }

  canvas.addEventListener("webglcontextlost", lost);
  canvas.addEventListener("webglcontextrestored", restored);

  function createMesh(
    vptr: number,
    vertexCount: number,
    colorPointer: number,
    iptr: number,
    indexCount: number,
    normalPointer = 0,
    uvPointer = 0,
    weightPointer = 0,
  ): number {
    return status(() => {
      const vertices = floats(vptr >>> 0, (vertexCount >>> 0) * 3);
      const indices = new Uint16Array(
        buffer(iptr >>> 0, (indexCount >>> 0) * 2, 2),
        iptr >>> 0,
        indexCount >>> 0,
      );
      const vao = gl.createVertexArray();
      const vertex = gl.createBuffer();
      const index = gl.createBuffer();
      let uv: WebGLBuffer | null = null;
      let color: WebGLBuffer | null = null;
      let normal: WebGLBuffer | null = null;
      let weight: WebGLBuffer | null = null;
      let retained = false;
      try {
        if (!vao || !vertex || !index)
          throw new Error("WebGL mesh allocation failed");
        bindVertexArray(vao);
        gl.bindBuffer(gl.ARRAY_BUFFER, vertex);
        gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, index);
        gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
        if (normalPointer !== 0) {
          normal = gl.createBuffer();
          if (!normal) throw new Error("WebGL normal allocation failed");
          gl.bindBuffer(gl.ARRAY_BUFFER, normal);
          gl.bufferData(
            gl.ARRAY_BUFFER,
            floats(normalPointer >>> 0, (vertexCount >>> 0) * 3),
            gl.STATIC_DRAW,
          );
          gl.enableVertexAttribArray(4);
          gl.vertexAttribPointer(4, 3, gl.FLOAT, false, 0, 0);
        }
        if (colorPointer !== 0) {
          color = gl.createBuffer();
          if (!color) throw new Error("WebGL color allocation failed");
          gl.bindBuffer(gl.ARRAY_BUFFER, color);
          gl.bufferData(
            gl.ARRAY_BUFFER,
            floats(colorPointer >>> 0, (vertexCount >>> 0) * 3),
            gl.STATIC_DRAW,
          );
          gl.enableVertexAttribArray(1);
          gl.vertexAttribPointer(1, 3, gl.FLOAT, false, 0, 0);
        }
        if (uvPointer !== 0) {
          uv = gl.createBuffer();
          if (!uv) throw new Error("WebGL UV allocation failed");
          gl.bindBuffer(gl.ARRAY_BUFFER, uv);
          gl.bufferData(
            gl.ARRAY_BUFFER,
            floats(uvPointer >>> 0, (vertexCount >>> 0) * 2),
            gl.STATIC_DRAW,
          );
          gl.enableVertexAttribArray(2);
          gl.vertexAttribPointer(2, 2, gl.FLOAT, false, 0, 0);
        }
        if (weightPointer !== 0) {
          weight = gl.createBuffer();
          if (!weight) throw new Error("WebGL weight allocation failed");
          gl.bindBuffer(gl.ARRAY_BUFFER, weight);
          gl.bufferData(
            gl.ARRAY_BUFFER,
            new Uint8Array(
              buffer(weightPointer >>> 0, vertexCount >>> 0),
              weightPointer >>> 0,
              vertexCount >>> 0,
            ),
            gl.STATIC_DRAW,
          );
          gl.enableVertexAttribArray(3);
          gl.vertexAttribPointer(3, 1, gl.UNSIGNED_BYTE, true, 0, 0);
        }
        check();
        const handle = id();
        meshes.set(handle, {
          vao,
          vertex,
          index,
          count: indices.length,
          ...(color ? { color } : {}),
          ...(normal ? { normal } : {}),
          ...(weight ? { weight } : {}),
          ...(uv ? { uv } : {}),
        });
        retained = true;
        return handle;
      } finally {
        bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        if (!retained) {
          gl.deleteVertexArray(vao);
          gl.deleteBuffer(vertex);
          gl.deleteBuffer(index);
          gl.deleteBuffer(color);
          gl.deleteBuffer(normal);
          gl.deleteBuffer(weight);
          gl.deleteBuffer(uv);
        }
      }
    });
  }

  let instanceBuffer: WebGLBuffer | null = null;
  let instanceCount = 0;

  function prepareParameterStorage(
    program: WebGlRenderProgram,
    count: number,
    textureCount: number,
  ): void {
    if (
      count * 4 > maxParameterBytes ||
      textureCount + 2 > maxParameterTextures
    )
      throw new Error("Custom material exceeds parameter limits");
    if (
      count === 0 ||
      program.parameters === gl.INVALID_INDEX ||
      (parameterBuffer && count * 4 <= parameterCapacity)
    )
      return;
    parameterBuffer ??= gl.createBuffer();
    if (!parameterBuffer) throw new Error("Parameter buffer allocation failed");
    const capacity = Math.max(count * 4, parameterCapacity * 2, 256);
    gl.bindBuffer(gl.UNIFORM_BUFFER, parameterBuffer);
    gl.bufferData(gl.UNIFORM_BUFFER, capacity, gl.STREAM_DRAW);
    check();
    parameterCapacity = capacity;
  }

  function drawMesh(
    programId: number,
    meshId: number,
    mvpPointer: number,
    materialPointer: number,
    textureId = 0,
    poseId = 0,
    poseWeight = 0,
  ): number {
    return status(() => {
      const program = programs.get(programId >>> 0);
      const mesh = meshes.get(meshId >>> 0);
      if (!program || !mesh) throw new Error("WebGL resource handle is stale");
      useProgram(program.object);
      matrixUniform(program.mvp, mvpPointer >>> 0, 16);
      vector3Uniform(program.material, materialPointer >>> 0, 3);
      if (program.texture) {
        const texture = textureId === 0 ? null : textures.get(textureId >>> 0);
        if (texture === undefined)
          throw new Error("WebGL texture handle is stale");
        gl.activeTexture(gl.TEXTURE0);
        gl.bindSampler(0, null);
        gl.bindTexture(gl.TEXTURE_2D, texture);
        programInt(program, program.texture, 0);
      }
      const target =
        IPP_MESH_POSES && poseId !== 0 ? meshes.get(poseId >>> 0) : undefined;
      if (IPP_MESH_POSES && poseId !== 0 && !target)
        throw new Error("WebGL mesh pose handle is stale");
      bindVertexArray(mesh.vao);
      try {
        if (IPP_MESH_POSES && target) {
          gl.uniform1f(program.poseWeight!, poseWeight);
          gl.bindBuffer(gl.ARRAY_BUFFER, target.vertex);
          gl.enableVertexAttribArray(7);
          gl.vertexAttribPointer(7, 3, gl.FLOAT, false, 0, 0);
          if (mesh.normal && target.normal) {
            gl.bindBuffer(gl.ARRAY_BUFFER, target.normal);
            gl.enableVertexAttribArray(8);
            gl.vertexAttribPointer(8, 3, gl.FLOAT, false, 0, 0);
          }
        }
        if (IPP_PARTICLES && instanceCount > 0) {
          gl.bindBuffer(gl.ARRAY_BUFFER, instanceBuffer);
          for (let slot = 9; slot < 14; slot++) {
            gl.enableVertexAttribArray(slot);
            gl.vertexAttribPointer(
              slot,
              4,
              gl.FLOAT,
              false,
              80,
              (slot - 9) * 16,
            );
            gl.vertexAttribDivisor(slot, 1);
          }
          gl.drawElementsInstanced(
            gl.TRIANGLES,
            mesh.count,
            gl.UNSIGNED_SHORT,
            0,
            instanceCount,
          );
          gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vertex);
          for (let slot = 9; slot < 14; slot++) {
            gl.vertexAttribDivisor(slot, 0);
            gl.disableVertexAttribArray(slot);
            gl.vertexAttribPointer(slot, 3, gl.FLOAT, false, 0, 0);
          }
        } else {
          gl.drawElements(gl.TRIANGLES, mesh.count, gl.UNSIGNED_SHORT, 0);
        }
      } finally {
        if (IPP_MESH_POSES && target) {
          // Disabling alone retains the target buffer in the VAO. Replace both
          // pointers before a target resource may unload or be replaced.
          gl.bindBuffer(gl.ARRAY_BUFFER, mesh.vertex);
          for (const slot of [7, 8]) {
            gl.vertexAttribPointer(slot, 3, gl.FLOAT, false, 0, 0);
            gl.disableVertexAttribArray(slot);
          }
          gl.bindBuffer(gl.ARRAY_BUFFER, null);
        }
      }
      checkDraw();
      return 1;
    });
  }

  const skinImports = IPP_SKELETAL_ANIMATION
    ? {
        mesh_skin(
          meshId: number,
          indicesPointer: number,
          weightsPointer: number,
          count: number,
        ): number {
          return status(() => {
            const mesh = meshes.get(meshId >>> 0);
            if (!mesh || mesh.skin)
              throw new Error("Invalid mesh skin attachment");
            const indices = gl.createBuffer();
            const weights = gl.createBuffer();
            let retained = false;
            try {
              if (!indices || !weights)
                throw new Error("WebGL skin allocation failed");
              bindVertexArray(mesh.vao);
              gl.bindBuffer(gl.ARRAY_BUFFER, indices);
              gl.bufferData(
                gl.ARRAY_BUFFER,
                new Uint8Array(
                  buffer(indicesPointer >>> 0, (count >>> 0) * 4),
                  indicesPointer >>> 0,
                  (count >>> 0) * 4,
                ),
                gl.STATIC_DRAW,
              );
              gl.enableVertexAttribArray(5);
              gl.vertexAttribPointer(5, 4, gl.UNSIGNED_BYTE, false, 0, 0);
              gl.bindBuffer(gl.ARRAY_BUFFER, weights);
              gl.bufferData(
                gl.ARRAY_BUFFER,
                floats(weightsPointer >>> 0, (count >>> 0) * 4),
                gl.STATIC_DRAW,
              );
              gl.enableVertexAttribArray(6);
              gl.vertexAttribPointer(6, 4, gl.FLOAT, false, 0, 0);
              check();
              mesh.skin = [indices, weights];
              retained = true;
              return 1;
            } finally {
              bindVertexArray(null);
              gl.bindBuffer(gl.ARRAY_BUFFER, null);
              if (!retained) {
                gl.deleteBuffer(indices);
                gl.deleteBuffer(weights);
              }
            }
          });
        },
        set_skin_palette(
          programId: number,
          pointer: number,
          count: number,
        ): number {
          return status(() => {
            const program = programs.get(programId >>> 0);
            if (!program || count < 1 || count > 32)
              throw new Error("Invalid skin palette");
            if (!program.joints) return 1;
            useProgram(program.object);
            matrixUniform(program.joints, pointer >>> 0, count * 16);
            checkDraw();
            return 1;
          });
        },
      }
    : {};

  const imports: WebAssembly.Imports[string] = {
    set_draw_checks(enabled: number): void {
      exhaustiveDrawChecks = enabled !== 0;
    },
    prepare_custom_parameters(
      handle: number,
      count: number,
      textureCount: number,
    ): number {
      return status(() => {
        const program = programs.get(handle >>> 0);
        if (!program) throw new Error("Unknown custom program");
        prepareParameterStorage(program, count >>> 0, textureCount >>> 0);
        checkDraw();
        return 1;
      });
    },
    set_custom_parameters(
      handle: number,
      pointer: number,
      count: number,
      textureCount: number,
      alphaMode: number,
      alphaCutoff: number,
    ): number {
      return status(() => {
        const program = programs.get(handle >>> 0);
        if (!program) throw new Error("Unknown custom program");
        prepareParameterStorage(program, count >>> 0, textureCount >>> 0);
        useProgram(program.object);
        if (count > 0 && program.parameters !== gl.INVALID_INDEX) {
          gl.bindBuffer(gl.UNIFORM_BUFFER, parameterBuffer);
          gl.bufferSubData(
            gl.UNIFORM_BUFFER,
            0,
            wholeWords(pointer >>> 0, count),
            (pointer >>> 0) / 4,
            count,
          );
          if (!parameterBufferBound) {
            gl.bindBufferBase(gl.UNIFORM_BUFFER, 0, parameterBuffer);
            parameterBufferBound = true;
          }
          if (!program.parametersBound) {
            gl.uniformBlockBinding(program.object, program.parameters, 0);
            program.parametersBound = true;
          }
        }
        programInt(
          program,
          parameterLocation(program, "u_alpha_mode"),
          alphaMode,
        );
        programFloat(
          program,
          parameterLocation(program, "u_alpha_cutoff"),
          alphaCutoff,
        );
        checkDraw();
        return 1;
      });
    },
    bind_custom_texture(
      handle: number,
      index: number,
      pointer: number,
      length: number,
      texture: number,
    ): number {
      return status(() => {
        const program = programs.get(handle >>> 0),
          object = textures.get(texture >>> 0);
        if (!program || !object) throw new Error("Custom texture unavailable");
        const unit = index + 2;
        useProgram(program.object);
        gl.activeTexture(gl.TEXTURE0 + unit);
        gl.bindSampler(unit, null);
        gl.bindTexture(gl.TEXTURE_2D, object);
        programInt(
          program,
          parameterLocation(
            program,
            `p_${source(pointer >>> 0, length >>> 0)}`,
          ),
          unit,
        );
        checkDraw();
        return 1;
      });
    },
    set_alpha_blend(enabled: number): number {
      return status(() => {
        const mode = enabled ? 2 : 1;
        if (blendMode === mode) {
          checkDraw();
          return 1;
        }
        blendMode = mode;
        if (enabled) {
          gl.enable(gl.BLEND);
          gl.blendEquation(gl.FUNC_ADD);
          gl.blendFuncSeparate(
            gl.SRC_ALPHA,
            gl.ONE_MINUS_SRC_ALPHA,
            gl.ONE,
            gl.ONE_MINUS_SRC_ALPHA,
          );
        } else gl.disable(gl.BLEND);
        setDepthMask(!enabled);
        checkDraw();
        return 1;
      });
    },
    ...(IPP_SURFACES
      ? {
          set_surface_double_sided(enabled: number): number {
            return status(() => {
              if (enabled !== 0) gl.disable(gl.CULL_FACE);
              else {
                gl.cullFace(gl.BACK);
                gl.enable(gl.CULL_FACE);
              }
              checkDraw();
              return 1;
            });
          },
        }
      : {}),
    ...(IPP_SURFACES
      ? {
          create_surface_path(
            boundsPointer: number,
            segmentPointer: number,
            count: number,
            bandPointer: number,
            bandCount: number,
          ): number {
            return status(() => {
              count >>>= 0;
              bandCount >>>= 0;
              if (count === 0 || bandCount === 0)
                throw new Error("Surface path exceeds device limits");
              floats(boundsPointer >>> 0, 4);
              const segments = floats(segmentPointer >>> 0, count * 8);
              const texels = count * 2;
              const width = Math.max(2, Math.min(texels, maxTextureSize & ~1));
              const height = Math.ceil(texels / width);
              if (height > maxTextureSize)
                throw new Error("Surface path exceeds device limits");
              const upload = new Float32Array(width * height * 4);
              upload.set(segments);
              const bandValues = new Uint32Array(
                buffer(bandPointer >>> 0, bandCount * 8, 4),
                bandPointer >>> 0,
                bandCount * 2,
              );
              const bandWidth = Math.min(bandCount, maxTextureSize);
              const bandHeight = Math.ceil(bandCount / bandWidth);
              if (bandHeight > maxTextureSize)
                throw new Error("Surface bands exceed device limits");
              const bandUpload = new Uint32Array(bandWidth * bandHeight * 2);
              bandUpload.set(bandValues);
              const texture = gl.createTexture();
              const bandTexture = gl.createTexture();
              const vao = gl.createVertexArray();
              if (!texture || !bandTexture || !vao) {
                if (texture) gl.deleteTexture(texture);
                if (bandTexture) gl.deleteTexture(bandTexture);
                if (vao) gl.deleteVertexArray(vao);
                throw new Error("Surface path allocation failed");
              }
              let retained = false;
              try {
                gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, null);
                gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
                for (const [unit, object] of [
                  [gl.TEXTURE0, texture],
                  [gl.TEXTURE1, bandTexture],
                ] as const) {
                  gl.activeTexture(unit);
                  gl.bindTexture(gl.TEXTURE_2D, object);
                  gl.texParameteri(
                    gl.TEXTURE_2D,
                    gl.TEXTURE_MIN_FILTER,
                    gl.NEAREST,
                  );
                  gl.texParameteri(
                    gl.TEXTURE_2D,
                    gl.TEXTURE_MAG_FILTER,
                    gl.NEAREST,
                  );
                  gl.texParameteri(
                    gl.TEXTURE_2D,
                    gl.TEXTURE_WRAP_S,
                    gl.CLAMP_TO_EDGE,
                  );
                  gl.texParameteri(
                    gl.TEXTURE_2D,
                    gl.TEXTURE_WRAP_T,
                    gl.CLAMP_TO_EDGE,
                  );
                }
                gl.activeTexture(gl.TEXTURE0);
                gl.bindTexture(gl.TEXTURE_2D, texture);
                gl.texImage2D(
                  gl.TEXTURE_2D,
                  0,
                  gl.RGBA32F,
                  width,
                  height,
                  0,
                  gl.RGBA,
                  gl.FLOAT,
                  upload,
                );
                gl.activeTexture(gl.TEXTURE1);
                gl.bindTexture(gl.TEXTURE_2D, bandTexture);
                gl.texImage2D(
                  gl.TEXTURE_2D,
                  0,
                  gl.RG32UI,
                  bandWidth,
                  bandHeight,
                  0,
                  gl.RG_INTEGER,
                  gl.UNSIGNED_INT,
                  bandUpload,
                );
                gl.bindTexture(gl.TEXTURE_2D, null);
                gl.activeTexture(gl.TEXTURE0);
                check();
                const handle = id();
                surfacePaths.set(handle, {
                  texture,
                  bandTexture,
                  vao,
                  count,
                  width,
                  bandCount,
                  bandWidth,
                });
                retained = true;
                return handle;
              } finally {
                if (!retained) {
                  gl.deleteTexture(texture);
                  gl.deleteTexture(bandTexture);
                  gl.deleteVertexArray(vao);
                }
              }
            });
          },
          draw_surface_path(
            programHandle: number,
            pathHandle: number,
            boundsPointer: number,
            curveStart: number,
            curveCount: number,
            bandOffset: number,
            mvpPointer: number,
            placementPointer: number,
            clipPointer: number,
            colorPointer: number,
            fillRule: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              const path = surfacePaths.get(pathHandle >>> 0);
              if (!program || !path)
                throw new Error("Stale surface program or path handle");
              curveStart >>>= 0;
              curveCount >>>= 0;
              bandOffset >>>= 0;
              if (curveCount === 0 || curveStart + curveCount > path.count)
                throw new Error("Invalid surface curve range");
              if (bandOffset + 32 > path.bandCount)
                throw new Error("Invalid surface band range");
              if (blendMode !== 2) {
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.SRC_ALPHA,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 2;
              }
              useProgram(program.object);
              const uniform = (name: string) =>
                parameterLocation(program, name);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              programVec4At(program, uniform("u_bounds"), boundsPointer >>> 0);
              programVec4At(
                program,
                uniform("u_placement"),
                placementPointer >>> 0,
              );
              programVec4At(program, uniform("u_clip"), clipPointer >>> 0);
              programVec4At(program, uniform("u_color"), colorPointer >>> 0);
              programInt(program, uniform("u_curves"), 0);
              programInt(program, uniform("u_curve_start"), curveStart);
              programInt(program, uniform("u_curve_count"), curveCount);
              programInt(program, uniform("u_curve_width"), path.width);
              programInt(program, uniform("u_fill_rule"), fillRule >>> 0);
              programInt(program, uniform("u_bands"), 1);
              programInt(program, uniform("u_band_offset"), bandOffset);
              programInt(program, uniform("u_band_width"), path.bandWidth);
              const [viewportWidth, viewportHeight] = activeSurfaceViewport();
              programVec4(
                program,
                uniform("u_viewport"),
                viewportWidth,
                viewportHeight,
                0,
                0,
              );
              bindPathTextures(path);
              bindVertexArray(path.vao);
              gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
              releaseBandTexture();
              checkDraw();
              return 1;
            });
          },
          delete_surface_path(handle: number): void {
            const path = surfacePaths.get(handle >>> 0);
            surfacePaths.delete(handle >>> 0);
            if (path && !disposed && !gl.isContextLost()) {
              gl.deleteTexture(path.texture);
              gl.deleteTexture(path.bandTexture);
              gl.deleteVertexArray(path.vao);
            }
          },
          create_surface_instances(
            pathHandle: number,
            instancePointer: number,
            count: number,
          ): number {
            return status(() => {
              const values = surfaceInstanceValues!(
                pathHandle,
                instancePointer,
                count,
              );
              const vao = gl.createVertexArray();
              const vbo = gl.createBuffer();
              if (!vao || !vbo) {
                if (vao) gl.deleteVertexArray(vao);
                if (vbo) gl.deleteBuffer(vbo);
                throw new Error("Surface instance allocation failed");
              }
              bindVertexArray(vao);
              gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
              gl.bufferData(gl.ARRAY_BUFFER, values, gl.STATIC_DRAW);
              for (let slot = 0; slot < 4; slot += 1) {
                gl.enableVertexAttribArray(slot);
                gl.vertexAttribPointer(slot, 4, gl.FLOAT, false, 64, slot * 16);
                gl.vertexAttribDivisor(slot, 1);
              }
              bindVertexArray(null);
              gl.bindBuffer(gl.ARRAY_BUFFER, null);
              try {
                check();
              } catch (error) {
                gl.deleteVertexArray(vao);
                gl.deleteBuffer(vbo);
                throw error;
              }
              const handle = id();
              surfaceInstanceStreams!.set(handle, {
                vao,
                vbo,
                count: count >>> 0,
              });
              return handle;
            });
          },
          update_surface_instances(
            streamHandle: number,
            pathHandle: number,
            instancePointer: number,
            count: number,
          ): number {
            return status(() => {
              const stream = surfaceInstanceStreams!.get(streamHandle >>> 0);
              if (!stream) throw new Error("Stale surface instance handle");
              const values = surfaceInstanceValues!(
                pathHandle,
                instancePointer,
                count,
              );
              // Replace the complete store; queued draws keep the previous one.
              gl.bindBuffer(gl.ARRAY_BUFFER, stream.vbo);
              gl.bufferData(gl.ARRAY_BUFFER, values, gl.STATIC_DRAW);
              gl.bindBuffer(gl.ARRAY_BUFFER, null);
              // Outside exhaustive mode the device checks this frame's end instead.
              checkDraw();
              stream.count = count >>> 0;
              return 1;
            });
          },
          delete_surface_instances(streamHandle: number): void {
            const stream = surfaceInstanceStreams!.get(streamHandle >>> 0);
            surfaceInstanceStreams!.delete(streamHandle >>> 0);
            if (stream && !disposed && !gl.isContextLost()) {
              gl.deleteVertexArray(stream.vao);
              gl.deleteBuffer(stream.vbo);
            }
          },
          draw_surface_instances(
            programHandle: number,
            pathHandle: number,
            streamHandle: number,
            mvpPointer: number,
            clipPointer: number,
            fillRule: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              const path = surfacePaths.get(pathHandle >>> 0);
              const stream = surfaceInstanceStreams!.get(streamHandle >>> 0);
              if (!program || !path || !stream)
                throw new Error("Stale surface instance handle");
              if (stream.count === 0) return 1;
              if (blendMode !== 2) {
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.SRC_ALPHA,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 2;
              }
              useProgram(program.object);
              const uniform = (name: string) =>
                parameterLocation(program, name);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              programVec4At(program, uniform("u_clip"), clipPointer >>> 0);
              programInt(program, uniform("u_curves"), 0);
              programInt(program, uniform("u_curve_width"), path.width);
              programInt(program, uniform("u_bands"), 1);
              programInt(program, uniform("u_band_width"), path.bandWidth);
              programInt(program, uniform("u_fill_rule"), fillRule >>> 0);
              const [viewportWidth, viewportHeight] = activeSurfaceViewport();
              programVec4(
                program,
                uniform("u_viewport"),
                viewportWidth,
                viewportHeight,
                0,
                0,
              );
              bindVertexArray(stream.vao);
              bindPathTextures(path);
              gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, stream.count);
              releaseBandTexture();
              checkDraw();
              return 1;
            });
          },
          draw_surface_bitmap(
            programHandle: number,
            textureHandle: number,
            mvpPointer: number,
            placementPointer: number,
            clipPointer: number,
            colorPointer: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              const texture = textures.get(textureHandle >>> 0);
              if (!program || !texture)
                throw new Error("Stale surface bitmap handle");
              if (blendMode !== 2) {
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.SRC_ALPHA,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 2;
              }
              if (!surfaceQuadVao) surfaceQuadVao = gl.createVertexArray();
              if (!surfaceQuadVao)
                throw new Error("Surface quad allocation failed");
              useProgram(program.object);
              const uniform = (name: string) =>
                parameterLocation(program, name);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              programVec4At(
                program,
                uniform("u_placement"),
                placementPointer >>> 0,
              );
              programVec4At(program, uniform("u_clip"), clipPointer >>> 0);
              programVec4At(program, uniform("u_color"), colorPointer >>> 0);
              programInt(program, uniform("u_texture"), 0);
              // Unit 0 keeps the texture until the next draw that samples it.
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, texture);
              bindVertexArray(surfaceQuadVao);
              gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
              checkDraw();
              return 1;
            });
          },
          surface_cache_limit(): number {
            if (disposed || gl.isContextLost()) return 0;
            return Math.min(maxTextureSize, maxViewport[0]!, maxViewport[1]!);
          },
          create_surface_cache_target(width: number, height: number): number {
            return status(() => {
              width >>>= 0;
              height >>>= 0;
              const limit = Math.min(
                maxTextureSize,
                maxViewport[0]!,
                maxViewport[1]!,
              );
              if (
                width === 0 ||
                height === 0 ||
                width > limit ||
                height > limit
              )
                throw new Error("Invalid Surface cache target dimensions");
              const texture = gl.createTexture();
              const framebuffer = gl.createFramebuffer();
              if (!texture || !framebuffer) {
                if (texture) gl.deleteTexture(texture);
                if (framebuffer) gl.deleteFramebuffer(framebuffer);
                throw new Error("Surface cache target allocation failed");
              }
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, texture);
              // Linear sRGB storage matches the main target: blending runs in
              // linear space and sampling decodes before filtering.
              gl.texImage2D(
                gl.TEXTURE_2D,
                0,
                gl.SRGB8_ALPHA8,
                width,
                height,
                0,
                gl.RGBA,
                gl.UNSIGNED_BYTE,
                null,
              );
              gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
              gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
              gl.texParameteri(
                gl.TEXTURE_2D,
                gl.TEXTURE_WRAP_S,
                gl.CLAMP_TO_EDGE,
              );
              gl.texParameteri(
                gl.TEXTURE_2D,
                gl.TEXTURE_WRAP_T,
                gl.CLAMP_TO_EDGE,
              );
              gl.bindTexture(gl.TEXTURE_2D, null);
              const previous = currentTarget();
              bindFramebuffers(framebuffer, framebuffer);
              gl.framebufferTexture2D(
                gl.FRAMEBUFFER,
                gl.COLOR_ATTACHMENT0,
                gl.TEXTURE_2D,
                texture,
                0,
              );
              const complete =
                gl.checkFramebufferStatus(gl.FRAMEBUFFER) ===
                gl.FRAMEBUFFER_COMPLETE;
              if (complete) {
                setViewport([0, 0, width, height]);
                gl.clearColor(0, 0, 0, 0);
                gl.clear(gl.COLOR_BUFFER_BIT);
              }
              bindFramebuffers(previous.framebuffer, previous.readFramebuffer);
              setViewport(previous.viewport);
              try {
                if (!complete)
                  throw new Error("Surface cache framebuffer incomplete");
                check();
              } catch (error) {
                gl.deleteTexture(texture);
                gl.deleteFramebuffer(framebuffer);
                throw error;
              }
              const handle = id();
              surfaceCacheTargets!.set(handle, {
                texture,
                framebuffer,
                width,
                height,
              });
              return handle;
            });
          },
          resize_surface_cache_target(
            handle: number,
            width: number,
            height: number,
          ): number {
            return status(() => {
              const target = surfaceCacheTargets!.get(handle >>> 0);
              if (!target) throw new Error("Stale Surface cache target handle");
              if (surfaceCacheTarget?.handle === handle >>> 0)
                throw new Error("Cannot resize the bound Surface cache target");
              width >>>= 0;
              height >>>= 0;
              const limit = Math.min(
                maxTextureSize,
                maxViewport[0]!,
                maxViewport[1]!,
              );
              if (
                width === 0 ||
                height === 0 ||
                width > limit ||
                height > limit
              )
                throw new Error("Invalid Surface cache target dimensions");
              // Respecifying the attached level keeps the attachment; contents
              // are undefined until the next repaint clears them.
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, target.texture);
              gl.texImage2D(
                gl.TEXTURE_2D,
                0,
                gl.SRGB8_ALPHA8,
                width,
                height,
                0,
                gl.RGBA,
                gl.UNSIGNED_BYTE,
                null,
              );
              gl.bindTexture(gl.TEXTURE_2D, null);
              target.width = width;
              target.height = height;
              check();
              return 1;
            });
          },
          begin_surface_cache_target(handle: number): number {
            return status(() => {
              const target = surfaceCacheTargets!.get(handle >>> 0);
              if (!target) throw new Error("Stale Surface cache target handle");
              if (surfaceCacheTarget)
                throw new Error("Surface cache targets cannot nest");
              if (IPP_GUI && glyphAtlasTarget)
                throw new Error("Surface cache target inside atlas population");
              surfaceCacheTarget = {
                handle: handle >>> 0,
                width: target.width,
                height: target.height,
                saved: currentTarget(),
              };
              bindFramebuffers(target.framebuffer, target.framebuffer);
              setViewport([0, 0, target.width, target.height]);
              gl.disable(gl.SCISSOR_TEST);
              gl.disable(gl.STENCIL_TEST);
              setDepthMask(false);
              // Repaints run outside begin_frame: every draw reapplies its blending.
              blendMode = undefined;
              gl.clearColor(0, 0, 0, 0);
              gl.clear(gl.COLOR_BUFFER_BIT);
              // The end of the repaint checks the whole pass.
              try {
                checkDraw();
              } catch (error) {
                restoreSurfaceCacheTarget();
                throw error;
              }
              return 1;
            });
          },
          end_surface_cache_target(): number {
            return status(() => {
              restoreSurfaceCacheTarget();
              // A failed repaint must not be kept as a complete image.
              check();
              return 1;
            });
          },
          draw_surface_cache(
            programHandle: number,
            handle: number,
            mvpPointer: number,
            sizePointer: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              const target = surfaceCacheTargets!.get(handle >>> 0);
              if (!program || !target)
                throw new Error("Stale Surface cache handle");
              if (surfaceCacheTarget?.handle === handle >>> 0)
                throw new Error("Cannot sample the bound Surface cache target");
              const size = floats(sizePointer >>> 0, 2);
              const width = size[0]!;
              const height = size[1]!;
              if (!(width > 0 && height > 0))
                throw new Error("Invalid Surface cache composite size");
              if (blendMode !== 4) {
                // Premultiplied colour: opacity was applied once when painting.
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 4;
              }
              if (!surfaceQuadVao) surfaceQuadVao = gl.createVertexArray();
              if (!surfaceQuadVao)
                throw new Error("Surface quad allocation failed");
              useProgram(program.object);
              const uniform = (name: string) =>
                parameterLocation(program, name);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              programVec4(program, uniform("u_placement"), 0, 0, width, height);
              programVec4(program, uniform("u_clip"), 0, 0, width, height);
              programInt(program, uniform("u_surface_cache"), 0);
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, target.texture);
              bindVertexArray(surfaceQuadVao);
              gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
              // A later repaint of this image never samples its own target.
              gl.bindTexture(gl.TEXTURE_2D, null);
              // Composites are routine draws; the frame end reports their errors.
              checkDraw();
              return 1;
            });
          },
          delete_surface_cache_target(handle: number): void {
            const target = surfaceCacheTargets!.get(handle >>> 0);
            if (!target) return;
            surfaceCacheTargets!.delete(handle >>> 0);
            forgetFramebuffer(target.framebuffer);
            if (!disposed && !gl.isContextLost()) {
              gl.deleteFramebuffer(target.framebuffer);
              gl.deleteTexture(target.texture);
            }
          },
        }
      : {}),
    // GUI boxes, retained batches and glyph atlases; omitted from non-GUI bridges.
    ...(IPP_GUI
      ? {
          create_gui_batch(
            vertexPointer: number,
            byteLength: number,
            layoutPointer: number,
          ): number {
            return status(() =>
              createRetainedBatch!(
                guiBatches!,
                vertexPointer >>> 0,
                byteLength >>> 0,
                layoutPointer >>> 0,
                "GUI",
              ),
            );
          },
          update_gui_batch(
            batchHandle: number,
            vertexPointer: number,
            byteLength: number,
          ): number {
            return status(() => {
              updateRetainedBatch!(
                guiBatches!.get(batchHandle >>> 0),
                vertexPointer >>> 0,
                byteLength >>> 0,
                "GUI",
              );
              return 1;
            });
          },
          delete_gui_batch(batchHandle: number): void {
            const batch = guiBatches!.get(batchHandle >>> 0);
            if (batch) {
              guiBatches!.delete(batchHandle >>> 0);
              gl.deleteVertexArray(batch.vao);
              gl.deleteBuffer(batch.vbo);
            }
          },
          draw_gui_batch(
            programHandle: number,
            batchHandle: number,
            mvpPointer: number,
            clipPointer: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              if (!program) throw new Error("Stale GUI batch program handle");
              const batch = guiBatches!.get(batchHandle >>> 0);
              if (!batch) throw new Error("Stale GUI batch handle");
              if (blendMode !== 2) {
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.SRC_ALPHA,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 2;
              }
              useProgram(program.object);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              const [viewportWidth, viewportHeight] = activeSurfaceViewport();
              programVec4(
                program,
                parameterLocation(program, "u_viewport"),
                viewportWidth,
                viewportHeight,
                0,
                0,
              );
              programVec4At(
                program,
                parameterLocation(program, "u_clip"),
                clipPointer >>> 0,
              );
              bindVertexArray(batch.vao);
              gl.drawArrays(gl.TRIANGLES, 0, batch.count);
              checkDraw();
              return 1;
            });
          },
          create_glyph_batch(
            vertexPointer: number,
            byteLength: number,
            layoutPointer: number,
          ): number {
            return status(() =>
              createRetainedBatch!(
                glyphBatches!,
                vertexPointer >>> 0,
                byteLength >>> 0,
                layoutPointer >>> 0,
                "Glyph",
              ),
            );
          },
          update_glyph_batch(
            batchHandle: number,
            vertexPointer: number,
            byteLength: number,
          ): number {
            return status(() => {
              updateRetainedBatch!(
                glyphBatches!.get(batchHandle >>> 0),
                vertexPointer >>> 0,
                byteLength >>> 0,
                "glyph",
              );
              return 1;
            });
          },
          delete_glyph_batch(batchHandle: number): void {
            const batch = glyphBatches!.get(batchHandle >>> 0);
            if (batch) {
              glyphBatches!.delete(batchHandle >>> 0);
              gl.deleteVertexArray(batch.vao);
              gl.deleteBuffer(batch.vbo);
            }
          },
          draw_glyph_batch(
            programHandle: number,
            batchHandle: number,
            atlasHandle: number,
            mvpPointer: number,
            clipPointer: number,
          ): number {
            return status(() => {
              const program = programs.get(programHandle >>> 0);
              if (!program) throw new Error("Stale glyph batch program handle");
              const batch = glyphBatches!.get(batchHandle >>> 0);
              if (!batch) throw new Error("Stale glyph batch handle");
              const atlas = textures.get(atlasHandle >>> 0);
              if (!atlas) throw new Error("Stale glyph atlas texture handle");
              if (blendMode !== 2) {
                gl.enable(gl.BLEND);
                gl.blendEquation(gl.FUNC_ADD);
                gl.blendFuncSeparate(
                  gl.SRC_ALPHA,
                  gl.ONE_MINUS_SRC_ALPHA,
                  gl.ONE,
                  gl.ONE_MINUS_SRC_ALPHA,
                );
                setDepthMask(false);
                blendMode = 2;
              }
              useProgram(program.object);
              programMatrixAt(program, program.mvp, mvpPointer >>> 0);
              programVec4At(
                program,
                parameterLocation(program, "u_clip"),
                clipPointer >>> 0,
              );
              programInt(program, parameterLocation(program, "u_atlas"), 0);
              // Unit 0 keeps the atlas until the next draw that samples it.
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, atlas);
              bindVertexArray(batch.vao);
              gl.drawArrays(gl.TRIANGLES, 0, batch.count);
              checkDraw();
              return 1;
            });
          },
          create_glyph_atlas_page(width: number, height: number): number {
            return status(() => {
              width >>>= 0;
              height >>>= 0;
              const texture = gl.createTexture();
              const framebuffer = gl.createFramebuffer();
              if (!texture || !framebuffer) {
                if (texture) gl.deleteTexture(texture);
                if (framebuffer) gl.deleteFramebuffer(framebuffer);
                throw new Error("Glyph atlas allocation failed");
              }
              gl.activeTexture(gl.TEXTURE0);
              gl.bindTexture(gl.TEXTURE_2D, texture);
              // Single-channel coverage; the text shader samples red.
              gl.texImage2D(
                gl.TEXTURE_2D,
                0,
                gl.R8,
                width,
                height,
                0,
                gl.RED,
                gl.UNSIGNED_BYTE,
                null,
              );
              gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
              gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
              gl.texParameteri(
                gl.TEXTURE_2D,
                gl.TEXTURE_WRAP_S,
                gl.CLAMP_TO_EDGE,
              );
              gl.texParameteri(
                gl.TEXTURE_2D,
                gl.TEXTURE_WRAP_T,
                gl.CLAMP_TO_EDGE,
              );
              const previous = currentTarget();
              bindFramebuffers(framebuffer, framebuffer);
              gl.framebufferTexture2D(
                gl.FRAMEBUFFER,
                gl.COLOR_ATTACHMENT0,
                gl.TEXTURE_2D,
                texture,
                0,
              );
              const complete =
                gl.checkFramebufferStatus(gl.FRAMEBUFFER) ===
                gl.FRAMEBUFFER_COMPLETE;
              setViewport([0, 0, width, height]);
              gl.clearColor(0, 0, 0, 0);
              gl.clear(gl.COLOR_BUFFER_BIT);
              bindFramebuffers(previous.framebuffer, previous.readFramebuffer);
              setViewport(previous.viewport);
              try {
                if (!complete)
                  throw new Error("Glyph atlas framebuffer incomplete");
                check();
              } catch (error) {
                gl.deleteTexture(texture);
                gl.deleteFramebuffer(framebuffer);
                throw error;
              }
              const textureHandle = id();
              textures.set(textureHandle, texture);
              const pageHandle = id();
              glyphAtlasPages!.set(pageHandle, {
                texture: textureHandle,
                framebuffer,
                width,
                height,
              });
              return pageHandle;
            });
          },
          delete_glyph_atlas_page(pageHandle: number): void {
            const page = glyphAtlasPages!.get(pageHandle >>> 0);
            if (page) {
              glyphAtlasPages!.delete(pageHandle >>> 0);
              forgetFramebuffer(page.framebuffer);
              gl.deleteFramebuffer(page.framebuffer);
              const tex = textures.get(page.texture);
              if (tex) {
                textures.delete(page.texture);
                gl.deleteTexture(tex);
              }
            }
          },
          begin_glyph_atlas_page(pageHandle: number): number {
            return status(() => {
              const page = glyphAtlasPages!.get(pageHandle >>> 0);
              if (!page) throw new Error("Stale glyph atlas page handle");
              // Switching between pages keeps the target saved by the first
              // begin: the host target, or a cache target inside a repaint.
              glyphAtlasTarget = {
                width: page.width,
                height: page.height,
                saved: glyphAtlasTarget?.saved ?? currentTarget(),
              };
              bindFramebuffers(page.framebuffer, page.framebuffer);
              setViewport([0, 0, page.width, page.height]);
              return 1;
            });
          },
          end_glyph_atlas_page(): number {
            return status(() => {
              if (glyphAtlasTarget) {
                const { saved } = glyphAtlasTarget;
                bindFramebuffers(saved.framebuffer, saved.readFramebuffer);
                setViewport(saved.viewport);
                glyphAtlasTarget = undefined;
              }
              // Atlas draws skip per-draw checks; a failed population must not be
              // kept as populated coverage.
              check();
              return 1;
            });
          },
          glyph_atlas_texture(pageHandle: number): number {
            const page = glyphAtlasPages!.get(pageHandle >>> 0);
            return page ? page.texture : 0;
          },
        }
      : {}),
    create_program(
      vptr: number,
      vlen: number,
      fptr: number,
      flen: number,
    ): number {
      return status(() => {
        // Depth bits describe the bound framebuffer. Surface cache targets and
        // glyph atlas pages have no depth, and programs may be created lazily
        // while one is bound during a repaint.
        const offscreen =
          (IPP_SURFACES && surfaceCacheTarget !== undefined) ||
          (IPP_GUI && glyphAtlasTarget !== undefined);
        if (
          (!offscreen && gl.getParameter(gl.DEPTH_BITS) < 16) ||
          gl.getParameter(gl.MAX_VERTEX_ATTRIBS) <
            (IPP_PARTICLES
              ? 14
              : IPP_MESH_POSES
                ? 9
                : IPP_SKELETAL_ANIMATION
                  ? 7
                  : 5)
        ) {
          throw new Error(
            "Restored WebGL depth/attribute baseline unavailable",
          );
        }
        shaderProgramAttempts++;
        let vertex: WebGLShader | undefined;
        let fragment: WebGLShader | undefined;
        let object: WebGLProgram | null = null;
        try {
          vertex = compile(gl.VERTEX_SHADER, source(vptr >>> 0, vlen >>> 0));
          fragment = compile(
            gl.FRAGMENT_SHADER,
            source(fptr >>> 0, flen >>> 0),
          );
          object = gl.createProgram();
          if (!object) throw new Error("WebGL program allocation failed");
          gl.attachShader(object, vertex);
          gl.attachShader(object, fragment);
          gl.linkProgram(object);
          if (!gl.getProgramParameter(object, gl.LINK_STATUS)) {
            throw new Error(
              gl.getProgramInfoLog(object) || "WebGL program link failed",
            );
          }
          const mvp = gl.getUniformLocation(object, "u_mvp");
          const material = gl.getUniformLocation(object, "u_material");
          check();
          const handle = id();
          programs.set(handle, {
            object,
            parameters: gl.getUniformBlockIndex(object, "IppParameters"),
            parametersBound: false,
            parameterLocations: new Map(),
            values: new Map(),
            mvp,
            material,
            lighting: Object.fromEntries(
              [
                "u_model",
                "u_normal",
                "u_camera",
                "u_ambient",
                "u_surface",
                "u_lights[0]",
                "u_light_count",
                ...(IPP_SHADOWS
                  ? ["u_shadow_map", "u_shadow_matrix", "u_shadow_settings"]
                  : []),
              ].map((name) => [name, gl.getUniformLocation(object!, name)]),
            ),
            ...(IPP_MESH_POSES
              ? { poseWeight: gl.getUniformLocation(object, "u_pose_weight") }
              : {}),
            ...(IPP_SKELETAL_ANIMATION
              ? { joints: gl.getUniformLocation(object, "u_joints[0]") }
              : {}),
            texture: gl.getUniformLocation(object, "u_texture"),
          });
          shaderProgramsCreated++;
          object = null;
          return handle;
        } finally {
          if (vertex) gl.deleteShader(vertex);
          if (fragment) gl.deleteShader(fragment);
          if (object) gl.deleteProgram(object);
        }
      });
    },
    ...(IPP_MESH_POSES ? { draw_pose: drawMesh } : {}),
    create_mesh: createMesh,
    begin_frame(
      width: number,
      height: number,
      clearPointer: number,
      vptr: number,
      vlen: number,
      fptr: number,
      flen: number,
    ): number {
      return status(() => {
        resize(width >>> 0, height >>> 0);
        invalidateSubmission();
        const clear = floats(clearPointer >>> 0, 4);
        beginLinearTarget(
          width,
          height,
          source(vptr >>> 0, vlen >>> 0),
          source(fptr >>> 0, flen >>> 0),
        );
        setViewport([0, 0, width, height]);
        gl.enable(gl.DEPTH_TEST);
        gl.enable(gl.CULL_FACE);
        gl.disable(gl.BLEND);
        gl.disable(gl.SCISSOR_TEST);
        gl.disable(gl.DITHER);
        gl.disable(gl.POLYGON_OFFSET_FILL);
        gl.disable(gl.SAMPLE_ALPHA_TO_COVERAGE);
        gl.disable(gl.SAMPLE_COVERAGE);
        gl.disable(gl.RASTERIZER_DISCARD);
        gl.disable(gl.STENCIL_TEST);
        gl.depthFunc(gl.LESS);
        setDepthMask(true);
        gl.colorMask(true, true, true, true);
        gl.frontFace(gl.CCW);
        gl.cullFace(gl.BACK);
        gl.clearColor(clear[0]!, clear[1]!, clear[2]!, clear[3]!);
        gl.clearDepth(1);
        gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
        checkDraw();
        return 1;
      });
    },
    ...(IPP_PARTICLES
      ? {
          set_instances: (pointer: number, count: number) =>
            status(() => {
              pointer >>>= 0;
              count >>>= 0;
              instanceCount = count;
              if (count === 0) {
                checkDraw();
                return 1;
              }
              if (count > 0) {
                instanceBuffer ??= gl.createBuffer();
                if (!instanceBuffer)
                  throw new Error("Instance buffer allocation failed");
                gl.bindBuffer(gl.ARRAY_BUFFER, instanceBuffer);
                if (instanceCapacity < count * 80) {
                  instanceCapacity = Math.max(
                    count * 80,
                    instanceCapacity * 2,
                    1024,
                  );
                  gl.bufferData(
                    gl.ARRAY_BUFFER,
                    instanceCapacity,
                    gl.STREAM_DRAW,
                  );
                }
                gl.bufferSubData(
                  gl.ARRAY_BUFFER,
                  0,
                  wholeFloats(pointer, count * 20),
                  pointer / 4,
                  count * 20,
                );
              }
              checkDraw();
              return 1;
            }),
          set_additive: (enabled: number) =>
            status(() => {
              if (enabled && blendMode !== 3) {
                gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE, gl.ONE, gl.ONE);
                blendMode = 3;
              }
              checkDraw();
              return 1;
            }),
        }
      : {}),
    draw: drawMesh,
    /**
     * Present, then poll the error state only when `poll` is set. Loss during
     * the frame always fails, so recovery never waits for a sampled check.
     */
    end_frame(poll: number): number {
      return status(() => {
        presentLinearTarget();
        bindVertexArray(null);
        useProgram(null);
        if (IPP_SHADOWS) {
          boundShadowMap = undefined;
          gl.activeTexture(gl.TEXTURE1);
          gl.bindTexture(gl.TEXTURE_2D, null);
          gl.bindSampler(1, null);
        }
        {
          gl.activeTexture(gl.TEXTURE0);
          gl.bindTexture(gl.TEXTURE_2D, null);
          gl.bindSampler(0, null);
        }
        if (poll) check();
        else live();
        return 1;
      });
    },
    delete_mesh(handle: number): void {
      const mesh = meshes.get(handle >>> 0);
      invalidateSubmission();
      meshes.delete(handle >>> 0);
      if (mesh && !disposed && !gl.isContextLost()) deleteMesh(mesh);
    },
    delete_program(handle: number): void {
      invalidateSubmission();
      const program = programs.get(handle >>> 0);
      programs.delete(handle >>> 0);
      if (program && !disposed && !gl.isContextLost())
        gl.deleteProgram(program.object);
    },
    is_context_lost(): number {
      return disposed || gl.isContextLost() ? 1 : 0;
    },
    error_message(pointer: number, capacity: number): number {
      const bytes = encoder.encode(lastError);
      const length = Math.min(bytes.length, capacity >>> 0);
      new Uint8Array(buffer(pointer >>> 0, length), pointer >>> 0, length).set(
        bytes.subarray(0, length),
      );
      return length;
    },
  };

  Object.assign(imports, skinImports);

  {
    imports.set_lighting = (
      handle: number,
      model: number,
      normal: number,
      surface: number,
      camera: number,
      ambient: number,
      lights: number,
      count: number,
      changed: number,
    ): number =>
      status(() => {
        const program = programs.get(handle >>> 0);
        if (!program?.lighting) throw new Error("Stale lighting program");
        const u = program.lighting;
        useProgram(program.object);
        matrixUniform(u.u_model!, model >>> 0, 16);
        matrixUniform(u.u_normal!, normal >>> 0, 16);
        if (changed & 4) vector3Uniform(u.u_surface!, surface >>> 0, 3);
        if (changed & 1) vector4Uniform(u.u_camera!, camera >>> 0, 4);
        if (changed & 2) vector3Uniform(u.u_ambient!, ambient >>> 0, 3);
        if (changed & 8 && count > 0)
          vector4Uniform(u["u_lights[0]"]!, lights >>> 0, count * 16);
        if (changed & 16) gl.uniform1i(u.u_light_count!, count);
        checkDraw();
        return 1;
      });
  }

  if (IPP_SHADOWS) {
    imports.shadow_map_limit = (): number =>
      Math.min(maxTextureSize, maxViewport[0]!, maxViewport[1]!);
    imports.create_shadow_map = (size: number): number =>
      status(() => {
        size >>>= 0;
        if (
          !size ||
          size > gl.getParameter(gl.MAX_TEXTURE_SIZE) ||
          size > maxViewport[0]! ||
          size > maxViewport[1]!
        )
          throw new Error("Shadow map exceeds WebGL limits");
        const texture = gl.createTexture();
        const framebuffer = gl.createFramebuffer();
        const previous = currentTarget();
        let retained = false;
        try {
          if (!texture || !framebuffer)
            throw new Error("Shadow map allocation failed");
          boundShadowMap = undefined;
          gl.activeTexture(gl.TEXTURE1);
          gl.bindTexture(gl.TEXTURE_2D, texture);
          gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, null);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_LEVEL, 0);
          gl.texImage2D(
            gl.TEXTURE_2D,
            0,
            gl.DEPTH_COMPONENT24,
            size,
            size,
            0,
            gl.DEPTH_COMPONENT,
            gl.UNSIGNED_INT,
            null,
          );
          bindFramebuffers(framebuffer, framebuffer);
          gl.framebufferTexture2D(
            gl.FRAMEBUFFER,
            gl.DEPTH_ATTACHMENT,
            gl.TEXTURE_2D,
            texture,
            0,
          );
          gl.drawBuffers([gl.NONE]);
          gl.readBuffer(gl.NONE);
          if (
            gl.checkFramebufferStatus(gl.FRAMEBUFFER) !==
            gl.FRAMEBUFFER_COMPLETE
          )
            throw new Error("WebGL depth framebuffer unavailable");
          check();
          const handle = id();
          shadows!.set(handle, { texture, framebuffer, size });
          retained = true;
          return handle;
        } finally {
          bindFramebuffers(previous.framebuffer, previous.readFramebuffer);
          gl.bindTexture(gl.TEXTURE_2D, null);
          if (!retained) {
            gl.deleteTexture(texture);
            gl.deleteFramebuffer(framebuffer);
          }
        }
      });
    imports.begin_shadow = (
      handle: number,
      slot: number,
      grid: number,
    ): number =>
      status(() => {
        const map = shadows!.get(handle >>> 0);
        if (!map) throw new Error("Stale shadow map");
        shadowTarget = currentTarget();
        boundShadowMap = undefined;
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, null);
        bindFramebuffers(map.framebuffer, readFramebuffer);
        const tile = map.size / grid;
        setViewport([
          (slot % grid) * tile,
          Math.floor(slot / grid) * tile,
          tile,
          tile,
        ]);
        gl.colorMask(false, false, false, false);
        if (slot === 0) {
          blendMode = undefined;
          setDepthMask(true);
          gl.clearDepth(1);
          gl.clear(gl.DEPTH_BUFFER_BIT);
        }
        check();
        return 1;
      });
    imports.end_shadow = (): number =>
      status(() => {
        if (shadowTarget) {
          const saved = shadowTarget;
          shadowTarget = undefined;
          bindFramebuffers(saved.framebuffer, saved.readFramebuffer);
          setViewport(saved.viewport);
          gl.colorMask(true, true, true, true);
        }
        check();
        return 1;
      });
    imports.bind_shadow = (
      handle: number,
      mapId: number,
      matrix: number,
      settings: number,
      count: number,
      changed: number,
    ): number =>
      status(() => {
        const program = programs.get(handle >>> 0);
        const map = shadows!.get(mapId >>> 0);
        if (!program?.lighting || !map) throw new Error("Stale shadow binding");
        if (
          !program.lighting.u_shadow_map &&
          !program.lighting.u_shadow_matrix &&
          !program.lighting.u_shadow_settings
        ) {
          checkDraw();
          return 1;
        }
        useProgram(program.object);
        if (boundShadowMap !== mapId) {
          gl.activeTexture(gl.TEXTURE1);
          gl.bindSampler(1, null);
          gl.bindTexture(gl.TEXTURE_2D, map.texture);
          boundShadowMap = mapId;
        }
        if (changed & 128) gl.uniform1i(program.lighting.u_shadow_map!, 1);
        if (changed & 32 && count > 0)
          matrixUniform(
            program.lighting.u_shadow_matrix!,
            matrix >>> 0,
            count * 16,
          );
        if (changed & 64 && count > 0)
          vector4Uniform(
            program.lighting.u_shadow_settings!,
            settings >>> 0,
            count * 4,
          );
        checkDraw();
        return 1;
      });
    imports.delete_shadow_map = (handle: number): void => {
      boundShadowMap = undefined;
      const map = shadows!.get(handle >>> 0);
      shadows!.delete(handle >>> 0);
      if (map) forgetFramebuffer(map.framebuffer);
      if (map && !disposed && !gl.isContextLost()) {
        gl.deleteTexture(map.texture);
        gl.deleteFramebuffer(map.framebuffer);
      }
    };
  }

  {
    imports.create_mesh_uv = createMesh;
    imports.draw_textured = drawMesh;
    imports.create_texture = (
      width: number,
      height: number,
      pointer: number,
      byteLength: number,
    ): number =>
      status(() => {
        width >>>= 0;
        height >>>= 0;
        byteLength >>>= 0;
        const expectedBytes = width * height * 4;
        if (
          !Number.isSafeInteger(expectedBytes) ||
          width === 0 ||
          height === 0 ||
          width > maxTextureSize ||
          height > maxTextureSize ||
          (byteLength !== 0 && expectedBytes !== byteLength)
        ) {
          throw new Error("Invalid texture size or MAX_TEXTURE_SIZE exceeded");
        }
        const pixels =
          byteLength === 0
            ? null
            : new Uint8Array(
                buffer(pointer >>> 0, byteLength),
                pointer >>> 0,
                byteLength,
              );
        const texture = gl.createTexture();
        if (!texture) throw new Error("WebGL texture allocation failed");
        let retained = false;
        try {
          gl.activeTexture(gl.TEXTURE0);
          gl.bindTexture(gl.TEXTURE_2D, texture);
          gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, null);
          gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
          gl.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
          gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
          gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, 0);
          gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
          gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
          gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_BASE_LEVEL, 0);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_LEVEL, 0);
          gl.texImage2D(
            gl.TEXTURE_2D,
            0,
            gl.SRGB8_ALPHA8,
            width,
            height,
            0,
            gl.RGBA,
            gl.UNSIGNED_BYTE,
            pixels,
          );
          check();
          const handle = id();
          textures.set(handle, texture);
          retained = true;
          return handle;
        } finally {
          gl.bindTexture(gl.TEXTURE_2D, null);
          if (!retained) gl.deleteTexture(texture);
        }
      });
    imports.upload_texture_rows = (
      handle: number,
      width: number,
      firstRow: number,
      rows: number,
      pointer: number,
      byteLength: number,
    ): number =>
      status(() => {
        const texture = textures.get(handle >>> 0);
        width >>>= 0;
        firstRow >>>= 0;
        rows >>>= 0;
        byteLength >>>= 0;
        if (
          !texture ||
          width === 0 ||
          rows === 0 ||
          width > maxTextureSize ||
          firstRow + rows > maxTextureSize ||
          width * rows * 4 !== byteLength
        ) {
          throw new Error("Invalid texture upload rows or stale handle");
        }
        const pixels = new Uint8Array(
          buffer(pointer >>> 0, byteLength),
          pointer >>> 0,
          byteLength,
        );
        try {
          gl.activeTexture(gl.TEXTURE0);
          gl.bindTexture(gl.TEXTURE_2D, texture);
          gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, null);
          gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
          gl.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
          gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
          gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, 0);
          gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
          gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
          gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
          gl.texSubImage2D(
            gl.TEXTURE_2D,
            0,
            0,
            firstRow,
            width,
            rows,
            gl.RGBA,
            gl.UNSIGNED_BYTE,
            pixels,
          );
          check();
          return 1;
        } finally {
          gl.bindTexture(gl.TEXTURE_2D, null);
        }
      });
    imports.delete_texture = (handle: number): void => {
      const texture = textures.get(handle >>> 0);
      textures.delete(handle >>> 0);
      if (texture && !disposed && !gl.isContextLost())
        gl.deleteTexture(texture);
    };
  }

  check();
  return {
    imports,
    setMemory(value) {
      if (disposed) throw new Error("WebGL device disposed");
      memory = value;
    },
    resize,
    capture() {
      live();
      const width = gl.drawingBufferWidth;
      const height = gl.drawingBufferHeight;
      const pixels = new Uint8Array(width * height * 4);
      bindFramebuffers(null, null);
      gl.bindBuffer(gl.PIXEL_PACK_BUFFER, null);
      gl.pixelStorei(gl.PACK_ALIGNMENT, 1);
      gl.pixelStorei(gl.PACK_ROW_LENGTH, 0);
      gl.pixelStorei(gl.PACK_SKIP_PIXELS, 0);
      gl.pixelStorei(gl.PACK_SKIP_ROWS, 0);
      gl.finish();
      gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
      check();
      const stride = width * 4;
      const row = new Uint8Array(stride);
      for (let y = 0; y < Math.floor(height / 2); y++) {
        const top = y * stride;
        const bottom = (height - y - 1) * stride;
        row.set(pixels.subarray(top, top + stride));
        pixels.copyWithin(top, bottom, bottom + stride);
        pixels.set(row, bottom);
      }
      return pixels;
    },
    isContextLost: () => disposed || gl.isContextLost(),
    dispose() {
      if (IPP_PARTICLES) {
        gl.deleteBuffer(instanceBuffer);
        instanceBuffer = null;
        instanceCount = 0;
      }
      if (disposed) return;
      canvas.removeEventListener("webglcontextlost", lost);
      canvas.removeEventListener("webglcontextrestored", restored);
      if (!gl.isContextLost()) {
        releaseLinearTarget();
        gl.deleteBuffer(parameterBuffer);
        parameterBuffer = null;
        parameterCapacity = 0;
        instanceCapacity = 0;
        for (const mesh of meshes.values()) deleteMesh(mesh);
        if (IPP_SHADOWS)
          for (const map of shadows!.values()) {
            gl.deleteTexture(map.texture);
            gl.deleteFramebuffer(map.framebuffer);
          }
        if (IPP_SHADOWS) shadows!.clear();
        if (IPP_GUI) {
          for (const batch of [
            ...guiBatches!.values(),
            ...glyphBatches!.values(),
          ]) {
            gl.deleteVertexArray(batch.vao);
            gl.deleteBuffer(batch.vbo);
          }
          for (const page of glyphAtlasPages!.values())
            gl.deleteFramebuffer(page.framebuffer);
        }
        for (const texture of textures.values()) gl.deleteTexture(texture);
        if (IPP_SURFACES) {
          for (const path of surfacePaths.values()) {
            gl.deleteTexture(path.texture);
            gl.deleteTexture(path.bandTexture);
            gl.deleteVertexArray(path.vao);
          }
          for (const target of surfaceCacheTargets!.values()) {
            gl.deleteFramebuffer(target.framebuffer);
            gl.deleteTexture(target.texture);
          }
        }
        surfacePaths.clear();
        if (IPP_SURFACES) {
          for (const stream of surfaceInstanceStreams!.values()) {
            gl.deleteVertexArray(stream.vao);
            gl.deleteBuffer(stream.vbo);
          }
          surfaceInstanceStreams!.clear();
        }
        gl.deleteVertexArray(surfaceQuadVao);
        surfaceQuadVao = null;
        for (const program of programs.values())
          gl.deleteProgram(program.object);
      }
      if (IPP_GUI) {
        guiBatches!.clear();
        glyphBatches!.clear();
        glyphAtlasPages!.clear();
        glyphAtlasTarget = undefined;
      }
      if (IPP_SURFACES) {
        surfaceCacheTargets!.clear();
        surfaceCacheTarget = undefined;
      }
      meshes.clear();
      textures.clear();
      if (IPP_SHADOWS) {
        shadows!.clear();
        shadowTarget = undefined;
      }
      programs.clear();
      memory = undefined;
      disposed = true;
    },
    loseContext() {
      live();
      const extension = gl.getExtension("WEBGL_lose_context");
      if (!extension) throw new Error("WEBGL_lose_context unavailable");
      extension.loseContext();
    },
    restoreContext() {
      if (disposed) throw new Error("WebGL device disposed");
      // Extension objects remain usable during loss; getExtension may not.
      if (!lossExtension) throw new Error("WEBGL_lose_context unavailable");
      lossExtension.restoreContext();
    },
    info() {
      live();
      const debug = gl.getExtension("WEBGL_debug_renderer_info");
      return {
        api: "WebGL 2",
        shaderProgramsCreated,
        shaderProgramAttempts,
        shaderProgramsLive: programs.size,
        version: gl.getParameter(gl.VERSION),
        shadingLanguage: gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
        vendor: gl.getParameter(gl.VENDOR),
        renderer: gl.getParameter(gl.RENDERER),
        unmaskedRenderer: debug
          ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL)
          : null,
        unmaskedVendor: debug
          ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL)
          : null,
        maxVertexAttributes: gl.getParameter(gl.MAX_VERTEX_ATTRIBS),
        maxViewport: Array.from(maxViewport),
        maxTextureSize,
        ...(IPP_SURFACES
          ? { surfaceCacheTargetsLive: surfaceCacheTargets!.size }
          : {}),
        depthBits: gl.getParameter(gl.DEPTH_BITS),
        contextAttributes: gl.getContextAttributes(),
        colorSpace: gl.drawingBufferColorSpace,
      };
    },
  };
}
