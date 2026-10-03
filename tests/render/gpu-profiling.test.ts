import { createWebGlCallCounts } from "../../crates/ipp-render-gl/src/services/render/webgl_call_counts.js";
import assert from "node:assert/strict";
import test from "node:test";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import type { ProfileCapture } from "../../packages/ipp-client/src/profiling.js";
import {
  runBrowserEnvironment,
  type BrowserEnvironmentContext,
} from "../browser/environment.js";
import { runNativeEnvironment } from "../integration/environment.js";
import { invoke } from "./evidence.js";
import { encodePng } from "./retained-gui-images.js";
import type { WorkloadFrame } from "./retained-gui-scenario.js";
import type { RenderDeviceInfo } from "../../packages/ipp-client/src/presentation.js";

async function exercise(
  env: BrowserEnvironmentContext,
  connection: Record<string, unknown>,
) {
  let device: RenderDeviceInfo | undefined;
  const module = `${env.urls.origin}/target/surface-build/gpu-profiling-fixture.js`;
  const call = <T>(operation: string, args: readonly unknown[] = []) =>
    env.execute(operation, args, () =>
      invoke<T>(env.page, module, operation, args),
    );
  const artifact = async (name: string, value: unknown) =>
    writeFile(
      resolve(env.evidence.directory, name),
      JSON.stringify(value, null, 2),
    );
  const capture = async (label: string) => {
    const summary = await call<WorkloadFrame>("capture", [
      label,
      { next: true },
    ]);
    assert.equal(summary.failedDrawCalls, 0);
    if (!device && summary.statistics) {
      device = summary.statistics.device;
      await artifact("device.json", device);
    }
    assert.ok(summary.drawCalls > 0, "completed frame contains real rendering");
    const image = await invoke<{
      width: number;
      height: number;
      pixels: string;
    }>(env.page, module, "capturePixels", [label]);
    const pixels = new Uint8Array(Buffer.from(image.pixels, "base64"));
    let different = 0;
    for (let offset = 4; offset < pixels.length; offset += 4)
      if (
        pixels[offset] !== pixels[0] ||
        pixels[offset + 1] !== pixels[1] ||
        pixels[offset + 2] !== pixels[2]
      )
        different++;
    assert.ok(
      different > 100,
      "completed image contains visible Surface content",
    );
    await writeFile(
      resolve(env.evidence.directory, `${label}.png`),
      encodePng({ ...image, pixels }),
    );
    return summary;
  };
  try {
    await call("initialize", [
      { generatedModuleUrl: env.urls.generated, ...connection },
    ]);
    const captureId = await call<string>("profileStart", ["passes", true]);
    for (let sequence = 0; sequence < 3; sequence++) {
      await call("workload", [
        {
          rows: 3,
          columns: 12,
          sequence,
          mode: "typing",
          cursor: true,
          cache: {
            direct_distance: 0,
            texels_per_metre: 80,
            max_refresh_hz: 60,
          },
        },
      ]);
      await capture(`profile-${sequence}`);
    }
    const before = await call<string[]>("profileWorldIds");
    await call("profileDestroyWorkloadWorlds");
    const after = await call<string[]>("profileWorldIds");
    const removed = before.filter((id) => !after.includes(id));
    assert.ok(removed.length > 0, "real temporary Canvas World was destroyed");
    const profile = await call<ProfileCapture>("profileStop");
    await artifact("gpu-capture.json", profile);
    assert.equal(profile.gpu?.glCalls?.availability, "available");
    assert.equal(profile.gpu!.glCalls!.scope, "device-context");
    assert.equal(profile.gpu!.glCalls!.stopReason, "owner-stopped");
    assert.ok(
      BigInt(profile.gpu!.glCalls!.window.end!) >=
        BigInt(profile.gpu!.glCalls!.window.start!),
    );
    assert.ok(BigInt(profile.gpu!.glCalls!.draws) > 0n);
    assert.ok(BigInt(profile.gpu!.glCalls!.state) > 0n);
    assert.ok(BigInt(profile.gpu!.glCalls!.uploads) > 0n);

    assert.equal(profile.captureId, captureId);
    assert.ok(profile.gpu, "render adapter supplied GPU capture evidence");
    assert.ok(
      profile.gpu.records.length > 0,
      "issued passes have explicit measurement or unavailable records",
    );
    for (const record of profile.gpu.records) {
      assert.equal(record.captureId, captureId);
      assert.equal(record.hostId, profile.hostId);
      assert.notEqual(
        record.availability.status,
        "pending",
        "stopped artifact has no live queries",
      );
      if (record.availability.status === "available")
        assert.ok(BigInt(record.availability.duration!) >= 0n);
    }
    const destroyed = profile.gpu.records.filter(
      (record) => record.world && removed.includes(record.world.id),
    );
    assert.ok(
      destroyed.length > 0,
      "issued samples retain destroyed World's original identity",
    );
    assert.ok(
      destroyed.every((record) => record.world!.compositionId !== null),
      "composition was copied before destruction",
    );
    if (process.env.IPP_PROFILE_REQUIRE_GPU === "1") {
      const renderer = device?.unmaskedRenderer ?? device?.renderer;
      assert.ok(
        renderer && !/^(WebKit WebGL|WebKit|unknown)$/i.test(renderer),
        "hardware evidence requires an identified renderer",
      );
      assert.doesNotMatch(
        renderer,
        /swiftshader|llvmpipe|softpipe|lavapipe/i,
        "software query results cannot satisfy hardware evidence",
      );
      assert.ok(
        profile.gpu.records.some(
          (record) => record.availability.status === "available",
        ),
        "hardware evidence requires real available duration",
      );
    }
    await assert.rejects(
      call("profileStart", ["frame", true]),
      /busy/i,
      "unreleased artifact retains exclusive owner",
    );
    await call("profileRelease");

    await call("workload", [
      { rows: 3, columns: 12, sequence: 4, mode: "typing", cursor: true },
    ]);
    await call("profileStart", ["frame", true]);
    await capture("before-loss");
    await call("recover");
    await capture("after-loss");
    const lost = await call<ProfileCapture>("profileStop");
    await artifact("gpu-context-loss.json", lost);
    assert.ok(lost.gpu!.records.length > 0);
    assert.equal(lost.gpu!.glCalls!.availability, "available");
    assert.equal(lost.gpu!.glCalls!.stopReason, "context-lost");
    assert.ok(BigInt(lost.gpu!.glCalls!.draws) > 0n);
    assert.ok(
      BigInt(lost.gpu!.glCalls!.window.end!) >=
        BigInt(lost.gpu!.glCalls!.window.start!),
    );
    const originalContexts = new Set(
      lost.gpu!.records.map((record) => record.contextId),
    );
    await call("profileRelease");
    await call("profileStart", ["frame", true]);
    await capture("replacement-context");
    const replacement = await call<ProfileCapture>("profileStop");
    await artifact("gpu-replacement.json", replacement);
    assert.notEqual(
      replacement.gpu!.glCalls!.contextId,
      lost.gpu!.glCalls!.contextId,
    );
    assert.equal(replacement.gpu!.glCalls!.stopReason, "owner-stopped");
    assert.ok(
      replacement.gpu!.records.some(
        (record) => !originalContexts.has(record.contextId),
      ),
      "new capture identifies replacement context",
    );
    await call("profileRelease");
    await call("profileStart", ["off", true]);
    await capture("gl-only");
    const callsOnly = await call<ProfileCapture>("profileStop");
    await artifact("gl-only-capture.json", callsOnly);
    assert.equal(callsOnly.gpu!.records.length, 0);
    assert.equal(callsOnly.gpu!.glCalls!.availability, "available");
    assert.ok(BigInt(callsOnly.gpu!.glCalls!.draws) > 0n);
    await call("profileRelease");
    await call("profileStart", ["all"]);
    await capture("combined-scopes");
    const combined = await call<ProfileCapture>("profileStop");
    await artifact("gpu-combined-scopes.json", combined);
    if (replacement.gpu!.capability === "timestamps") {
      assert.ok(
        combined.gpu!.records.some((record) => record.scope === "frame"),
      );
      assert.ok(
        combined.gpu!.records.some((record) => record.scope === "surface"),
      );
    } else if (replacement.gpu!.capability === "elapsed") {
      assert.equal(combined.gpu!.availability.status, "unsupported");
      assert.equal(combined.gpu!.records.length, 0);
    }
    await call("profileRelease");
  } finally {
    await call("profileRelease").catch(() => {});
    await call("close");
  }
}

for (const arrangement of ["browser", "native"] as const)
  test(`gpu-profiling:${arrangement}: real frames preserve GPU capture and lifetime identity`, {
    timeout: 600000,
  }, async (context) => {
    const workspace = process.cwd();
    const directory = resolve(
      arrangement === "browser"
        ? "target/browser-build/render-instrumentation"
        : "target/gles-host-instrumentation",
    );
    const executable = resolve(directory, "gles_host");
    const build = {
      name:
        arrangement === "browser"
          ? ("render-instrumentation" as const)
          : ("gles" as const),
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm:
        arrangement === "browser"
          ? resolve(directory, "runtime.wasm")
          : executable,
      contractArtifact: resolve(directory, "contract.bin"),
    };
    const browser = {
      workspace,
      build,
      rendering: arrangement === "browser",
      operationTimeoutMs: 30000,
      evidenceParent: resolve("target/integration-artifacts/gpu-profiling"),
    };
    if (arrangement === "browser")
      await runBrowserEnvironment(
        "gpu-profiling-worker",
        browser,
        context.signal,
        (env) =>
          exercise(env, {
            workerScriptUrl: env.urls.workerScript,
            wasmUrl: env.urls.wasm,
          }),
      );
    else
      await runNativeEnvironment(
        "gpu-profiling-gles",
        {
          executable,
          schemaArtifact: build.contractArtifact,
          workingDirectory: workspace,
          extraArguments: [
            "--egl-dir",
            process.env.IPP_EGL_LIBRARY_DIR ?? "/usr/lib64",
          ],
          readinessTimeoutMs: 30000,
          operationTimeoutMs: 30000,
          evidenceParent: browser.evidenceParent,
          environment:
            process.env.IPP_PROFILE_REQUIRE_GPU === "1"
              ? {}
              : { LIBGL_ALWAYS_SOFTWARE: "1" },
        },
        context.signal,
        (native) => {
          assert.ok(native.presentationUrl);
          return runBrowserEnvironment(
            "gpu-profiling-native-client",
            { ...browser, rendering: false },
            native.signal,
            (env) =>
              exercise(env, {
                nativeHost: {
                  url: native.url,
                  presentationUrl: native.presentationUrl,
                },
              }),
          );
        },
      );
  });

test("gpu-profiling:browser: bridge counts physical calls and freezes at actual loss", () => {
  const original = {
    getParameter() {
      assert.equal(this, original);
      return 42;
    },
    getUniformLocation() {
      assert.equal(this, original);
      return null;
    },
    drawArrays() {
      assert.equal(this, original);
    },
  };
  const calls = createWebGlCallCounts(
    original as unknown as WebGL2RenderingContext,
  );
  calls.imports.gl_calls_start();
  calls.gl.getParameter(0);
  calls.gl.getUniformLocation({} as WebGLProgram, "value");
  assert.throws(() =>
    calls.query(() => {
      calls.gl.getParameter(0);
      throw Error("query fault");
    }),
  );
  calls.gl.getParameter(0);
  calls.contextLost();
  const lossEnd = calls.imports.gl_calls_count(6);
  calls.gl.drawArrays(0, 0, 3);
  calls.query(() => calls.gl.getParameter(0));
  assert.equal(calls.imports.gl_calls_count(0), 0n);
  assert.equal(calls.imports.gl_calls_count(1), 1n);
  assert.equal(calls.imports.gl_calls_count(3), 2n);
  assert.equal(calls.imports.gl_calls_count(4), 1n);
  assert.equal(calls.imports.gl_calls_count(7), 1n);
  assert.equal(calls.imports.gl_calls_count(6), lossEnd);
  calls.imports.gl_calls_start();
  calls.gl.drawArrays(0, 0, 3);
  assert.equal(calls.imports.gl_calls_count(0), 1n);
  assert.equal(calls.imports.gl_calls_count(7), 0n);
});
