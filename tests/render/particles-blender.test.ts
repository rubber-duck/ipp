import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import { runBrowserEnvironment } from "../browser/environment.js";
import { startBlender } from "./blender-environment.js";
import { invoke, recordCapture } from "./evidence.js";
import type * as Fixture from "./blender-fixture.js";

test("Blender native and baked particles travel through the addon, HTTPS/WSS, generated adapter and completed WebGL frames", {
  timeout: 180000,
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
    "blender particles",
    {
      workspace,
      build,
      mismatchBuild: build,
      operationTimeoutMs: 45000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/particles",
      ),
    },
    context.signal,
    async (env) => {
      const cdp = await env.page.context().newCDPSession(env.page);
      await cdp.send("Browser.setPermission", {
        permission: { name: "loopback-network" },
        setting: "granted",
        origin: env.urls.origin,
      });
      const blender = await startBlender(env, {
        fixture: "tests/blender/particles_fixture.py",
      });
      const fragment = new URLSearchParams({
        endpoint: blender.ready.origin,
        token: blender.ready.token,
      });
      await env.page.goto(
        `${env.urls.origin}/target/blender-viewer/index.html#${fragment}`,
      );
      const module = `${env.urls.origin}/target/blender-test/blender-fixture.js`;
      const call = <T>(name: string, args: unknown[] = []) =>
        invoke<T>(env.page, module, name, args);
      const captured = new Set<string>();
      const capture = async (label: string, revision = 0) => {
        const result = await call<Awaited<ReturnType<typeof Fixture.capture>>>(
          "capture",
          [label, revision],
        );
        await recordCapture(
          env.page,
          module,
          env.evidence.directory,
          captured,
          label,
          { canvasSelector: "canvas" },
        );
        await env.evidence.writeJson(`${label}.json`, result);
        return result;
      };
      const initial = await capture("native");
      assert.equal(
        initial.diagnostics.filter((d) => d.code === "particle-unsupported")
          .length,
        0,
      );
      assert.ok(initial.inspection.resources.some((r) => r.kind === 16));
      const changed = await blender.command({ action: "bake" });
      fragment.set("stream", "100");
      fragment.set("refresh", "1");
      await env.page.goto(
        `${env.urls.origin}/target/blender-viewer/index.html#${fragment}`,
      );
      // A fresh adapter streams baked cache references before scene sampling.
      await env.page.reload();
      const baked = await capture("baked", changed);
      assert.equal(
        baked.diagnostics.filter((d) => d.code === "particle-unsupported")
          .length,
        0,
      );
      assert.ok(baked.inspection.resources.some((r) => r.kind === 15));
      const effect = baked.entities.find(([name]) =>
        name.includes(":particles:"),
      )![0];
      await call("seek", [effect, 0.8]);
      const sampled = await capture("sampled");
      assert.ok(sampled.drawCalls >= 2);
      assert.ok(sampled.summary.foregroundPixels > 1000);
      await call("seek", [effect, 1.5]);
      await capture("later");
      const difference = await call<{ changedPixels: number }>("compare", [
        "sampled",
        "later",
      ]);
      assert.ok(difference.changedPixels > 100);
      await call("seek", [effect, 0.8]);
      await capture("rewind");
      assert.equal(
        (
          await call<{ changedPixels: number }>("compare", [
            "sampled",
            "rewind",
          ])
        ).changedPixels,
        0,
      );
    },
  );
});
