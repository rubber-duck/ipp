/** Exercise only the published files through HTTP, the real worker and WebGL. */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { gzipSync } from "node:zlib";
import test from "node:test";
import { chromium } from "playwright";
import { browserLaunchOptions } from "../build/browser-options.mjs";

const site = resolve("target/gallery-site");
const evidenceRoot = resolve("target/integration-artifacts/gallery-site");
const mime = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css",
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
};

async function serveSite(prefix) {
  const requests = [];
  const server = createServer(async (request, response) => {
    const path = decodeURIComponent(
      new URL(request.url, "http://localhost").pathname,
    );
    try {
      assert.ok(path.startsWith(prefix), `Request escaped the site: ${path}`);
      const file = resolve(site, path.slice(prefix.length) || "index.html");
      assert.ok(file.startsWith(site + sep));
      const bytes = await readFile(file);
      // Match Pages' automatic gzip and weak validators. Recovery from an
      // explicit eviction remains a separate runtime concern, outside packaging.
      const compressed =
        bytes.length > 256 &&
        request.headers["accept-encoding"]?.includes("gzip");
      const body = compressed ? gzipSync(bytes) : bytes;
      response.writeHead(200, {
        "Content-Type": mime[extname(file)] ?? "application/octet-stream",
        "Content-Length": body.length,
        "Cache-Control": "max-age=600",
        ETag: `${compressed ? "W/" : ""}"${createHash("sha256").update(bytes).digest("hex")}"`,
        ...(compressed
          ? { "Content-Encoding": "gzip", Vary: "Accept-Encoding" }
          : {}),
      });
      response.end(body);
      requests.push({ path, status: 200 });
    } catch {
      requests.push({ path, status: 404 });
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((done, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", done);
  });
  return {
    url: `http://127.0.0.1:${server.address().port}${prefix}`,
    requests,
    async close() {
      const closed = new Promise((done) => server.close(done));
      server.closeAllConnections();
      await closed;
    },
  };
}

async function captureScene(page) {
  return page.evaluate(async () => {
    const handle = window.ippWorldCanvas;
    if (!handle) throw new Error("Gallery did not expose its live canvas");
    const deadline = performance.now() + 60_000;
    for (;;) {
      await handle.flush();
      const state = await handle.client.inspect();
      const failed = state.resources.find(
        (resource) => resource.status === "failed",
      );
      if (failed) throw new Error(`${failed.source}: ${failed.error}`);
      if (state.resources.every((resource) => resource.status === "loaded")) {
        if (state.renderDiagnostics.length)
          throw new Error(JSON.stringify(state.renderDiagnostics));
        const frame = await handle.capture();
        if (frame.tick < state.tick)
          throw new Error("Capture predates the inspected World");
        const pixels = new Uint8ClampedArray(frame.pixels);
        let foreground = 0;
        for (let offset = 0; offset < pixels.length; offset += 4) {
          const difference =
            Math.abs(pixels[offset] - pixels[0]) +
            Math.abs(pixels[offset + 1] - pixels[1]) +
            Math.abs(pixels[offset + 2] - pixels[2]);
          if (difference > 40 && pixels[offset + 3] > 200) foreground++;
        }
        const canvas = document.createElement("canvas");
        canvas.width = frame.width;
        canvas.height = frame.height;
        canvas
          .getContext("2d")
          .putImageData(new ImageData(pixels, frame.width, frame.height), 0, 0);
        return {
          width: frame.width,
          height: frame.height,
          drawCalls: frame.drawCalls,
          triangles: frame.triangles,
          foreground,
          entities: state.entities.length,
          sources: state.resources.map(({ source }) => source),
          png: canvas.toDataURL("image/png"),
        };
      }
      if (performance.now() >= deadline)
        throw new Error("Gallery resources did not finish loading");
      await handle.client.waitForFrame(state.tick);
    }
  });
}

async function selectScene(page, scene) {
  await page.locator("#scene-picker-trigger").click();
  await page.locator(`#world-${scene}`).click();
}

test("the release gallery is complete and renders at root and project URLs", {
  timeout: 240_000,
}, async () => {
  const report = JSON.parse(
    await readFile(resolve(site, "build-report.json"), "utf8"),
  );
  for (const entry of report.artifacts) {
    assert.ok(
      !/\.(?:tsx?|map)$/.test(entry.path),
      `Development file published: ${entry.path}`,
    );
    assert.ok(
      !/(?:export\.wasm|fixture|authoring|hierarchy)/.test(entry.path),
      `Unrelated file published: ${entry.path}`,
    );
    const bytes = await readFile(resolve(site, entry.path));
    assert.equal(
      createHash("sha256").update(bytes).digest("hex"),
      entry.sha256,
    );
  }
  await mkdir(evidenceRoot, { recursive: true });
  const evidence = await mkdtemp(resolve(evidenceRoot, "run-"));
  const browser = await chromium.launch(browserLaunchOptions());
  try {
    for (const prefix of ["/", "/preview/ipp/"]) {
      const server = await serveSite(prefix);
      const context = await browser.newContext({
        viewport: { width: 1280, height: 900 },
      });
      const page = await context.newPage();
      page.setDefaultTimeout(60_000);
      const errors = [];
      const logs = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) =>
        logs.push({ type: message.type(), text: message.text() }),
      );
      const label = prefix === "/" ? "root" : "project";
      try {
        // The fragment also proves direct links do not require an SPA rewrite.
        const initialScene = prefix === "/" ? "gui" : "platformer";
        await page.goto(`${server.url}#${initialScene}`);
        for (const [index, scene] of [
          initialScene,
          "platformer",
          "shapes",
          "lighting",
          "particles",
          "gui",
          "shapes",
        ].entries()) {
          if (index > 0) await selectScene(page, scene);
          await page.waitForFunction((expected) => {
            const state = document.querySelector("#status")?.dataset.state;
            return (
              document.querySelector(".viewer-shell")?.dataset.page ===
                expected &&
              (state === "ready" || state === "error")
            );
          }, scene);
          assert.equal(
            await page.locator("#status").getAttribute("data-state"),
            "ready",
            await page.locator("#status").innerText(),
          );
          const { png, ...frame } = await captureScene(page);
          await writeFile(
            resolve(evidence, `${label}-${scene}.png`),
            Buffer.from(png.split(",")[1], "base64"),
          );
          await writeFile(
            resolve(evidence, `${label}-${scene}.json`),
            JSON.stringify(frame, null, 2),
          );
          assert.ok(
            frame.drawCalls > 0 && frame.triangles > 0,
            `${scene}: no rendered geometry`,
          );
          assert.ok(
            frame.foreground > 1000,
            `${scene}: captured frame is blank`,
          );
          assert.ok(frame.entities > 0, `${scene}: no World entities`);
        }
        assert.deepEqual(errors, []);
        assert.deepEqual(
          server.requests.filter(({ status }) => status !== 200),
          [],
        );
        for (const required of [
          "runtime/runtime.wasm",
          "assets/shared/shure-tech-mono.ippf",
          "assets/platformer/platformer.ipp",
          "assets/gui/projector/shell.ippm",
        ])
          assert.ok(
            server.requests.some(({ path }) => path === prefix + required),
            `Missing real resource request: ${required}`,
          );
      } catch (error) {
        await page
          .screenshot({ path: resolve(evidence, `${label}-failure.png`) })
          .catch(() => {});
        throw error;
      } finally {
        await writeFile(
          resolve(evidence, `${label}-network.json`),
          JSON.stringify(server.requests, null, 2),
        );
        await writeFile(
          resolve(evidence, `${label}-console.json`),
          JSON.stringify({ errors, logs }, null, 2),
        );
        await context.close();
        await server.close();
      }
    }
  } finally {
    await browser.close();
  }
  console.log(`Static gallery evidence: ${evidence}`);
});
