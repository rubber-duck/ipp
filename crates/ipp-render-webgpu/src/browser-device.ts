/** Browser-only bindings of the worker's WebGPU renderer. */
interface ElementImage {
  readonly width: number;
  readonly height: number;
  close(): void;
}

interface BrowserGpuDevice {
  readonly lost: Promise<{ reason: string; message: string }>;
  readonly queue: {
    drawElementImageToTexture?(
      source: { source: ElementImage },
      destination: {
        texture: { width: number; height: number };
        size: { width: number; height: number };
      },
    ): void;
  };
  destroy(): void;
}

export async function createWebGpuDevice(
  canvas: OffscreenCanvas,
  fail: (error: Error) => void,
) {
  const bindings = await import(
    new URL("bindings_bg.js", import.meta.url).href
  );
  let device: BrowserGpuDevice | undefined;
  let lost = false;
  let disposed = false;
  let readback: OffscreenCanvasRenderingContext2D | null = null;
  let staged: ElementImage | undefined;
  let stage = 0;
  const upload = (id: number, texture: { width: number; height: number }) => {
    if (!staged || id !== stage || !device?.queue.drawElementImageToTexture) {
      throw new Error("WebGPU element image import is unavailable");
    }
    device.queue.drawElementImageToTexture(
      { source: staged },
      { texture, size: { width: texture.width, height: texture.height } },
    );
  };
  return {
    bindings,
    async start(exports: WebAssembly.Exports) {
      bindings.__wbg_set_wasm(exports);
      (exports.__wbindgen_start as () => void)();
      const active: BrowserGpuDevice = await bindings.open_webgpu(
        canvas,
        upload,
      );
      if (disposed) {
        active.destroy();
        return;
      }
      device = active;
      void active.lost.then(({ reason, message }) => {
        lost = true;
        if (!disposed)
          fail(new Error(`WebGPU device lost (${reason}): ${message}`));
      });
    },
    stageHtmlImage(image: ElementImage) {
      if (staged || stage === 0xffffffff) {
        image.close();
        throw new Error("HTML image staging capacity exhausted");
      }
      staged = image;
      return ++stage;
    },
    releaseWebImage(id: number) {
      if (id === stage) {
        staged?.close();
        staged = undefined;
      }
    },
    capture() {
      // Only explicit capture requests pay for a synchronous browser readback.
      readback ??= new OffscreenCanvas(canvas.width, canvas.height).getContext(
        "2d",
        { willReadFrequently: true },
      );
      if (!readback) throw new Error("WebGPU readback canvas unavailable");
      if (readback.canvas.width !== canvas.width)
        readback.canvas.width = canvas.width;
      if (readback.canvas.height !== canvas.height)
        readback.canvas.height = canvas.height;
      readback.drawImage(canvas, 0, 0);
      return new Uint8Array(
        readback.getImageData(0, 0, canvas.width, canvas.height).data.buffer,
      );
    },
    info: () => ({ api: "WebGPU" }),
    isContextLost: () => lost,
    dispose() {
      disposed = true;
      staged?.close();
      staged = undefined;
      device?.destroy();
      readback = null;
    },
  };
}
