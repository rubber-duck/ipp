import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";
import { lifecycleTargetTransport } from "./lifecycle-target-transport.js";
import { lifecycleTargets } from "./scenarios/lifecycle-targets.js";

for (const diagnostics of [false, true]) {
  test(diagnostics
    ? "diagnostic lifecycle native membership and constant indexed work"
    : "native lifecycle target membership and bulk delivery within the default budget", {
    timeout: 90_000,
  }, async (context) => {
    // Every Host answers lifecycle statistics; the diagnostic variant reads them.
    const profile = resolve("target/world-host-build/native");
    const contract = await import(
      pathToFileURL(resolve(profile, "generated.js")).href
    );
    await runNativeEnvironment(
      diagnostics
        ? "lifecycle-targets-diagnostics-native"
        : "lifecycle-targets-native",
      {
        executable: resolve(
          profile,
          process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
        ),
        schemaArtifact: resolve(profile, "contract.bin"),
        workingDirectory: process.cwd(),
        operationTimeoutMs: 60_000,
      },
      context.signal,
      async (environment) =>
        environment.execute("target-lifecycle-default-budget", {}, async () => {
          const controlled = lifecycleTargetTransport(
            contract,
            webSocketTransport(environment.url),
          );
          const hosts = await Promise.all([
            contract.IppHostClient.connectTransport(
              webSocketTransport(environment.url),
            ),
            contract.IppHostClient.connectTransport(controlled.transport),
            contract.IppHostClient.connectTransport(
              webSocketTransport(environment.url),
            ),
          ]);
          try {
            return await lifecycleTargets(
              hosts[0],
              hosts[1],
              hosts[2],
              controlled.probe,
              diagnostics,
            );
          } finally {
            await Promise.allSettled(hosts.map((host) => host.close()));
          }
        }),
    );
  });

  for (const mode of ["development", "production"]) {
    test(diagnostics
      ? `diagnostic lifecycle ${mode} worker constant indexed work`
      : `${mode} worker lifecycle targets through executed WASM`, {
      timeout: 90_000,
    }, async (context) => {
      const profile = resolve("target/world-host-build/wasm");
      const build: BrowserBuildConfiguration = {
        name: "world-host",
        generatedModule: resolve(profile, "generated.js"),
        runtimeWasm: resolve(profile, "runtime.wasm"),
        contractArtifact: resolve(profile, "contract.bin"),
      };
      await runBrowserEnvironment(
        `lifecycle-targets-${diagnostics ? "diagnostics-" : ""}worker-${mode}`,
        {
          workspace: process.cwd(),
          build,
          operationTimeoutMs: 60_000,
        },
        context.signal,
        async (environment) =>
          environment.execute(
            "target-lifecycle-default-budget",
            { mode, diagnostics },
            () =>
              environment.page.evaluate(
                async ({ urls, mode, diagnostics }) => {
                  const contract = await import(urls.generated);
                  const driver = await import(
                    `${urls.origin}/target/multiplex-tests/lifecycle-target-transport.js`
                  );
                  const scenario = await import(
                    `${urls.origin}/target/multiplex-tests/lifecycle-targets.js`
                  );
                  const owner = driver.createWorkerHost(
                    `${urls.origin}/target/multiplex-tests/lifecycle-worker-${mode}.js`,
                    urls.wasm,
                    contract.MAX_MESSAGE_BYTES,
                  );
                  const controlled = driver.lifecycleTargetTransport(
                    contract,
                    owner.connect(),
                  );
                  const hosts = await Promise.all([
                    contract.IppHostClient.connectTransport(owner.connect()),
                    contract.IppHostClient.connectTransport(
                      controlled.transport,
                    ),
                    contract.IppHostClient.connectTransport(owner.connect()),
                  ]);
                  try {
                    return await scenario.lifecycleTargets(
                      hosts[0],
                      hosts[1],
                      hosts[2],
                      controlled.probe,
                      diagnostics,
                    );
                  } finally {
                    await Promise.allSettled(hosts.map((host) => host.close()));
                    await owner.close();
                  }
                },
                { urls: environment.urls, mode, diagnostics },
              ),
          ),
      );
    });
  }
}
