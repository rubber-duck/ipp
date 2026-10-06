import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../harness/browser.js";

const profile = resolve("target/browser-build/headless");
const build: BrowserBuildConfiguration = {
  name: "headless",
  generatedModule: resolve(profile, "generated.js"),
  runtimeWasm: resolve(profile, "runtime.wasm"),
  contractArtifact: resolve(profile, "contract.bin"),
};

for (const mode of ["development", "production"]) {
  for (const shared of [false, true]) {
    test(`${mode} gated worker startup cancellation preserves ${shared ? "shared endpoints" : "private owner shutdown"}`, {
      timeout: 30_000,
    }, async (context) => {
      await runBrowserEnvironment(
        `worker-startup-${mode}-${shared ? "shared" : "private"}`,
        {
          workspace: process.cwd(),
          build,
          operationTimeoutMs: 5_000,
        },
        context.signal,
        async (environment) => {
          let release!: () => void;
          const gate = new Promise<void>((resolve) => {
            release = resolve;
          });
          let fetched!: () => void;
          const requested = new Promise<void>((resolve) => {
            fetched = resolve;
          });
          await environment.page.route(environment.urls.wasm, async (route) => {
            fetched();
            await gate;
            await route.continue().catch(() => {});
          });
          const moduleUrl = `${environment.urls.origin}/target/multiplex-tests/worker-startup.js`;
          try {
            await environment.execute(
              "start actual gated worker",
              { mode, shared },
              async () => {
                await environment.page.evaluate(
                  async ({ moduleUrl, configuration, shared }) => {
                    const scenario = (await import(
                      moduleUrl
                    )) as typeof import("./pages/worker-startup.js");
                    await scenario.start(configuration, shared);
                  },
                  {
                    moduleUrl,
                    shared,
                    configuration: {
                      generated: environment.urls.generated,
                      wasm: environment.urls.wasm,
                      worker: `${environment.urls.origin}/target/multiplex-tests/lifecycle-worker-${mode}.js`,
                    },
                  },
                );
                await requested;
              },
            );
            assert.equal(environment.page.workers().length, 1);
            const workerClosed = shared
              ? undefined
              : environment.page.workers()[0]!.waitForEvent("close");
            await environment.execute(
              "cancel before releasing WASM fetch",
              {},
              async () => {
                const result = await environment.page.evaluate(
                  async (moduleUrl) => {
                    const scenario = (await import(
                      moduleUrl
                    )) as typeof import("./pages/worker-startup.js");
                    return scenario.cancel();
                  },
                  moduleUrl,
                );
                await workerClosed;
                assert.deepEqual(result, {
                  ready: 0,
                  sent: 0,
                  replies: 0,
                  cancelled: true,
                });
                assert.equal(environment.page.workers().length, shared ? 1 : 0);
                return result;
              },
            );
            if (shared) {
              await environment.execute(
                "reuse pending endpoint capacity without WASM",
                {},
                () =>
                  environment.page.evaluate(async (moduleUrl) => {
                    const scenario = (await import(
                      moduleUrl
                    )) as typeof import("./pages/worker-startup.js");
                    return scenario.closeUnstarted();
                  }, moduleUrl),
              );
              release();
              await environment.execute(
                "survivor and actual late-ready race",
                {},
                () =>
                  environment.page.evaluate(async (moduleUrl) => {
                    const scenario = (await import(
                      moduleUrl
                    )) as typeof import("./pages/worker-startup.js");
                    return scenario.completeShared();
                  }, moduleUrl),
              );
            }
          } finally {
            release();
            await environment.page.unroute(environment.urls.wasm);
            await environment.page.evaluate(async (moduleUrl) => {
              const scenario = (await import(
                moduleUrl
              )) as typeof import("./pages/worker-startup.js");
              await scenario.close();
            }, moduleUrl);
          }
        },
      );
    });
  }
}
