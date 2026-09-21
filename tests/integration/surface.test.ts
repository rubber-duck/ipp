import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { WorldPersistenceHostClient } from "@ipp/client";
import { runNativeEnvironment } from "./environment.js";
import {
  exerciseSurfaceLifecycle,
  surfaceSnapshot,
  type SurfaceTestClient,
} from "./surface-scenario.js";

test("Surface edits, overlays, animation and snapshots cross a real native connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/surface-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  assert.throws(
    () =>
      contract.encodeSurfaceItems({
        nextId: 2,
        items: [{ id: 1, content: { kind: "bitmap", size: [0, 1] } }],
      }),
    /bitmap size/,
  );
  for (const patch of [{ scale: [-1, 1] }, { opacity: 1.1 }, { fontSize: 0 }]) {
    assert.throws(() =>
      contract.encodeSurfaceEdit({
        action: "update",
        entity: 1n,
        id: 1,
        patch,
      }),
    );
    assert.throws(() =>
      contract.encodeSurfaceEdit({
        action: "insert",
        entity: 1n,
        id: 1,
        index: 0,
        content: { kind: "label", text: "A" },
        style: patch,
      }),
    );
  }
  await runNativeEnvironment(
    "surfaces",
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/surfaces/native",
      ),
    },
    context.signal,
    async (env) => {
      const host = await env.track<
        WorldPersistenceHostClient<SurfaceTestClient>
      >(
        contract.IppHostClient.connectWebSocket(env.url, {
          signal: env.signal,
        }),
      );
      const client = await host.createWorld({
        symbolicId: "surface-lifecycle",
      });
      const upload = async (kind: number, path: string) => {
        const bytes = new Uint8Array(await readFile(resolve(workspace, path)));
        return client.createAsset(kind, bytes.buffer);
      };
      const [font, panel, icon, bitmap] = await Promise.all([
        upload(17, "target/font-assets/shure-tech-mono.ippf"),
        upload(18, "target/surface-assets/panel.ippd"),
        upload(18, "target/surface-assets/icon.ippd"),
        upload(2, "target/surface-assets/badge.ippt"),
      ]);
      const glyphs = JSON.parse(
        await readFile(
          resolve(workspace, "target/surface-assets/glyphs.json"),
          "utf8",
        ),
      );
      const result = await env.execute("item lifecycle", {}, () =>
        exerciseSurfaceLifecycle(
          client,
          { font, panel, icon, bitmap },
          glyphs.A,
        ),
      );
      const bytes = await host.saveWorld();
      const before = await surfaceSnapshot(client, result.entity);
      await host.detachWorld();
      const restored = await host.loadWorld(bytes, {
        symbolicId: "surface-restored",
      });
      const entity = (await restored.inspect()).entities.find(
        (entity) => entity.metadata.symbolicId === "surface-api",
      )!;
      const after = await surfaceSnapshot(restored, entity.id);
      assert.deepEqual(after.collection, before.collection);
      assert.equal(after.collection.nextId, 6);
      assert.equal(after.properties.item_5_asset?.kind, "asset");
    },
  );
});
