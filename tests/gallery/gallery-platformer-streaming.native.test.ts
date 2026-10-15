import assert from "node:assert/strict";
import test from "node:test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { createServer } from "node:http";
import { once } from "node:events";
import { pathToFileURL } from "node:url";
import type { HostContract, HostState } from "../../tools/shared-host/host.js";
import { runNativeEnvironment } from "../harness/native.js";

const execute = promisify(execFile);

test("native robot streaming preserves continuity, latest intent, cache and authored route", {
  timeout: 180_000,
}, async (context) => {
  const workspace = process.cwd();
  const product = resolve("target/gles-host");
  const eglDirectory = process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64";
  const ioRead = [
    { prefix: "ipp-gallery://assets/", directory: resolve("target") },
    {
      prefix: "https://platformer.ipp.invalid/",
      directory: resolve("target/gallery-platformer-native-assets"),
    },
  ];
  const assetDirectory = resolve("target/gallery-platformer-native-assets");
  const manifest = JSON.parse(
    await readFile(join(assetDirectory, "manifest.json"), "utf8"),
  ) as { clips: { name: string; clip: { source: string } }[] };
  const modes = new Map(
    manifest.clips.map((clip) => [
      clip.clip.source.split("/").at(-1)!,
      clip.name.replace("Platformer_", "").toLowerCase(),
    ]),
  );
  const counts = new Map<string, Record<string, number>>();
  const releases = new Map<string, () => void>();
  const gate = (kind: string) =>
    new Promise<void>((resolve) => releases.set(kind, resolve));
  const gates = new Map(
    ["continuity", "newest", "closing", "leaving"].map((kind) => [
      kind,
      gate(kind),
    ]),
  );
  const producer = createServer(async (request, response) => {
    const [kind, file] = (request.url ?? "").slice(1).split("/");
    if (!kind || !file || !gates.has(kind)) {
      response.writeHead(404).end();
      return;
    }
    const observed = counts.get(kind) ?? { walk: 0, run: 0, crawl: 0 };
    counts.set(kind, observed);
    if (file === "release") {
      releases.get(kind)!();
      response.end("released");
      return;
    }
    if (file === "requests") {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify(observed));
      return;
    }
    const mode = modes.get(file);
    if (!mode) {
      response.writeHead(404).end();
      return;
    }
    observed[mode] = observed[mode]! + 1;
    try {
      const bytes = await readFile(join(assetDirectory, file));
      response.writeHead(200, {
        "content-length": bytes.length,
        etag: `"${file}"`,
        "cache-control": "no-store",
      });
      response.flushHeaders();
      if (mode === "run") await gates.get(kind);
      response.end(bytes);
    } catch (error) {
      response.destroy(error as Error);
    }
  });
  producer.listen(0, "127.0.0.1");
  await once(producer, "listening");
  const address = producer.address();
  assert.ok(address && typeof address !== "string");
  const prefix = `http://127.0.0.1:${address.port}/`;
  try {
    await runNativeEnvironment(
      "native-robot-streaming",
      {
        executable: join(product, "gles_host"),
        schemaArtifact: join(product, "contract.bin"),
        workingDirectory: workspace,
        readinessTimeoutMs: 30_000,
        operationTimeoutMs: 120_000,
        closeTimeoutMs: 5_000,
        extraArguments: [
          "--http-prefix",
          prefix,
          "--egl-dir",
          eglDirectory,
          ...ioRead.flatMap(({ prefix, directory }) => [
            "--io-read",
            prefix,
            directory,
          ]),
        ],
      },
      context.signal,
      async (environment) => {
        const directory = environment.evidence.directory;
        const stateDirectory = join(directory, "shared-host");
        await mkdir(stateDirectory);
        const contract = (await import(
          pathToFileURL(join(product, "generated.js")).href
        )) as HostContract;
        const host: HostState = {
          pid: process.pid,
          url: environment.url,
          presentationUrl: environment.presentationUrl!,
          worktree: workspace,
          commit: "integration-harness",
          contract: contract.SCHEMA_HASH.toString(16),
          client: product,
          font: resolve("target/font-assets/shure-tech-mono.ippf"),
          eglDirectory,
          log: join(directory, "server-stderr.log"),
          startedAt: new Date().toISOString(),
          ioRead,
        };
        await writeFile(
          join(stateDirectory, "host.json"),
          JSON.stringify(host),
        );
        const observer = await environment.track(
          contract.IppHostClient.connectWebSocket(environment.url),
        );
        const reply = await execute(
          process.execPath,
          [
            resolve("tools/shared-host/shared-host.mjs"),
            "run",
            "tests/gallery/drivers/native-platformer-streaming.ts",
            prefix,
            "--name",
            "native-robot-streaming",
            "--host",
            stateDirectory,
            "--out",
            join(directory, "captures"),
            "--json",
          ],
          { cwd: workspace, signal: environment.signal, maxBuffer: 16 << 20 },
        );
        await writeFile(
          join(directory, "robot-streaming-result.json"),
          reply.stdout,
        );
        const result = JSON.parse(reply.stdout) as {
          report: { failure?: string; frames: string[] };
          files: string[];
        };
        assert.equal(result.report.failure, undefined, result.report.failure);
        assert.ok(result.report.frames.length >= 8);
        assert.equal(result.files.length, result.report.frames.length);
        assert.deepEqual(
          await observer.listWorlds(),
          [],
          "the application, scene and movement Canvas Worlds are all retired",
        );
      },
    );
  } finally {
    releases.forEach((release) => release());
    producer.closeAllConnections();
    await new Promise<void>((resolve, reject) =>
      producer.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
