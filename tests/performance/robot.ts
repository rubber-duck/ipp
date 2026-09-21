/** Actual generated-client/worker/WASM/WebGL platformer, with host-only profiling hooks. */
import assert from "node:assert/strict";
import test from "node:test";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { sampleWorkerAllocations } from "./worker-profiling.js";
import type { AnimationWorldClient } from "@ipp/client";
import { runBrowserEnvironment } from "../browser/environment.js";
import { openGallery, galleryEnvironment } from "../render/gallery-driver.js";

interface ProfileResult {
  names: string[];
  frames: number[][];
  stages: number[];
  allocations: number[];
  categories: { name: string; calls: number; bytes: number }[];
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
    galleryEnvironment,
    context.signal,
    async (environment) => {
      await environment.page.setViewportSize({ width: 800, height: 600 });
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
        "Build with the profiling feature",
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
        profile.categories.reduce((sum, c) => sum + c.calls, 0),
        profile.allocations[0],
      );
      assert.equal(
        profile.categories.reduce((sum, c) => sum + c.bytes, 0),
        profile.allocations[1],
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
