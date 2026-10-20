import assert from "node:assert/strict";
import test from "node:test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { HostContract, HostState } from "../../tools/shared-host/host.js";
import { runNativeEnvironment } from "../harness/native.js";

const execute = promisify(execFile);

for (const [part, command, minimumFrames] of [
  ["spatial", "spatial", 3],
  ["carousel", "carousel", 13],
  ["quality", "quality", 8],
  ["presentation", "exercise", 17],
  ["exterior", "exterior", 8],
  ["wheel", "wheel", 3],
  ["forms", "forms", 8],
] as const) {
  for (const [label, args] of [
    ["wide", []],
    ["narrow", ["narrow"]],
    ...(part === "spatial"
      ? ([
          ["ultrawide", ["ultrawide"]],
          ["portrait", ["portrait"]],
        ] as const)
      : []),
    ...(part === "quality" ? [["hidpi", ["hidpi"]] as const] : []),
  ] as const) {
    // Each viewport owns its Host, budget and cleanup; one slow layout fails alone.
    test(`native cockpit ${part} ${label} routes projected controls and releases its Worlds`, {
      timeout: 420_000,
    }, async (context) => {
      const workspace = process.cwd();
      const product = resolve("target/gles-host");
      const eglDirectory = process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64";
      const ioRead = [
        { prefix: "ipp-gallery://assets/", directory: resolve("target") },
        {
          prefix: "https://cockpit.ipp.invalid/",
          directory: resolve("target/demos-cockpit-native-assets"),
        },
      ];
      await runNativeEnvironment(
        `native-cockpit-${part}-${label}`,
        {
          executable: join(product, "gles_host"),
          schemaArtifact: join(product, "contract.bin"),
          workingDirectory: workspace,
          readinessTimeoutMs: 30_000,
          operationTimeoutMs: 360_000,
          closeTimeoutMs: 5_000,
          extraArguments: [
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
              "tests/gallery/drivers/native-cockpit.ts",
              command,
              ...args,
              ...(part === "quality"
                ? ["diagnostics", environment.url, environment.presentationUrl!]
                : []),
              "--name",
              `native-cockpit-${part}-${label}`,
              "--host",
              stateDirectory,
              "--out",
              join(directory, label),
              "--json",
            ],
            { cwd: workspace, signal: environment.signal, maxBuffer: 16 << 20 },
          );
          await writeFile(
            join(directory, `cockpit-${label}.json`),
            reply.stdout,
          );
          const result = JSON.parse(reply.stdout) as {
            report: { failure?: string; frames: string[] };
            files: string[];
          };
          assert.equal(result.report.failure, undefined, result.report.failure);
          assert.ok(result.report.frames.length >= minimumFrames);
          assert.equal(result.files.length, result.report.frames.length);
          assert.deepEqual(
            await observer.listWorlds(),
            [],
            "the application and all scene Worlds are retired",
          );
        },
      );
    });
  }
}
