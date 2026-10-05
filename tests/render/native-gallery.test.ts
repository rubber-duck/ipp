import assert from "node:assert/strict";
import test from "node:test";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import type { DatasetPage } from "@ipp/client";
import type { HostContract, HostState } from "../../tools/shared-host/host.js";
import type { RgbaImage } from "../../tools/shared-host/images.js";
import { runNativeEnvironment } from "../integration/environment.js";
import { NativeGalleryDriver } from "./native-gallery-driver.js";
import { exerciseNativeGalleryScene } from "./native-gallery-scene-assertions.js";
import {
  exerciseGalleryCharts,
  exerciseChartRow,
  type GalleryChartsState,
  type GalleryChartsDriver,
} from "../integration/scenarios/gallery-charts.js";

import {
  exerciseStreamingCharts,
  prepareStreamingChartLifecycle,
  type StreamingChartsState,
  type StreamingChartsDriver,
} from "../integration/scenarios/gallery-chart-streaming.js";

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
    readonly readDataset: (name: string) => Promise<DatasetPage>;
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
          readDataset: (name) => observer.datasets.read(name),
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
        "charts",
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

test("native unified charts retain fixed samples, focus and pick World-qualified source rows without leaked participants", {
  timeout: 180_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "charts",
    async ({ driver, directory, signal, worlds, record }) => {
      await driver.start(
        "charts",
        ["--width", "720", "--height", "480"],
        signal,
      );
      assert.equal(
        (await worlds()).length,
        11,
        "The root camera scene owns five Canvas charts and five spatial legend children",
      );
      const chartsDriver: GalleryChartsDriver = {
        inspect: async () =>
          (await driver.call("inspect", [], signal)).report
            .state as unknown as GalleryChartsState,
        action: (name, args) =>
          driver.call(
            "action",
            [name, ...(args === undefined ? [] : [JSON.stringify(args)])],
            signal,
          ),
        capture: (label) => driver.capture(join(directory, label), signal),
        record: async (label, value) => {
          await writeFile(
            join(directory, `${label}.json`),
            `${JSON.stringify(value, (_key, item) => (typeof item === "bigint" ? String(item) : item), 2)}\n`,
          );
          await record(label, { artifact: `${label}.json` });
        },
      };
      await exerciseGalleryCharts(chartsDriver);
      await exerciseChartRow(chartsDriver, 720 / 480);
      const previous = (await worlds())[0]!;
      await driver.call("reload", [], signal);
      assert.equal((await worlds()).length, 11);
      assert.notEqual((await worlds())[0]!.id, previous.id);
      await driver.capture(join(directory, "charts-reloaded"), signal);
      await driver.close();
      assert.equal(
        (await worlds()).length,
        0,
        "Chart disposal releases root and all Canvas child Worlds",
      );
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

function streamingChartsDriver(
  driver: NativeGalleryDriver,
  directory: string,
  signal: AbortSignal,
  record: (kind: string, value: unknown) => Promise<void>,
): StreamingChartsDriver {
  return {
    inspect: async () =>
      (await driver.call("inspect", [], signal)).report
        .state as unknown as StreamingChartsState,
    action: (name, args) =>
      driver.call(
        "action",
        [name, ...(args === undefined ? [] : [JSON.stringify(args)])],
        signal,
      ),
    capture: (label) => driver.capture(join(directory, label), signal),
    record: async (label, value) => {
      await writeFile(
        join(directory, `${label}.json`),
        `${JSON.stringify(value, (_key, item) => (typeof item === "bigint" ? String(item) : item), 2)}\n`,
      );
      await record(label, { artifact: `${label}.json` });
    },
  };
}

test("native gallery streams bounded data windows and expires picked rows", {
  timeout: 150_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "streaming-charts",
    async ({ driver, directory, signal, worlds, readDataset, record }) => {
      await driver.start(
        "charts",
        ["--width", "720", "--height", "480"],
        signal,
      );
      const charts = streamingChartsDriver(driver, directory, signal, record);
      const result = await exerciseStreamingCharts(charts, 720 / 480);
      for (const name of result.sourceNames)
        await assert.rejects(readDataset(name), /^Error: MissingSource$/);
      const finalSources = (await charts.inspect()).data.sources.map(
        (source) => source.name,
      );
      await driver.close();
      assert.equal((await worlds()).length, 0);
      for (const name of finalSources)
        await assert.rejects(readDataset(name), /^Error: MissingSource$/);
    },
  );
});

test("native gallery reloads streaming charts and releases Worlds and sources", {
  timeout: 150_000,
}, async (context) => {
  await runGallery(
    context.signal,
    "streaming-chart-lifecycle",
    async ({ driver, directory, signal, worlds, readDataset, record }) => {
      await driver.start(
        "charts",
        ["--width", "720", "--height", "480"],
        signal,
      );
      const charts = streamingChartsDriver(driver, directory, signal, record);
      await prepareStreamingChartLifecycle(charts);
      for (let pass = 0; pass < 2; pass++) {
        const current = await charts.inspect();
        const sourceNames = current.data.sources.map((source) => source.name);
        const previous = (await worlds()).map((world) => String(world.id));
        await driver.call("reload", [], signal);
        const replacements = await worlds();
        assert.equal(replacements.length, 11);
        assert.ok(
          replacements.every((world) => !previous.includes(String(world.id))),
        );
        for (const name of sourceNames)
          await assert.rejects(readDataset(name), /^Error: MissingSource$/);
        const state = await charts.inspect();
        assert.equal(state.data.mode, "streaming");
        assert.equal(state.data.window, current.data.window);
        assert.equal(state.data.feed.playing, false);
        assert.equal(state.data.feed.inFlight, false);
        assert.equal(state.data.feed.error, null);
        assert.ok(
          state.data.sources.every((source) => source.kind === "streaming"),
        );
        assert.ok(
          state.charts.every(
            (chart) =>
              chart.sourceKind === "streaming" &&
              chart.bindingComponent === "StreamingDataSourceBinding" &&
              chart.binding.availability.reason === "Ready",
          ),
        );
        assert.equal(state.selection, null);
        assert.equal(state.hover, null);
        await charts.record(`stream-reload-${pass}`, state);
      }
      const finalSources = (await charts.inspect()).data.sources.map(
        (source) => source.name,
      );
      await driver.close();
      assert.equal((await worlds()).length, 0);
      for (const name of finalSources)
        await assert.rejects(readDataset(name), /^Error: MissingSource$/);
    },
  );
});
