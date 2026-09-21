import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { resolve, relative } from "node:path";
import { writeFile } from "node:fs/promises";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import type { run } from "./blender-disk-fixture.js";

const execute = promisify(execFile);
test("standard Blender disk export imports a reusable action library and renders after deserialization", {
  timeout: 120000,
}, async (context) => {
  const workspace = process.cwd(),
    directory = resolve(workspace, "target/browser-build/render-expanded");
  const build = {
    name: "render-expanded" as const,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "Blender disk pipeline",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 45000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/blender-disk",
      ),
    },
    context.signal,
    async (environment) => {
      const exported = resolve(environment.evidence.directory, "export"),
        bundled = resolve(environment.evidence.directory, "bundle");
      const blender = await execute(
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
        { signal: context.signal },
      );
      await writeFile(
        resolve(environment.evidence.directory, "export.log"),
        blender.stdout + blender.stderr,
      );
      const imported = await execute(
        process.execPath,
        [
          "tools/import_blender_scene.mjs",
          exported,
          bundled,
          "--namespace",
          "disk-test",
          "--clips-only",
        ],
        { cwd: workspace, signal: context.signal },
      );
      await writeFile(
        resolve(environment.evidence.directory, "import.log"),
        imported.stdout + imported.stderr,
      );
      await environment.page.exposeFunction(
        "recordDisk",
        async (label: string, url: string) =>
          writeDataUrl(
            resolve(environment.evidence.directory, label + ".png"),
            url,
          ),
      );
      const bundleUrl = new URL(
        relative(workspace, bundled) + "/",
        environment.urls.origin + "/",
      ).href;
      const result = await invoke<Awaited<ReturnType<typeof run>>>(
        environment.page,
        environment.urls.origin + "/dist/tests/render/blender-disk-fixture.js",
        "run",
        [environment.urls, bundleUrl],
      );
      assert.ok(
        result.clips.includes("Walk") &&
          result.clips.includes("Run") &&
          result.clips.includes("Survey"),
      );
      assert.ok(result.changedPixels > 100);
      await environment.evidence.writeJson("result.json", result);
    },
  );
});
