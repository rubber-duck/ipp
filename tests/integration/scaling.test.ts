import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";
import { scalingAndInspection } from "./scenarios/scaling.js";

for (const count of [1_000, 4_000, 16_000])
  for (const deep of [false, true]) {
    test(`native scaling and paged inspection: ${count} ${deep ? "deep" : "flat"} entities`, {
      timeout: 120_000,
    }, async (context) => {
      const workspace = resolve(process.cwd());
      const profile = resolve(workspace, "target/scaling-host-build/native");
      const contract = await import(
        pathToFileURL(resolve(profile, "generated.js")).href
      );
      await runNativeEnvironment(
        `scaling ${count} ${deep ? "deep" : "flat"}`,
        {
          executable: resolve(
            profile,
            process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
          ),
          schemaArtifact: resolve(profile, "contract.bin"),
          workingDirectory: workspace,
          operationTimeoutMs: 90_000,
        },
        context.signal,
        async (environment) =>
          environment.execute("scaling", { count, deep }, async () => {
            const host = await environment.track<
              Parameters<typeof scalingAndInspection>[0]
            >(
              contract.IppHostClient.connectWebSocket(environment.url, {
                signal: environment.signal,
                timeoutMs: 30_000,
              }),
            );
            const result = await scalingAndInspection(host, count, deep);
            environment.evidence.record("scaling_timings", result);
            return result;
          }),
      );
    });
  }
