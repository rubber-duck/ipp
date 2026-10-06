/** Instrumentation-only physical WebGL calls, including calls after bridge cache suppression. */
export function createWebGlCallCounts(context: WebGL2RenderingContext) {
  let active = false;
  let profilerDepth = 0;
  let counts = [0n, 0n, 0n, 0n, 0n];
  let overflowed = false;
  let lossEnd: bigint | undefined;
  const maximum = (1n << 64n) - 1n;
  const wrappers = new Map<PropertyKey, unknown>();
  const draws = new Set([
    "drawArrays",
    "drawElements",
    "drawArraysInstanced",
    "drawElementsInstanced",
  ]);
  const uploads = new Set([
    "bufferData",
    "bufferSubData",
    "texImage2D",
    "texSubImage2D",
    "texImage3D",
    "texSubImage3D",
    "compressedTexImage2D",
    "compressedTexSubImage2D",
    "compressedTexImage3D",
    "compressedTexSubImage3D",
    "generateMipmap",
  ]);
  const state = new Set([
    "viewport",
    "enable",
    "disable",
    "depthFunc",
    "depthMask",
    "colorMask",
    "frontFace",
    "cullFace",
    "clearColor",
    "clearDepth",
    "clear",
    "scissor",
    "polygonOffset",
    "blendFunc",
    "blendFuncSeparate",
    "blendEquation",
    "pixelStorei",
    "activeTexture",
    "drawBuffers",
    "vertexAttribDivisor",
    "vertexAttribPointer",
    "enableVertexAttribArray",
    "disableVertexAttribArray",
    "useProgram",
    "getUniformLocation",
    "getUniformBlockIndex",
  ]);
  const record = (category: number) => {
    if (!active) return;
    if (counts[category]! === maximum) overflowed = true;
    else counts[category] = counts[category]! + 1n;
  };
  const gl = new Proxy(context, {
    get(target, property) {
      const value = Reflect.get(target, property, target);
      if (typeof value !== "function") return value;
      if (wrappers.has(property)) return wrappers.get(property);
      const name = String(property);
      const category = draws.has(name)
        ? 0
        : uploads.has(name)
          ? 2
          : state.has(name) ||
              /^(bind|uniform|vertexAttrib\d|texParameter|framebuffer)/.test(
                name,
              )
            ? 1
            : 3;
      const wrapper = (...args: unknown[]) => {
        record(profilerDepth ? 4 : category);
        return Reflect.apply(value, target, args);
      };
      wrappers.set(property, wrapper);
      return wrapper;
    },
    set(target, property, value) {
      return Reflect.set(target, property, value, target);
    },
  });
  return {
    gl,
    query<T>(operation: () => T): T {
      profilerDepth++;
      try {
        return operation();
      } finally {
        profilerDepth--;
      }
    },
    queryExtensionCall() {
      record(4);
    },
    contextLost() {
      if (!active) return;
      // Same worker performance clock as ipp_profiling.now, converted to nanoseconds.
      lossEnd = BigInt(Math.trunc(performance.now() * 1_000_000));
      active = false;
    },
    imports: {
      gl_calls_start() {
        counts = [0n, 0n, 0n, 0n, 0n];
        overflowed = false;
        lossEnd = undefined;
        active = true;
        return 1;
      },
      gl_calls_stop() {
        active = false;
      },
      gl_calls_count(index: number): bigint {
        if (index === 6) return lossEnd ?? 0n;
        if (index === 7) return BigInt(lossEnd !== undefined);
        return index === 5 ? BigInt(overflowed) : (counts[index] ?? 0n);
      },
    },
  };
}
