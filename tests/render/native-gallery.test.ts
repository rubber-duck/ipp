import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import type { Inspection } from "@ipp/client";
import type { HostContract, HostState } from "../../tools/shared-host/host.js";
import type { RgbaImage } from "../../tools/shared-host/images.js";
import { runNativeEnvironment } from "../integration/environment.js";
import { NativeGalleryDriver } from "./native-gallery-driver.js";
import { exerciseNativeGalleryScene } from "./native-gallery-scene-assertions.js";

/** Content must cover a meaningful frame region, independent of the source's own geometry math. */
function visiblePixels(frame: RgbaImage): number {
  const background = [...frame.pixels.slice(0, 3)];
  let visible = 0;
  for (let y = 0; y < frame.height; y++) {
    for (let x = 0; x < frame.width; x++) {
      const index = (y * frame.width + x) * 4;
      if (
        background.some(
          (value, axis) => Math.abs(frame.pixels[index + axis]! - value) > 20,
        )
      )
        visible++;
    }
  }
  return visible;
}

/** Check chart bodies, excluding the sheet heading and panel titles. */
function chartBodyPixels(frame: RgbaImage): readonly number[] {
  return [0.24, 0.66].flatMap((top) =>
    [0.12, 0.57].map((left) => {
      let count = 0;
      for (
        let y = Math.floor(top * frame.height);
        y < (top + 0.21) * frame.height;
        y++
      ) {
        for (
          let x = Math.floor(left * frame.width);
          x < (left + 0.31) * frame.width;
          x++
        ) {
          const index = (y * frame.width + x) * 4;
          const channels = [...frame.pixels.slice(index, index + 3)];
          if (
            Math.max(...channels) > 110 &&
            Math.max(...channels) - Math.min(...channels) > 50
          )
            count++;
        }
      }
      return count;
    }),
  );
}

function changedPixels(before: RgbaImage, after: RgbaImage): number {
  assert.equal(after.width, before.width);
  assert.equal(after.height, before.height);
  let count = 0;
  for (let index = 0; index < before.pixels.length; index += 4) {
    if (
      [0, 1, 2].some(
        (axis) =>
          Math.abs(before.pixels[index + axis]! - after.pixels[index + axis]!) >
          20,
      )
    )
      count++;
  }
  return count;
}

async function exerciseChart(
  driver: NativeGalleryDriver,
  scene: string,
  directory: string,
  signal: AbortSignal,
  initial: RgbaImage,
) {
  const occupancy = chartBodyPixels(initial);
  assert.ok(
    occupancy.every((count) => count > 40),
    `${scene} renders coloured data in all four chart bodies: ${occupancy}`,
  );
  await driver.call("action", ["changeSamples"], signal);
  const changed = await driver.capture(
    join(directory, `${scene}-changed`),
    signal,
  );
  assert.ok(
    changedPixels(initial, changed) > 500,
    `${scene} sample action changes actual chart pixels`,
  );
  if (scene === "charts2d")
    await driver.call("action", ["setParameter", "0.4"], signal);
  else await driver.call("action", ["rotate", "true"], signal);
  const altered = await driver.capture(
    join(directory, `${scene}-altered`),
    signal,
  );
  assert.ok(
    changedPixels(changed, altered) > 200,
    `${scene} parameter or rotation changes rendered data`,
  );
  const state = await driver.call("inspect", [], signal);
  assert.deepEqual(
    state.report.options,
    scene === "charts2d"
      ? { changed: true, parameter: 0.4 }
      : { changed: true, rotated: true },
  );
  if (scene === "charts2d") {
    const verifyParameter = async (changed: boolean, parameter: number) => {
      await driver.capture(
        join(directory, `${scene}-samples-${changed}-parameter-${parameter}`),
        signal,
      );
      const inspection = await driver.call("inspect", [], signal);
      assert.deepEqual(inspection.report.options, { changed, parameter });
      const world = (inspection.report.state as { world: Inspection }).world;
      assert.equal(world.controllers?.length, 1);
      const controller = world.controllers![0]!;
      assert.equal(controller.state, "paused");
      assert.ok(
        Math.abs(controller.time - parameter) < 0.0001,
        `acknowledged chart controller time ${controller.time} matches reported parameter ${parameter}`,
      );
    };
    await driver.call("action", ["changeSamples"], signal);
    await verifyParameter(true, 1);
    await driver.call("options", ['{"changed":false,"parameter":1}'], signal);
    await verifyParameter(false, 1);
    await driver.call("options", ['{"changed":true,"parameter":0.4}'], signal);
    await verifyParameter(true, 0.4);
  }
  await driver.call("reload", [], signal);
  assert.deepEqual(
    (await driver.call("inspect", [], signal)).report.options,
    state.report.options,
    `${scene} preserves chart controls through reload`,
  );
  const reloaded = await driver.capture(
    join(directory, `${scene}-reloaded`),
    signal,
  );
  assert.ok(
    chartBodyPixels(reloaded).every((count) => count > 40),
    `${scene} retains all chart bodies after reload`,
  );
}

const eglDirectory = process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64";
const workspace = process.cwd();
const product = resolve("target/gles-host");

async function runGallery(
  signal: AbortSignal,
  name: string,
  scenario: (context: {
    readonly driver: NativeGalleryDriver;
    readonly directory: string;
    readonly signal: AbortSignal;
    readonly worlds: () => Promise<
      readonly { readonly id: bigint; readonly symbolicId: string }[]
    >;
    readonly record: (kind: string, value: unknown) => Promise<void>;
  }) => Promise<void>,
  mapped = true,
) {
  const ioRead = mapped
    ? [
        { prefix: "ipp-gallery://assets/", directory: resolve("target") },
        {
          prefix: "https://platformer.ipp.invalid/",
          directory: resolve("target/gallery-platformer-native-assets"),
        },
      ]
    : [];
  return runNativeEnvironment(
    name,
    {
      executable: join(product, "gles_host"),
      schemaArtifact: join(product, "contract.bin"),
      workingDirectory: workspace,
      readinessTimeoutMs: 30_000,
      operationTimeoutMs: 120_000,
      closeTimeoutMs: 5_000,
      extraArguments: [
        "--egl-dir",
        eglDirectory,
        ...ioRead.flatMap(({ prefix, directory }) => [
          "--io-read",
          prefix,
          directory,
        ]),
      ],
    },
    signal,
    async (environment) => {
      const directory = environment.evidence.directory;
      const state = join(directory, "shared-host");
      await mkdir(state);
      const contract = (await import(
        pathToFileURL(join(product, "generated.js")).href
      )) as HostContract;
      const host: HostState = {
        pid: process.pid,
        url: environment.url,
        presentationUrl: environment.presentationUrl!,
        worktree: workspace,
        commit: "integration-harness",
        contract: contract.SCHEMA_HASH.toString(16),
        client: product,
        font: resolve("target/font-assets/shure-tech-mono.ippf"),
        eglDirectory,
        log: join(directory, "server-stderr.log"),
        startedAt: new Date().toISOString(),
        ioRead,
      };
      await writeFile(join(state, "host.json"), JSON.stringify(host));
      const observer = await environment.track(
        contract.IppHostClient.connectWebSocket(environment.url),
      );
      const driver = new NativeGalleryDriver(
        workspace,
        state,
        `native-${name}`,
      );
      const failures: unknown[] = [];
      try {
        await scenario({
          driver,
          directory,
          signal: environment.signal,
          worlds: () => observer.listWorlds(),
          record: (kind, value) => environment.evidence.record(kind, value),
        });
      } catch (error) {
        failures.push(error);
      }
      try {
        // A failed start may still have opened the detached process; inspect its state before closing.
        const session = join(state, "sessions", `${driver.name}.json`);
        if (
          await readFile(session).then(
            () => true,
            () => false,
          )
        ) {
          await driver.close();
        }
      } catch (error) {
        failures.push(error);
      }
      try {
        assert.equal(
          (await observer.listWorlds()).length,
          0,
          "session disposal releases every scene-owned World",
        );
      } catch (error) {
        failures.push(error);
      }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1)
        throw new AggregateError(failures, failures.map(String).join("; "));
    },
  );
}

test("native gallery preserves options and current scene across failed reload, then cleans repeated remounts", {
  timeout: 180_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "reload",
    async ({ driver, directory, signal, worlds, record }) => {
      const entry = join(directory, "selected-scene.ts");
      const valid = `export {geometryScene as default} from ${JSON.stringify(resolve("examples/world-gallery/worlds/geometry/scene.tsx"))};\n`;
      await writeFile(entry, valid);
      await driver.start(
        "shapes",
        ["--module", entry, "--width", "480", "--height", "320"],
        signal,
      );
      const initial = await worlds();
      assert.equal(initial.length, 1);
      const before = await driver.capture(join(directory, "before"), signal);
      assert.equal(before.width, 480);
      assert.equal(before.height, 320);
      assert.ok(
        visiblePixels(before) > 1500,
        "built-in gallery meshes render through GLES",
      );
      await driver.call("options", ['{"shape":"cube"}'], signal);
      await driver.call("action", ["resetCamera"], signal);
      await driver.call("reload", [], signal);
      let mounted = await worlds();
      assert.equal(mounted.length, 1);
      assert.notEqual(
        mounted[0]!.id,
        initial[0]!.id,
        "reload recreates the World rather than retaining runtime handles",
      );
      const inspection = await driver.call("inspect", [], signal);
      assert.equal(
        (inspection.report.options as Record<string, unknown>).shape,
        "cube",
      );
      await writeFile(entry, "export const = ;\n");
      await assert.rejects(
        driver.call("reload", [], signal),
        /Build failed|Expected|Unexpected/,
      );
      assert.equal(
        (await worlds())[0]!.id,
        mounted[0]!.id,
        "compilation failure leaves current World untouched",
      );
      const afterFailure = await driver.call("inspect", [], signal);
      assert.equal(
        afterFailure.report.generation,
        inspection.report.generation,
      );
      assert.equal(
        (afterFailure.report.options as Record<string, unknown>).shape,
        "cube",
      );
      const frame = await driver.capture(
        join(directory, "compile-failure-preserved"),
        signal,
      );
      assert.ok(visiblePixels(frame) > 1500);
      await writeFile(entry, valid);
      for (let round = 0; round < 3; round++) {
        await driver.call("reload", [], signal);
        mounted = await worlds();
        assert.equal(
          mounted.length,
          1,
          "each reload releases the previous primary World",
        );
      }
      await record("gallery_reload_evidence", {
        initial,
        mounted,
        inspection: inspection.report,
        preserved: afterFailure.report,
        visiblePixels: visiblePixels(frame),
      });
    },
  );
});

test("native gallery captures all shared scenes, including moving particles and the saved native Platformer", {
  timeout: 300_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "scenes",
    async ({ driver, directory, signal, worlds, record }) => {
      for (const scene of [
        "shapes",
        "lighting",
        "particles",
        "platformer",
        "gui",
        "charts2d",
        "charts3d",
      ]) {
        await driver.start(
          scene,
          ["--width", "720", "--height", "480"],
          signal,
        );
        try {
          const frame = await driver.capture(join(directory, scene), signal);
          assert.ok(
            visiblePixels(frame) > 100,
            `${scene} produces visible completed pixels`,
          );
          const state = await driver.call("inspect", [], signal);
          assert.equal(state.report.scene, scene);
          if (scene === "charts2d" || scene === "charts3d")
            await exerciseChart(driver, scene, directory, signal, frame);
          if (scene === "particles") {
            const next = await driver.capture(
              join(directory, "particles-next"),
              signal,
            );
            assert.ok(
              visiblePixels(next) > 100,
              "running particles need no stationary-pixel barrier",
            );
          }
          await exerciseNativeGalleryScene(driver, scene, directory, signal);
          await record("gallery_native_scene", {
            scene,
            worlds: await worlds(),
            visiblePixels: visiblePixels(frame),
            state: state.report,
          });
        } finally {
          await driver.close();
        }
        assert.equal(
          (await worlds()).length,
          0,
          `${scene} releases its primary and composed Worlds`,
        );
      }
    },
  );
});

test("native gallery explains missing saved asset mappings before creating a World", {
  timeout: 30_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "unmapped",
    async ({ driver, signal, worlds, record }) => {
      await assert.rejects(
        driver.start("platformer", [], signal),
        /saved asset namespace.*host start --gallery/s,
      );
      assert.equal((await worlds()).length, 0);
      await record("gallery_missing_asset_namespace", {
        scene: "platformer",
        worlds: await worlds(),
      });
    },
    false,
  );
});
