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
    const handle = window.ippDemoCanvas;
    if (!handle) throw new Error("Gallery did not expose its live canvas");
    const output = window.ippDemoApplication.sceneOutput;
    const client = handle.client;
    const deadline = performance.now() + 60_000;
    for (;;) {
      await window.ippDemoApplication.flush();
      await handle.flush();
      const state = await client.inspect();
      const cockpit =
        window.ippDemoApplication.sceneId === "cockpit"
          ? await window.ippDemoApplication.inspect()
          : undefined;
      const resources = [
        ...state.resources,
        ...(cockpit?.panels.flatMap((panel) => panel.resources) ?? []),
      ];
      const failed = resources.find((resource) => resource.status === "failed");
      if (failed) throw new Error(`${failed.source}: ${failed.error}`);
      if (resources.every((resource) => resource.status === "loaded")) {
        if (state.renderDiagnostics.length)
          throw new Error(JSON.stringify(state.renderDiagnostics));
        const focusedPanel =
          cockpit?.panels[Math.round(cockpit.options.carousel)];
        const frame = await handle.capture({
          afterOutputs: [
            ...window.ippDemoApplication.frameOutputs,
            ...(focusedPanel
              ? [{ kind: "canvas", world: focusedPanel.world }]
              : []),
          ],
        });
        const source = frame.sources.find(
          (source) =>
            source.output.entity === output.entity &&
            source.output.world.id === output.world.id &&
            source.output.world.incarnation === output.world.incarnation,
        );
        if (!source || source.tick < state.tick)
          throw new Error("Capture predates the inspected World");
        const { width, height } = frame.view.binding.viewport;
        const pixels = new Uint8ClampedArray(frame.pixels);
        let foreground = 0;
        let fallbackRed = 0;
        let blueInk = 0;
        for (let offset = 0; offset < pixels.length; offset += 4) {
          if (
            pixels[offset] > 240 &&
            pixels[offset + 1] < 16 &&
            pixels[offset + 2] < 16
          )
            fallbackRed++;
          if (
            pixels[offset + 2] > pixels[offset] + 20 &&
            pixels[offset + 1] > 70
          )
            blueInk++;
          const difference =
            Math.abs(pixels[offset] - pixels[0]) +
            Math.abs(pixels[offset + 1] - pixels[1]) +
            Math.abs(pixels[offset + 2] - pixels[2]);
          if (difference > 40 && pixels[offset + 3] > 200) foreground++;
        }
        const canvas = document.createElement("canvas");
        canvas.width = width;
        canvas.height = height;
        canvas
          .getContext("2d")
          .putImageData(new ImageData(pixels, width, height), 0, 0);
        return {
          width,
          height,
          drawCalls: frame.drawCalls,
          triangles: frame.triangles,
          foreground,
          fallbackRed,
          blueInk,
          sequence: String(frame.sequence),
          cockpit: cockpit && {
            sceneWorld: `${output.world.incarnation}:${output.world.id}`,
            focusedPanel: Math.round(cockpit.options.carousel),
            panels: cockpit.panels.map((panel) => ({
              world: `${panel.world.incarnation}:${panel.world.id}`,
              entities: panel.entities.length,
              sources: panel.resources.map(({ source }) => source),
              completed: frame.sources.some(
                (source) =>
                  source.output.kind === "canvas" &&
                  source.output.world.id === panel.world.id &&
                  source.output.world.incarnation === panel.world.incarnation &&
                  source.tick >= panel.tick,
              ),
            })),
            exterior: [
              "cockpit-space",
              "cockpit-planet",
              "cockpit-atmosphere",
              "cockpit-starfield",
              "cockpit-bridge-front",
            ].map((name) => {
              const entity = state.entities.find(
                (entity) => entity.metadata.symbolicId === name,
              );
              const material = entity?.components.find(
                (component) =>
                  component.component === client.components.CustomMaterial.id,
              );
              return {
                name,
                custom: !!material,
                properties: Object.keys(material?.properties ?? {}),
              };
            }),
          },
          entities: state.entities.length,
          sources: state.resources.map(({ source }) => source),
          png: canvas.toDataURL("image/png"),
        };
      }
      if (performance.now() >= deadline)
        throw new Error("Gallery resources did not finish loading");
      await client.waitForFrame(state.tick);
    }
  });
}

async function press(page, id) {
  // Browser mouse events enqueue transport work. A declaration flush alone
  // does not establish that the previous click opened and laid out its menu.
  await page.waitForFunction(async (id) => {
    const app = window.ippDemoApplication;
    const canvas = window.ippDemoCanvas;
    await app.flush();
    await canvas.frame({ afterOutputs: app.frameOutputs });
    const root = await canvas.host.openWorld(app.output.world);
    try {
      const state = await root.inspect();
      const entity = state.entities.find(
        (entity) => entity.metadata.symbolicId === id,
      );
      const fields = (entity, name) =>
        entity?.components.find(
          (component) => component.component === root.components[name].id,
        )?.fields;
      const bounds = fields(entity, "CanvasBounds");
      if (id.startsWith("demo-scene-picker/options/")) {
        const menu = state.entities.find(
          (entity) => entity.metadata.symbolicId === "demo-scene-picker/list",
        );
        if (
          !fields(menu, "GuiOverlay") ||
          fields(menu, "GuiBehavior")?.visible !== true
        )
          return false;
      }
      return (
        bounds &&
        [bounds.x, bounds.y, bounds.width, bounds.height].every(
          Number.isFinite,
        ) &&
        bounds.width > 0 &&
        bounds.height > 0
      );
    } finally {
      await root.close();
    }
  }, id);
  const point = await page.evaluate(async (id) => {
    const app = window.ippDemoApplication;
    const canvas = window.ippDemoCanvas;
    const root = await canvas.host.openWorld(app.output.world);
    try {
      const state = await root.inspect();
      const entity = state.entities.find(
        (entity) => entity.metadata.symbolicId === id,
      );
      const bounds = entity.components.find(
        (component) => component.component === root.components.CanvasBounds.id,
      ).fields;
      const rect = document
        .querySelector("#ipp-demo-canvas")
        .getBoundingClientRect();
      const viewport = canvas.viewport;
      return {
        entity: String(entity.id),
        x:
          rect.x +
          (((bounds.x + bounds.width / 2) * viewport.devicePixelRatio) /
            viewport.width) *
            rect.width,
        y:
          rect.y +
          (((bounds.y + bounds.height / 2) * viewport.devicePixelRatio) /
            viewport.height) *
            rect.height,
      };
    } finally {
      await root.close();
    }
  }, id);
  await page.mouse.move(point.x, point.y);
  await page.waitForFunction(async (entity) => {
    const app = window.ippDemoApplication;
    const canvas = window.ippDemoCanvas;
    await canvas.frame({ afterOutputs: app.frameOutputs });
    const root = await canvas.host.openWorld(app.output.world);
    try {
      const page = await root.inspectPage({ collection: "guiPointers" });
      return page.guiPointers?.some(
        (pointer) =>
          String(pointer.target.entity) === entity && pointer.state.hovered,
      );
    } finally {
      await root.close();
    }
  }, point.entity);
  await page.mouse.click(point.x, point.y);
  await page.evaluate(async () => {
    const app = window.ippDemoApplication;
    await app.flush();
    await window.ippDemoCanvas.frame({ afterOutputs: app.frameOutputs });
  });
}

async function selectScene(page, scene) {
  await press(page, "demo-scene-picker");
  await press(page, `demo-scene-picker/options/${scene}`);
}

test("the release gallery is complete and renders at root and project URLs", {
  // Both URL layouts render every scene; software WebGL needs a full-run budget.
  timeout: 360_000,
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
  const cockpitManifest = JSON.parse(
    await readFile(resolve(site, "assets/cockpit/manifest.json"), "utf8"),
  );
  const exteriorPaths = [
    ...cockpitManifest.exterior.bodies.map(({ baseTexture }) => baseTexture),
    ...Object.values(cockpitManifest.exterior.textures),
  ].map((source) => {
    assert.ok(
      source.startsWith("/assets/"),
      `Invalid exterior texture: ${source}`,
    );
    return `assets/cockpit/${source.slice("/assets/".length)}`;
  });
  await mkdir(evidenceRoot, { recursive: true });
  const evidence = await mkdtemp(resolve(evidenceRoot, "run-"));
  const browser = await chromium.launch(browserLaunchOptions());
  try {
    for (const prefix of ["/", "/preview/ipp/"]) {
      const server = await serveSite(prefix);
      const context = await browser.newContext({
        viewport: { width: 800, height: 600 },
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
        const initialScene = prefix === "/" ? "shapes" : "platformer";
        await page.goto(`${server.url}#${initialScene}`);
        let cockpitWorlds = [];
        for (const [index, scene] of [
          initialScene,
          "platformer",
          "shapes",
          "lighting",
          "particles",
          "charts",
          "cockpit",
          "shapes",
        ].entries()) {
          if (index > 0) await selectScene(page, scene);
          await page.waitForFunction(
            (expected) =>
              window.ippDemoApplication?.sceneId === expected &&
              !!window.ippDemoCanvas,
            scene,
          );
          await page.evaluate(async () => {
            await window.ippDemoApplication.ready;
            await window.ippDemoApplication.flush();
          });
          assert.equal(
            await page.evaluate(() => window.ippDemoApplication.sceneId),
            scene,
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
          if (scene === "cockpit") {
            assert.equal(
              frame.cockpit.panels.length,
              6,
              "all projected GUI panel Worlds load",
            );
            assert.equal(
              new Set(frame.cockpit.panels.map(({ world }) => world)).size,
              6,
            );
            assert.ok(
              frame.cockpit.panels.every(
                (panel) => panel.entities > 0 && panel.sources.length > 0,
              ),
              "all six panel Worlds load their entities and assets",
            );
            assert.ok(
              frame.cockpit.panels[frame.cockpit.focusedPanel].completed,
              "the focused panel contributes current completed output; off-focus panels may cache",
            );
            for (const exterior of frame.cockpit.exterior) {
              assert.ok(
                exterior.custom,
                `${exterior.name} uses the runtime exterior shader`,
              );
              for (const name of [
                "base",
                "cities",
                "role",
                "phase",
                "cloudPhase",
                "cityPhase",
                "intensity",
                "cityGain",
                "starGain",
              ])
                assert.ok(
                  exterior.properties.includes(name),
                  `${exterior.name} exposes ${name}`,
                );
            }
            assert.ok(
              frame.fallbackRed < 256,
              "WebGL exterior and instrument shaders avoid solid-red fallback",
            );
            assert.ok(
              frame.blueInk > 500,
              "projected panels contribute visible instrument ink",
            );
            cockpitWorlds = [
              frame.cockpit.sceneWorld,
              ...frame.cockpit.panels.map(({ world }) => world),
            ];
          }
          if (scene === "shapes") {
            const worlds = await page.evaluate(() =>
              window.ippDemoCanvas.host.listWorlds(),
            );
            assert.ok(
              worlds.every(
                (world) =>
                  !cockpitWorlds.includes(`${world.incarnation}:${world.id}`),
              ),
              "returning to Shapes retires the cockpit and all six panel Worlds",
            );
            assert.equal(
              worlds.length,
              2,
              "scene replacements retire their owned Worlds",
            );
          }
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
          "assets/platformer/pose-origins.json",
          "assets/cockpit/cockpit.ipp",
          "assets/cockpit/manifest.json",
          "assets/drawings/waveform.ippd",
          "assets/drawings/waveform-pulse.ippd",
          ...new Set(exteriorPaths),
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
