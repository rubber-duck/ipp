import {
  HtmlCanvasProducer,
  htmlCanvasAvailable,
} from "../support/html-canvas-producer.js";

export async function create(device = false) {
  if (!htmlCanvasAvailable()) return { available: false };
  const captureCanvas = document.createElement("canvas");
  captureCanvas.width = 64;
  captureCanvas.height = 64;
  const content = document.createElement("div");
  content.style.cssText =
    "width:64px;height:64px;background:rgba(128,64,32,.5);position:relative";
  const child = document.createElement("canvas");
  child.width = child.height = 16;
  child.style.cssText = "display:block;position:absolute;left:0;top:0";
  content.append(child);
  captureCanvas.append(content);
  const presentation = document.createElement("canvas");
  presentation.width = presentation.height = 64;
  document.body.replaceChildren(captureCanvas, presentation);
  const ctx = child.getContext("2d")!;
  ctx.fillStyle = "#00ff00";
  ctx.fillRect(0, 0, 16, 16);
  const worker = new Worker(
    new URL(
      device ? "./html-canvas-device-worker.js" : "./html-canvas-worker.js",
      import.meta.url,
    ),
    { type: "module" },
  );
  const queued: Record<string, unknown>[] = [];
  let waiting:
    | {
        type: string;
        resolve: (value: any) => void;
        reject: (error: Error) => void;
        timer: ReturnType<typeof setTimeout>;
      }
    | undefined;
  let producer!: HtmlCanvasProducer;
  let holdAfter = Infinity;
  let pendingAck = 0;
  let generation = 1;
  let failed: Error | undefined;
  const latencies: number[] = [];
  function next(type: string): Promise<any> {
    if (failed) return Promise.reject(failed);
    const index = queued.findIndex((value) => value.type === type);
    if (index >= 0) return Promise.resolve(queued.splice(index, 1)[0]);
    if (waiting) throw new Error("Only one test observation may wait");
    return new Promise((resolve, reject) => {
      waiting = {
        type,
        resolve,
        reject,
        timer: setTimeout(() => {
          waiting = undefined;
          reject(new Error(`Timed out waiting for ${type}`));
        }, 10_000),
      };
    });
  }
  function failure(error: Error): void {
    failed = error;
    if (waiting) {
      clearTimeout(waiting.timer);
      waiting.reject(error);
      waiting = undefined;
    }
  }
  worker.onerror = (event) => failure(new Error(event.message));
  worker.onmessage = ({ data }) => {
    if (data.type === "error") {
      failure(new Error(data.error));
      return;
    }
    if (data.type === "frame") {
      latencies.push(data.latencyMs);
      if (data.sequence > holdAfter) pendingAck = data.sequence;
      else producer.acknowledge(data.sequence);
    }
    if (waiting && waiting.type === data.type) {
      const current = waiting;
      waiting = undefined;
      clearTimeout(current.timer);
      current.resolve(data);
    } else queued.push(data);
  };
  try {
    const offscreen = presentation.transferControlToOffscreen();
    worker.postMessage({ canvas: offscreen }, [offscreen]);
    const info = await next("ready");
    if (
      typeof info.renderer !== "string" ||
      /swiftshader|llvmpipe|softpipe|software|lavapipe|microsoft basic|webkit webgl/i.test(
        info.renderer,
      )
    )
      throw new Error(
        "Hardware worker WebGL is required before source capture",
      );
    producer = new HtmlCanvasProducer(
      captureCanvas,
      content,
      (frame) => worker.postMessage({ frame, generation }, [frame.image]),
      failure,
    );
    await next("frame");
    async function fresh(after: number): Promise<void> {
      while ((await next("frame")).sequence <= after) {}
    }
    const call = async (operation: string, reply: string) => {
      worker.postMessage({ operation });
      return next(reply);
    };
    const driver = {
      available: true,
      info,
      async capture() {
        return call("capture", "capture");
      },
      async update(color: string, width = 64, height = 64) {
        const before = producer.statistics.captured;
        content.style.width = `${width}px`;
        content.style.height = `${height}px`;
        ctx.fillStyle = color;
        ctx.fillRect(0, 0, 16, 16);
        producer.request();
        await fresh(before);
        return driver.capture();
      },
      async originPolicy() {
        const frames = ["same-frame", "cross-frame"].map((path, index) => {
          const frame = document.createElement("iframe");
          frame.style.cssText = `position:absolute;left:${20 + index * 20}px;top:0;width:16px;height:16px;border:0`;
          const url = new URL(path, location.href);
          if (index === 1) url.hostname = "localhost";
          const loaded = new Promise<void>((resolve, reject) => {
            const timer = setTimeout(
              () => reject(new Error("Origin fixture did not load")),
              5000,
            );
            frame.onload = () => {
              clearTimeout(timer);
              resolve();
            };
            frame.onerror = () => {
              clearTimeout(timer);
              reject(new Error("Origin fixture failed"));
            };
          });
          frame.src = url.href;
          content.append(frame);
          return { frame, loaded };
        });
        try {
          await Promise.all(frames.map((value) => value.loaded));
          const before = producer.statistics.captured;
          producer.request();
          await fresh(before);
          return await driver.capture();
        } finally {
          for (const { frame } of frames) frame.remove();
          const before = producer.statistics.captured;
          producer.request();
          await fresh(before);
        }
      },
      async coalesce() {
        const initial = producer.statistics.captured;
        holdAfter = initial;
        ctx.fillStyle = "#ff0000";
        ctx.fillRect(0, 0, 16, 16);
        producer.request();
        await fresh(initial);
        for (const color of ["#ff00ff", "#ffff00", "#0000ff"]) {
          const painted = new Promise<void>((resolve, reject) => {
            const timer = setTimeout(
              () => reject(new Error("Paint coalescing timed out")),
              5000,
            );
            captureCanvas.addEventListener(
              "paint",
              () => {
                clearTimeout(timer);
                resolve();
              },
              { once: true },
            );
          });
          ctx.fillStyle = color;
          ctx.fillRect(0, 0, 16, 16);
          producer.request();
          await painted;
        }
        const before = { ...producer.statistics };
        holdAfter = Infinity;
        producer.acknowledge(pendingAck);
        await fresh(before.captured);
        return {
          before,
          after: { ...producer.statistics },
          capture: await driver.capture(),
        };
      },
      async deviceChecks() {
        producer.suspend();
        return call("deviceChecks", "deviceChecked");
      },
      async recoverExact() {
        producer.suspend();
        content.style.background = "rgb(255,0,255)";
        ctx.fillStyle = "#ff0000";
        ctx.fillRect(0, 0, 16, 16);
        return call("recover", "recovered");
      },
      async rejectStale() {
        await call("invalidate", "invalidated");
        // One immutable browser snapshot is transferred back through the producer's
        // already-established path with the obsolete generation, then closed.
        generation = 1;
        const replacement = new OffscreenCanvas(8, 8);
        const rc = replacement.getContext("webgl2", { antialias: false })!;
        rc.clearColor(1, 0, 0, 1);
        rc.clear(rc.COLOR_BUFFER_BIT);
        const image = replacement.transferToImageBitmap();
        worker.postMessage(
          {
            generation,
            frame: {
              image,
              sequence: 999,
              width: 8,
              height: 8,
              capturedAt: performance.timeOrigin + performance.now(),
            },
          },
          [image],
        );
        return next("rejected");
      },
      async dispose() {
        producer.suspend();
        try {
          return await call("dispose", "disposed");
        } finally {
          producer.dispose();
          worker.terminate();
          captureCanvas.remove();
          presentation.remove();
        }
      },
      statistics() {
        return { producer: { ...producer.statistics }, latencies };
      },
    };
    (globalThis as any).htmlCanvasDriver = driver;
    return { available: true, info };
  } catch (error) {
    producer?.dispose();
    worker.terminate();
    captureCanvas.remove();
    presentation.remove();
    throw error;
  }
}
