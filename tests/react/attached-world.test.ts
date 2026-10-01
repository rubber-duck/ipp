import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { Client, HostClientBase } from "@ipp/client";
import { runNativeEnvironment } from "../integration/environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

test("React AttachedWorld lifecycle through native WebSocket", {
  timeout: 90_000,
}, async (context) => {
  process.env.NODE_ENV = "production";
  const { exerciseAttachedWorlds } = await import("./attached-world-case.js");
  const { exerciseAttachedWorldClosures } = await import(
    "./attached-world-closures.js"
  );
  const { exerciseAttachedWorldJournals } = await import(
    "./attached-world-journals.js"
  );
  const workspace = process.cwd();
  const { exerciseAttachedWorldRecovery } = await import(
    "./attached-world-recovery.js"
  );
  const profile = resolve(workspace, "target/surface-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "react attached worlds",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 30_000,
    },
    context.signal,
    async (environment) => {
      const host = await environment.track<HostClientBase<Client>>(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      const report = await environment.execute(
        "React AttachedWorld lifecycle",
        {},
        () => exerciseAttachedWorlds(host),
      );
      environment.evidence.record("React AttachedWorld lifecycle", report);
      assert.equal(report.length, 20);
      const closureReport = await environment.execute(
        "React AttachedWorld closure fences",
        {},
        () =>
          exerciseAttachedWorldClosures(
            () =>
              contract.IppHostClient.connectWebSocket(environment.url, {
                signal: environment.signal,
              }),
            host,
          ),
      );
      assert.equal(closureReport.length, 3);
      const journalReport = await environment.execute(
        "React AttachedWorld partial pages",
        {},
        () => exerciseAttachedWorldJournals(host),
      );
      assert.equal(journalReport.length, 2);
      const recovery = await environment.execute(
        "React AttachedWorld review regressions",
        {},
        () => exerciseAttachedWorldRecovery(host),
      );
      assert.equal(recovery.length, 24);
    },
  );
});

test("React CanvasWorld through native WebSocket", {
  timeout: 90_000,
}, async (context) => {
  process.env.NODE_ENV = "production";
  const { exerciseCanvasWorlds } = await import("./canvas-world-case.js");
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/surface-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  await runNativeEnvironment(
    "react canvas worlds",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 30_000,
    },
    context.signal,
    async (environment) => {
      const host = await environment.track<HostClientBase<Client>>(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      const report = await environment.execute("React CanvasWorld", {}, () =>
        exerciseCanvasWorlds(host),
      );
      environment.evidence.record("React CanvasWorld", report);
      assert.equal(report.length, 4);
    },
  );
});

for (const variant of ["development", "production"] as const) {
  test(`${variant}: React CanvasWorld through worker WASM`, {
    timeout: 90_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(
      workspace,
      "target/browser-build/headless-surfaces",
    );
    const build: BrowserBuildConfiguration = {
      name: "headless-surfaces",
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, "runtime.wasm"),
      exportWasm: resolve(profile, "export.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    await runBrowserEnvironment(
      `react canvas worlds ${variant}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 30_000,
      },
      context.signal,
      async (environment) => {
        const report = await environment.execute("React CanvasWorld", {}, () =>
          environment.page.evaluate(
            async ({ urls, variant }) => {
              const contract = await import(urls.generated);
              const fixture = await import(
                `${urls.origin}/target/react-attached/fixture-${variant}.js`
              );
              const host = await contract.IppHostClient.connectWorker(
                urls.workerScript,
                urls.wasm,
              );
              try {
                return (await fixture.exerciseCanvasWorlds(host)) as string[];
              } catch (error) {
                throw new Error(
                  error instanceof Error
                    ? `${error.message}\n${error.stack}`
                    : String(error),
                );
              } finally {
                await host.close();
              }
            },
            { urls: environment.urls, variant },
          ),
        );
        environment.evidence.record("React CanvasWorld", report);
        assert.equal(report.length, 4);
      },
    );
  });
}

for (const variant of ["development", "production"] as const) {
  test(`${variant}: React AttachedWorld lifecycle through worker WASM`, {
    timeout: 90_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(
      workspace,
      "target/browser-build/headless-surfaces",
    );
    const build: BrowserBuildConfiguration = {
      name: "headless-surfaces",
      generatedModule: resolve(profile, "generated.js"),
      runtimeWasm: resolve(profile, "runtime.wasm"),
      exportWasm: resolve(profile, "export.wasm"),
      contractArtifact: resolve(profile, "contract.bin"),
    };
    await runBrowserEnvironment(
      `react attached worlds ${variant}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 30_000,
      },
      context.signal,
      async (environment) => {
        const report = await environment.execute(
          "React AttachedWorld lifecycle",
          {},
          () =>
            environment.page.evaluate(
              async ({ urls, variant }) => {
                const contract = await import(urls.generated);
                const fixture = await import(
                  `${urls.origin}/target/react-attached/fixture-${variant}.js`
                );
                const host = await contract.IppHostClient.connectWorker(
                  urls.workerScript,
                  urls.wasm,
                );
                try {
                  const lifecycle = (await fixture.exerciseAttachedWorlds(
                    host,
                  )) as string[];
                  const closures = (await fixture.exerciseAttachedWorldClosures(
                    () =>
                      contract.IppHostClient.connectWorker(
                        urls.workerScript,
                        urls.wasm,
                      ),
                  )) as string[];
                  const journals = (await fixture.exerciseAttachedWorldJournals(
                    host,
                  )) as string[];
                  const recovery = (await fixture.exerciseAttachedWorldRecovery(
                    host,
                  )) as string[];
                  return [...lifecycle, ...closures, ...journals, ...recovery];
                } catch (error) {
                  throw new Error(
                    error instanceof Error
                      ? `${error.message}\n${error.stack}`
                      : String(error),
                  );
                } finally {
                  await host.close();
                }
              },
              { urls: environment.urls, variant },
            ),
        );
        environment.evidence.record("React AttachedWorld lifecycle", report);
        assert.equal(report.length, 49);
      },
    );
  });
}
