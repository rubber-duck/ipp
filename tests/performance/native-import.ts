/** The same disk adapter and real generated client, targeting native WebSocket. */
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { WorldPersistenceHostClient } from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";
import { importBlenderScene } from "../../integrations/blender/client/disk-import.js";
import { runNativeEnvironment } from "../integration/environment.js";
import { addStressFeatures, readyStressFeatures } from "./features.js";
import { checkStressFeatures } from "./feature-checks.js";

const [input, output, native] = process.argv
  .slice(2)
  .map((path) => resolve(path));
assert.ok(
  input && output && native,
  "Expected export, bundle and native host directories",
);
await mkdir(output, { recursive: true });
const source = JSON.parse(await readFile(resolve(input, "scene.json"), "utf8"));
const fixture = JSON.parse(
  await readFile(resolve(input, "../fixture.json"), "utf8"),
);
const catalog = JSON.parse(
  await readFile(resolve(input, "catalog.json"), "utf8"),
);
const assetName = /^[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*$/;
for (const name of Object.keys(catalog)) {
  assert.match(name, assetName);
  const path = resolve(output, name);
  await mkdir(dirname(path), { recursive: true });
  await copyFile(resolve(input, "assets", name), path);
}
const server = createServer(async (request, response) => {
  const name = request.url?.slice(1);
  try {
    if (!name || !assetName.test(name) || !Object.hasOwn(catalog, name))
      throw new Error("Invalid source");
    response.end(await readFile(resolve(input, "assets", name)));
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
try {
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const contract = await import(
    pathToFileURL(resolve(native, "generated.js")).href
  );
  const prefix = "https://stress.ipp.invalid/";
  const publish = async (bytes: Uint8Array<ArrayBuffer>) => {
    const name = `generated/${randomUUID()}`;
    await mkdir(resolve(output, "generated"), { recursive: true });
    await writeFile(resolve(output, name), bytes);
    return prefix + name;
  };
  const result = await runNativeEnvironment(
    "native Blender benchmark import",
    {
      executable: resolve(native, "ipp-server"),
      schemaArtifact: resolve(native, "contract.bin"),
      workingDirectory: process.cwd(),
      extraArguments: ["--file-root", output, "--file-prefix", prefix],
      operationTimeoutMs: 900000,
      closeTimeoutMs: 60000,
      evidenceParent: resolve(output, "../import-evidence"),
    },
    AbortSignal.timeout(1800000),
    async (environment) => {
      const host = await environment.track<
        WorldPersistenceHostClient<BlenderClient>
      >(
        contract.IppHostClient.connectWebSocket(environment.url, {
          timeoutMs: 60000,
          logLevel: "error",
          signal: environment.signal,
        }),
      );
      let client: BlenderClient | undefined;
      const create = host.createWorld.bind(host);
      host.createWorld = async (options) => {
        client = await create(options);
        return client;
      };
      const assetName = (uri: string) => {
        const match = /^\/assets\/([A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*)$/.exec(
          uri,
        );
        assert.ok(match, "Invalid exported URI");
        return match[1]!;
      };
      const imported = await importBlenderScene(
        host,
        contract,
        source,
        {
          resolve: (uri) => prefix + assetName(uri),
          read: (uri) => `http://127.0.0.1:${address.port}/${assetName(uri)}`,
          publishAnimation: publish,
        },
        { symbolicId: "stress", clipsOnly: true, deferPresentation: true },
      );
      assert.ok(client);
      const features = await addStressFeatures(
        client,
        contract,
        fixture,
        async (_kind, bytes) => publish(bytes),
      );
      const entities = new Map(
        (await client.inspect()).entities.map((entity) => [
          entity.metadata.symbolicId,
          entity.id,
        ]),
      );
      const controllers: bigint[] = [features.controller];
      for (
        let offset = 0;
        offset < imported.manifest.clips.length;
        offset += 64
      ) {
        const drivers = imported.manifest.clips
          .slice(offset, offset + 64)
          .flatMap((entry) =>
            entry.clip.properties.map((property, track) => ({
              target: entities.get(entry.target)!,
              property,
              track,
              source: entry.clip.source,
            })),
          );
        controllers.push(
          await client.createAnimationController({
            drivers,
            speed: 1,
            looping: false,
          }),
        );
      }
      assert.ok(imported.manifest.camera);
      await readyStressFeatures(client, true);
      for (const id of controllers) {
        await client.controlAnimationController(id, { action: "play" });
        await client.controlAnimationController(id, { action: "pause" });
      }
      const connected = client;
      const coverage = await checkStressFeatures(
        client,
        fixture,
        async (time) => {
          for (let offset = 0; offset < controllers.length; offset += 32) {
            await Promise.all(
              controllers.slice(offset, offset + 32).map((id) =>
                connected.controlAnimationController(id, {
                  action: "seek",
                  time,
                }),
              ),
            );
          }
        },
        entities.get(imported.manifest.camera)!,
      );
      await writeFile(
        resolve(output, "features.json"),
        JSON.stringify(
          coverage,
          (_, value) => (typeof value === "bigint" ? String(value) : value),
          2,
        ),
      );
      // Save stopped controllers: the native measurement Host owns activation/time.
      for (const id of controllers)
        await client.controlAnimationController(id, { action: "stop" });
      await writeFile(
        resolve(output, "camera.txt"),
        imported.manifest.camera + "\n",
      );
      await writeFile(resolve(output, "benchmark.ipp"), await host.saveWorld());
      await writeFile(
        resolve(output, "manifest.json"),
        JSON.stringify(imported.manifest, null, 2),
      );
      return {
        entities: imported.entities,
        controllers: controllers.length,
        clips: imported.manifest.clips.length,
      };
    },
  );
  console.log("Native import", result.value);
} finally {
  await new Promise<void>((done) => server.close(() => done()));
}
