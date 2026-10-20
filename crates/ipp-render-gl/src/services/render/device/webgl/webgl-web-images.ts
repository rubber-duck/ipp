/** Snapshot captured by this context's associated HTMLCanvasElement. */
export interface WebGlElementImage {
  readonly width: number;
  readonly height: number;
  close(): void;
}

/** One-use browser snapshots; the Host reserves total image capacity before staging. */
export function createWebGlWebImages(
  gl: WebGL2RenderingContext,
  options: {
    id(): number;
    live(): void;
    check(): void;
    texture(
      id: number,
    ): { object: WebGLTexture; width: number; height: number } | undefined;
  },
) {
  // This adapter's additional staging bound is not the Host's total image budget.
  const maximumBytes = 64 * 1024 * 1024;
  let pending:
    | { id: number; image: ImageBitmap | WebGlElementImage; element: boolean }
    | undefined;

  function release(id: number): void {
    if (pending?.id !== id) return;
    const image = pending.image;
    pending = undefined;
    image.close();
  }

  return {
    stage(image: ImageBitmap | WebGlElementImage, element = false): number {
      let retained = false;
      try {
        options.live();
        if (pending) throw new Error("Web image staging capacity exhausted");
        // ElementImage extents can be fractional after DOM/page zoom.
        const bytes = Math.ceil(image.width) * Math.ceil(image.height) * 4;
        if (
          image.width <= 0 ||
          image.height <= 0 ||
          !Number.isSafeInteger(bytes) ||
          bytes > maximumBytes
        )
          throw new Error(
            "Invalid web image or staging byte capacity exceeded",
          );
        const id = options.id();
        pending = { id, image, element };
        retained = true;
        return id;
      } finally {
        if (!retained && pending?.image !== image) image.close();
      }
    },
    copy(
      id: number,
      destination: number,
      width: number,
      height: number,
    ): number {
      const image = pending?.id === id ? pending.image : undefined;
      try {
        options.live();
        const texture = options.texture(destination);
        if (!image || !texture)
          throw new Error("Stale web image or destination texture");
        if (
          (!pending?.element &&
            (image.width !== width || image.height !== height)) ||
          texture.width !== width ||
          texture.height !== height
        )
          throw new Error(
            `Web image extent ${image.width}x${image.height}, destination ${texture.width}x${texture.height}, requested ${width}x${height}`,
          );
        // Leave the current texture unit selected. Preserve its binding and all
        // unpack state; no framebuffer, program, VAO or renderer cache is touched.
        const previous = gl.getParameter(
          gl.TEXTURE_BINDING_2D,
        ) as WebGLTexture | null;
        const unpackBuffer = gl.getParameter(
          gl.PIXEL_UNPACK_BUFFER_BINDING,
        ) as WebGLBuffer | null;
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
        const values = names.map((name) => gl.getParameter(name) as number);
        try {
          gl.bindTexture(gl.TEXTURE_2D, texture.object);
          gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, null);
          names.forEach((name) =>
            gl.pixelStorei(name, name === gl.UNPACK_ALIGNMENT ? 1 : 0),
          );
          // The admitted producer supplies top-left, straight-alpha sRGB pixels.
          // ImageBitmap's own orientation/premultiplication is fixed at creation;
          // WebGL unpack flags cannot normalize an arbitrary bitmap producer.
          if (pending?.element) {
            // ElementImage authority belongs to this exact canvas, including
            // its transferred OffscreenCanvas. No ImageBitmap/readback bridge.
            // Its zoomed DOM extent need not match the texture: rasterize the
            // full snapshot into the admitted destination extent explicitly.
            const context = gl as WebGL2RenderingContext & {
              texElementSubImage2D(
                target: number,
                level: number,
                x: number,
                y: number,
                image: WebGlElementImage,
                options: { width: number; height: number },
              ): void;
            };
            context.texElementSubImage2D(gl.TEXTURE_2D, 0, 0, 0, image, {
              width,
              height,
            });
          } else
            gl.texSubImage2D(
              gl.TEXTURE_2D,
              0,
              0,
              0,
              gl.RGBA,
              gl.UNSIGNED_BYTE,
              image as ImageBitmap,
            );
          options.check();
          return 1;
        } finally {
          gl.bindTexture(gl.TEXTURE_2D, previous);
          gl.bindBuffer(gl.PIXEL_UNPACK_BUFFER, unpackBuffer);
          names.forEach((name, index) => gl.pixelStorei(name, values[index]!));
        }
      } finally {
        // WebGL takes the image source during this synchronous call. Closing the
        // JS snapshot does not retire the renderer-owned destination texture.
        release(id);
      }
    },
    release,
    reset(): void {
      if (pending) release(pending.id);
    },
  };
}
