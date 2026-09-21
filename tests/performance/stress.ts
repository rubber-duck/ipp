/** Opt-in stress profile through the maintained real browser environment. */
import assert from "node:assert/strict";
import type { EntitySnapshot } from "@ipp/client";
import test from "node:test";
import { readFile, writeFile } from "node:fs/promises";
import { resolve, join, relative } from "node:path";
import { runBrowserEnvironment } from "../browser/environment.js";
import { galleryEnvironment } from "../render/gallery-driver.js";
import { invoke, writeDataUrl } from "../render/evidence.js";
import {
  sampleWorkerAllocations,
  sampleWorkerCpu,
} from "./worker-profiling.js";
import { verifyHardwareRenderer } from "#ipp-browser-options";

interface Probe {
  name: string;
  entity: EntitySnapshot;
}
interface FixtureProbe {
  frame: number;
  name: string;
  position_blender: number[];
  quaternion_wxyz: number[];
}
interface Profile {
  memoryBytes: number;
  shadowDrawCalls: number | null;
  frames: number[][];
  categories: { name: string; calls: number; bytes: number }[];
  allocations: number[];
  stages: number[];
  names: string[];
}
interface Profiler {
  start(profile?: boolean): void;
  count(): number;
  stop(): Profile;
  growMemory(): number;
}

test("Blender stress benchmark covers deformation, materials, constraints, geometry, lights and particles", {
  timeout: 1800000,
}, async (context) => {
  const directory = resolve(
    process.env.IPP_STRESS_DIR ?? "target/stress-benchmark/smoke",
  );
  const fixture = JSON.parse(
    await readFile(join(directory, "fixture.json"), "utf8"),
  );
  const build = (name: "render-expanded" | "headless") => {
    const root = resolve(
      process.env.IPP_BROWSER_BUILD_DIR ?? "target/browser-build",
      name,
    );
    return {
      name,
      generatedModule: join(root, "generated.js"),
      runtimeWasm: join(root, "runtime.wasm"),
      exportWasm: join(root, "export.wasm"),
      contractArtifact: join(root, "contract.bin"),
    };
  };
  const result = await runBrowserEnvironment(
    "Blender stress benchmark",
    {
      ...galleryEnvironment,
      build: build("render-expanded"),
      mismatchBuild: build("headless"),
      operationTimeoutMs: 900000,
      evidenceParent: resolve("target/integration-artifacts/stress"),
    },
    context.signal,
    async (environment) => {
      const page = environment.page;
      page.on("console", (message) => {
        if (message.type() === "info") console.log(message.text());
      });
      const moduleUrl = `${environment.urls.origin}/target/stress-benchmark/fixture.js`;
      const bundle = `${environment.urls.origin}/${relative(process.cwd(), directory)}/bundle/`;
      const call = <T>(name: string, ...args: unknown[]) =>
        invoke<T>(page, moduleUrl, name, args);
      await call("open", environment.urls, bundle);
      const worker = page.workers().at(-1)!;
      assert.ok(worker, "runtime worker missing");
      const hidden = (value: boolean) =>
        page.evaluate((hidden) => {
          Object.defineProperty(document, "hidden", {
            configurable: true,
            value: hidden,
          });
          document.dispatchEvent(new Event("visibilitychange"));
        }, value);
      const start = (profile: boolean) =>
        worker.evaluate(
          (profile) =>
            (
              globalThis as unknown as { ippProfile: Profiler }
            ).ippProfile.start(profile),
          profile,
        );
      const stop = () =>
        worker.evaluate(() =>
          (globalThis as unknown as { ippProfile: Profiler }).ippProfile.stop(),
        );
      await hidden(true);
      await start(false);
      await stop();
      const load = () =>
        call<{ entities: number; drivers: number }>(
          "load",
          Number(process.env.IPP_STRESS_GROUP ?? 64),
          fixture,
          process.env.IPP_STRESS_PERSISTENCE === "1",
        );
      const loading =
        process.env.IPP_STRESS_LOAD_CPU === "1"
          ? await sampleWorkerCpu(
              page.context().browser()!,
              environment.urls.workerScript,
              load,
            )
          : { window: await load() };
      const setup = loading.window;
      await environment.evidence.writeJson("loading.json", loading);
      assert.ok(setup.entities >= fixture.cubes + 20);
      console.log(`Stress loaded: ${JSON.stringify(setup)}`);
      const capture = async (label: string) => {
        const result = await call<{
          png: string;
          draws: number;
          triangles: number;
          foreground: number;
          backend: Record<string, unknown>;
        }>("capture", label);
        if (process.env.IPP_BROWSER_ANGLE)
          verifyHardwareRenderer(result.backend.unmaskedRenderer);
        await writeDataUrl(
          join(environment.evidence.directory, label + ".png"),
          result.png,
        );
        const { png, ...stats } = result;
        return stats;
      };
      const names = [
        "drop-000-000",
        `drop-${String(fixture.grid - 1).padStart(3, "0")}-${String(fixture.grid - 1).padStart(3, "0")}`,
        "parented-light-00",
        "benchmark-camera",
        "walker-00-rig",
        "particles-baked/ParticleSystem",
        ...new Set<string>(
          (fixture.probes as FixtureProbe[]).map((probe) => probe.name),
        ),
      ];
      const captureFirst = await capture("held-before");
      const first = await call("probes", names);
      const collect = async (allocation: boolean, count: number) => {
        await start(allocation);
        for (;;) {
          await new Promise((done) => setTimeout(done, 250));
          context.signal.throwIfAborted();
          if (
            (await worker.evaluate(() =>
              (
                globalThis as unknown as { ippProfile: Profiler }
              ).ippProfile.count(),
            )) >= count
          )
            break;
        }
        const data = await stop();
        assert.equal(
          data.categories.reduce((n, c) => n + c.calls, 0),
          data.allocations[0],
        );
        return data;
      };
      const sampleCpu = async (label: string) => {
        if (process.env.IPP_STRESS_CPU !== "1") return;
        const result = await sampleWorkerCpu(
          page.context().browser()!,
          environment.urls.workerScript,
          () => collect(false, Number(process.env.IPP_STRESS_FRAMES ?? 60)),
        );
        await environment.evidence.writeJson(`${label}-cpu.json`, result);
      };
      await collect(false, fixture.cubes > 1000 ? 1 : 3);
      const timing = await collect(
        false,
        Number(process.env.IPP_STRESS_FRAMES ?? 20),
      );
      console.log(
        `Stress timing: ${JSON.stringify(timing.frames.map((f) => f[0]))}`,
      );
      const allocation = await collect(true, fixture.cubes > 1000 ? 2 : 5);
      const state = await call("probes", names);
      assert.deepEqual(state, first);
      await worker.evaluate(() =>
        (
          globalThis as unknown as { ippProfile: Profiler }
        ).ippProfile.growMemory(),
      );
      const frame = await capture("held");
      const heldDifference = await call<{ changedPixels: number }>(
        "difference",
        "held-before",
        "held",
      );
      assert.equal(heldDifference.changedPixels, 0);
      const rows = [{ timing, allocation, frame }];
      await writeFile(
        join(environment.evidence.directory, "measurements.json"),
        JSON.stringify(rows[0], null, 2),
      );
      if (fixture.cubes <= 64) {
        assert.ok(
          allocation.allocations[0]! / allocation.frames.length <= 100,
          "warmed smoke allocation count regressed",
        );
        assert.ok(
          allocation.allocations[1]! / allocation.frames.length <= 128 * 1024,
          "warmed smoke allocation bytes regressed",
        );
        assert.equal(
          allocation.categories
            .filter((c) => c.name.startsWith("ipp.") && c.name !== "ipp.render")
            .reduce((n, c) => n + c.calls, 0),
          0,
          "warmed core system allocations regressed",
        );
      }
      assert.equal(allocation.allocations[0], 0, "warmed frame allocated");
      console.log(
        `Stress profile: ${timing.frames.length} timed frames, exact held state and pixels`,
      );
      let culling;
      if (process.env.IPP_STRESS_COMPARE_CULLING === "1") {
        const added = await call<number>("enableCulling");
        assert.ok(added >= fixture.cubes);
        await collect(false, 2);
        const timing = await collect(
          false,
          Number(process.env.IPP_STRESS_FRAMES ?? 20),
        );
        const allocation = await collect(true, fixture.cubes > 1000 ? 2 : 5);
        const frame = await capture("culled");
        const difference = await call<{ changedPixels: number }>(
          "difference",
          "held-before",
          "culled",
        );
        assert.equal(
          difference.changedPixels,
          0,
          "conservative culling changed visible pixels",
        );
        assert.equal(
          allocation.categories
            .filter((c) => c.name.startsWith("ipp.") && c.name !== "ipp.render")
            .reduce((n, c) => n + c.calls, 0),
          0,
          "bounds evaluation reintroduced core allocations",
        );
        assert.equal(
          allocation.allocations[0],
          0,
          "warmed culled frame allocated",
        );
        const comparison = { timing, allocation, frame, difference };
        culling = { added, ...comparison, rows: [comparison] };
        await writeFile(
          join(environment.evidence.directory, "culling.json"),
          JSON.stringify(culling, null, 2),
        );
        console.log(
          `Stress culling: ${added} bounds, ${timing.shadowDrawCalls} shadow draws, exact pixels`,
        );
      }
      if (process.env.IPP_STRESS_JS === "1") {
        const sampled = await sampleWorkerAllocations(
          environment.page.context().browser()!,
          environment.urls.workerScript,
          () => collect(false, 3),
        );
        await writeFile(
          join(environment.evidence.directory, "javascript-allocations.json"),
          JSON.stringify(sampled),
        );
      }
      const checkpoints = [];
      for (const time of [0, 2.5, 5, 10]) {
        await call("seek", time);
        const probes = await call<Probe[]>("probes", names);
        for (const expected of fixture.probes as FixtureProbe[]) {
          if (Math.abs((expected.frame - 1) / fixture.fps - time) > 1e-6)
            continue;
          const actual = probes
            .find((p) => p.name === expected.name)!
            .entity.effective.find((c) => "qx" in c.fields)!.fields;
          const [x, y, z] = expected.position_blender;
          for (const [key, value] of Object.entries({ x, y: z, z: -y! }))
            assert.ok(
              Math.abs(Number(actual[key]) - value!) < 0.0001,
              `${expected.name} ${key} at ${time}`,
            );
          const [w, qx, qy, qz] = expected.quaternion_wxyz;
          const q = [qx, qz, -qy!, w];
          const observed = ["qx", "qy", "qz", "qw"].map((key) =>
            Number(actual[key]),
          );
          const error = Math.min(
            ...[1, -1].map((sign) =>
              Math.max(
                ...q.map((value, i) => Math.abs(sign * value! - observed[i]!)),
              ),
            ),
          );
          assert.ok(
            error < 0.00001,
            `physics rotation ${expected.name} at ${time}: ${error}`,
          );
        }
        const light = probes.find(
          (p) => p.name === "parented-light-00",
        )!.entity;
        assert.equal(
          light.effective.find((c) => "parent" in c.fields)!.fields.parent,
          probes.find((p) => p.name === "drop-000-000")!.entity.id,
        );
        const baked = probes.find(
          (p) => p.name === "particles-baked/ParticleSystem",
        )!.entity;
        assert.ok(
          Math.abs(
            Number(
              baked.effective.find((c) => "time" in c.fields)!.fields.time,
            ) - time,
          ) < 1e-5,
        );
        checkpoints.push({
          time,
          probes,
          frame: await capture(`time-${time}`),
        });
        console.log(`Stress timeline verified at ${time} seconds`);
      }
      const difference = await call<{ changedPixels: number }>(
        "difference",
        "time-0",
        "time-10",
      );
      assert.ok(difference.changedPixels > 100, "physics/camera did not move");
      await call("play");
      await sampleCpu("held");
      await hidden(false);
      // Exercise the complete camera/light path before counting allocations:
      // reaching particle steady state alone does not visit every caster set.
      await call("warmNative", fixture.seconds + 0.25);
      await call("play");
      await call("warmNative", 3.25);
      const moving = await collect(
        false,
        Number(process.env.IPP_STRESS_FRAMES ?? 20),
      );
      const movingAllocations = await collect(true, 5);
      await writeFile(
        join(environment.evidence.directory, "moving-allocations.json"),
        JSON.stringify(movingAllocations, null, 2),
      );
      assert.equal(
        movingAllocations.allocations[0],
        0,
        "warmed moving frame allocated",
      );
      assert.equal(
        movingAllocations.categories
          .filter((c) => c.name.startsWith("ipp.") && c.name !== "ipp.render")
          .reduce((count, category) => count + category.calls, 0),
        0,
        "warmed moving evaluation reintroduced core allocations",
      );
      console.log(
        `Stress moving timing: ${JSON.stringify(moving.frames.map((f) => f[0]))}`,
      );
      await sampleCpu("moving");
      await hidden(true);
      await call("pause");
      const movingFrame = await capture("moving");
      const movingProbes = await call("probes", names);
      // Camera motion and culling change the rest of the draw count. Compare
      // native instances in one fixed closeup while every clock is paused.
      const particleCamera = await call<bigint>("particleView");
      const nativePresent = await capture("native-present");
      await call("restartNative");
      const nativeCleared = await capture("native-cleared");
      const nativeDifference = await call<{ changedPixels: number }>(
        "difference",
        "native-present",
        "native-cleared",
      );
      assert.ok(
        nativePresent.triangles > nativeCleared.triangles + 24 &&
          nativeDifference.changedPixels > 100,
        "native particles did not add visible instances",
      );
      await call("closeDetailView", particleCamera);
      console.log(
        `Stress native instances verified: ${nativePresent.triangles - nativeCleared.triangles} additional triangles`,
      );
      const rig = await call<{ controller: bigint; camera: bigint }>(
        "rigCloseup",
      );
      await call("rigSeek", rig.controller, 0.125);
      await capture("rig-first");
      await call("rigSeek", rig.controller, 0.625);
      await capture("rig-second");
      const rigDifference = await call<{ changedPixels: number }>(
        "difference",
        "rig-first",
        "rig-second",
      );
      assert.ok(
        rigDifference.changedPixels > 100,
        "imported Rigify pose did not visibly deform the human",
      );
      await call("closeRigView", rig.controller, rig.camera);
      const features = await call("verifyFeatures");
      const poseDifferences = [];
      // View the outside row: the full fixture's other panels otherwise obscure
      // the selected surface. Variant two has the same authored weight curve.
      const panel = Math.max(0, fixture.mesh_pose_panels - 8) + 2;
      for (const name of [
        `pose-panel-${String(panel).padStart(2, "0")}`,
        "walker-00-human",
      ]) {
        const pose = await call<{ controller: bigint; camera: bigint }>(
          "poseCloseup",
          name,
        );
        await call("rigSeek", pose.controller, 0);
        await capture(`pose-${name}-first`);
        await call("rigSeek", pose.controller, 1);
        await capture(`pose-${name}-second`);
        const difference = await call<{ changedPixels: number }>(
          "difference",
          `pose-${name}-first`,
          `pose-${name}-second`,
        );
        assert.ok(
          difference.changedPixels > 100,
          `${name}: mesh-pose animation did not visibly deform the surface`,
        );
        poseDifferences.push({ name, ...difference });
        await call("closeRigView", pose.controller, pose.camera);
      }
      const data = {
        features,
        poseDifferences,
        fixture,
        setup,
        captureFirst,
        rows,
        culling,
        checkpoints,
        moving,
        movingAllocations,
        movingFrame,
        movingProbes,
        nativePresent,
        nativeCleared,
        nativeDifference,
        rigDifference,
      };
      await writeFile(
        join(environment.evidence.directory, "profile.json"),
        JSON.stringify(
          data,
          (_k, v) => (typeof v === "bigint" ? v.toString() : v),
          2,
        ),
      );
      await call("close");
      return setup;
    },
  );
  console.log(`Stress profile: ${result.evidenceDirectory}`);
});
