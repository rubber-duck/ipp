import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { resolve, relative } from "node:path";
import { promisify } from "node:util";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import type { Inspection } from "@ipp/client";
import type { BlenderSnapshot } from "../../integrations/blender/client/types.js";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";

const execute = promisify(execFile);

test("real Blender disk export saves and reloads reference-only hierarchy through worker WASM", {
  timeout: 240_000,
}, async (context) => {
  const workspace = resolve(process.cwd());
  const profile = resolve(workspace, "target/browser-build/render-expanded");
  const build = {
    name: "render-expanded" as const,
    generatedModule: resolve(profile, "generated.js"),
    runtimeWasm: resolve(profile, "runtime.wasm"),
    exportWasm: resolve(profile, "export.wasm"),
    contractArtifact: resolve(profile, "contract.bin"),
  };
  const evidenceParent = resolve(
    workspace,
    "target/integration-artifacts/blender-disk-headless",
  );
  await mkdir(evidenceParent, { recursive: true });
  const evidence = await mkdtemp(resolve(evidenceParent, "run-"));
  const exported = resolve(evidence, "export");
  const bundled = resolve(evidence, "bundle");
  const run = async (file: string, args: string[], log: string) => {
    try {
      const result = await execute(file, args, {
        cwd: workspace,
        signal: context.signal,
        maxBuffer: 4 * 1024 * 1024,
      });
      await writeFile(resolve(evidence, log), result.stdout + result.stderr);
    } catch (error) {
      const failure = error as Error & { stdout?: string; stderr?: string };
      await writeFile(
        resolve(evidence, log),
        `${failure.stack ?? failure}\n${failure.stdout ?? ""}${failure.stderr ?? ""}`,
      );
      throw error;
    }
  };
  await run(
    process.env.BLENDER_BIN ?? "blender",
    [
      "-b",
      resolve(workspace, "tests/fixtures/blender/fox.blend"),
      "-t",
      "4",
      "--python-exit-code",
      "1",
      "--python",
      resolve(workspace, "integrations/blender/export_scene.py"),
      "--",
      exported,
    ],
    "export.log",
  );
  await run(
    process.execPath,
    [
      "tools/import_blender_scene.mjs",
      exported,
      bundled,
      "--namespace",
      "disk-headless",
      "--clips-only",
    ],
    "import.log",
  );
  const scene: BlenderSnapshot = JSON.parse(
    await readFile(resolve(exported, "scene.json"), "utf8"),
  );
  const catalog = JSON.parse(
    await readFile(resolve(bundled, "catalog.json"), "utf8"),
  );
  const sourceCatalog = JSON.parse(
    await readFile(resolve(exported, "catalog.json"), "utf8"),
  );
  assert.ok(scene.scene.entities.length >= 5);
  assert.ok(Object.keys(catalog).length > 0);
  for (const [name, description] of Object.entries(catalog) as [
    string,
    { bytes: number },
  ][]) {
    assert.equal(
      (await readFile(resolve(bundled, name))).byteLength,
      description.bytes,
    );
  }
  for (const name of Object.keys(sourceCatalog)) {
    const source = await readFile(resolve(exported, "assets", name));
    const copied = await readFile(resolve(bundled, name));
    assert.equal(
      createHash("sha256").update(copied).digest("hex"),
      createHash("sha256").update(source).digest("hex"),
    );
  }
  await runBrowserEnvironment(
    "blender-disk-headless",
    {
      workspace,
      build,
      mismatchBuild: build,
      rendering: false,
      operationTimeoutMs: 60_000,
      evidenceParent: resolve(
        "target/integration-artifacts/blender-disk-headless",
      ),
    },
    context.signal,
    async (environment) => {
      const bundleUrl = new URL(
        relative(workspace, bundled) + "/",
        environment.urls.origin + "/",
      ).href;
      const result = await environment.execute(
        "blender-disk-headless",
        {},
        () =>
          environment.page.evaluate(
            async ({ urls, bundle }) => {
              const contract = await import(urls.generated);
              const canvas = document.createElement("canvas");
              canvas.width = 320;
              canvas.height = 240;
              const host = await contract.IppHostClient.connectWorker(
                urls.workerScript,
                urls.wasm,
                {
                  canvas: canvas.transferControlToOffscreen(),
                  resourceUrls: [
                    {
                      prefix: "https://disk-headless.ipp.invalid/",
                      baseUrl: bundle,
                    },
                  ],
                },
              );
              let loaded;
              let client: BlenderClient | undefined;
              let loadFailure: unknown;
              try {
                const bytes = new Uint8Array(
                  await (await fetch(bundle + "world.ipp")).arrayBuffer(),
                );
                const scene: BlenderSnapshot = await (
                  await fetch(bundle + "blender-scene.json")
                ).json();
                const manifest = await (
                  await fetch(bundle + "manifest.json")
                ).json();
                loaded = await host.loadWorld(bytes);
                const worldClient: BlenderClient = await host.openWorld(
                  loaded.root,
                );
                client = worldClient;
                const state: Inspection = await worldClient.inspect();
                const byName = new Map(
                  state.entities.map((entity) => [
                    entity.metadata.symbolicId,
                    entity,
                  ]),
                );
                const byId = new Map(
                  scene.scene.entities.map((entity) => [entity.id, entity]),
                );
                let links = 0;
                for (const entity of scene.scene.entities) {
                  const actual = byName.get(entity.name ?? entity.id);
                  if (!actual)
                    throw new Error(`Missing disk entity ${entity.name}`);
                  if (entity.parent) {
                    const parent = byId.get(entity.parent);
                    if (!parent)
                      throw new Error(
                        `Missing exported parent ${entity.parent}`,
                      );
                    const expected = byName.get(parent.name ?? parent.id);
                    if (actual.link.parent !== expected?.id)
                      throw new Error(`Disk link mismatch for ${entity.name}`);
                    links++;
                  }
                  if (
                    entity.parent_bone !== undefined &&
                    !actual.components.some(
                      (component) =>
                        component.component ===
                          worldClient.components.ParentJoint?.id &&
                        component.fields.ordinal === entity.parent_bone,
                    )
                  )
                    throw new Error(`Disk joint mismatch for ${entity.name}`);
                }
                if (!links) throw new Error("Fixture had no hierarchy links");
                if (state.controllers?.length)
                  throw new Error("Clips-only import activated controllers");
                if (manifest.camera && !byName.has(manifest.camera))
                  throw new Error("Disk manifest selected a missing camera");
                return {
                  entities: state.entities.length,
                  links,
                  assets: state.resources.length,
                };
              } catch (error) {
                loadFailure = error;
                throw error;
              } finally {
                const cleanupFailures: unknown[] = [];
                const cleanup = async (action: () => Promise<unknown>) => {
                  try {
                    await action();
                  } catch (error) {
                    cleanupFailures.push(error);
                  }
                };
                if (client) {
                  const exactClient = client;
                  await cleanup(() => exactClient.close());
                }
                if (loaded)
                  for (const world of loaded.created.values())
                    await cleanup(() => host.destroyWorld(world));
                await cleanup(() => host.close());
                if (cleanupFailures.length)
                  throw new AggregateError(
                    loadFailure === undefined
                      ? cleanupFailures
                      : [loadFailure, ...cleanupFailures],
                    "Blender disk reload cleanup failed",
                  );
              }
            },
            { urls: environment.urls, bundle: bundleUrl },
          ),
      );
      assert.equal(result.entities, scene.scene.entities.length);
      assert.ok(result.links > 0);
      assert.ok(result.assets > 0);
      await environment.evidence.writeJson("disk-headless-result.json", result);
    },
  );
});
