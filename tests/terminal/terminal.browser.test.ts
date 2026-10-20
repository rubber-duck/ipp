import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../harness/browser.js";
import { BrowserTerminalDriver } from "./drivers/browser-terminal.js";
import { terminalScenario } from "./scenarios/terminal.js";

for (const crossOriginIsolation of [false, true]) {
  test(`terminal Worker over IPP; cross-origin isolation ${crossOriginIsolation}`, {
    timeout: 120000,
  }, async (context) => {
    const workspace = process.cwd(),
      profile = resolve(workspace, "target/browser-build/render");
    await runBrowserEnvironment(
      "terminal",
      {
        workspace,
        build: {
          name: "render",
          generatedModule: resolve(profile, "generated.js"),
          runtimeWasm: resolve(profile, "runtime.wasm"),
          contractArtifact: resolve(profile, "contract.bin"),
        },
        crossOriginIsolation,
        operationTimeoutMs: 20000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/terminal",
        ),
      },
      context.signal,
      async (env) => {
        const driver = new BrowserTerminalDriver(env.page);
        await driver.start(`${env.urls.origin}/target/terminal-build/`);
        env.own({ close: () => driver.close() });
        assert.equal(
          await env.page.evaluate(() => crossOriginIsolated),
          crossOriginIsolation,
        );
        await terminalScenario(driver);
        assert.deepEqual(await driver.checkErrors(), []);
      },
    );
  });
}
