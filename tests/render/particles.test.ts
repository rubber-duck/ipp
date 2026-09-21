import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { invoke, writeDataUrl } from "./evidence.js";

test("particles batch thousands of sprites, preserve state through recovery and support custom meshes and cache scrubbing", {
  timeout: 120000,
}, async (context) => {
  const workspace = process.cwd(),
    directory = resolve(workspace, "target/browser-build/render-particles");
  const build = {
    name: "render-particles" as const,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
  await runBrowserEnvironment(
    "particles",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 20000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/particles",
      ),
    },
    context.signal,
    async (env) => {
      const module = `${env.urls.origin}/dist/tests/render/particles-fixture.js`;
      const call = <T>(name: string, args: readonly unknown[] = []) =>
        env.execute(name, args, () => invoke<T>(env.page, module, name, args));
      const capture = async (name: string) => {
        const result = await call<{
          green: number;
          red: number;
          blue: number;
          draws: number;
        }>("capture", [name]);
        await writeDataUrl(
          resolve(env.evidence.directory, `${name}.png`),
          await call<string>("captureDataUrl", [name]),
        );
        return result;
      };
      try {
        await call("initialize", [
          {
            generatedModuleUrl: env.urls.generated,
            workerScriptUrl: env.urls.workerScript,
            wasmUrl: env.urls.wasm,
          },
        ]);
        assert.ok((await capture("sprites")).green > 2000);
        await call("recover");
        assert.ok((await capture("recovered")).green > 2000);
        assert.equal(await call("equal", ["sprites", "recovered"]), true);
        const loaded = await call<{ draws: number }>("restoreLive");
        assert.equal(loaded.draws, 0);
        await call("update", [
          "ParticleEmitter",
          { enabled: true, restart: 1 },
        ]);
        await capture("restarted");
        assert.equal(await call("equal", ["sprites", "restarted"]), true);
        await call("mesh");
        assert.ok((await capture("custom-mesh")).blue > 2000);
        await call("customVertex");
        await capture("custom-vertex");
        assert.equal(
          await call("equal", ["custom-mesh", "custom-vertex"]),
          true,
        );
        await call("playback", [0]);
        assert.ok((await capture("cache-start")).blue > 100);
        await call("update", ["ParticlePlayback", { time: 1 }]);
        await capture("cache-end");
        assert.equal(await call("equal", ["cache-start", "cache-end"]), false);
        await call("update", ["ParticlePlayback", { time: 0 }]);
        await capture("cache-rewind");
        assert.equal(
          await call("equal", ["cache-start", "cache-rewind"]),
          true,
        );
        assert.equal(
          (await call<{ draws: number }>("removeProducer")).draws,
          0,
        );
        const alpha = await call<{ center: number[] }>("transparency");
        await writeDataUrl(
          resolve(env.evidence.directory, "alpha-order.png"),
          await call<string>("captureDataUrl", ["alpha-order"]),
        );
        assert.ok(
          [188, 4, 137].every((v, i) => Math.abs(v - alpha.center[i]!) <= 3),
          JSON.stringify(alpha),
        );
      } finally {
        await call("close");
      }
    },
  );
});
