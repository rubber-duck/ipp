import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { AssetWorldClient } from "@ipp/client";
import { runNativeEnvironment } from "./environment.js";

for (const enabled of [false, true])
  test(`native filesystem reads are ${enabled ? "explicitly configured" : "disabled by default"}`, {
    timeout: 20_000,
  }, async (context) => {
    const workspace = process.cwd();
    const profile = resolve(workspace, "target/world-host-build/native");
    const contract = await import(
      pathToFileURL(resolve(profile, "generated.js")).href
    );
    const root = await mkdtemp(resolve(tmpdir(), "ipp-file-access-"));
    const bytes = new Uint8Array(20);
    bytes.set(new TextEncoder().encode("IPPT"));
    const header = new DataView(bytes.buffer);
    header.setUint32(4, 3, true);
    header.setUint32(8, 1, true);
    header.setUint32(12, 1, true);
    bytes.set([32, 96, 128, 255], 16);
    await writeFile(resolve(root, "fixture.texture"), bytes);
    try {
      await runNativeEnvironment(
        `filesystem ${enabled}`,
        {
          executable: resolve(
            profile,
            process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
          ),
          schemaArtifact: resolve(profile, "contract.bin"),
          workingDirectory: workspace,
          extraArguments: enabled
            ? ["--file-root", root, "--file-prefix", "file:///review/"]
            : [],
          operationTimeoutMs: 10_000,
        },
        context.signal,
        async (environment) =>
          environment.execute(
            "read selected filesystem resource",
            { enabled },
            async () => {
              const client = await environment.track<AssetWorldClient>(
                contract.IppClient.connectWebSocket(environment.url, {
                  signal: environment.signal,
                }),
              );
              const session = client.session;
              const outcome = await client.batch([
                contract.Entity.create(1),
                contract.UnlitTexture.insert(contract.Entity.alias(1), {
                  source: "file:///review/fixture.texture",
                }),
              ]);
              assert.equal(outcome.ok, true);
              for (;;) {
                const page = await client.inspectPage({
                  collection: "resources",
                });
                const resource = page.resources[0];
                if (
                  resource &&
                  (resource.status === "loaded" || resource.status === "failed")
                ) {
                  assert.equal(resource.status, enabled ? "loaded" : "failed");
                  assert.equal(resource.representation.decoded, enabled);
                  assert.equal(client.session, session);
                  assert.equal(
                    (await client.inspectPage({ collection: "entities" }))
                      .entities.length,
                    1,
                  );
                  return {
                    status: resource.status,
                    representation: resource.representation,
                  };
                }
                await client.waitForFrame(page.tick);
              }
            },
          ),
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
