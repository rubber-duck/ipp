import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserScenario,
  runtimeConfigurationFor,
  type BrowserBuildConfiguration,
} from "./environment.js";

const workspace = process.cwd();
function build(
  name: "headless" | "headless-builtins",
): BrowserBuildConfiguration {
  const base = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(base, "generated.js"),
    runtimeWasm: resolve(base, "runtime.wasm"),
    exportWasm: resolve(base, "export.wasm"),
    contractArtifact: resolve(base, "contract.bin"),
  };
}

const largeMeshPath = resolve(
  workspace,
  "target/browser-fixtures/resource-progress.mesh",
);

async function writeLargeMesh(): Promise<number> {
  const vertexBytes = 3 * 6 * Float32Array.BYTES_PER_ELEMENT;
  const indexCount =
    Math.floor(
      (4 * 1024 * 1024 - 16 - vertexBytes) /
        (3 * Uint16Array.BYTES_PER_ELEMENT),
    ) * 3;
  const bytes = Buffer.alloc(
    16 + vertexBytes + indexCount * Uint16Array.BYTES_PER_ELEMENT,
  );
  bytes.write("IPPM", 0, "ascii");
  bytes.writeUInt32LE(1, 4);
  bytes.writeUInt32LE(3, 8);
  bytes.writeUInt32LE(indexCount, 12);
  let offset = 16;
  for (const vertex of [
    [-1, -1, 0, 1, 0, 0],
    [1, -1, 0, 0, 1, 0],
    [0, 1, 0, 0, 0, 1],
  ]) {
    for (const value of vertex) {
      bytes.writeFloatLE(value, offset);
      offset += Float32Array.BYTES_PER_ELEMENT;
    }
  }
  for (let index = 0; index < indexCount; index++) {
    bytes.writeUInt16LE(index % 3, offset);
    offset += Uint16Array.BYTES_PER_ELEMENT;
  }
  await mkdir(dirname(largeMeshPath), { recursive: true });
  await writeFile(largeMeshPath, bytes);
  return bytes.byteLength;
}

test("worker progresses multi-megabyte HTTP assets while animation frames are withheld", {
  timeout: 30_000,
}, async (testContext) => {
  const expectedBytes = await writeLargeMesh();
  await runBrowserScenario(
    "worker frame-independent resource progress",
    {
      workspace,
      build: build("headless"),
      mismatchBuild: build("headless-builtins"),
      operationTimeoutMs: 20_000,
    },
    testContext.signal,
    async (context) => {
      const report = await context.execute(
        "withhold worker frames during HTTP asset loading",
        {},
        () =>
          context.page.evaluate(
            async ({ moduleUrl, configuration, source, expectedBytes }) => {
              const probes = (await import(
                moduleUrl
              )) as typeof import("./lifecycle-probes.js");
              return await probes.observeResourceProgressWithoutFrames(
                configuration,
                source,
                expectedBytes,
              );
            },
            {
              moduleUrl: new URL(
                "/target/browser-build/lifecycle-probes.js",
                context.urls.origin,
              ).href,
              configuration: runtimeConfigurationFor(
                context.urls,
                context.urls.generated,
                context.urls.wasm,
                15_000,
              ),
              source: new URL(
                "/target/browser-fixtures/resource-progress.mesh",
                context.urls.origin,
              ).href,
              expectedBytes,
            },
          ),
      );
      assert.equal(report.committed.ok, true);
      assert.ok(report.afterCommitFrame.callbacks > 0);
      assert.equal(
        report.afterTransfer.callbacks,
        report.afterCommitFrame.callbacks,
      );
      assert.equal(report.afterTransfer.pending, 1);
      assert.equal(report.afterTransfer.streamedBytes, expectedBytes);
      assert.equal(report.afterTransfer.completedResponses, 1);
      assert.ok(
        report.afterInspectFrame.callbacks > report.afterCommitFrame.callbacks,
      );
      assert.ok(report.inspection.tick > report.committed.tick);
      assert.ok(
        report.inspection.resources.some(
          (resource) =>
            resource.source.endsWith("/resource-progress.mesh") &&
            resource.status === "loaded",
        ),
        JSON.stringify(report.inspection.resources, (_key, value) =>
          typeof value === "bigint" ? value.toString() : value,
        ),
      );
      assert.ok(
        report.events.some(
          (resource) =>
            resource.source.endsWith("/resource-progress.mesh") &&
            resource.status === "loaded",
        ),
      );
    },
  );
});

for (const initiallyHidden of [false, true]) {
  test(`worker ${initiallyHidden ? "hidden" : "visible"} startup and rAF visibility lifecycle`, {
    timeout: 30_000,
  }, async (testContext) => {
    await runBrowserScenario(
      `worker ${initiallyHidden ? "hidden" : "visible"} startup visibility lifecycle`,
      {
        workspace,
        build: build("headless"),
        mismatchBuild: build("headless-builtins"),
      },
      testContext.signal,
      async (context) => {
        const report = await context.execute(
          "observe real visibility lifecycle",
          {},
          () =>
            context.page.evaluate(
              async ({ moduleUrl, configuration, initiallyHidden }) => {
                const probes = (await import(
                  moduleUrl
                )) as typeof import("./lifecycle-probes.js");
                return await probes.observeVisibility(
                  configuration,
                  initiallyHidden,
                );
              },
              {
                initiallyHidden,
                moduleUrl: new URL(
                  "/target/browser-build/lifecycle-probes.js",
                  context.urls.origin,
                ).href,
                configuration: runtimeConfigurationFor(
                  context.urls,
                  context.urls.generated,
                  context.urls.wasm,
                ),
              },
            ),
        );
        if (initiallyHidden) {
          assert.equal(report.initial.time, 0);
          assert.equal(report.initialScheduler.callbacks, 0);
          assert.equal(report.initialScheduler.pending, 0);
        } else {
          assert.ok(report.initial.time > 0);
          assert.ok(report.initialScheduler.callbacks > 0);
          assert.equal(report.initialScheduler.pending, 1);
          assert.equal(report.pausedScheduler.cancelled, 1);
        }
        assert.equal(report.pausedScheduler.pending, 0);
        assert.deepEqual(report.whilePausedScheduler, report.pausedScheduler);
        assert.ok(
          report.resumedScheduler.callbacks > report.pausedScheduler.callbacks,
        );
        assert.equal(report.resumedScheduler.pending, 1);
        assert.ok(report.firstPausedFrame.tick > report.paused.tick);
        assert.ok(report.secondPausedFrame.tick > report.firstPausedFrame.tick);
        assert.equal(report.firstPausedFrame.time, report.paused.time);
        assert.equal(report.secondPausedFrame.time, report.paused.time);
        assert.equal(report.created.ok, true);
        assert.equal(report.whilePaused.time, report.paused.time);
        const entity = report.whilePaused.entities.find(
          (candidate) =>
            candidate.metadata.symbolicId === "created-while-paused",
        );
        assert.ok(entity);
        assert.equal(entity.effective[0]?.fields.value, 44);
        assert.ok(report.resumed.time > report.paused.time);
        assert.ok(report.resumed.time - report.paused.time <= 0.25);
      },
    );
  });
}

test("worker bounds undelivered output and fails a stalled receiver explicitly", {
  timeout: 30_000,
}, async (testContext) => {
  await runBrowserScenario(
    "worker stalled receiver",
    {
      workspace,
      build: build("headless"),
      mismatchBuild: build("headless-builtins"),
    },
    testContext.signal,
    async (context) => {
      const report = await context.execute(
        "withhold MessagePort delivery acknowledgements",
        {},
        () =>
          context.page.evaluate(
            async ({ moduleUrl, configuration }) => {
              const probes = (await import(
                moduleUrl
              )) as typeof import("./lifecycle-probes.js");
              return await probes.observeStalledReceiver(configuration);
            },
            {
              moduleUrl: new URL(
                "/target/browser-build/lifecycle-probes.js",
                context.urls.origin,
              ).href,
              configuration: runtimeConfigurationFor(
                context.urls,
                context.urls.generated,
                context.urls.wasm,
              ),
            },
          ),
      );
      assert.equal(report.delivered, 64);
      assert.equal(report.frames, 62);
      assert.match(
        report.error,
        /connection congestion: reliable output capacity exhausted/,
      );
    },
  );
});
