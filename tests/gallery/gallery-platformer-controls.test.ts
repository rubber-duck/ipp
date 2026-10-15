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

test("native robot controls stay above the body, face the camera and route real input", {
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
  await runNativeEnvironment(
    "native-robot-controls",
    {
      executable: join(product, "gles_host"),
      schemaArtifact: join(product, "contract.bin"),
      workingDirectory: workspace,
      readinessTimeoutMs: 30_000,
      operationTimeoutMs: 120_000,
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
      await writeFile(join(stateDirectory, "host.json"), JSON.stringify(host));
      const observer = await environment.track(
        contract.IppHostClient.connectWebSocket(environment.url),
      );
      for (const [operation, count] of [
        ["exercise", 6],
        ["poses", 27],
      ] as const) {
        const reply = await execute(
          process.execPath,
          [
            resolve("tools/shared-host/shared-host.mjs"),
            "run",
            "tests/gallery/drivers/native-platformer-controls.ts",
            operation,
            "--name",
            "native-robot-" + operation,
            "--host",
            stateDirectory,
            "--out",
            join(directory, operation),
            "--json",
          ],
          { cwd: workspace, signal: environment.signal, maxBuffer: 16 << 20 },
        );
        await writeFile(
          join(directory, "robot-" + operation + ".json"),
          reply.stdout,
        );
        const result = JSON.parse(reply.stdout) as {
          report: { failure?: string; frames: string[] };
          files: string[];
        };
        assert.equal(result.report.failure, undefined, result.report.failure);
        assert.equal(result.report.frames.length, count);
        assert.equal(result.files.length, count);
      }
      assert.deepEqual(
        await observer.listWorlds(),
        [],
        "the application, scene and movement Canvas Worlds are all retired",
      );
    },
  );
});
