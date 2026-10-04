import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";

interface SchedulerProbe {
  start(): void;
  release(): void;
  cancel(): void;
  state(): number[];
}

const profile = resolve("target/browser-build/render-instrumentation");

test("actual WASM worker advances bounded local and gated IO tasks while frames are suspended", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserEnvironment(
    "task-scheduler",
    {
      workspace: process.cwd(),
      build: {
        name: "render-instrumentation",
        generatedModule: resolve(profile, "generated.js"),
        runtimeWasm: resolve(profile, "runtime.wasm"),
        contractArtifact: resolve(profile, "contract.bin"),
      },
      operationTimeoutMs: 8_000,
    },
    context.signal,
    async (environment) => {
      const moduleUrl = `${environment.urls.origin}/target/multiplex-tests/task-scheduler.js`;
      try {
        await environment.execute(
          "start production worker and generated client",
          {},
          () =>
            environment.page.evaluate(
              async ({ moduleUrl, urls }) => {
                const scenario = (await import(
                  moduleUrl
                )) as typeof import("./task-scheduler.js");
                await scenario.start(urls);
              },
              { moduleUrl, urls: environment.urls },
            ),
        );
        assert.equal(environment.page.workers().length, 1);
        const worker = environment.page.workers()[0]!;
        await environment.execute(
          "observe service-only fairness, context and cancellation",
          {},
          async () => {
            const observations = await worker.evaluate(async () => {
              const probe = (
                globalThis as unknown as { ippScheduler: SchedulerProbe }
              ).ippScheduler;
              const waitFor = async (ready: () => boolean) => {
                const deadline = performance.now() + 3_000;
                while (!ready()) {
                  if (performance.now() > deadline)
                    throw new Error(`Scheduler stalled: ${probe.state()}`);
                  await new Promise<void>((resolve) => setTimeout(resolve, 2));
                }
              };
              probe.start();
              const initial = probe.state();
              await waitFor(() => probe.state()[0] === 256);
              const gated = probe.state();
              probe.cancel();
              await waitFor(() => probe.state()[4] === 1);
              probe.release();
              await waitFor(() => probe.state()[3] === 1);
              return { initial, gated, complete: probe.state() };
            });
            assert.deepEqual(observations.initial, [0, 0, 0, 0, 2, 0]);
            assert.deepEqual(observations.gated, [256, 1, 0, 0, 2, 0]);
            assert.deepEqual(observations.complete, [256, 1, 1, 1, 1, 0]);
            return observations;
          },
        );
      } finally {
        await environment.page.evaluate(async (moduleUrl) => {
          const scenario = (await import(
            moduleUrl
          )) as typeof import("./task-scheduler.js");
          await scenario.close();
        }, moduleUrl);
      }
    },
  );
});
