/** Owned ordinary-release Host; captures and diagnostics are outside timing. */
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { cpus, loadavg, platform, release } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { runNativeEnvironment } from "../harness/native.js";

const config = JSON.parse(process.argv[2]!);
const output = resolve(config.output);
const product = resolve("target/performance-build", config.product);
const build = JSON.parse(
  await readFile(join(product, "build-identity.json"), "utf8"),
);
const hash = async (file: string) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");
assert.equal(build.profile, "release");
assert.equal(config.framePacing, "uncapped");
assert.equal(build.instrumented, config.instrumented);
assert.equal(
  build.source.sourceSha256,
  config.source.sourceSha256,
  "Rebuild stale release Host",
);
assert.equal(await hash(join(product, "gles_host")), build.executable);
assert.equal(await hash(join(product, "contract.bin")), build.contract);
await mkdir(output, { recursive: true });
const controller = new AbortController();
const timer = setTimeout(
  () => controller.abort(new Error("Cockpit comparison timed out")),
  1100000,
);
const interrupted = () =>
  controller.abort(new Error("Cockpit comparison interrupted"));
process.once("SIGINT", interrupted);
process.once("SIGTERM", interrupted);
const ioRead = [
  { prefix: "ipp-gallery://assets/", directory: resolve("target") },
  {
    prefix: "https://cockpit.ipp.invalid/",
    directory: resolve("target/demos-cockpit-native-assets"),
  },
];
try {
  await runNativeEnvironment(
    "cockpit-surface-comparison",
    {
      executable: join(product, "gles_host"),
      schemaArtifact: join(product, "contract.bin"),
      workingDirectory: process.cwd(),
      evidenceParent: output,
      readinessTimeoutMs: 30000,
      operationTimeoutMs: 1000000,
      closeTimeoutMs: 5000,
      extraArguments: [
        "--uncapped",
        "--egl-dir",
        config.eglDirectory,
        ...ioRead.flatMap(({ prefix, directory }) => [
          "--io-read",
          prefix,
          directory,
        ]),
      ],
    },
    controller.signal,
    async (environment) => {
      const state = join(environment.evidence.directory, "host-state");
      await mkdir(state);
      await writeFile(
        join(state, "host.json"),
        JSON.stringify({
          pid: environment.processId,
          url: environment.url,
          presentationUrl: environment.presentationUrl,
          worktree: process.cwd(),
          commit: config.source.revision,
          contract: "verified-build",
          client: product,
          font: resolve("target/font-assets/shure-tech-mono.ippf"),
          eglDirectory: config.eglDirectory,
          log: join(environment.evidence.directory, "server-stderr.log"),
          startedAt: new Date().toISOString(),
          ioRead,
        }),
      );
      const loadBefore = loadavg();
      const run = await promisify(execFile)(
        process.execPath,
        [
          resolve("tools/shared-host/shared-host.mjs"),
          "run",
          "tests/performance/scenarios/cockpit-surfaces.ts",
          JSON.stringify({
            ...config,
            output,
            hostPid: environment.processId,
            url: environment.url,
            presentationUrl: environment.presentationUrl,
          }),
          "--host",
          state,
          "--name",
          "cockpit-surface-comparison",
          "--out",
          output,
          "--json",
        ],
        { signal: environment.signal, maxBuffer: 32 << 20 },
      );
      await writeFile(join(output, "client-stderr.log"), run.stderr);
      const result = JSON.parse(run.stdout);
      const assets = {} as Record<string, string>;
      for (const file of [
        "target/demos-cockpit-native-assets/manifest.json",
        "target/demos-cockpit-native-assets/cockpit.ipp",
        "target/font-assets/shure-tech-mono.ippf",
      ])
        assets[file] = await hash(resolve(file));
      const report = {
        config,
        build,
        assets,
        result,
        machine: {
          platform: platform(),
          release: release(),
          cpu: cpus()[0]?.model,
          cores: cpus().length,
          loadBefore,
          loadAfter: loadavg(),
          thermalAndPower: "not controlled; no thermal observation",
        },
        pipelineRun: process.env.IPP_PIPELINE_RUN ?? null,
        evidence: environment.evidence.directory,
      };
      await writeFile(
        join(output, "report.json"),
        JSON.stringify(report, null, 2),
      );
      assert.equal(result.report.failure, null, result.report.failure);
      console.log(
        JSON.stringify({
          output,
          device: result.report.device,
          runs: result.report.runs.map(
            (run: { mode: string; timing: unknown }) => ({
              mode: run.mode,
              timing: run.timing,
            }),
          ),
        }),
      );
    },
  );
} finally {
  clearTimeout(timer);
  process.removeListener("SIGINT", interrupted);
  process.removeListener("SIGTERM", interrupted);
}
