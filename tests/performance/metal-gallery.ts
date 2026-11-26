/** Run paired gallery windows on independently owned native Hosts. */
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { cpus, loadavg, release } from "node:os";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import { guiCases } from "./scenarios/gui-cases.js";
import { runNativeEnvironment } from "../harness/native.js";
import {
  compareGuiRuns,
  type GuiComparisonRun,
} from "./scenarios/gui-comparison.js";
import type { GalleryBenchmarkConfig } from "./scenarios/gallery.js";

const config: Omit<GalleryBenchmarkConfig, "backend" | "repeat"> & {
  scene?: string;
  themes?: string[] | null;
  projections?: string[] | null;
  metal: string;
  gles: string;
  repetitions: number;
  renderer?: "metal" | "gles";
} = JSON.parse(process.argv[2]!);
const output = resolve(config.output);
await mkdir(output, { recursive: true });

const hash = async (file: string) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");
const assets: Record<string, string> = {};
const gui = config.scene === "gui-rendering";
for (const directory of gui
  ? ["target/font-assets", "target/demos-drawings"]
  : [
      "target/gallery-assets",
      "target/gallery-platformer-native-assets",
      "target/demos-cockpit-native-assets",
      "target/demos-drawings",
      "target/font-assets",
    ]) {
  const entries = await readdir(directory, {
    recursive: true,
    withFileTypes: true,
  });
  for (const entry of entries.filter((entry) => entry.isFile())) {
    const path = join(entry.parentPath, entry.name);
    assets[path] = await hash(path);
  }
}
const ioRead = gui
  ? []
  : [
      { prefix: "ipp-gallery://assets/", directory: resolve("target") },
      {
        prefix: "https://platformer.ipp.invalid/",
        directory: resolve("target/gallery-platformer-native-assets"),
      },
      {
        prefix: "https://cockpit.ipp.invalid/",
        directory: resolve("target/demos-cockpit-native-assets"),
      },
    ];

const controller = new AbortController();
const interrupt = () => controller.abort();
process.once("SIGTERM", interrupt);
process.once("SIGINT", interrupt);
const failures: { backend: string; repeat: number; error: string }[] = [];
const runs: GuiComparisonRun[] = [];
const report = {
  config,
  assets,
  machine: { cpu: cpus()[0]?.model, os: release() },
  runs,
  failures,
  pipelineRun: process.env.IPP_PIPELINE_RUN,
};
const save = () =>
  writeFile(join(output, "report.json"), JSON.stringify(report, null, 2));

const cases = gui ? guiCases(config) : [undefined];
try {
  for (let repeat = 0; repeat < config.repetitions; repeat++) {
    const order =
      repeat % 2 === 0
        ? (["metal", "gles"] as const)
        : (["gles", "metal"] as const);
    for (const boardCase of repeat % 2 ? cases.toReversed() : cases) {
      for (const backend of order) {
        if (config.renderer && backend !== config.renderer) continue;
        controller.signal.throwIfAborted();
        const product = resolve(config[backend]);
        const executable = join(
          product,
          backend === "metal" ? "ipp-metal-host" : "gles_host",
        );
        const schemaArtifact = join(product, "contract.bin");
        const finish = join(output, `${backend}-${repeat}`);
        await mkdir(finish, { recursive: true });
        const identity = {
          executable: await hash(executable),
          contract: await hash(schemaArtifact),
          client: await hash(join(product, "generated.js")),
        };
        const before = loadavg();
        try {
          const run = await runNativeEnvironment(
            `${backend}-${repeat}`,
            {
              executable,
              schemaArtifact,
              workingDirectory: process.cwd(),
              evidenceParent: finish,
              readinessTimeoutMs: 30_000,
              operationTimeoutMs: 300_000,
              ...(gui ? { closeTimeoutMs: 15000 } : {}),
              environment: { IPP_LOG: "info" },
              extraArguments: [
                "--uncapped",
                ...(backend === "gles"
                  ? ["--egl-dir", config.eglDirectory]
                  : []),
                ...ioRead.flatMap(({ prefix, directory }) => [
                  "--io-read",
                  prefix,
                  directory,
                ]),
              ],
            },
            controller.signal,
            async (environment) => {
              const hostDirectory = join(
                environment.evidence.directory,
                "host-state",
              );
              await mkdir(hostDirectory);
              await writeFile(
                join(hostDirectory, "host.json"),
                JSON.stringify({
                  pid: environment.processId,
                  url: environment.url,
                  presentationUrl: environment.presentationUrl,
                  worktree: process.cwd(),
                  commit: config.source.revision,
                  contract: identity.contract,
                  client: product,
                  font: resolve("target/font-assets/shure-tech-mono.ippf"),
                  eglDirectory: config.eglDirectory,
                  log: join(
                    environment.evidence.directory,
                    "server-stderr.log",
                  ),
                  startedAt: new Date().toISOString(),
                  ioRead,
                }),
              );
              const { stdout, stderr } = await promisify(execFile)(
                process.execPath,
                [
                  resolve("tools/shared-host/shared-host.mjs"),
                  "run",
                  gui
                    ? "tests/performance/drivers/native-gui-rendering.ts"
                    : "tests/performance/drivers/native-gallery.ts",
                  JSON.stringify({
                    ...config,
                    boardCase,
                    backend,
                    repeat,
                    output: finish,
                    url: environment.url,
                    presentationUrl: environment.presentationUrl,
                  }),
                  "--host",
                  hostDirectory,
                  "--name",
                  `gallery-${backend}-${repeat}`,
                  "--out",
                  finish,
                  "--json",
                ],
                { signal: environment.signal, maxBuffer: 4 << 20 },
              );
              await writeFile(
                join(
                  gui ? environment.evidence.directory : finish,
                  "client-stderr.log",
                ),
                stderr,
              );
              const result = JSON.parse(stdout).report;
              const device =
                backend === "metal"
                  ? (
                      await readFile(
                        join(
                          environment.evidence.directory,
                          "server-stderr.log",
                        ),
                        "utf8",
                      )
                    ).match(/\[IPP Metal\] device=([^\r\n]+)/)?.[1]
                  : (result.device?.renderer ??
                    result.device?.unmaskedRenderer);
              if (result.failures?.length && !device)
                throw new Error(JSON.stringify(result.failures));
              assert.ok(device, "No hardware renderer identity");
              assert.doesNotMatch(
                device,
                /llvmpipe|softpipe|swiftshader|lavapipe|software/i,
              );
              return {
                backend,
                repeat,
                before,
                after: loadavg(),
                identity,
                device,
                result,
                evidence: environment.evidence.directory,
              };
            },
          );
          runs.push(run.value);
          for (const failure of run.value.result.failures)
            failures.push({
              backend,
              repeat,
              error: `${failure.name}: ${failure.error}`,
            });
        } catch (error) {
          failures.push({ backend, repeat, error: String(error) });
        }
        await save();
      }
    }
  }
} finally {
  await save();
  if (gui)
    await writeFile(
      join(output, "comparison.csv"),
      compareGuiRuns(runs, config.instrumented),
    );
  process.removeListener("SIGTERM", interrupt);
  process.removeListener("SIGINT", interrupt);
}
assert.equal(
  failures.length,
  0,
  `Benchmark cases with missing evidence: ${JSON.stringify(failures)}`,
);
