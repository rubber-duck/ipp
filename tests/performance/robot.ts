import type { ProfileCapture } from "../../packages/ipp-client/src/profiling.js";
/** Actual generated-client/worker/WASM/WebGL platformer, with host-only profiling hooks. */
import assert from "node:assert/strict";
import test from "node:test";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { sampleWorkerAllocations } from "./worker-profiling.js";
import type { AnimationWorldClient } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery, galleryEnvironment } from "../render/gallery-driver.js";

interface ProfileResult extends ProfileCapture {
  frames: number[][];
}
interface ProfileWorker {
  ippProfile: {
    start(profile?: boolean): void;
    count(): number;
    growMemory(): number;
    stop(): ProfileResult;
  };
}

test("platformer profiling preserves state and completed frames", {
  timeout: 240_000,
}, async (context) => {
  const result = await runBrowserEnvironment(
    "platformer performance",
    {
      ...galleryEnvironment,
      build: {
        name: "render-instrumentation",
        generatedModule: resolve(
          "target/browser-build/render-instrumentation/generated.js",
        ),
        runtimeWasm: resolve(
          "target/browser-build/render-instrumentation/runtime.wasm",
        ),
        contractArtifact: resolve(
          "target/browser-build/render-instrumentation/contract.bin",
        ),
      },
    },
    context.signal,
    async (environment) => {
      // The published gallery keeps its ordinary runtime URL. Redirect that
      // distribution as a whole to the verified instrumentation product.
      await environment.page
        .context()
        .route("**/target/browser-build/render/**", async (route) => {
          const suffix = new URL(route.request().url()).pathname.split(
            "/target/browser-build/render/",
          )[1]!;
          const response = await route.fetch({
            url: `${environment.urls.origin}/target/browser-build/render-instrumentation/${suffix}`,
          });
          await route.fulfill({ response });
        });
      await environment.page.setViewportSize({ width: 1200, height: 800 });
      const g = await openGallery(environment);
      try {
        await g.navigate("platformer");
      } catch (error) {
        await writeFile(
          join(environment.evidence.directory, "startup-state.json"),
          JSON.stringify(
            await g.page.evaluate(() => ({ text: document.body.innerText })),
            null,
            2,
          ),
        );
        throw error;
      }
      await g.waitFor((s) => s.resources.every((r) => r.status === "loaded"));
      await g.page.locator("#platformer-pause").click();
      await g.waitFor((s) =>
        s.controllers!.every((c) => c.state !== "playing"),
      );
      // Paused controllers still run their normal restore/sample passes. Keep the
      // exact same pose while timing without inspection or frame capture.
      const initial = await g.inspect();
      assert.ok(initial.entities.length > 0);
      assert.equal(initial.controllers!.length, 4);
      const worker = g.page.workers().at(-1)!;
      assert.ok(
        await worker.evaluate(() => "ippProfile" in globalThis),
        "Build with the instrumentation feature",
      );
      const playing = process.env.IPP_PROFILE_PLAYING === "1";
      const rows: unknown[] = [];
      const active = initial
        .controllers!.filter((c) => c.state === "paused")
        .map((c) => c.id);
      const collect = async (profile: boolean, frames: number) => {
        await worker.evaluate(
          (profile) =>
            (globalThis as unknown as ProfileWorker).ippProfile.start(profile),
          profile,
        );
        if (profile)
          await worker.evaluate(() =>
            (globalThis as unknown as ProfileWorker).ippProfile.growMemory(),
          );
        // Poll diagnostics only; no per-frame inspection/transport or captures in samples.
        for (;;) {
          await new Promise((resolve) => setTimeout(resolve, 250));
          if (
            (await worker.evaluate(() =>
              (globalThis as unknown as ProfileWorker).ippProfile.count(),
            )) >= frames
          )
            break;
          context.signal.throwIfAborted();
        }
        return worker.evaluate(() =>
          (globalThis as unknown as ProfileWorker).ippProfile.stop(),
        );
      };
      const before = await g.capture("baseline");
      assert.ok(before.frame.drawCalls > 0);
      assert.ok(before.summary.foregroundPixels > 1000);
      if (playing) {
        await g.page.evaluate(async (ids) => {
          const client = window.ippWorldCanvas!.client as AnimationWorldClient;
          for (const id of ids) {
            client.playback(id, { action: "seek", time: 0 });
            client.playback(id, { action: "play" });
          }
          await client.inspect();
        }, active);
      }
      await collect(false, 15);
      const timing = await collect(false, playing ? 50 : 100);
      const profile = await collect(true, 20);
      assert.equal(
        profile.categories.reduce(
          (sum, c) => sum + BigInt(c.allocationCalls),
          0n,
        ),
        BigInt(profile.allocations.calls),
      );
      assert.equal(
        profile.categories.reduce(
          (sum, c) => sum + BigInt(c.requestedBytes),
          0n,
        ),
        BigInt(profile.allocations.requestedBytes),
      );
      const jsAllocations =
        process.env.IPP_PROFILE_JS === "1"
          ? await sampleWorkerAllocations(
              g.page.context().browser()!,
              worker.url(),
              () => collect(false, 30),
            )
          : undefined;
      const state = await g.inspect();
      if (playing) {
        assert.notDeepEqual(state.entities, initial.entities);
        assert.ok(
          state.controllers!.some((c) => c.state === "playing" && c.time > 0),
        );
        await g.page.evaluate(async (ids) => {
          const client = window.ippWorldCanvas!.client as AnimationWorldClient;
          for (const id of ids) client.playback(id, { action: "pause" });
          await client.inspect();
        }, active);
      } else {
        assert.deepEqual(state.entities, initial.entities);
        assert.deepEqual(state.controllers, initial.controllers);
      }
      await worker.evaluate(() =>
        (globalThis as unknown as ProfileWorker).ippProfile.growMemory(),
      );
      await g.capture("profiled");
      const difference = await g.difference("baseline", "profiled");
      if (playing) assert.ok(difference.changedPixels > 100);
      else assert.equal(difference.changedPixels, 0);
      rows.push({
        timing,
        profile,
        jsAllocations,
        difference,
        controllers: state.controllers!.map((c) => ({
          id: c.id,
          time: c.time,
          state: c.state,
        })),
      });
      console.log(
        `Platformer profile: ${timing.frames.length} frames, ${playing ? "moving scene verified" : "matching state and pixels"}`,
      );
      const complexity = {
        entities: initial.entities.length,
        controllers: initial.controllers!.length,
        drivers: initial.controllers!.reduce(
          (n, c) => n + c.description.drivers.length,
          0,
        ),
        targets: initial.controllers!.flatMap((c) =>
          c.description.drivers.map((d) => d.property),
        ),
        frame: before.frame,
      };
      await writeFile(
        join(environment.evidence.directory, "profile.json"),
        JSON.stringify(
          { playing, complexity, rows },
          (_, v) => (typeof v === "bigint" ? v.toString() : v),
          2,
        ),
      );
      return { complexity, rows };
    },
  );
  console.log(`Platformer profile: ${result.evidenceDirectory}`);
});
