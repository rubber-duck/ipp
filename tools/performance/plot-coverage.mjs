/** Build/drive ordinary Plot frames; the pipeline owns reservation and prerequisites. */
import assert from "node:assert/strict";
import { build } from "esbuild";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir, copyFile } from "node:fs/promises";
import { resolve, join, extname, sep, dirname } from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { cpus, release, loadavg } from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import { chromium } from "playwright";
import {
  browserLaunchOptions,
  verifyHardwareRenderer,
} from "../build/browser-options.mjs";
const root = process.cwd(),
  product = resolve("target/performance-build/plot-coverage");
if (process.argv[2] === "build") {
  await mkdir(product, { recursive: true });
  const configuration = {
    extends: resolve("tsconfig.json"),
    compilerOptions: {
      noEmit: true,
      paths: {
        "@ipp/client": [resolve("packages/ipp-client/src/index.ts")],
        "@ipp/host-contract": [
          resolve("target/browser-build/render/generated.ts"),
        ],
      },
    },
    include: [resolve("tests/performance/pages/plot-coverage.ts")],
  };
  const tsconfig = join(product, "tsconfig.json");
  await writeFile(tsconfig, JSON.stringify(configuration));
  execFileSync(
    process.execPath,
    ["node_modules/typescript/bin/tsc", "-p", tsconfig],
    { stdio: "inherit" },
  );
  await build({
    entryPoints: ["tests/performance/pages/plot-coverage.ts"],
    outfile: join(product, "page.js"),
    bundle: true,
    format: "esm",
    platform: "browser",
  });
} else {
  const config = JSON.parse(process.argv[2]),
    out = resolve(config.output),
    runtime = resolve(config.runtime);
  await mkdir(out, { recursive: true });
  const json = (value) =>
    JSON.stringify(
      value,
      (_, v) => (typeof v === "bigint" ? v.toString() : v),
      2,
    );
  const hash = async (path) =>
    createHash("sha256")
      .update(await readFile(path))
      .digest("hex");
  const identity = JSON.parse(
    await readFile(join(runtime, "build-identity.json"), "utf8"),
  );
  assert.equal(identity.instrumented, false);
  const report = {
    configuration: config,
    source: config.source,
    runtime: identity,
    runtimeSha256: await hash(join(runtime, "runtime.wasm")),
    contractSha256: await hash(join(runtime, "contract.bin")),
    fixtureSha256: await hash(join(product, "page.js")),
    machine: { cpu: cpus()[0].model, os: release(), loadBefore: loadavg() },
    results: [],
  };
  if (process.platform === "darwin")
    report.machine.thermal = execFileSync("/usr/bin/pmset", ["-g", "therm"], {
      encoding: "utf8",
    });
  await writeFile(
    join(out, "source.patch"),
    execFileSync("git", ["diff", "HEAD"], { encoding: "utf8" }),
  );
  for (const path of execFileSync(
    "git",
    ["ls-files", "--others", "--exclude-standard", "-z"],
    { encoding: "utf8" },
  )
    .split("\0")
    .filter(Boolean)) {
    const destination = join(out, "source-untracked", path);
    await mkdir(dirname(destination), { recursive: true });
    await copyFile(resolve(path), destination);
  }
  let browser;
  const server = createServer(async (req, res) => {
    try {
      const pathname = decodeURIComponent(
        new URL(req.url, "http://localhost").pathname,
      );
      if (pathname === "/") {
        res.setHeader("content-type", "text/html");
        res.end(
          "<!doctype html><style>html,body{margin:0;background:black}canvas{display:block}</style>",
        );
        return;
      }
      const base = pathname.startsWith("/runtime/") ? runtime : root;
      const path = resolve(
        base,
        `.${pathname.startsWith("/runtime/") ? pathname.slice(8) : pathname}`,
      );
      if (!path.startsWith(base + sep))
        throw new Error("Invalid artifact path");
      const bytes = await readFile(path);
      res.setHeader(
        "content-type",
        extname(path) === ".wasm"
          ? "application/wasm"
          : extname(path) === ".js"
            ? "text/javascript"
            : "application/octet-stream",
      );
      res.end(bytes);
    } catch {
      res.statusCode = 404;
      res.end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const errors = [];
  const stop = () => {
    report.interrupted = true;
    void browser?.close();
    server.closeAllConnections();
    server.close();
  };
  const timer = setTimeout(stop, 900000);
  process.once("SIGINT", stop);
  process.once("SIGTERM", stop);
  try {
    report.launch = {
      ...browserLaunchOptions(true, { framePacing: "uncapped" }),
      headless: false,
    };
    browser = await chromium.launch(report.launch);
    report.browser = browser.version();
    const cdp = await browser.newBrowserCDPSession();
    report.gpu = await cdp.send("SystemInfo.getInfo");
    assert.equal(report.gpu.gpu.featureStatus.gpu_compositing, "enabled");
    const page = await browser.newPage({
      viewport: { width: 1600, height: 1000 },
      deviceScaleFactor: 1,
    });
    page.on("pageerror", (e) => errors.push(String(e)));
    const all = [
      "line",
      "multiline",
      "markers",
      "bars",
      "pie",
      "area",
      "band",
      "filled-markers",
      "multiband",
      "painted-band",
    ];
    const cases = config.cases ? config.cases.split(",") : all;
    assert.ok(cases.every((name) => all.includes(name)));
    for (const count of config.rows)
      for (const name of cases) {
        await page.goto(origin);
        const ready = await page.evaluate(
          async ({ count, name }) => {
            const module = await import(
              "/target/performance-build/plot-coverage/page.js"
            );
            globalThis.plotCoverage = await module.open(
              "/runtime",
              count,
              name,
            );
            return {
              device: globalThis.plotCoverage.device,
              unsupported: globalThis.plotCoverage.unsupported,
            };
          },
          { count, name },
        );
        if (ready.unsupported) {
          report.results.push({ name, count, unsupported: ready.unsupported });
          continue;
        }
        report.device = ready.device;
        verifyHardwareRenderer(
          ready.device.unmaskedRenderer ?? ready.device.renderer,
        );
        await delay(config.warmupMs);
        const entry = {
          name,
          count,
          samples: [],
          before: await page.evaluate(() => plotCoverage.statistics()),
        };
        report.results.push(entry);
        for (let repeat = 0; repeat < config.repetitions; repeat++)
          entry.samples.push(
            await page.evaluate(
              (ms) => plotCoverage.sample(ms),
              config.sampleMs,
            ),
          );
        entry.after = await page.evaluate(() => plotCoverage.statistics());
        const png = await page.screenshot({
          path: join(out, `${name}-${count}.png`),
        });
        entry.paintedPixels = await page.evaluate(async (base64) => {
          const image = new Image();
          image.src = `data:image/png;base64,${base64}`;
          await image.decode();
          const canvas = document.createElement("canvas");
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext("2d");
          context.drawImage(image, 0, 0);
          const pixels = context.getImageData(
            0,
            0,
            canvas.width,
            canvas.height,
          ).data;
          let painted = 0;
          for (let i = 0; i < pixels.length; i += 4)
            if (Math.max(...pixels.slice(i, i + 3)) > 80) painted++;
          return painted;
        }, png.toString("base64"));
        assert.ok(entry.paintedPixels > 100, "Empty Plot benchmark frame");
        await page.evaluate(() => plotCoverage.close());
        await writeFile(join(out, "report.json"), json(report));
        console.log(
          name,
          count,
          entry.samples.map((s) => s.fps.toFixed(1)).join("/"),
        );
      }
    assert.deepEqual(errors, []);
    report.complete = true;
  } catch (error) {
    report.failure = { message: String(error), stack: error.stack };
    throw error;
  } finally {
    clearTimeout(timer);
    process.removeListener("SIGINT", stop);
    process.removeListener("SIGTERM", stop);
    report.errors = errors;
    report.machine.loadAfter = loadavg();
    await writeFile(join(out, "report.json"), json(report));
    await browser?.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}
