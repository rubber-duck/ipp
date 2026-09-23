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
  session: bigint;
  tick: bigint;
  width: number;
  height: number;
  drawCalls: number;
  triangles: number;
  backend: Record<string, unknown>;
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
        const capture = async (label: string) => {
          const result = await call<CaptureReport>("capture", [label]);
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
            const valid = await capture("tiny-camera-valid");
            assert.equal(valid.backend.invalidCamera, false);
            assert.equal(valid.drawCalls, 1);
            const resized =
              await call<CameraQueryObservation>("resizeTinyCamera");
            assert.equal(resized.selection.ok, false);
            assert.equal(resized.selection.session, selected.selection.session);
            assert.equal(resized.selection.camera, selected.selection.camera);
            assert.equal(resized.pick.session, selected.selection.session);
            assert.equal(resized.pick.camera, selected.selection.camera);
            assert.ok(
              !resized.pick.ok && resized.pick.error === "InvalidValue",
            );
            const invalid = await capture("tiny-camera-invalid-viewport");
            assert.equal(invalid.width, 1);
            assert.equal(invalid.height, 2048);
            assert.equal(invalid.backend.invalidCamera, true);
            assert.equal(invalid.drawCalls, 0);
            assert.equal(invalid.triangles, 0);
            requireBlank(
              invalid.summary,
              "Non-finite camera projection clears its viewport",
            );
            const repaired =
              await call<CameraQueryObservation>("repairTinyCamera");
            assert.ok(repaired.selection.ok);
            assert.equal(
              repaired.selection.session,
              selected.selection.session,
            );
            assert.equal(repaired.selection.camera, selected.selection.camera);
            assert.equal(repaired.pick.session, selected.selection.session);
            assertHit(repaired.pick, declared.target);
            const restored = await capture("tiny-camera-repaired");
            assert.equal(restored.width, 1);
            assert.equal(restored.height, 2048);
            assert.equal(restored.backend.invalidCamera, false);
            assert.equal(restored.drawCalls, 1);
            assert.equal(restored.triangles, 12);
            requireVisible(
              restored.summary,
              "Repaired camera resumes rendering in the same viewport",
            );
            assert.ok(
              (
                await compare(
                  "tiny-camera-invalid-viewport",
                  "tiny-camera-repaired",
                )
              ).changedFraction > 0.2,
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
            assert.equal(ready.backend.testMeshAllocationAttempts, 1);
            const failed = await call<GpuFailureObservation>("failGpuMesh");
            assertGpuFailureObservation(failed, selection.session, false);
            const unavailable = await capture("gpu-allocation-failed");
            assert.equal(unavailable.drawCalls, 1);
            assert.equal(unavailable.backend.failedDrawCalls, 1);
            assert.equal(unavailable.backend.testInjectedMeshFailures, 1);
            assert.equal(unavailable.backend.testMeshAllocationAttempts, 2);
            assert.ok(
              (await compare("gpu-unaffected-ready", "gpu-allocation-failed"))
                .changedFraction < 0.002,
              "Failed GPU allocation must preserve unrelated rendered geometry",
            );
            const continued =
              await call<GpuFailureObservation>("observeGpuFailure");
            assertGpuFailureObservation(continued, selection.session, false);
            assert.equal(continued.entity, failed.entity);
            const stillFailed = await capture("gpu-failure-retained");
            assert.equal(
              stillFailed.backend.testMeshAllocationAttempts,
              2,
              "A failed resource must not retry GPU allocation every frame",
            );
            assert.equal(stillFailed.backend.failedDrawCalls, 1);
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
            assert.equal(restored.backend.failedDrawCalls, 0);
            assert.equal(restored.backend.testInjectedMeshFailures, 1);
            assert.equal(restored.backend.testMeshAllocationAttempts, 4);
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
            assert.equal(pendingFrame.backend.totalUploadedBytes, 0);
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
              cpuOnly.backend.totalUploadedBytes,
              0,
              "Picking-only mesh must not allocate GPU buffers",
            );
            return;
          }

          const declared = await call<{
            target: bigint;
            noCamera: GeometryPickResultEvent;
          }>("prepareVisibleScene");
          assert.equal(declared.noCamera.ok, false);
          assert.equal(declared.noCamera.camera, null);
          const blank = await capture("no-active-camera");
          requireBlank(
            blank.summary,
            "A populated scene without an active camera",
          );
          assert.equal(blank.drawCalls, 0);

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
                { kind: "pan", x: 0.15, y: 0.1, width: 320, height: 240 },
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
              assert.equal(zoomed.camera, selected.camera);
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
          assert.ok(
            (await compare("no-active-camera", "front-camera"))
              .changedFraction > 0.02,
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

          const overlay = await call<{
            base: { x: number };
            effective: { x: number };
            pick: GeometryPickResultEvent;
          }>("overlayCamera");
          assert.equal(overlay.base.x, 1.5);
          assert.equal(overlay.effective.x, 0);
          assertHit(overlay.pick, declared.target);
          await capture("effective-camera-overlay");
          assert.ok(
            (await compare("front-camera", "effective-camera-overlay"))
              .changedFraction < 0.002,
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
            (await compare("effective-camera-overlay", "perspective-camera"))
              .changedFraction > 0.005,
          );

          const released = await call<GeometryPickResultEvent>(
            "releaseCameraOverlay",
          );
          assert.ok(released.ok && released.hit === null);
          await capture("released-camera-overlay");
          assert.ok(
            (await compare("shifted-camera", "released-camera-overlay"))
              .changedFraction < 0.002,
          );
        } catch (error) {
          await capture("failure").catch((captureError: unknown) =>
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
  assert.ok(result.camera !== null);
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
  assert.equal(observation.rim.camera, observation.selection.camera);
  assert.ok(observation.rim.ok && observation.rim.hit !== null);
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
