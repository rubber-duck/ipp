/** Import any standard Blender disk export through a real worker and save its World.
 * python tools/ipp.py import-blender EXPORT_DIR OUTPUT_DIR [--namespace NAME] [--world FILE] [--clips-only]
 * The Python command prepares the matching runtime and checks the browser environment.
 */
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir, copyFile } from "node:fs/promises";
import { resolve, extname, dirname } from "node:path";
import { parseArgs } from "node:util";
import { chromium } from "playwright";
import { browserLaunchOptions } from "#ipp-browser-options";
import { bundleBrowser, workspace } from "./build/helpers.mjs";

const { positionals, values } = parseArgs({
  allowPositionals: true,
  options: {
    namespace: { type: "string", default: "scene" },
    world: { type: "string", default: "world.ipp" },
    "clips-only": { type: "boolean", default: false },
    "defer-presentation": { type: "boolean", default: false },
  },
});
if (
  positionals.length !== 2 ||
  !/^[a-zA-Z0-9_-]+$/.test(values.namespace) ||
  !/^[a-zA-Z0-9_.-]+\.ipp$/.test(values.world)
)
  throw new Error(
    "Expected EXPORT_DIR OUTPUT_DIR [--namespace NAME] [--world FILE.ipp] [--clips-only]",
  );
const [input, output] = positionals.map((path) => resolve(path));
await mkdir(output, { recursive: true });
const source = JSON.parse(await readFile(resolve(input, "scene.json"), "utf8"));
const catalog = JSON.parse(
  await readFile(resolve(input, "catalog.json"), "utf8"),
);
const assetName = /^[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*$/;
for (const name of Object.keys(catalog)) {
  if (!assetName.test(name)) throw new Error("Invalid asset name");
  const from = resolve(input, "assets", name),
    to = resolve(output, name);
  await mkdir(dirname(to), { recursive: true });
  if (from !== to) await copyFile(from, to);
}
const entry = resolve(workspace, "target/blender-import/import.js");
await bundleBrowser(
  resolve(workspace, "integrations/blender/client/disk-import.ts"),
  entry,
);
const runtime = resolve(
  workspace,
  process.env.IPP_BROWSER_BUILD_DIR ?? "target/browser-build",
  "render-expanded",
);
const type = {
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
};
const server = createServer(async (request, response) => {
  try {
    const pathname = new URL(request.url, "http://localhost").pathname;
    if (pathname === "/animation" && request.method === "POST") {
      const parts = [];
      for await (const part of request) parts.push(part);
      const data = Buffer.concat(parts);
      const name = createHash("sha256").update(data).digest("hex");
      await writeFile(resolve(output, name), data);
      catalog[name] = {
        bytes: data.length,
        contentType: "application/octet-stream",
      };
      response.writeHead(200, { "Content-Type": "text/plain" }).end(name);
      return;
    }
    if (pathname === "/") {
      response
        .writeHead(200, { "Content-Type": "text/html" })
        .end("<!doctype html><title>Blender scene import</title>");
      return;
    }
    const asset = /^\/(source|bundle)\/(.+)$/.exec(pathname);
    const runtimeFile = /^\/runtime\/([a-zA-Z0-9_.-]+)$/.exec(pathname);
    const path =
      pathname === "/import.js"
        ? entry
        : asset && assetName.test(asset[2]) && Object.hasOwn(catalog, asset[2])
          ? resolve(
              asset[1] === "source" ? resolve(input, "assets") : output,
              asset[2],
            )
          : runtimeFile
            ? resolve(runtime, runtimeFile[1])
            : null;
    if (!path) {
      response.writeHead(404).end();
      return;
    }
    const bytes = await readFile(path);
    response
      .writeHead(200, {
        "Content-Type": type[extname(path)] ?? "application/octet-stream",
        "Cache-Control": "no-store",
      })
      .end(bytes);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
let browser;
try {
  browser = await chromium.launch(browserLaunchOptions());
  const page = await browser.newPage();
  page.on("console", (message) => {
    console.log(message.text());
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(
    async ({ source, namespace, clipsOnly, deferPresentation }) => {
      const contract = await import("/runtime/generated.js");
      const { importBlenderScene } = await import("/import.js");
      const prefix = `https://${namespace}.ipp.invalid/`;
      const canvas = document.createElement("canvas");
      canvas.width = 400;
      canvas.height = 300;
      document.body.append(canvas);
      const host = await contract.IppHostClient.connectWorker(
        "/runtime/wasm-worker.js",
        "/runtime/runtime.wasm",
        {
          canvas: canvas.transferControlToOffscreen(),
          timeoutMs: 60000,
          logLevel: "error",
          resourceUrls: [
            { prefix, baseUrl: new URL("/bundle/", location.href).href },
          ],
        },
      );
      console.info("Import Host ready");
      const createWorld = host.createWorld.bind(host);
      host.createWorld = async (options) => {
        const client = await createWorld(options),
          batch = client.batch.bind(client);
        let total = 0;
        client.batch = async (operations) => {
          const start = performance.now();
          const result = await batch(operations);
          total += operations.length;
          console.info(
            `Import batch acknowledged: ${operations.length} operations, ${total} total, ${(performance.now() - start).toFixed(1)} ms, ok=${result.ok}`,
          );
          return result;
        };
        return client;
      };
      const assetName = (source) => {
        const match = /^\/assets\/([A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*)$/.exec(
          source,
        );
        if (!match) throw new Error(`Invalid exported source ${source}`);
        return match[1];
      };
      let failure;
      try {
        const result = await importBlenderScene(
          host,
          contract,
          source,
          {
            resolve: (source) => prefix + assetName(source),
            read: (source) =>
              new URL("/source/" + assetName(source), location.href).href,
            publishAnimation: async (bytes) => {
              const response = await fetch("/animation", {
                method: "POST",
                body: bytes,
              });
              if (!response.ok) throw new Error("Animation publication failed");
              return prefix + (await response.text());
            },
          },
          { symbolicId: namespace, clipsOnly, deferPresentation },
        );
        console.info("World saved");
        return { ...result, bytes: Array.from(result.bytes) };
      } catch (error) {
        failure = error;
        console.error("Import failed", String(error));
        throw error;
      } finally {
        try {
          await host.close();
        } catch (error) {
          if (!failure) throw error;
          console.error("Import cleanup failed", String(error));
        }
      }
    },
    {
      source,
      namespace: values.namespace,
      clipsOnly: values["clips-only"],
      deferPresentation: values["defer-presentation"],
    },
  );
  await writeFile(resolve(output, values.world), Buffer.from(result.bytes));
  await writeFile(
    resolve(output, "manifest.json"),
    JSON.stringify(result.manifest, null, 2) + "\n",
  );
  await writeFile(
    resolve(output, "catalog.json"),
    JSON.stringify(catalog, null, 2) + "\n",
  );
  await writeFile(
    resolve(output, "blender-scene.json"),
    JSON.stringify(source, null, 2) + "\n",
  );
  console.log(
    `Saved ${result.entities} entities and ${result.manifest.clips.length} reusable clips (${result.bytes.length} World bytes).`,
  );
} finally {
  await browser?.close();
  await new Promise((done) => server.close(done));
}
