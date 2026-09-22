import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import {
  assertEquivalentTextCoverage,
  exerciseRetainedGui,
  type RetainedGuiReport,
  type WorkloadFrame,
} from "./retained-gui-scenario.js";

/** Both builds render; only the GUI build presents text through retained batches. */
const BUILDS = [
  { name: "render-surfaces", retained: false },
  { name: "headless-gui", retained: true },
] as const;

export async function runRetainedGui(
  signal: AbortSignal,
  iterations: number,
  output: string,
) {
  const reports: Array<
    { build: string; evidence: string } & RetainedGuiReport
  > = [];
  for (const { name, retained } of BUILDS) {
    const directory = resolve("target/browser-build", name);
    const build = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      `retained-gui-${name}`,
      {
        workspace: process.cwd(),
        build,
        mismatchBuild: build,
        rendering: true,
        deviceScaleFactor: 1,
        operationTimeoutMs: 30000,
        evidenceParent: output,
      },
      signal,
      async (env) => {
        const module = `${env.urls.origin}/target/surface-build/fixture.js`;
        const call = <T>(name: string, args: readonly unknown[] = []) =>
          env.execute(name, args, () =>
            invoke<T>(env.page, module, name, args),
          );
        try {
          await call("initialize", [
            {
              generatedModuleUrl: env.urls.generated,
              workerScriptUrl: env.urls.workerScript,
              wasmUrl: env.urls.wasm,
            },
          ]);
          const report = await exerciseRetainedGui(
            {
              call,
              capture: async (label) => {
                const result = await call<WorkloadFrame>("capture", [label]);
                const path = resolve(env.evidence.directory, `${label}.png`);
                // Record the artifact path; PNG data URLs would exhaust the event log.
                await env.execute(`write ${label}.png`, [label], async () => {
                  await writeDataUrl(
                    path,
                    await invoke<string>(env.page, module, "captureDataUrl", [
                      label,
                    ]),
                  );
                  return path;
                });
                return result;
              },
            },
            retained,
            iterations,
          );
          reports.push({
            build: name,
            evidence: env.evidence.directory,
            ...report,
          });
          await writeFile(
            resolve(env.evidence.directory, "workload.json"),
            JSON.stringify(report, null, 2),
          );
        } finally {
          await call("close");
        }
      },
    );
  }
  await writeFile(
    resolve(output, "comparison.json"),
    JSON.stringify(reports, null, 2),
  );
  const [analytic, retained] = reports;
  assertEquivalentTextCoverage(analytic!, retained!);
  return reports;
}
