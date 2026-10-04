import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { WorldPersistenceHostClient } from "@ipp/client";
import { runNativeEnvironment } from "./environment.js";
import {
  canvasSnapshot,
  exerciseSurfaceLifecycle,
  SURFACE_CACHE_POLICY,
  surfaceCachePolicy,
  type SurfaceTestClient,
} from "./surface-scenario.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  SURFACE,
  CANVAS,
  selectSystems,
} from "./system-selections.js";

test("Surface Canvas entities, field writes, animation and graph snapshots cross a real native connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/world-host-build/native");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
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
      const created = await host.createWorld({
        selectedSystems: selectSystems(ATTACHMENTS, SURFACE, CANVAS, LIFECYCLE),
        symbolicId: "surface-lifecycle",
      });
      const client = await host.openWorld(created.reference);
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
      const result = await env.execute("Canvas entity lifecycle", {}, () =>
        exerciseSurfaceLifecycle(
          host,
          client,
          { font, panel, icon, bitmap },
          glyphs.A,
          { encodeRowsTable: contract.encodeRowsTable },
        ),
      );
      assert.equal(
        (
          await client.batch([
            {
              kind: "create",
              alias: 81,
              metadata: { symbolicId: "persist-cylinder", classes: [] },
            },
            contract.CylinderSurface.insert(contract.Entity.alias(81), {
              width: 1.2,
              height: 0.8,
              curvature: 0.7,
              layer_spacing: 0.04,
            }),
            {
              kind: "create",
              alias: 82,
              metadata: { symbolicId: "persist-sphere", classes: [] },
            },
            contract.SphereSurface.insert(contract.Entity.alias(82), {
              width: 1.4,
              height: 0.9,
              curvature: -0.6,
              layer_spacing: -0.02,
            }),
          ])
        ).ok,
        true,
        "Generated curved providers did not cross native transport",
      );
      const bytes = await host.saveWorld(client.session);
      const before = await canvasSnapshot(
        result.canvasClient,
        result.canvasEntity,
      );
      const graph = await host.inspectWorldGraph(bytes);
      const canvasNode = graph.nodes.find(
        (node) => node.symbolicId === "surface-api-canvas",
      );
      assert.ok(canvasNode, "World graph omitted the attached Canvas World");
      const loaded = await host.loadWorld(bytes, {
        symbolicId: "surface-restored",
        worldNames: new Map([[canvasNode.id, "surface-api-canvas-restored"]]),
      });
      const canvasWorld = loaded.created.get(canvasNode.id);
      assert.ok(canvasWorld, "World graph did not restore the Canvas World");
      const restored = await host.openWorld(loaded.root);
      const restoredCanvas = await host.openWorld(canvasWorld);
      const restoredSnapshot = await restored.inspect();
      for (const [symbol, component, expected] of [
        [
          "persist-cylinder",
          restored.components.CylinderSurface!.id,
          { width: 1.2, height: 0.8, curvature: 0.7, layer_spacing: 0.04 },
        ],
        [
          "persist-sphere",
          restored.components.SphereSurface!.id,
          { width: 1.4, height: 0.9, curvature: -0.6, layer_spacing: -0.02 },
        ],
      ] as const) {
        const provider = restoredSnapshot.entities
          .find((entity) => entity.metadata.symbolicId === symbol)
          ?.components.find((item) => item.component === component);
        assert.ok(provider, `Graph transfer omitted ${symbol}`);
        for (const [field, value] of Object.entries(expected))
          assert.ok(
            Math.abs(Number(provider.fields[field]) - value) < 1e-6,
            `Graph transfer changed ${symbol}.${field}`,
          );
      }

      const restoredAnchor = (await restored.inspect()).entities.find(
        (entity) => entity.metadata.symbolicId === "surface-api",
      );
      assert.ok(restoredAnchor, "Restored Surface anchor disappeared");
      const restoredSurface = restoredAnchor.components.find(
        (item) => item.component === restored.components.FlatSurface!.id,
      );
      assert.ok(restoredSurface, "Restored Surface lost its dimensions");
      assert.equal(restoredSurface.fields.width, 4);
      assert.equal(restoredSurface.fields.height, 3);
      const restoredCanvasEntity = (
        await restoredCanvas.inspect()
      ).entities.find((entity) => entity.metadata.symbolicId === "canvas");
      assert.ok(restoredCanvasEntity, "Restored Canvas entity disappeared");
      assert.deepEqual(
        await canvasSnapshot(restoredCanvas, restoredCanvasEntity.id),
        before,
      );
      const attachment = restoredAnchor.components.find(
        (item) => item.component === restored.components.WorldAttachment!.id,
      );
      assert.ok(attachment, "Restored Surface lost its WorldAttachment");
      assert.equal(attachment.fields.mode, 1);
      // A SurfaceCanvas attachment presents the child World's canvas and
      // names no output of its own.
      assert.equal(attachment.fields.output, null);
      assert.deepEqual(
        await surfaceCachePolicy(restored, restoredAnchor.id),
        SURFACE_CACHE_POLICY,
      );
    },
  );
});
