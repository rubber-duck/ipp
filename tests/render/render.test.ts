import { responseGate } from "../browser/response-gate.js";
import { invoke, writeDataUrl, recordCapture } from "./evidence.js";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import test from "node:test";
import type { Page } from "playwright";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import {
  type ImageDifference,
  requireBlank,
  requireShiftedAndScaled,
  requireVisible,
} from "./image-assertions.js";

const workspace = resolve(process.cwd());
const render = browserBuild("render");
const overlays = browserBuild("headless");

for (const variant of ["development", "production"] as const) {
  test(`${variant}: React scene preserves ownership cleanup and WebGL recovery`, {
    timeout: 60_000,
  }, async (context) => {
    const result = await runBrowserEnvironment(
      `${variant} react wasm unlit cube`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
        operationTimeoutMs: 12_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/render",
        ),
      },
      context.signal,
      async (scenario) => {
        const moduleUrl = `${scenario.urls.origin}/target/gallery-fixtures/${
          variant === "production" ? "fixture-production.js" : "fixture.js"
        }`;
        const configuration = {
          generatedModuleUrl: scenario.urls.generated,
          workerScriptUrl: scenario.urls.workerScript,
          wasmUrl: scenario.urls.wasm,
          meshSource: `${scenario.urls.origin}/target/gallery-build/cube.mesh`,
          timeoutMs: 10_000,
        };
        const captured = new Set<string>();
        let scenarioFailure: unknown;
        try {
          const setup = await invoke<AssetSetupReport>(
            scenario.page,
            moduleUrl,
            "initializeCube",
            [configuration],
          );
          assert.equal(setup.resource.kind, 1);
          assert.equal(setup.resource.source, configuration.meshSource);
          assert.equal(setup.resource.variant, 0);
          assert.equal(setup.resource.status, "loaded");

          await invoke(scenario.page, moduleUrl, "showMaterialOverride");
          const override = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "react-override",
          );
          await verifyAgainstBackground(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "react-override",
            () => {
              requireVisible(override.summary, "React material override");
              assert.equal(override.drawCalls, 1);
              assert.equal(override.triangles, 12);
              assert.equal(override.backend.totalUploadedBytes, 648);
              const ingress = ingressCounters(override.backend);
              assert.ok(ingress.messages > 0);
              assert.equal(ingress.partsMessages, 0);
              assert.equal(ingress.transferredAssetBytes, 0);
            },
          );

          const eulerAngles = { rx: 0.47, ry: -0.83, rz: 0.29 } as const;
          const eulerQuaternion = {
            qx: 0.15418571507595633,
            qy: -0.4187813894992057,
            qz: 0.03569826184816953,
            qw: 0.8941893240117834,
          } as const;
          const eulerInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "setReactTransform",
            [eulerAngles],
          );
          assertTransformRotation(
            eulerInspection,
            "effective",
            eulerQuaternion,
            setup.transformComponent,
          );
          const euler = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "euler-rotation",
          );
          const rotatedDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["react-override", "euler-rotation"],
          );
          assert.ok(rotatedDifference.changedFraction > 0.01);

          const quaternionInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "setReactTransform",
            [eulerQuaternion],
          );
          assertTransformRotation(
            quaternionInspection,
            "effective",
            eulerQuaternion,
            setup.transformComponent,
          );
          const quaternion = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "equivalent-quaternion",
          );
          const equivalentDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["euler-rotation", "equivalent-quaternion"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "equivalent-quaternion",
            "euler-rotation",
            () => {
              assert.deepEqual(quaternion.summary, euler.summary);
              assert.ok(equivalentDifference.changedFraction < 0.002);
            },
          );

          const producerQuaternion = {
            qx: 0,
            qy: 0.3002931752092615,
            qz: 0,
            qw: 0.9538469525677271,
          } as const;
          const producerInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "updateProducerRotation",
            [producerQuaternion],
          );
          assertTransformRotation(
            producerInspection,
            "base",
            producerQuaternion,
            setup.transformComponent,
          );
          assertTransformRotation(
            producerInspection,
            "effective",
            eulerQuaternion,
            setup.transformComponent,
          );

          const rejected = await invoke<{
            readonly message: string;
            readonly inspection: Inspection;
          }>(scenario.page, moduleUrl, "attemptReactTransform", [
            { rx: 0, qw: 1 },
          ]);
          assert.equal(
            rejected.message,
            "Transform cannot mix rx/ry/rz with qx/qy/qz/qw",
          );
          assertTransformRotation(
            rejected.inspection,
            "base",
            producerQuaternion,
            setup.transformComponent,
          );
          assertTransformRotation(
            rejected.inspection,
            "effective",
            eulerQuaternion,
            setup.transformComponent,
          );

          const correctedInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "setReactTransform",
            [eulerAngles],
          );
          assertTransformRotation(
            correctedInspection,
            "effective",
            eulerQuaternion,
            setup.transformComponent,
          );
          const corrected = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "corrected-euler-rotation",
          );
          const correctedDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["euler-rotation", "corrected-euler-rotation"],
          );
          assert.deepEqual(corrected.summary, euler.summary);
          assert.ok(correctedDifference.changedFraction < 0.002);

          const clearedInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "setReactTransform",
            [{}],
          );
          assertTransformRotation(
            clearedInspection,
            "effective",
            producerQuaternion,
            setup.transformComponent,
          );
          await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "cleared-to-producer-rotation",
          );
          const clearedRotationDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["corrected-euler-rotation", "cleared-to-producer-rotation"],
          );
          assert.ok(clearedRotationDifference.changedFraction > 0.01);

          // Restore this reusable fixture before the material/lifetime checks.
          await invoke(scenario.page, moduleUrl, "updateProducerRotation", [
            { qx: 0, qy: 0, qz: 0, qw: 1 },
          ]);
          await invoke(scenario.page, moduleUrl, "setReactTransform");

          const hidden = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "updateHiddenProducerColor",
          );
          assert.deepEqual(materialFields(hidden, "base"), {
            r: approximately(0.95),
            g: approximately(0.18),
            b: approximately(0.12),
          });
          assert.deepEqual(materialFields(hidden, "effective"), {
            r: approximately(0.16),
            g: approximately(0.95),
            b: approximately(0.28),
          });

          const fallbackInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "removeProducerMaterial",
          );
          assert.equal(
            optionalMaterialFields(fallbackInspection, "base"),
            null,
          );
          assert.deepEqual(materialFields(fallbackInspection, "effective"), {
            r: approximately(0.16),
            g: approximately(0.95),
            b: approximately(0.28),
          });
          const fallback = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "auto-material-fallback",
          );

          const reboundInspection = await invoke<Inspection>(
            scenario.page,
            moduleUrl,
            "reinsertProducerMaterial",
          );
          assert.deepEqual(materialFields(reboundInspection, "base"), {
            r: approximately(0.88),
            g: approximately(0.12),
            b: approximately(0.06),
          });
          assert.deepEqual(materialFields(reboundInspection, "effective"), {
            r: approximately(0.16),
            g: approximately(0.95),
            b: approximately(0.28),
          });
          const rebound = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "auto-material-rebound",
          );
          const fallbackDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["react-override", "auto-material-fallback"],
          );
          const reboundDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["auto-material-fallback", "auto-material-rebound"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "auto-material-rebound",
            "auto-material-fallback",
            () => {
              assert.deepEqual(fallback.summary, override.summary);
              assert.deepEqual(rebound.summary, override.summary);
              assert.ok(fallbackDifference.changedFraction < 0.002);
              assert.ok(reboundDifference.changedFraction < 0.002);
            },
          );

          await invoke(scenario.page, moduleUrl, "clearMaterialOverride");
          const cleared = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "cleared-to-latest-base",
          );
          const colorDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["react-override", "cleared-to-latest-base"],
          );
          const centerPixel = await invoke<
            readonly [number, number, number, number]
          >(scenario.page, moduleUrl, "sampleCaptured", [
            "cleared-to-latest-base",
            160,
            120,
          ]);
          const expectedCenter = [
            linearToSrgb8(1 * 0.88),
            linearToSrgb8(0.22 * 0.12),
            linearToSrgb8(0.08 * 0.06),
          ] as const;
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "cleared-to-latest-base",
            "react-override",
            () => {
              requireVisible(cleared.summary, "cleared material override");
              assert.ok(colorDifference.changedFraction > 0.015);
              assert.ok(
                cleared.summary.meanRgb[0] > override.summary.meanRgb[0],
              );
              assert.ok(
                override.summary.meanRgb[1] > cleared.summary.meanRgb[1],
              );
              for (
                let channel = 0;
                channel < expectedCenter.length;
                channel += 1
              ) {
                assert.ok(
                  Math.abs(
                    (centerPixel[channel] ?? 0) -
                      (expectedCenter[channel] ?? 0),
                  ) <= 3,
                  `center channel ${channel} expected ${expectedCenter[channel]} from exact linear-to-sRGB conversion, received ${centerPixel[channel]}`,
                );
              }
              assert.equal(centerPixel[3], 255);
            },
          );

          await invoke(scenario.page, moduleUrl, "unmountReactScene");
          const unmounted = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "react-unmounted-latest-base",
          );
          const clearUnmountDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["cleared-to-latest-base", "react-unmounted-latest-base"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "react-unmounted-latest-base",
            "cleared-to-latest-base",
            () => {
              requireVisible(unmounted.summary, "unmounted React declaration");
              assert.ok(clearUnmountDifference.changedFraction < 0.002);
            },
          );

          await invoke(scenario.page, moduleUrl, "moveAndScaleCube");
          const moved = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "transformed",
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "transformed",
            "react-unmounted-latest-base",
            () => requireShiftedAndScaled(unmounted.summary, moved.summary),
          );

          await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "before-context-loss",
          );
          const recovery = await invoke<{
            beforeGeneration: number;
            afterGeneration: number;
          }>(scenario.page, moduleUrl, "recoverContext");
          assert.ok(recovery.afterGeneration > recovery.beforeGeneration);
          await saveCapture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "after-context-restore",
          );
          const recoveryDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["before-context-loss", "after-context-restore"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "after-context-restore",
            "before-context-loss",
            () => assert.ok(recoveryDifference.changedFraction < 0.002),
          );

          await invoke(
            scenario.page,
            moduleUrl,
            "deleteOwnedCubeInFailedBatch",
          );
          const deleted = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "failed-batch-entity-deleted",
          );
          await verifyAgainstBackground(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "failed-batch-entity-deleted",
            () => {
              requireBlank(
                deleted.summary,
                "cube deleted before batch failure",
              );
              assert.equal(deleted.drawCalls, 0);
              assert.equal(deleted.triangles, 0);
            },
          );

          await invoke(scenario.page, moduleUrl, "mountReactOwnedCube");
          const reactOwned = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "react-owned-mounted",
          );
          await verifyAgainstBackground(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "react-owned-mounted",
            () => {
              requireVisible(reactOwned.summary, "React-owned cube");
              assert.equal(reactOwned.drawCalls, 1);
              assert.equal(reactOwned.triangles, 12);
            },
          );
          const ownedUnmount = await invoke<{ readonly entityExists: boolean }>(
            scenario.page,
            moduleUrl,
            "unmountReactOwnedCube",
          );
          assert.equal(ownedUnmount.entityExists, false);
          const reactOwnedUnmounted = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "react-owned-unmounted",
          );
          await verifyAgainstBackground(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "react-owned-unmounted",
            () => {
              requireBlank(
                reactOwnedUnmounted.summary,
                "unmounted React-owned cube",
              );
              assert.equal(reactOwnedUnmounted.drawCalls, 0);
              assert.equal(reactOwnedUnmounted.triangles, 0);
            },
          );

          return {
            backend: override.backend,
            session: override.session,
            firstTick: override.tick,
            finalTick: reactOwnedUnmounted.tick,
            captures: [...captured],
          };
        } catch (error) {
          scenarioFailure = error;
          throw error;
        } finally {
          try {
            await invoke(scenario.page, moduleUrl, "closeCube");
          } catch (cleanupError) {
            if (scenarioFailure !== undefined) {
              throw new AggregateError(
                [scenarioFailure, cleanupError],
                "render scenario and fixture cleanup failed",
              );
            }
            throw cleanupError;
          }
        }
      },
    );
    assert.ok(result.value.finalTick > result.value.firstTick);
    assert.equal(result.value.captures.length, 15);
    await assertLoopbackClosed(result.origin);
  });
}

test("render worker diagnostics honor levels, partial batch effects, and idle silence", {
  timeout: 120_000,
}, async (context) => {
  for (const logLevel of ["debug", "info", "off"] as const) {
    const result = await runBrowserEnvironment(
      `${logLevel} render diagnostics`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
        operationTimeoutMs: 12_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/render-diagnostics",
        ),
      },
      context.signal,
      async (scenario) => {
        const moduleUrl = `${scenario.urls.origin}/target/gallery-fixtures/fixture.js`;
        const configuration = {
          generatedModuleUrl: scenario.urls.generated,
          workerScriptUrl: scenario.urls.workerScript,
          wasmUrl: scenario.urls.wasm,
          meshSource: `${scenario.urls.origin}/target/gallery-build/cube.mesh`,
          timeoutMs: 10_000,
          logLevel,
        };
        try {
          await invoke<AssetSetupReport>(
            scenario.page,
            moduleUrl,
            "initializeCube",
            [configuration],
          );
          // Inspection can observe readiness before its event reaches the client.
          // A later frame follows all resource events from that boundary.
          await invoke(
            scenario.page,
            moduleUrl,
            "observeDiagnosticIdleFrames",
            [1],
          );
          const startup = diagnosticLines(scenario.browserLog());

          const createdAt = startup.length;
          await invoke<bigint>(
            scenario.page,
            moduleUrl,
            "createDiagnosticEntity",
          );
          const afterCreate = diagnosticLines(scenario.browserLog());

          const rejectedAt = afterCreate.length;
          const rejected = await invoke<{
            readonly ok: boolean;
            readonly error?: { readonly operation: number };
          }>(scenario.page, moduleUrl, "rejectDiagnosticEntityMutation");
          assert.equal(rejected.ok, false);
          assert.equal(rejected.error?.operation, 2);
          const afterReject = diagnosticLines(scenario.browserLog());

          const idleAt = afterReject.length;
          const idleTicks = await invoke<bigint[]>(
            scenario.page,
            moduleUrl,
            "observeDiagnosticIdleFrames",
            [3],
          );
          assert.equal(idleTicks.length, 3);
          assert.ok(idleTicks[0]! < idleTicks[1]!);
          assert.ok(idleTicks[1]! < idleTicks[2]!);
          const afterIdle = diagnosticLines(scenario.browserLog());

          const deletedAt = afterIdle.length;
          await invoke(scenario.page, moduleUrl, "deleteDiagnosticEntity");
          const afterDelete = diagnosticLines(scenario.browserLog());

          await invoke(scenario.page, moduleUrl, "closeCube");
          const all = diagnosticLines(scenario.browserLog());
          await scenario.evidence.writeJson("diagnostic-console.json", all);

          if (logLevel === "off") {
            assert.deepEqual(all, []);
          } else if (logLevel === "info") {
            assertIncludesEvents(all, [
              "worker.starting",
              "worker.ready",
              "renderer.initialized",
              "session.connected",
              "resource.loaded",
              "session.closing",
              "session.closed",
            ]);
            assertOmitsEvents(all, DEBUG_DIAGNOSTIC_EVENTS);
          } else {
            assertIncludesEvents(startup, [
              "worker.starting",
              "worker.ready",
              "renderer.initialized",
              "session.connected",
              "resource.loaded",
            ]);
            const createLines = afterCreate.slice(createdAt);
            // Console delivery from the page and worker has no shared ordering.
            // Assert each producer's sequence independently.
            assertEventOrder(createLines, [
              "command.sent",
              "command.completed",
            ]);
            assertEventOrder(createLines, [
              "buffer.receive",
              "buffer.queued",
              "buffer.processing",
              "batch.begin",
              "entity.create",
              "batch.commit",
              "buffer.complete",
            ]);
            assertCorrelationDetails(createLines);
            const rejectLines = afterReject.slice(rejectedAt);
            assertIncludesEvents(rejectLines, [
              "command.sent",
              "batch.reject",
              "command.rejected",
            ]);
            assertIncludesEvents(rejectLines, [
              "entity.create",
              "entity.delete",
            ]);
            assertCorrelationDetails(rejectLines);
            assert.deepEqual(afterIdle.slice(idleAt), []);
            const deleteLines = afterDelete.slice(deletedAt);
            assertEventOrder(deleteLines, [
              "command.sent",
              "command.completed",
            ]);
            assertEventOrder(deleteLines, [
              "batch.begin",
              "entity.delete",
              "batch.commit",
              "buffer.complete",
            ]);
            assertCorrelationDetails(deleteLines);
            assertOmitsEvents(all, HOT_PATH_DIAGNOSTIC_EVENTS);
          }
          return { logLevel, lines: all.length };
        } finally {
          await invoke(scenario.page, moduleUrl, "closeCube");
        }
      },
    );
    await assertLoopbackClosed(result.origin);
  }
});

const DEBUG_DIAGNOSTIC_EVENTS = [
  "command.sent",
  "command.completed",
  "buffer.receive",
  "buffer.queued",
  "buffer.processing",
  "buffer.complete",
  "batch.begin",
  "batch.commit",
  "entity.create",
  "entity.delete",
] as const;

const HOT_PATH_DIAGNOSTIC_EVENTS = [
  "frame",
  "draw",
  "evaluation",
  "inspect",
  "capture",
  "ack",
] as const;

interface DiagnosticLine {
  readonly level: string;
  readonly text: string;
}

function diagnosticLines(
  entries: readonly { readonly level: string; readonly text: string }[],
): DiagnosticLine[] {
  return entries.filter(({ text }) => text.includes("[IPP "));
}

function diagnosticEvent(line: DiagnosticLine): string | undefined {
  return line.text.match(
    /\[IPP [^\]]+\]\s+(?:\[session=[^\]]+\]\s+)?([^\s]+)/,
  )?.[1];
}

function assertIncludesEvents(
  lines: readonly DiagnosticLine[],
  expected: readonly string[],
): void {
  const events = lines.map(diagnosticEvent);
  for (const event of expected) {
    assert.ok(
      events.includes(event),
      `missing diagnostic ${event}; received ${lines.map(({ text }) => text).join("\n")}`,
    );
  }
}

function assertOmitsEvents(
  lines: readonly DiagnosticLine[],
  forbidden: readonly string[],
): void {
  const events = lines.map(diagnosticEvent);
  for (const event of forbidden) {
    assert.equal(
      events.includes(event),
      false,
      `unexpected diagnostic ${event}; received ${lines.map(({ text }) => text).join("\n")}`,
    );
  }
}

function assertEventOrder(
  lines: readonly DiagnosticLine[],
  expected: readonly string[],
): void {
  const events = lines.map(diagnosticEvent);
  let previous = -1;
  for (const event of expected) {
    const index = events.indexOf(event, previous + 1);
    assert.ok(
      index > previous,
      `diagnostic ${event} was missing or out of order: ${events.join(", ")}`,
    );
    previous = index;
  }
}

function assertCorrelationDetails(lines: readonly DiagnosticLine[]): void {
  for (const line of lines) {
    const event = diagnosticEvent(line);
    if (event?.startsWith("command.")) {
      assert.match(line.text, /\bsession=\d+/);
      assert.match(line.text, /\brequest=\d+/);
      assert.match(line.text, /\bbatch=\d+/);
      if (event === "command.sent") assert.match(line.text, /\bbytes=[1-9]\d*/);
    } else if (event?.startsWith("buffer.")) {
      assert.match(line.text, /\brequest=\d+/);
      assert.match(line.text, /\bbatch=\d+/);
    } else if (event?.startsWith("batch.") || event?.startsWith("entity.")) {
      assert.match(line.text, /\bbatch=\d+/);
    }
  }
}

test("HTTP resources remain declarative across pending, failure, cancellation, and replacement", {
  timeout: 60_000,
}, async (context) => {
  const delayedGate = responseGate();
  const staleGate = responseGate();
  const sessionGate = responseGate();
  const gates = new Map([
    ["?delayed-shared", delayedGate],
    ["?cancelled-demand", staleGate],
    ["?replaced-session", sessionGate],
  ]);
  const result = await runBrowserEnvironment(
    "delayed HTTP resource lifecycle",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      operationTimeoutMs: 12_000,
      closeTimeoutMs: 5_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/render-resources",
      ),
      beforeArtifactResponse: async (url, signal) =>
        gates.get(url.search)?.hold(signal),
    },
    context.signal,
    async (scenario) => {
      const moduleUrl = `${scenario.urls.origin}/target/gallery-fixtures/fixture.js`;
      const staticMesh = `${scenario.urls.origin}/target/gallery-build/cube.mesh`;
      const delayedSource = `${staticMesh}?delayed-shared`;
      const configuration = {
        generatedModuleUrl: scenario.urls.generated,
        workerScriptUrl: scenario.urls.workerScript,
        wasmUrl: scenario.urls.wasm,
        meshSource: delayedSource,
        timeoutMs: 10_000,
        providerScenario: {
          duplicateReference: true,
          unaffectedSource: "ipp://mesh/cube?width=2&height=2&length=2",
          waitForSelected: false,
        },
      };
      const captured = new Set<string>();
      let failure: unknown;
      try {
        const setup = await invoke<AssetSetupReport>(
          scenario.page,
          moduleUrl,
          "initializeCube",
          [configuration],
        );
        await withTimeout(
          delayedGate.requested,
          5_000,
          "delayed mesh provider request",
        );
        assert.equal(setup.resource.status, "start");
        assert.equal(setup.mutationBatches, 1);
        assert.equal(
          delayedGate.count(),
          1,
          "shared source must issue one HTTP request",
        );

        const pendingObservation = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "inspectResourceScene",
        );
        assert.equal(
          pendingObservation.resources.filter(
            ({ source }) => source === delayedSource,
          ).length,
          1,
        );
        assert.equal(
          resource(pendingObservation, delayedSource).status,
          "start",
        );
        const pending = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          captured,
          "http-resource-pending",
        );
        requireVisible(
          pending.summary,
          "unaffected item while HTTP is pending",
        );
        assert.equal(pending.drawCalls, 1);
        assert.equal(pending.triangles, 12);

        delayedGate.release();
        const readyObservation = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "waitForSelectedResource",
          ["loaded"],
        );
        assert.equal(
          resource(readyObservation, delayedSource).status,
          "loaded",
        );
        assert.equal(readyObservation.mutationBatches, 1);
        assert.deepEqual(
          readyObservation.entityIds,
          pendingObservation.entityIds,
          "provider completion must not require another scene mutation",
        );
        assert.equal(delayedGate.count(), 1);
        const ready = await capture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          captured,
          "http-resource-ready",
        );
        assert.equal(ready.drawCalls, 3);
        assert.equal(ready.triangles, 36);
        const automaticDifference = await invoke<ImageDifference>(
          scenario.page,
          moduleUrl,
          "compareCaptured",
          ["http-resource-pending", "http-resource-ready"],
        );
        assert.ok(automaticDifference.changedFraction > 0.005);

        await invoke(scenario.page, moduleUrl, "closeCube");
        for (const [missingSource, expectedError, label] of [
          [
            `${scenario.urls.origin}/target/gallery-build/missing-resource.mesh`,
            /404/,
            "http-resource-failed",
          ],
          [
            `${scenario.urls.origin}/__fixtures__/invalid.wasm`,
            /InvalidAsset/,
            "http-resource-malformed",
          ],
          [
            "ipc://unregistered-resource",
            /unavailable/,
            "resource-provider-unavailable",
          ],
        ] as const) {
          await invoke(scenario.page, moduleUrl, "initializeCube", [
            {
              ...configuration,
              meshSource: missingSource,
              providerScenario: {
                unaffectedSource: "ipp://mesh/cube?width=2&height=2&length=2",
                waitForSelected: false,
              },
            },
          ]);
          const failedObservation = await invoke<ResourceSceneObservation>(
            scenario.page,
            moduleUrl,
            "waitForSelectedResource",
            ["failed"],
          );
          const failed = resource(failedObservation, missingSource);
          assert.equal(failed.status, "failed");
          assert.match(failed.error ?? "", expectedError);
          const failedFrame = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            label,
          );
          assert.equal(failedFrame.drawCalls, 1);
          assert.equal(failedFrame.triangles, 12);
          requireVisible(failedFrame.summary, label);
        }

        await invoke(scenario.page, moduleUrl, "closeCube");
        const staleSource = `${staticMesh}?cancelled-demand`;
        await invoke(scenario.page, moduleUrl, "initializeCube", [
          {
            ...configuration,
            meshSource: staleSource,
          },
        ]);
        await withTimeout(
          staleGate.requested,
          5_000,
          "cancellable mesh provider request",
        );
        const replacementSource = "ipp://mesh/sphere?radius=0.32";
        const changed = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "changeSharedResourceSource",
          [replacementSource],
        );
        assert.equal(resource(changed, staleSource).status, "start");
        assert.ok(
          changed.resources.some(({ source }) => source === replacementSource),
        );
        const removed = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "removePrimaryResourceEntity",
        );
        assert.ok(
          removed.resources.every(({ source }) => source !== staleSource),
          "last demand removal must clear the pending source",
        );
        await withTimeout(
          staleGate.aborted,
          5_000,
          "last demand cancels actual HTTP request",
        );
        staleGate.release();
        const afterStale = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "inspectResourceScene",
        );
        assert.ok(
          afterStale.resources.every(({ source }) => source !== staleSource),
        );

        const sessionSource = `${staticMesh}?replaced-session`;
        await invoke(scenario.page, moduleUrl, "initializeCube", [
          { ...configuration, meshSource: sessionSource },
        ]);
        await withTimeout(
          sessionGate.requested,
          5_000,
          "old session pending HTTP request",
        );
        const oldWorld = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "inspectResourceScene",
        );
        assert.equal(resource(oldWorld, sessionSource).status, "start");
        const replacement = await invoke<AssetSetupReport>(
          scenario.page,
          moduleUrl,
          "initializeCube",
          [
            {
              ...configuration,
              meshSource: staticMesh,
              providerScenario: undefined,
            },
          ],
        );
        await withTimeout(
          sessionGate.aborted,
          5_000,
          "session close cancels actual HTTP request",
        );
        sessionGate.release();
        const replacementObservation = await invoke<ResourceSceneObservation>(
          scenario.page,
          moduleUrl,
          "inspectResourceScene",
        );
        // A new worker owns a fresh Host; numeric session IDs are scoped to it.
        assert.equal(replacement.resource.status, "loaded");
        assert.ok(
          replacementObservation.resources.every(
            ({ source }) => source !== staleSource && source !== sessionSource,
          ),
        );
        return {
          requests: delayedGate.count(),
          replacementSession: replacementObservation.session,
        };
      } catch (error) {
        failure = error;
        throw error;
      } finally {
        for (const gate of gates.values()) gate.release();
        try {
          await invoke(scenario.page, moduleUrl, "closeCube");
        } catch (cleanupError) {
          if (failure !== undefined) {
            throw new AggregateError(
              [failure, cleanupError],
              "resource scenario and cleanup failed",
            );
          }
          throw cleanupError;
        }
      }
    },
  );
  assert.equal(result.value.requests, 1);
  await assertLoopbackClosed(result.origin);
});

function browserBuild(name: "render" | "headless"): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    // Parent extends the shared browser profile union with render.
    name: name as BrowserBuildConfiguration["name"],
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

async function capture(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  captured: Set<string>,
  label: string,
): Promise<CaptureReport> {
  const report = await invoke<CaptureReport>(page, moduleUrl, "captureCube", [
    label,
  ]);
  await saveCapture(page, moduleUrl, evidenceDirectory, captured, label);
  return report;
}

async function saveCapture(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  captured: Set<string>,
  label: string,
): Promise<void> {
  await recordCapture(page, moduleUrl, evidenceDirectory, captured, label, {
    canvasSelector: "#ipp-cube-canvas",
  });
}

async function verifyAgainstBackground(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  actual: string,
  verify: () => void,
): Promise<void> {
  try {
    verify();
  } catch (error) {
    const [actualImage, expectedImage, differenceImage] = await Promise.all([
      invoke<string>(page, moduleUrl, "captureDataUrl", [actual]),
      invoke<string>(page, moduleUrl, "backgroundDataUrl"),
      invoke<string>(page, moduleUrl, "differenceFromBackgroundDataUrl", [
        actual,
      ]),
    ]);
    await writeFailureImages(
      evidenceDirectory,
      actualImage,
      expectedImage,
      differenceImage,
    );
    throw error;
  }
}

async function verifyPair(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  actual: string,
  expected: string,
  verify: () => void,
): Promise<void> {
  try {
    verify();
  } catch (error) {
    const [actualImage, expectedImage, differenceImage] = await Promise.all([
      invoke<string>(page, moduleUrl, "captureDataUrl", [actual]),
      invoke<string>(page, moduleUrl, "captureDataUrl", [expected]),
      invoke<string>(page, moduleUrl, "differenceDataUrl", [expected, actual]),
    ]);
    await writeFailureImages(
      evidenceDirectory,
      actualImage,
      expectedImage,
      differenceImage,
    );
    throw error;
  }
}

async function writeFailureImages(
  directory: string,
  actual: string,
  expected: string,
  difference: string,
): Promise<void> {
  await Promise.all([
    writeDataUrl(join(directory, "failure-actual.png"), actual),
    writeDataUrl(join(directory, "failure-expected.png"), expected),
    writeDataUrl(join(directory, "failure-diff.png"), difference),
  ]);
}

function materialFields(
  inspection: Inspection,
  layer: "base" | "effective",
): Readonly<Record<string, number | bigint>> {
  const material = optionalMaterialFields(inspection, layer);
  if (!material) throw new Error(`missing ${layer} UnlitMaterial inspection`);
  return material;
}

function optionalMaterialFields(
  inspection: Inspection,
  layer: "base" | "effective",
): Readonly<Record<string, number | bigint>> | null {
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === "cube",
  );
  const material = entity?.[layer].find(({ component }) => component === 4);
  if (!material) return null;
  return {
    r: approximately(Number(material.fields.r)),
    g: approximately(Number(material.fields.g)),
    b: approximately(Number(material.fields.b)),
  };
}

function assertTransformRotation(
  inspection: Inspection,
  layer: "base" | "effective",
  expected: Readonly<Record<"qx" | "qy" | "qz" | "qw", number>>,
  componentId: number,
): void {
  const entity = inspection.entities.find(
    ({ metadata }) => metadata.symbolicId === "cube",
  );
  const transform = entity?.[layer].find(
    ({ component }) => component === componentId,
  );
  if (!transform) throw new Error(`missing ${layer} Transform inspection`);
  for (const field of ["qx", "qy", "qz", "qw"] as const) {
    assert.ok(
      Math.abs(Number(transform.fields[field]) - expected[field]) < 1e-6,
      `${layer} Transform.${field} expected ${expected[field]}, received ${transform.fields[field]}`,
    );
  }
}

function ingressCounters(backend: Readonly<Record<string, unknown>>): {
  readonly messages: number;
  readonly wasmCopyBytes: number;
  readonly partsMessages: number;
  readonly transferredAssetBytes: number;
} {
  const ingress = backend.ingress;
  if (typeof ingress !== "object" || ingress === null) {
    throw new Error("capture backend omitted ingress counters");
  }
  const counters = ingress as Readonly<Record<string, unknown>>;
  for (const name of [
    "messages",
    "wasmCopyBytes",
    "partsMessages",
    "transferredAssetBytes",
  ]) {
    if (typeof counters[name] !== "number") {
      throw new Error(`capture backend ingress.${name} is not numeric`);
    }
  }
  return counters as {
    readonly messages: number;
    readonly wasmCopyBytes: number;
    readonly partsMessages: number;
    readonly transferredAssetBytes: number;
  };
}

function approximately(value: number): number {
  return Math.round(value * 100) / 100;
}

function linearToSrgb8(value: number): number {
  const encoded =
    value <= 0.003_130_8 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, encoded)) * 255);
}

interface AssetSetupReport {
  readonly entity: bigint;
  readonly transformComponent: number;
  readonly mutationBatches: number;
  readonly resource: {
    readonly kind: number;
    readonly source: string;
    readonly variant: number;
    readonly status: "unloaded" | "start" | "progress" | "loaded" | "failed";
    readonly error?: string;
  };
}

interface ResourceSceneObservation {
  readonly session: bigint;
  readonly mutationBatches: number;
  readonly entityIds: readonly string[];
  readonly resources: readonly AssetSetupReport["resource"][];
}

interface CaptureReport {
  readonly session: bigint;
  readonly tick: bigint;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly contextGeneration: number;
  readonly backend: Readonly<Record<string, unknown>>;
  readonly summary: ReturnType<
    typeof import("./image-assertions.js").summarizeImage
  >;
}

interface Inspection {
  readonly entities: readonly {
    readonly metadata: { readonly symbolicId: string | null };
    readonly base: readonly ComponentSnapshot[];
    readonly effective: readonly ComponentSnapshot[];
  }[];
}

interface ComponentSnapshot {
  readonly component: number;
  readonly fields: Readonly<Record<string, number | bigint | string>>;
}

function resource(
  observation: ResourceSceneObservation,
  source: string,
): AssetSetupReport["resource"] {
  const match = observation.resources.find(
    (candidate) => candidate.source === source,
  );
  if (!match) throw new Error(`missing resource observation for ${source}`);
  return match;
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error(`Timed out waiting for ${label}`)),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
