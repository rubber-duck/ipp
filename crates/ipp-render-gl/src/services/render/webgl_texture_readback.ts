/** Context-owned texture storage readback; no CPU pixel array is assembled here. */
export function createWebGlTextureReadback(
  gl: WebGL2RenderingContext,
  options: {
    texture(id: number): WebGLTexture | undefined;
    id(): number;
    live(): void;
    check(): void;
    memory(pointer: number, length: number): ArrayBuffer;
    target(): { draw: WebGLFramebuffer | null; read: WebGLFramebuffer | null };
    bind(draw: WebGLFramebuffer | null, read: WebGLFramebuffer | null): void;
  },
) {
  const stages = new Map<
    number,
    { buffer: WebGLBuffer; fence: WebGLSync; bytes: number; ready: boolean }
  >();

  function remove(id: number, lost = false): void {
    const stage = stages.get(id);
    stages.delete(id);
    if (stage && !lost && !gl.isContextLost()) {
      gl.deleteSync(stage.fence);
      gl.deleteBuffer(stage.buffer);
    }
  }

  return {
    begin(textureId: number, width: number, height: number): number {
      options.live();
      const texture = options.texture(textureId >>> 0);
      const bytes = width * height * 4;
      if (
        !texture ||
        width <= 0 ||
        height <= 0 ||
        !Number.isSafeInteger(bytes) ||
        bytes > 0xffff_ffff
      )
        throw new Error("Invalid texture readback identity or dimensions");
      const target = options.target();
      const previous = gl.getParameter(
        gl.PIXEL_PACK_BUFFER_BINDING,
      ) as WebGLBuffer | null;
      const packNames = [
        gl.PACK_ALIGNMENT,
        gl.PACK_ROW_LENGTH,
        gl.PACK_SKIP_ROWS,
        gl.PACK_SKIP_PIXELS,
      ];
      const pack = packNames.map((name) => gl.getParameter(name) as number);
      const framebuffer = gl.createFramebuffer();
      const buffer = gl.createBuffer();
      let fence: WebGLSync | null = null;
      let retained = false;
      try {
        if (!framebuffer || !buffer)
          throw new Error("Texture staging allocation failed");
        options.bind(target.draw, framebuffer);
        gl.framebufferTexture2D(
          gl.READ_FRAMEBUFFER,
          gl.COLOR_ATTACHMENT0,
          gl.TEXTURE_2D,
          texture,
          0,
        );
        gl.readBuffer(gl.COLOR_ATTACHMENT0);
        if (
          gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER) !==
          gl.FRAMEBUFFER_COMPLETE
        )
          throw new Error("Texture readback framebuffer incomplete");
        gl.bindBuffer(gl.PIXEL_PACK_BUFFER, buffer);
        gl.bufferData(gl.PIXEL_PACK_BUFFER, bytes, gl.STREAM_READ);
        gl.pixelStorei(gl.PACK_ALIGNMENT, 1);
        for (const name of packNames.slice(1)) gl.pixelStorei(name, 0);
        // Uploaded storage row zero already denotes the authored top row. This
        // reads encoded SRGB8_ALPHA8 storage, with neither display flip nor
        // premultiplication/colour conversion.
        gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, 0);
        fence = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
        gl.flush(); // Paused Worlds need no later drawing/swap for submission.
        options.check();
        if (!fence) throw new Error("Texture readback fence allocation failed");
        const id = options.id();
        stages.set(id, { buffer, fence, bytes, ready: false });
        retained = true;
        return id;
      } finally {
        gl.bindBuffer(gl.PIXEL_PACK_BUFFER, previous);
        packNames.forEach((name, index) => gl.pixelStorei(name, pack[index]!));
        options.bind(target.draw, target.read);
        gl.deleteFramebuffer(framebuffer);
        if (!retained) {
          gl.deleteSync(fence);
          gl.deleteBuffer(buffer);
        }
      }
    },
    poll(id: number): number {
      options.live();
      const stage = stages.get(id >>> 0);
      if (!stage) throw new Error("Texture readback retired");
      if (stage.ready) return 2;
      const status = gl.clientWaitSync(stage.fence, 0, 0);
      if (status === gl.TIMEOUT_EXPIRED) return 1;
      if (status !== gl.ALREADY_SIGNALED && status !== gl.CONDITION_SATISFIED) {
        options.check();
        throw new Error("Texture readback fence failed");
      }
      stage.ready = true;
      return 2;
    },
    copy(id: number, offset: number, pointer: number, length: number): number {
      options.live();
      const stage = stages.get(id >>> 0);
      offset >>>= 0;
      pointer >>>= 0;
      length >>>= 0;
      if (!stage?.ready || offset + length > stage.bytes)
        throw new Error("Texture readback range unavailable");
      const destination = new Uint8Array(
        options.memory(pointer, length),
        pointer,
        length,
      );
      const previous = gl.getParameter(
        gl.PIXEL_PACK_BUFFER_BINDING,
      ) as WebGLBuffer | null;
      try {
        gl.bindBuffer(gl.PIXEL_PACK_BUFFER, stage.buffer);
        gl.getBufferSubData(gl.PIXEL_PACK_BUFFER, offset, destination);
        options.check();
        return 1;
      } finally {
        gl.bindBuffer(gl.PIXEL_PACK_BUFFER, previous);
      }
    },
    remove,
    reset(lost: boolean): void {
      for (const id of stages.keys()) remove(id, lost);
    },
  };
}
