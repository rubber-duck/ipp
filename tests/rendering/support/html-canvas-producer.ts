/** Experimental producer kept beside the maintained browser prototype until Host admission is approved. */
export interface ElementImageSnapshot {
  readonly width: number;
  readonly height: number;
  close(): void;
}

export interface HtmlDrawableCanvas extends HTMLCanvasElement {
  layoutSubtree: boolean;
  captureElementImage(element: Element): ElementImageSnapshot;
  requestPaint(): void;
}

export interface HtmlElementWebGl extends WebGL2RenderingContext {
  texElementSubImage2D(
    target: number,
    level: number,
    x: number,
    y: number,
    image: ElementImageSnapshot,
    config: { width: number; height: number },
  ): void;
}

export interface HtmlCanvasFrame {
  readonly image: ImageBitmap;
  readonly sequence: number;
  readonly width: number;
  readonly height: number;
  readonly capturedAt: number;
}

export class HtmlCanvasUnavailable extends Error {}

/** Latest Chrome's actual API, not a compatibility bridge for older versions. */
export function htmlCanvasAvailable(): boolean {
  return (
    "layoutSubtree" in HTMLCanvasElement.prototype &&
    "captureElementImage" in HTMLCanvasElement.prototype &&
    "requestPaint" in HTMLCanvasElement.prototype &&
    "texElementSubImage2D" in WebGL2RenderingContext.prototype
  );
}

/**
 * A single in-flight GPU snapshot. Paints while the consumer is busy coalesce;
 * no pixel buffer, image encoder, readback or ordinary IPP request is involved.
 * The harness must first verify GPU compositing: Chrome otherwise internally
 * reads transferToImageBitmap back into an unaccelerated bitmap.
 */
export class HtmlCanvasProducer {
  readonly canvas: HtmlDrawableCanvas;
  readonly context: HtmlElementWebGl;
  readonly offscreen: OffscreenCanvas;
  readonly #content: HTMLElement;
  readonly #send: (frame: HtmlCanvasFrame) => void;
  readonly #fail: (error: Error) => void;
  #texture: WebGLTexture | null = null;
  #framebuffer: WebGLFramebuffer | null = null;
  #width = 0;
  #height = 0;
  #sequence = 0;
  #inFlight = 0;
  #dirty = false;
  #disposed = false;
  #suspended = false;
  readonly statistics = {
    captured: 0,
    coalesced: 0,
    acknowledged: 0,
    closedBeforeTransfer: 0,
  };

  constructor(
    canvas: HTMLCanvasElement,
    content: HTMLElement,
    send: (frame: HtmlCanvasFrame) => void,
    fail: (error: Error) => void,
  ) {
    if (!htmlCanvasAvailable())
      throw new HtmlCanvasUnavailable("HTML-in-Canvas is unavailable");
    if (content.parentElement !== canvas)
      throw new Error("The content must belong to its capture canvas");
    this.canvas = canvas as HtmlDrawableCanvas;
    this.canvas.layoutSubtree = true;
    content.setAttribute("drawable", "");
    this.#content = content;
    this.#send = send;
    this.#fail = fail;
    this.offscreen = canvas.transferControlToOffscreen();
    const context = this.offscreen.getContext("webgl2", {
      alpha: true,
      antialias: false,
      depth: false,
      premultipliedAlpha: false,
      preserveDrawingBuffer: false,
    });
    if (!context || !("texElementSubImage2D" in context))
      throw new HtmlCanvasUnavailable("HTML-in-Canvas WebGL is unavailable");
    this.context = context as HtmlElementWebGl;
    this.canvas.addEventListener("paint", this.#paint);
    this.canvas.requestPaint();
  }

  #paint = (): void => {
    if (this.#disposed || this.#suspended) return;
    if (this.#inFlight) {
      this.#dirty = true;
      this.statistics.coalesced++;
      return;
    }
    let captured: ElementImageSnapshot | undefined;
    let image: ImageBitmap | undefined;
    try {
      captured = this.canvas.captureElementImage(this.#content);
      const width = Math.ceil(captured.width),
        height = Math.ceil(captured.height);
      const gl = this.context;
      const limit = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
      if (width < 1 || height < 1 || width > limit || height > limit)
        throw new Error("HTML canvas dimensions are outside the GPU limits");
      if (width !== this.#width || height !== this.#height) {
        if (this.#texture) gl.deleteTexture(this.#texture);
        if (this.#framebuffer) gl.deleteFramebuffer(this.#framebuffer);
        this.offscreen.width = width;
        this.offscreen.height = height;
        this.#texture = gl.createTexture();
        this.#framebuffer = gl.createFramebuffer();
        if (!this.#texture || !this.#framebuffer)
          throw new Error("HTML canvas GPU allocation failed");
        gl.bindTexture(gl.TEXTURE_2D, this.#texture);
        gl.texImage2D(
          gl.TEXTURE_2D,
          0,
          gl.RGBA8,
          width,
          height,
          0,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          null,
        );
        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.#framebuffer);
        gl.framebufferTexture2D(
          gl.READ_FRAMEBUFFER,
          gl.COLOR_ATTACHMENT0,
          gl.TEXTURE_2D,
          this.#texture,
          0,
        );
        if (
          gl.checkFramebufferStatus(gl.READ_FRAMEBUFFER) !==
          gl.FRAMEBUFFER_COMPLETE
        )
          throw new Error("HTML canvas framebuffer is incomplete");
        this.#width = width;
        this.#height = height;
      }
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
      gl.bindTexture(gl.TEXTURE_2D, this.#texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
      gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
      // Default upload sizing also scales with the associated canvas; name
      // destination dimensions explicitly, including the first paint after resize.
      gl.texElementSubImage2D(gl.TEXTURE_2D, 0, 0, 0, captured, {
        width,
        height,
      });
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, this.#framebuffer);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
      // HTML snapshots are top-left; the GL drawing buffer has a bottom-left origin.
      gl.blitFramebuffer(
        0,
        0,
        width,
        height,
        0,
        height,
        width,
        0,
        gl.COLOR_BUFFER_BIT,
        gl.NEAREST,
      );
      const error = gl.getError();
      if (error !== gl.NO_ERROR || gl.isContextLost())
        throw new Error(
          `HTML canvas GPU snapshot failed: GL ${error}, lost ${gl.isContextLost()}`,
        );
      image = this.offscreen.transferToImageBitmap();
      const sequence = ++this.#sequence;
      this.#inFlight = sequence;
      this.#dirty = false;
      this.statistics.captured++;
      this.#send({
        image,
        sequence,
        width,
        height,
        capturedAt: performance.timeOrigin + performance.now(),
      });
      image = undefined;
    } catch (error) {
      this.dispose();
      this.#fail(error instanceof Error ? error : new Error(String(error)));
    } finally {
      captured?.close();
      if (image) {
        image.close();
        this.statistics.closedBeforeTransfer++;
      }
    }
  };

  acknowledge(sequence: number): void {
    if (sequence !== this.#inFlight || this.#disposed) return;
    this.#inFlight = 0;
    this.statistics.acknowledged++;
    if (this.#dirty) this.canvas.requestPaint();
  }

  request(): void {
    if (!this.#disposed && !this.#suspended) this.canvas.requestPaint();
  }

  suspend(): void {
    this.#suspended = true;
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.canvas.removeEventListener("paint", this.#paint);
    const gl = this.context;
    gl.deleteTexture(this.#texture);
    gl.deleteFramebuffer(this.#framebuffer);
    this.#texture = null;
    this.#framebuffer = null;
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  }
}
