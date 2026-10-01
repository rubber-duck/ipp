import { outputProducer } from "../../packages/ipp-client/src/references.js";
import type { RenderStatisticsSnapshot } from "@ipp/client";
import assert from "node:assert/strict";
import { copyFile, mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import type {
  GeometryPickResultEvent,
  Inspection,
  AssetResourceSnapshot,
} from "@ipp/client";
import { PICKING_RING } from "../integration/camera-fixtures.js";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { invoke, recordCapture, writeDataUrl } from "./evidence.js";
import {
  requireBlank,
  requireVisible,
  type ImageDifference,
  type ImageSummary,
} from "./image-assertions.js";

const workspace = resolve(process.cwd());
const directory = resolve(workspace, "target/browser-build/render");
const build: BrowserBuildConfiguration = {
  name: "render",
  generatedModule: resolve(directory, "generated.js"),
  runtimeWasm: resolve(directory, "runtime.wasm"),
  exportWasm: resolve(directory, "export.wasm"),
  contractArtifact: resolve(directory, "contract.bin"),
};

interface CaptureReport {
  width: number;
  height: number;
  devicePixelRatio: number;
  sequence: bigint;
  context: bigint;
  drawCalls: number;
  triangles: number;
  failedDrawCalls: number;
  statistics: RenderStatisticsSnapshot;
  summary: ImageSummary;
}

for (const scenario of [
  "camera views",
  "camera navigation",
  "CPU compound geometry",
  "GPU allocation failure",
  "projection overflow",
] as const) {
  test(`WebGL ${scenario} use real correlated picking and completed frames`, {
    timeout: 60_000,
  }, async (context) => {
    const fixtureDirectory = resolve(workspace, "target/camera-fixtures");
    const fixturePath = "/target/camera-fixtures/picking-ring.geometry";
    await mkdir(fixtureDirectory, { recursive: true });
    await writeFile(
      resolve(fixtureDirectory, "picking-ring.geometry"),
      (
        await import(
          pathToFileURL(
            resolve(workspace, "target/browser-build/render/generated.js"),
          ).href
        )
      ).encodeBoundingShape(PICKING_RING),
    );
    if (scenario === "GPU allocation failure") {
      const failingDirectory = resolve(fixtureDirectory, "failing-gpu");
      await mkdir(failingDirectory, { recursive: true });
      await copyFile(
        build.runtimeWasm,
        resolve(failingDirectory, "runtime.wasm"),
      );
      // Inject one failure at the real device import boundary. All successful
      // operations use the shipped WebGL device and the unchanged Rust renderer.
      await writeFile(
        resolve(failingDirectory, "webgl.js"),
        `
import { createWebGlDevice as createRealDevice } from "/target/browser-build/render/webgl.js";
export function createWebGlDevice(canvas) {
  const device = createRealDevice(canvas);
  let attempts = 0;
  let failures = 0;
  for (const name of ["create_mesh", "create_mesh_uv"]) {
    const create = device.imports[name];
    if (typeof create !== "function") throw new Error("Missing mesh import " + name);
    device.imports[name] = (...args) => {
      attempts += 1;
      if (attempts === 2) { failures += 1; return 0; }
      return create(...args);
    };
  }
  const info = device.info.bind(device);
  device.info = () => ({ ...info(), testMeshAllocationAttempts: attempts, testInjectedMeshFailures: failures });
  return device;
}
`,
      );
    }
    let releaseResponse!: () => void;
    let enteredResponse!: () => void;
    const responseGate = new Promise<void>((resolve) => {
      releaseResponse = resolve;
    });
    const responseEntered = new Promise<void>((resolve) => {
      enteredResponse = resolve;
    });
    await runBrowserEnvironment(
      `WebGL ${scenario}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 20_000,
        closeTimeoutMs: 5_000,
        beforeArtifactResponse: async (url) => {
          if (url.pathname !== fixturePath) return;
          enteredResponse();
          await responseGate;
        },
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/cameras",
        ),
      },
      context.signal,
      async (environment) => {
        const module = `${environment.urls.origin}/dist/tests/render/camera-fixture.js`;
        await environment.page.exposeFunction(
          "recordCamera",
          (kind: string, value: unknown) =>
            environment.evidence.record(kind, value),
        );
        const captures = new Set<string>();
        const call = <T>(name: string, args: readonly unknown[] = []) =>
          environment.execute(name, args, () =>
            invoke<T>(environment.page, module, name, args),
          );
        const capture = async (label: string, included = true) => {
          const result = await call<CaptureReport>("capture", [
            label,
            included,
          ]);
          await recordCapture(
            environment.page,
            module,
            environment.evidence.directory,
            captures,
            label,
            { canvasSelector: "#camera-canvas" },
          );
          return result;
        };
        const compare = async (first: string, second: string) => {
          await writeDataUrl(
            join(environment.evidence.directory, `${first}-${second}-diff.png`),
            // Artifact data stays out of the recorded operation log.
            await invoke<string>(
              environment.page,
              module,
              "differenceDataUrl",
              [first, second],
            ),
          );
          return call<ImageDifference>("difference", [first, second]);
        };
        try {
          await call("initialize", [
            {
              generatedModuleUrl: environment.urls.generated,
              workerScriptUrl: environment.urls.workerScript,
              wasmUrl:
                scenario === "GPU allocation failure"
                  ? `${environment.urls.origin}/target/camera-fixtures/failing-gpu/runtime.wasm`
                  : environment.urls.wasm,
            },
          ]);
          if (scenario === "projection overflow") {
            const declared = await call<{ target: bigint }>(
              "prepareVisibleScene",
            );
            const selected =
              await call<CameraQueryObservation>("selectTinyCamera");
            assert.ok(selected.selection.ok);
            assertHit(selected.pick, declared.target);
            assert.ok(selected.selection.ok);
            const camera = outputProducer(
              selected.selection.view.output,
            )?.entity;
            const valid = await capture("tiny-camera-valid");
            assert.equal(valid.drawCalls, 1);
            const resized =
              await call<CameraQueryObservation>("resizeTinyCamera");
            assert.equal(resized.selection.ok, false);
            assert.equal(resized.selection.session, selected.selection.session);
            assert.deepEqual(resized.binding, {
              entity: camera,
              width: 1,
              height: 2048,
            });
            assert.equal(resized.pick.session, selected.selection.session);
            assert.ok(
              !resized.pick.ok && resized.pick.error === "InvalidValue",
            );
            assert.deepEqual(
              await call<{ reason: string }>("captureFailure"),
              { reason: "drawFailed" },
              "Non-finite camera projection has no successful presented draw",
            );
            const repaired =
              await call<CameraQueryObservation>("repairTinyCamera");
            assert.ok(repaired.selection.ok);
            assert.equal(
              repaired.selection.session,
              selected.selection.session,
            );
            assert.equal(
              outputProducer(repaired.selection.view.output)?.entity,
              camera,
            );
            assert.deepEqual(repaired.binding, resized.binding);
            assert.equal(repaired.pick.session, selected.selection.session);
            assertHit(repaired.pick, declared.target);
            const restored = await capture("tiny-camera-repaired");
            assert.equal(restored.width, 1);
            assert.equal(restored.height, 2048);
            assert.equal(restored.drawCalls, 1);
            assert.equal(restored.triangles, 12);
            requireVisible(
              restored.summary,
              "Repaired camera resumes rendering in the same viewport",
            );
            return;
          }
          if (scenario === "GPU allocation failure") {
            const selection = await call<GeometryPickResultEvent>(
              "prepareGpuFailureScene",
            );
            assert.equal(selection.ok, true);
            const ready = await capture("gpu-unaffected-ready");
            requireVisible(ready.summary, "Unaffected ready geometry");
            assert.equal(ready.drawCalls, 1);
            assert.equal(
              ready.statistics!.device.testMeshAllocationAttempts,
              1,
            );
            const failed = await call<GpuFailureObservation>("failGpuMesh");
            assertGpuFailureObservation(failed, selection.session, false);
            const unavailable = await capture("gpu-allocation-failed", false);
            assert.equal(unavailable.drawCalls, 1);
            assert.equal(unavailable.failedDrawCalls, 1);
            assert.equal(
              unavailable.statistics!.device.testInjectedMeshFailures,
              1,
            );
            assert.equal(
              unavailable.statistics!.device.testMeshAllocationAttempts,
              2,
            );
            assert.ok(
              (await compare("gpu-unaffected-ready", "gpu-allocation-failed"))
                .changedFraction < 0.002,
              "Failed GPU allocation must preserve unrelated rendered geometry",
            );
            const continued =
              await call<GpuFailureObservation>("observeGpuFailure");
            assertGpuFailureObservation(continued, selection.session, false);
            assert.equal(continued.entity, failed.entity);
            const stillFailed = await capture("gpu-failure-retained", false);
            assert.equal(
              stillFailed.statistics!.device.testMeshAllocationAttempts,
              2,
              "A failed resource must not retry GPU allocation every frame",
            );
            assert.equal(stillFailed.failedDrawCalls, 1);
            const recovered =
              await call<GpuFailureObservation>("recoverGpuFailure");
            assertGpuFailureObservation(recovered, selection.session, true);
            assert.equal(recovered.entity, failed.entity);
            const restored = await capture("gpu-recovered");
            requireVisible(
              restored.summary,
              "Recovered visual ring and ready cube",
            );
            assert.equal(restored.drawCalls, 2);
            assert.equal(restored.triangles, 20);
            assert.equal(restored.failedDrawCalls, 0);
            assert.equal(
              restored.statistics!.device.testInjectedMeshFailures,
              1,
            );
            assert.equal(
              restored.statistics!.device.testMeshAllocationAttempts,
              4,
            );
            assert.ok(
              (await compare("gpu-allocation-failed", "gpu-recovered"))
                .changedFraction > 0.03,
              "Context recovery must make the failed ring visible",
            );
            return;
          }
          if (scenario === "CPU compound geometry") {
            await call("cpuMeshScenario");
            const source = `${environment.urls.origin}${fixturePath}`;
            const target = await call<bigint>("declarePendingMesh", [source]);
            await environment.execute(
              "HTTP mesh response reached its gate",
              {},
              () => responseEntered,
            );
            const pending = await call<{
              resource: AssetResourceSnapshot;
              query: GeometryPickResultEvent;
            }>("observePendingMesh", [source]);
            assert.ok(
              pending.resource.status === "start" ||
                pending.resource.status === "progress",
            );
            assert.ok(
              !pending.query.ok &&
                pending.query.error === "GeometryUnavailable",
            );
            const pendingFrame = await capture("pending-cpu-interaction");
            requireBlank(pendingFrame.summary, "Pending interaction geometry");
            assert.equal(pendingFrame.statistics!.frame.totalUploadedBytes, 0);
            releaseResponse();
            const completed = await call<{
              resource: AssetResourceSnapshot;
              hole: GeometryPickResultEvent;
              rim: GeometryPickResultEvent;
            }>("completePendingMesh", [source]);
            assert.equal(completed.resource.id, pending.resource.id);
            assert.ok(completed.hole.ok && completed.hole.hit === null);
            assert.ok(completed.rim.ok && completed.rim.hit !== null);
            assert.equal(completed.rim.hit.entity, target);
            assert.equal(completed.rim.hit.part, 1);
            assert.notEqual(completed.rim.requestId, pending.query.requestId);
            const cpuOnly = await capture("cpu-only-interaction");
            requireBlank(cpuOnly.summary, "CPU picking has no visual mesh");
            assert.equal(cpuOnly.drawCalls, 0);
            assert.equal(cpuOnly.triangles, 0);
            assert.equal(
              cpuOnly.statistics!.frame.totalUploadedBytes,
              0,
              "Picking-only mesh must not allocate GPU buffers",
            );
            return;
          }

          const declared = await call<{
            target: bigint;
            rootBinding: null;
            noCamera: GeometryPickResultEvent;
          }>("prepareVisibleScene");
          assert.equal(
            declared.rootBinding,
            null,
            "A populated scene has no implicit root view",
          );
          assert.ok(
            !declared.noCamera.ok &&
              declared.noCamera.error === "InvalidEntity",
            "An unselected camera cannot answer root view queries",
          );

          if (scenario === "camera navigation") {
            for (const projection of [0, 1]) {
              const label = projection === 0 ? "perspective" : "orthographic";
              const selected = await call<GeometryPickResultEvent>(
                "prepareNavigation",
                [projection],
              );
              assertHit(selected, declared.target);
              const baseline = await capture(`${label}-navigation-front`);
              requireVisible(baseline.summary, "Navigation baseline cube");
              const rotated = await call<GeometryPickResultEvent>("navigate", [
                { kind: "rotate", yaw: 0.6, pitch: 0.25 },
              ]);
              assert.ok(rotated.ok && rotated.hit !== null);
              assert.equal(rotated.hit.entity, declared.target);
              const plane = rotated.hit.viewPlane!;
              assert.deepEqual(plane.point, rotated.hit.position);
              const forward = [
                -Math.sin(0.6) * Math.cos(0.25),
                Math.sin(0.25),
                -Math.cos(0.6) * Math.cos(0.25),
              ];
              plane.normal.forEach((value, i) =>
                assert.ok(Math.abs(value - forward[i]!) < 1e-5),
              );
              const orbit = await capture(`${label}-navigation-orbit`);
              requireVisible(
                orbit.summary,
                "Orbit preserves the visible pivot",
              );
              assert.ok(Math.abs(orbit.summary.centroidX! - 159.5) < 4);
              assert.ok(Math.abs(orbit.summary.centroidY! - 119.5) < 4);
              assert.ok(
                (
                  await compare(
                    `${label}-navigation-front`,
                    `${label}-navigation-orbit`,
                  )
                ).changedFraction > 0.005,
              );

              const panned = await call<GeometryPickResultEvent>("navigate", [
                { kind: "pan", x: 0.15, y: 0.1 },
                0.65,
                0.6,
              ]);
              assert.ok(panned.ok && panned.hit !== null);
              assert.equal(panned.hit.entity, declared.target);
              assert.deepEqual(panned.hit.viewPlane!.normal, plane.normal);
              assert.deepEqual(
                panned.hit.viewPlane!.point,
                panned.hit.position,
              );
              const pan = await capture(`${label}-navigation-pan`);
              requireVisible(pan.summary, "Panned cube");
              assert.ok(
                Math.abs(
                  pan.summary.centroidX! - orbit.summary.centroidX! - 48,
                ) < 5,
              );
              assert.ok(
                Math.abs(
                  pan.summary.centroidY! - orbit.summary.centroidY! - 24,
                ) < 5,
              );
              assert.ok(
                (
                  await compare(
                    `${label}-navigation-orbit`,
                    `${label}-navigation-pan`,
                  )
                ).changedFraction > 0.03,
              );

              const zoomed = await call<GeometryPickResultEvent>("navigate", [
                { kind: "zoom", amount: Math.log(2) },
                0.575,
                0.55,
              ]);
              assert.ok(zoomed.ok && zoomed.hit !== null);
              assert.equal(zoomed.hit.entity, declared.target);
              assert.deepEqual(zoomed.hit.viewPlane!.normal, plane.normal);
              assert.deepEqual(
                zoomed.hit.viewPlane!.point,
                zoomed.hit.position,
              );
              assert.ok(selected.ok);
              assert.equal(
                outputProducer(zoomed.view.output)?.entity,
                outputProducer(selected.view.output)?.entity,
              );
              const zoom = await capture(`${label}-navigation-zoom`);
              assert.ok(
                zoom.summary.coverage > 0.005 && zoom.summary.bounds !== null,
                "Zoomed cube retains measurable visible pixels",
              );
              assert.ok(zoom.summary.coverage < pan.summary.coverage * 0.4);
              assert.ok(Math.abs(zoom.summary.centroidX! - 183.5) < 4);
              assert.ok(Math.abs(zoom.summary.centroidY! - 131.5) < 4);
              assert.ok(
                (
                  await compare(
                    `${label}-navigation-pan`,
                    `${label}-navigation-zoom`,
                  )
                ).changedFraction > 0.015,
              );
            }
            return;
          }

          const front = await call<{
            selection: GeometryPickResultEvent;
            pick: GeometryPickResultEvent;
          }>("selectFront");
          assert.equal(front.selection.ok, true);
          assertHit(front.pick, declared.target);
          const centered = await capture("front-camera");
          requireVisible(centered.summary, "Front camera cube");
          assert.equal(centered.drawCalls, 1);
          assert.equal(centered.triangles, 12);
          assert.ok(
            centered.summary.centroidX !== null &&
              Math.abs(centered.summary.centroidX - 159.5) < 2,
          );

          const switched = await call<{
            selection: GeometryPickResultEvent;
            center: GeometryPickResultEvent;
            projected: GeometryPickResultEvent;
            mismatch: GeometryPickResultEvent;
          }>("selectShifted");
          assert.equal(switched.selection.ok, true);
          assert.ok(switched.center.ok && switched.center.hit === null);
          assertHit(switched.projected, declared.target);
          assert.ok(
            !switched.mismatch.ok &&
              switched.mismatch.error === "InvalidViewport",
          );
          const shifted = await capture("shifted-camera");
          requireVisible(shifted.summary, "Shifted camera cube");
          assert.equal(shifted.width, 320);
          assert.equal(shifted.height, 240);
          assert.ok(
            shifted.summary.centroidX !== null &&
              centered.summary.centroidX !== null &&
              centered.summary.centroidX - shifted.summary.centroidX > 80,
          );
          assert.ok(
            (await compare("front-camera", "shifted-camera")).changedFraction >
              0.02,
          );

          const moved = await call<{
            before: number;
            after: number;
            pick: GeometryPickResultEvent;
          }>("moveCamera");
          assert.equal(moved.before, 1.5);
          assert.equal(moved.after, 0);
          assertHit(moved.pick, declared.target);
          await capture("moved-camera");
          assert.ok(
            (await compare("front-camera", "moved-camera")).changedFraction <
              0.002,
          );

          assertHit(
            await call<GeometryPickResultEvent>("perspectiveCamera"),
            declared.target,
          );
          const perspective = await capture("perspective-camera");
          requireVisible(perspective.summary, "Perspective camera cube");
          assert.ok(
            perspective.summary.coverage < centered.summary.coverage * 0.9,
            "Perspective projection must change the rendered footprint",
          );
          assert.ok(
            (await compare("moved-camera", "perspective-camera"))
              .changedFraction > 0.005,
          );

          const restored = await call<GeometryPickResultEvent>("restoreCamera");
          assert.ok(restored.ok && restored.hit === null);
          await capture("restored-camera");
          assert.ok(
            (await compare("shifted-camera", "restored-camera"))
              .changedFraction < 0.002,
          );
        } catch (error) {
          await capture("failure", false).catch((captureError: unknown) =>
            environment.evidence.record("failure_capture_error", {
              captureError,
            }),
          );
          throw error;
        } finally {
          releaseResponse();
          await call("close");
        }
      },
    );
  });
}

function assertHit(result: GeometryPickResultEvent, entity: bigint) {
  assert.ok(result.ok && result.hit !== null);
  assert.equal(result.hit.entity, entity);
  assert.equal(result.hit.part, 0);
  assert.ok(Math.abs(result.hit.position[2] - 0.5) < 0.0001);
}

interface GpuFailureObservation {
  entity: bigint;
  selection: GeometryPickResultEvent;
  rim: GeometryPickResultEvent;
  hole: GeometryPickResultEvent;
  inspection: Inspection;
}

interface CameraQueryObservation {
  selection: GeometryPickResultEvent;
  binding?: { entity: bigint; width: number; height: number } | null;
  pick: GeometryPickResultEvent;
}

function assertGpuFailureObservation(
  observation: GpuFailureObservation,
  session: bigint,
  graphicsReady: boolean,
) {
  assert.ok(observation.selection.ok);
  assert.equal(observation.selection.session, session);
  assert.equal(observation.rim.session, session);
  assert.ok(observation.rim.ok && observation.rim.hit !== null);
  assert.equal(
    outputProducer(observation.rim.view.output)?.entity,
    outputProducer(observation.selection.view.output)?.entity,
  );
  assert.equal(observation.rim.hit.entity, observation.entity);
  assert.equal(observation.rim.hit.part, 1);
  assert.ok(observation.hole.ok && observation.hole.hit === null);
  const resource = observation.inspection.resources.find((resource) =>
    resource.source.endsWith("/assets/1/700#immutable"),
  );
  assert.ok(resource);
  assert.equal(resource.status, graphicsReady ? "loaded" : "failed");
  assert.equal(
    resource.representation.decoded,
    true,
    "GPU failure must preserve CPU resource availability",
  );
  assert.equal(resource.representation.graphicsReady, graphicsReady);
  assert.ok(resource.representation.residentBytes > 0n);
}
