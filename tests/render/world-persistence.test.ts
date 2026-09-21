import assert from "node:assert/strict";
import { writeFile, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";
import { requireVisible } from "./image-assertions.js";
import type * as Fixture from "./world-persistence-fixture.js";

const workspace = resolve(process.cwd());
const profile = resolve(workspace, "target/browser-build/render-expanded");
const build = {
  name: "render-expanded" as const,
  generatedModule: resolve(profile, "generated.js"),
  runtimeWasm: resolve(profile, "runtime.wasm"),
  exportWasm: resolve(profile, "export.wasm"),
  contractArtifact: resolve(profile, "contract.bin"),
};

test("World references and multi-entity controller state survive fresh-worker restore", {
  timeout: 90_000,
}, async (context) => {
  const contract = await import(pathToFileURL(build.generatedModule).href);
  const transform = contract.components.Transform;
  const bytes = contract.encodeAnimationClip({
    duration: 2,
    tracks: [
      {
        property: {
          component: transform.id,
          offsets: [transform.fields.y.offset],
        },
        keys: [
          {
            time: 0,
            value: { kind: "f32", value: -0.5 },
            interpolation: { kind: "linear" },
          },
          { time: 2, value: { kind: "f32", value: 1.5 } },
        ],
      },
    ],
  });
  const assetPath = resolve(profile, "persistence-external.ippa");
  await writeFile(assetPath, bytes);
  try {
    await runBrowserEnvironment(
      "external-source World persistence",
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 30_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/snapshots",
        ),
      },
      context.signal,
      async (environment) => {
        const module = `${environment.urls.origin}/dist/tests/render/world-persistence-fixture.js`;
        const call = <T>(name: string, args: unknown[] = []) =>
          invoke<T>(environment.page, module, name, args);
        const source = `${environment.urls.origin}/target/browser-build/render-expanded/persistence-external.ippa?revision=1`;
        try {
          const saved = await call<Awaited<ReturnType<typeof Fixture.prepare>>>(
            "prepare",
            [environment.urls, source],
          );
          assert.ok(saved.bytes > 32 && saved.bytes < 8192);
          assert.equal(saved.controller.state, "paused");
          assert.equal(saved.controller.transition?.duration, 1.5);
          assert.equal(saved.controller.transition?.easing, "linear");
          assert.ok((saved.controller.transition?.elapsed ?? 0) > 0);
          requireVisible(saved.before.summary, "saved paused animation");
          const restored =
            await call<Awaited<ReturnType<typeof Fixture.restore>>>("restore");
          assert.equal(restored.after.drawCalls, 2);
          assert.equal(restored.difference.changedPixels, 0);
          const recovered =
            await call<Awaited<ReturnType<typeof Fixture.recoverAndStop>>>(
              "recoverAndStop",
            );
          assert.equal(recovered.recovery.changedPixels, 0);
          assert.ok(recovered.difference.changedFraction > 0.01);
          for (const label of [
            "saved-paused",
            "restored-paused",
            "recovered-paused",
            "restored-stopped",
          ]) {
            const image = await call<string>("captureDataUrl", [label]);
            await writeDataUrl(
              resolve(environment.evidence.directory, `${label}.png`),
              image,
            );
          }
          const completed =
            await call<Awaited<ReturnType<typeof Fixture.restoreAndComplete>>>(
              "restoreAndComplete",
            );
          assert.equal(completed.controller.transition, undefined);
          assert.equal(completed.completed.drawCalls, 2);
          requireVisible(
            completed.completed.summary,
            "completed restored transition",
          );
          await writeDataUrl(
            resolve(
              environment.evidence.directory,
              "restored-transition-completed.png",
            ),
            await call<string>("captureDataUrl", [
              "restored-transition-completed",
            ]),
          );
          await rm(assetPath);
          const unavailable =
            await call<Awaited<ReturnType<typeof Fixture.restoreUnavailable>>>(
              "restoreUnavailable",
            );
          assert.equal(unavailable.entities, 3);
          assert.equal(unavailable.time, saved.controller.time);
          assert.equal(unavailable.transition?.duration, 1.5);
          assert.equal(
            unavailable.transition?.elapsed,
            saved.controller.transition?.elapsed,
          );
          assert.equal(unavailable.transition?.easing, "linear");
          assert.equal(unavailable.transition?.pending, true);
          await environment.evidence.writeJson("persistence.json", {
            saved,
            restored,
            recovered,
            unavailable,
          });
        } finally {
          await call("close").catch(() => {});
        }
      },
    );
  } finally {
    await rm(assetPath, { force: true });
  }
});
