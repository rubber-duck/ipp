import assert from "node:assert/strict";
import { resolve, join } from "node:path";
import test from "node:test";
import {
  runBrowserEnvironment,
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { responseGate } from "../browser/response-gate.js";
import { invoke, writeDataUrl } from "./evidence.js";
import type { ReplacementSample } from "./ready-geometry-fixture.js";

const workspace = resolve(process.cwd());
const initial = "ipp://mesh/cube?width=2&height=2&length=2";

for (const mode of ["development", "production"] as const) {
  test(`${mode}: geometry replacements retain visible assets until loaded`, {
    timeout: 60_000,
  }, async (context) => {
    const gates = new Map(
      ["mesh", "texture", "obsolete", "unmount"].map((key) => [
        key,
        responseGate(),
      ]),
    );
    const result = await runBrowserEnvironment(
      `${mode} prepared geometry`,
      {
        workspace,
        build: build("render-expanded"),
        mismatchBuild: build("headless"),
        operationTimeoutMs: 12_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/render-viewer",
        ),
        beforeArtifactResponse: async (url, signal) =>
          gates.get(url.search.slice(1))?.hold(signal),
      },
      context.signal,
      async (scenario) => {
        const moduleUrl = `${scenario.url}/target/gallery-fixtures/ready-geometry-${mode}.js`;
        const mesh = `${scenario.url}/target/gallery-fixtures/replacement.mesh`;
        const texture = `${scenario.url}/target/gallery-fixtures/replacement.texture?texture`;
        const call = <T>(name: string, ...args: unknown[]) =>
          invoke<T>(scenario.page, moduleUrl, name, args);
        const wait = (label: string, pending: Promise<unknown>) =>
          scenario.execute(label, {}, () => pending);
        const sample = async (label: string) => {
          const frame = await call<ReplacementSample>("sample");
          // The PNG is an artifact; the bounded event log keeps the rest.
          const { dataUrl, ...observation } = frame;
          await scenario.evidence.record(label, observation);
          await writeDataUrl(
            join(scenario.evidence.directory, `${label}.png`),
            dataUrl,
          );
          assert.equal(
            frame.frame.drawCalls,
            1,
            `${label}: no missing or duplicate draw`,
          );
          assert.ok(
            frame.summary.foregroundPixels > 1000,
            `${label}: geometry remains visible`,
          );
          assert.equal(
            frame.inspection.resources.find(
              (resource) => resource.source === frame.mesh,
            )?.status,
            "loaded",
          );
          assert.deepEqual(frame.errors, []);
          return frame;
        };
        try {
          await call(
            "initialize",
            {
              generatedModuleUrl: scenario.urls.generated,
              workerScriptUrl: scenario.urls.workerScript,
              wasmUrl: scenario.urls.wasm,
            },
            mode === "development",
          );
          await sample("initial");
          await call("edit", `${mesh}?mesh`, texture);
          await wait(
            "mesh and texture requests",
            Promise.all([
              gates.get("mesh")!.requested,
              gates.get("texture")!.requested,
            ]),
          );
          for (let i = 0; i < 4; i++) {
            const pending = await sample(`both-pending-${i}`);
            assert.equal(pending.mesh, initial);
            assert.equal(pending.texture, undefined);
            assert.equal(pending.difference.changedPixels, 0);
          }
          gates.get("mesh")!.release();
          await scenario.page.waitForFunction(async (url) => {
            const fixture = await import(url);
            const result = await fixture.inspect();
            return result.inspection.resources.some(
              (resource: { source: string; status: string }) =>
                resource.source.endsWith("?mesh") &&
                resource.status === "loaded",
            );
          }, moduleUrl);
          const meshOnly = await sample("mesh-ready-texture-pending");
          assert.equal(meshOnly.mesh, initial);
          assert.equal(meshOnly.difference.changedPixels, 0);
          gates.get("texture")!.release();
          await call("waitForSource", `${mesh}?mesh`);
          const replaced = await sample("both-ready");
          assert.equal(replaced.mesh, `${mesh}?mesh`);
          assert.equal(replaced.texture, texture);
          assert.ok(replaced.difference.changedPixels > 1000);
          assert.ok(
            replaced.events.some(
              (event) =>
                event.source === replaced.mesh && event.status === "loaded",
            ),
          );

          // The older HTTP request finishes after a newer built-in edit wins.
          await call("edit", `${mesh}?obsolete`);
          await wait("obsolete request", gates.get("obsolete")!.requested);
          const oldPending = await sample("obsolete-pending");
          assert.equal(oldPending.mesh, replaced.mesh);
          const latest = "ipp://mesh/sphere?radius=1";
          await call("edit", latest);
          await call("waitForSource", latest);
          await wait("obsolete cancellation", gates.get("obsolete")!.aborted);
          gates.get("obsolete")!.release();
          for (let i = 0; i < 3; i++)
            assert.equal((await sample(`latest-wins-${i}`)).mesh, latest);

          await call("edit", `${mesh}?invalid`, `${mesh}?invalid-texture`);
          await scenario.page.waitForFunction(async (url) => {
            const fixture = await import(url);
            return (await fixture.inspect()).inspection.resources.some(
              (resource: { status: string }) => resource.status === "failed",
            );
          }, moduleUrl);
          assert.equal((await sample("failed-retains-current")).mesh, latest);

          // Reuse a loaded source without requiring another Loaded transition.
          await call("edit", initial);
          await call("waitForSource", initial);
          assert.equal((await sample("reuse-loaded-source")).mesh, initial);
          await call("edit", `${mesh}?unmount`);
          await wait("unmount request", gates.get("unmount")!.requested);
          const removed = await call<ReplacementSample>("removeScene");
          assert.deepEqual(removed.inspection.resources, []);
          assert.ok(
            removed.inspection.entities.every(
              (entity) =>
                !entity.metadata.symbolicId?.startsWith("replacement-"),
            ),
          );
          await wait("unmount cancellation", gates.get("unmount")!.aborted);
          gates.get("unmount")!.release();
          return { tick: replaced.frame.tick };
        } finally {
          for (const gate of gates.values()) gate.release();
          await call("close");
        }
      },
    );
    assert.ok(result.value.tick > 0n);
    await assertLoopbackClosed(result.origin);
  });
}

function build(
  name: "render-expanded" | "headless",
): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}
