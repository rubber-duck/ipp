import { createServer } from "node:http";
import { once } from "node:events";
import { readFile, writeFile, mkdir, mkdtemp } from "node:fs/promises";
import { resolve, join } from "node:path";
import { chromium } from "playwright";
import {
  browserLaunchOptions,
  browserPin,
  verifyHardwareRenderer,
} from "#ipp-browser-options";
import { encodePng } from "../../harness/images.js";
import type {
  HtmlCanvasCapture,
  HtmlCanvasDriver,
} from "../scenarios/html-canvas.js";

export async function withHtmlCanvas<T>(
  enabled: boolean,
  action: (driver: HtmlCanvasDriver) => Promise<T>,
  device = false,
) {
  const parent = resolve("target/html-canvas-evidence");
  await mkdir(parent, { recursive: true });
  const evidence = await mkdtemp(join(parent, "run-"));
  const allowed = new Set([
    "html-canvas.js",
    "html-canvas-worker.js",
    "html-canvas-device-worker.js",
  ]);
  const shaders = new Set([
    "surface_bitmap.vert",
    "surface_bitmap.frag",
    "present.vert",
    "present.frag",
  ]);
  const server = createServer(async (request, response) => {
    const name = request.url?.slice(1);
    if (name === "same-frame" || name === "cross-frame") {
      response.setHeader("Content-Type", "text/html");
      response.end(
        `<body style="margin:0;background:${name === "same-frame" ? "#00ff00" : "#ff00ff"}"></body>`,
      );
      return;
    }
    if (name === "favicon.ico") {
      response.writeHead(204);
      response.end();
      return;
    }
    if (name === "") {
      response.setHeader("Content-Type", "text/html");
      response.end("<!doctype html><title>HTML canvas GPU prototype</title>");
      return;
    }
    if (!name || (!allowed.has(name) && !shaders.has(name))) {
      response.writeHead(404);
      response.end();
      return;
    }
    try {
      response.setHeader("Content-Type", "text/javascript");
      response.end(
        await readFile(
          resolve(
            shaders.has(name)
              ? "crates/ipp-render-gl/src/services/render/shaders"
              : "target/html-canvas",
            name,
          ),
        ),
      );
    } catch {
      response.writeHead(500);
      response.end();
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined;
  let page: import("playwright").Page | undefined;
  try {
    const options = browserLaunchOptions(true, { htmlInCanvas: enabled });
    // transferToImageBitmap has a CPU readback branch without GPU compositing.
    options.args = (options.args ?? []).filter(
      (argument) => argument !== "--disable-vulkan-surface",
    );
    browser = await chromium.launch(options);
    const cdp = await browser.newBrowserCDPSession();
    const gpu = await cdp.send("SystemInfo.getInfo");
    await writeFile(
      join(evidence, "environment.json"),
      JSON.stringify(
        { browser: browser.version(), pin: browserPin, options, gpu },
        null,
        2,
      ),
    );
    page = await browser.newPage();
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    page.on("console", (message) => {
      if (message.type() === "error") errors.push(message.text());
    });
    const address = server.address();
    if (!address || typeof address === "string")
      throw new Error("Missing loopback listener");
    await page.goto(`http://127.0.0.1:${address.port}/`);
    if (enabled && gpu.gpu.featureStatus?.gpu_compositing !== "enabled")
      throw new Error(
        `GPU compositing required before capture; found ${gpu.gpu.featureStatus?.gpu_compositing}`,
      );
    const state = await page.evaluate(
      async ({ moduleUrl, device }) => (await import(moduleUrl)).create(device),
      { moduleUrl: "/html-canvas.js", device },
    );
    if (!enabled) {
      if (state.available)
        throw new Error(
          "HTML-in-Canvas should report unavailable with its flag disabled",
        );
      return { evidence, unavailable: true };
    }
    if (!state.available) throw new Error("HTML-in-Canvas is unavailable");
    verifyHardwareRenderer(state.info.renderer);
    const driver: HtmlCanvasDriver = {
      call: (operation, args = []) =>
        page!.evaluate(
          ({ operation, args }) =>
            (globalThis as any).htmlCanvasDriver[operation](...args),
          { operation, args },
        ),
      save: async (label: string, frame: HtmlCanvasCapture) => {
        await writeFile(
          join(evidence, `${label}.png`),
          encodePng({
            width: frame.width,
            height: frame.height,
            pixels: Uint8Array.from(frame.pixels),
          }),
        );
      },
    };
    const result = await action(driver);
    await writeFile(
      join(evidence, "result.json"),
      JSON.stringify(result, null, 2),
    );
    if (errors.length) throw new Error(errors.join("\n"));
    return { evidence, result };
  } catch (error) {
    await writeFile(join(evidence, "failure.txt"), String(error));
    await page
      ?.screenshot({ path: join(evidence, "failure.png") })
      .catch(() => {});
    throw new Error(`${String(error)}; evidence: ${evidence}`, {
      cause: error,
    });
  } finally {
    try {
      await browser?.close();
    } finally {
      await new Promise<void>((done, reject) =>
        server.close((error) => (error ? reject(error) : done())),
      );
    }
  }
}
