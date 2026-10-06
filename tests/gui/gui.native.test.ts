import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import {
  runNativeEnvironment,
  type NativeEnvironmentContext,
} from "../harness/native.js";
import {
  exerciseGuiDensity,
  exerciseGuiLifecycle,
  exerciseGuiTransientRestore,
  type GuiContract,
  type GuiHost,
} from "./scenarios/gui-lifecycle.js";
import { exerciseGuiInput } from "./scenarios/gui-input.js";
import { exerciseGuiVirtualList } from "./scenarios/gui-virtual-list.js";

interface GeneratedHost extends GuiContract {
  IppHostClient: {
    connectWebSocket(
      url: string,
      options: { signal: AbortSignal },
    ): Promise<GuiHost>;
  };
}

async function generated(directory: string): Promise<GeneratedHost> {
  const contract = (await import(
    pathToFileURL(resolve(directory, "generated.js")).href
  )) as GeneratedHost;
  return contract;
}

async function fontBytes(): Promise<ArrayBuffer> {
  const font = await readFile(
    resolve(process.cwd(), "target/font-assets/shure-tech-mono.ippf"),
  );
  return font.buffer.slice(
    font.byteOffset,
    font.byteOffset + font.byteLength,
  ) as ArrayBuffer;
}

/** Headless native WebSocket Host with GUI, without presentation. */
async function headless(
  name: string,
  signal: AbortSignal,
  scenario: (
    environment: NativeEnvironmentContext,
    host: GuiHost,
    contract: GeneratedHost,
  ) => Promise<void>,
) {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/integration-artifacts/native");
  const contract = await generated(
    resolve(workspace, "target/integration-artifacts/client"),
  );
  await runNativeEnvironment(
    name,
    {
      executable: resolve(
        profile,
        process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
      ),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      operationTimeoutMs: 40_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/native",
      ),
    },
    signal,
    async (environment) => {
      const host = await environment.track(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      await scenario(environment, host, contract);
    },
  );
}

/** Presenting native WebSocket Host whose root output renders through
 * GLES; captures and physical input use its presentation surface. */
async function presented(
  name: string,
  signal: AbortSignal,
  scenario: (
    environment: NativeEnvironmentContext,
    host: GuiHost,
    contract: GeneratedHost,
  ) => Promise<void>,
) {
  const workspace = process.cwd();
  const profile = resolve(workspace, "target/gles-host");
  const contract = await generated(profile);
  await runNativeEnvironment(
    name,
    {
      executable: resolve(profile, "gles_host"),
      schemaArtifact: resolve(profile, "contract.bin"),
      workingDirectory: workspace,
      extraArguments: [
        "--egl-dir",
        process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
      ],
      readinessTimeoutMs: 30_000,
      operationTimeoutMs: 90_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/gui/gles",
      ),
    },
    signal,
    async (environment) => {
      const host = await environment.track(
        contract.IppHostClient.connectWebSocket(environment.url, {
          signal: environment.signal,
        }),
      );
      await scenario(environment, host, contract);
    },
  );
}

test("ordinary GUI declarations, batches, compare-and-set and persistence cross a real native connection", {
  timeout: 90_000,
}, async (context) => {
  await headless(
    "gui-lifecycle",
    context.signal,
    async (environment, host, contract) => {
      const result = await environment.execute("gui lifecycle", {}, () =>
        exerciseGuiLifecycle(host, contract),
      );
      environment.evidence.record("gui lifecycle", result);
      assert.equal(result.batchApplied, 50);
      assert.equal(result.failedBatchApplied, 1);
      assert.equal(result.largeBatchApplied, 18);
      assert.equal(result.failedLargeBatchApplied, 18);
    },
  );
});

test("per-Canvas density and transient state restore present through native WebSocket/GLES", {
  timeout: 120_000,
}, async (context) => {
  const font = await fontBytes();
  await presented(
    "gui-presented-lifecycle",
    context.signal,
    async (environment, host, contract) => {
      const density = await environment.execute("gui density", {}, () =>
        exerciseGuiDensity(host, contract),
      );
      environment.evidence.record("gui density", density);
      const restored = await environment.execute(
        "gui transient restore",
        {},
        () => exerciseGuiTransientRestore(host, contract, font),
      );
      environment.evidence.record("gui transient restore", restored);
      assert.equal(restored.frames.restoredPixels, 0);
    },
  );
});

test("GUI pointer, keyboard and native text input route through native WebSocket/GLES", {
  timeout: 180_000,
}, async (context) => {
  const font = await fontBytes();
  await presented("gui-input", context.signal, async (environment, host) => {
    const result = await environment.execute("gui input", {}, () =>
      exerciseGuiInput(host, font),
    );
    environment.evidence.record("gui input", result);
  });
});

test("a 100000-item VirtualList scrolls, anchors and restores through native WebSocket/GLES", {
  timeout: 120_000,
}, async (context) => {
  await presented(
    "gui-virtual-list",
    context.signal,
    async (environment, host) => {
      const result = await environment.execute("gui virtual list", {}, () =>
        exerciseGuiVirtualList(host),
      );
      environment.evidence.record("gui virtual list", result);
      assert.deepEqual(result.attached, [0, 10]);
      assert.ok(result.declaredAtOnce <= 14);
      assert.equal(result.frames.header, 0);
    },
  );
});
