import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import type { WorldPersistenceHostClient } from "@ipp/client";
import { runNativeEnvironment } from "./environment.js";
import {
  exerciseGuiLifecycle,
  type GuiTestClient,
} from "./scenarios/gui-lifecycle.js";
import { exerciseGuiInput } from "./scenarios/gui-input.js";

test("GUI roots, node identity and committed values cross a real native connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/gui-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  assert.equal(contract.CAPABILITIES.gui, true);
  assert.throws(() =>
    contract.encodeGuiTree({
      nextId: 2,
      nodes: [],
      controls: [{ id: 1, revision: 0, value: { kind: "bool", value: true } }],
    }),
  );
  await runNativeEnvironment(
    "gui",
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
        "target/integration-artifacts/gui/native",
      ),
    },
    context.signal,
    async (env) => {
      const host = await env.track<WorldPersistenceHostClient<GuiTestClient>>(
        contract.IppHostClient.connectWebSocket(env.url, {
          signal: env.signal,
        }),
      );
      const result = await env.execute("gui lifecycle", {}, () =>
        exerciseGuiLifecycle(host, contract),
      );
      env.evidence.record("gui lifecycle", result);
      assert.equal(result.batchApplied, 50);
      assert.equal(result.batchRequests, 1);
      assert.equal(result.failedBatchApplied, 1);
      assert.equal(result.largeBatchApplied, 18);
      assert.equal(result.largeBatchRequests, 4);
      assert.equal(result.failedLargeBatchApplied, 18);
      assert.equal(result.failedLargeBatchRequests, 3);
    },
  );
});

test("GUI pointer, keyboard and text input routes through a real native connection", {
  timeout: 60000,
}, async (context) => {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/gui-host");
  const contract = await import(
    pathToFileURL(resolve(profile, "generated.js")).href
  );
  assert.equal(contract.CAPABILITIES.gui, true);
  await runNativeEnvironment(
    "gui",
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
        "target/integration-artifacts/gui/native",
      ),
    },
    context.signal,
    async (env) => {
      const host = await env.track<WorldPersistenceHostClient<GuiTestClient>>(
        contract.IppHostClient.connectWebSocket(env.url, {
          signal: env.signal,
        }),
      );
      const font = await readFile(
        resolve(workspace, "target/font-assets/shure-tech-mono.ippf"),
      );
      const result = await env.execute("gui input", {}, () =>
        exerciseGuiInput(
          host,
          font.buffer.slice(
            font.byteOffset,
            font.byteOffset + font.byteLength,
          ) as ArrayBuffer,
        ),
      );
      env.evidence.record("gui input", result);
    },
  );
});
