import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runNativeEnvironment } from "./environment.js";

test("native World metadata grows and restores beyond the former byte ceiling", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { worldMetadataGrowth } = await import(
    "./scenarios/world-persistence.js"
  );
  await runNativeEnvironment(
    "World metadata growth",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) =>
      environment.execute("World metadata growth", {}, async () => {
        const host = await environment.track<
          Parameters<typeof worldMetadataGrowth>[0]
        >(
          contract.IppHostClient.connectWebSocket(environment.url, {
            signal: environment.signal,
          }),
        );
        return worldMetadataGrowth(host);
      }),
  );
});

test("native named Worlds preserve authored captures and session boundaries", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { namedWorldPersistence } = await import(
    "./scenarios/world-persistence.js"
  );
  await runNativeEnvironment(
    "native World persistence",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) =>
      environment.execute("multiple worlds", {}, () =>
        environment
          .track<Parameters<typeof namedWorldPersistence>[0]>(
            contract.IppHostClient.connectWebSocket(environment.url, {
              signal: environment.signal,
            }),
          )
          .then(namedWorldPersistence),
      ),
  );
});

test("native shared World sessions isolate correlations and scoped cleanup", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { sharedWorldSessions } = await import(
    "./scenarios/world-persistence.js"
  );
  await runNativeEnvironment(
    "shared World sessions",
    {
      executable: resolve(profile, "ipp-server"),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) =>
      sharedWorldSessions(() =>
        contract.IppHostClient.connectWebSocket(environment.url),
      ),
  );
});

test("native shared World assets retain source namespaces through disconnect and save", {
  timeout: 30_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  const { sharedWorldAssetSources } = await import(
    "./scenarios/world-persistence.js"
  );
  await runNativeEnvironment(
    "shared World asset persistence",
    {
      executable: resolve(profile, "ipp-server"),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20_000,
    },
    context.signal,
    async (environment) =>
      sharedWorldAssetSources(
        () => contract.IppHostClient.connectWebSocket(environment.url),
        contract,
      ),
  );
});
