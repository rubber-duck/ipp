import { invoke, writeDataUrl, recordCapture } from "./evidence.js";
import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import test from "node:test";
import type { ConsoleMessage, Page } from "playwright";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import {
  ASYMMETRIC_RGB,
  CUBE_MATERIAL,
  CUBE_SOURCE,
  type CaptureReport,
  LAYOUT_COLOR,
  type OptionalLayoutOrderReport,
  type OptionalLayoutSample,
  type OptionalLayoutSetupReport,
  PLANE_MATERIAL,
  QUAD_MATERIAL,
  QUAD_VERTEX_COLOR,
  type RejectionReport,
  type SamplerObservation,
  type WorldInspection,
  type TextureSetupReport,
  WEIGHT_COLOR,
} from "./texture-fixture.js";
import { requireVisible } from "./image-assertions.js";

const workspace = resolve(process.cwd());
const render = browserBuild("render");
const overlays = browserBuild("headless");

for (const variant of ["development", "production"] as const) {
  test(`${variant}: textured React scenes preserve exact sampling and retained recovery`, {
    timeout: 60_000,
  }, async (context) => {
    const pageErrors: string[] = [];
    const consoleErrors: string[] = [];
    const result = await runBrowserEnvironment(
      `${variant} React WASM texture pipeline`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
        operationTimeoutMs: 12_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/textures",
        ),
      },
      context.signal,
      async (scenario) => {
        scenario.page.on("pageerror", (error) =>
          pageErrors.push(error.message),
        );
        scenario.page.on("console", (message) =>
          recordConsoleError(message, consoleErrors),
        );
        const moduleUrl = `${scenario.urls.origin}/target/texture-build/${
          variant === "production" ? "fixture-production.js" : "fixture.js"
        }`;
        const assetBaseUrl = `${scenario.urls.origin}/target/texture-build`;
        const captured = new Set<string>();
        let scenarioFailure: unknown;
        try {
          const setup = await invoke<TextureSetupReport>(
            scenario.page,
            moduleUrl,
            "initializeTextures",
            [
              {
                generatedModuleUrl: scenario.urls.generated,
                workerScriptUrl: scenario.urls.workerScript,
                wasmUrl: scenario.urls.wasm,
                timeoutMs: 10_000,
                assetBaseUrl,
              },
            ],
          );
          assert.equal(setup.componentId, 6);
          assert.deepEqual(
            setup.resources.map(({ source, status }) => ({ source, status })),
            [
              { source: CUBE_SOURCE, status: "loaded" },
              {
                source: `${assetBaseUrl}/checker.texture`,
                status: "loaded",
              },
            ],
          );
          assertProgramCounts(setup.startupBackend, 0, 0);
          assert.deepEqual(setup.inspection.texture, {
            source: `${assetBaseUrl}/checker.texture`,
            variant: 0,
          });

          const checker = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "builtin-checker-cube",
          );
          const checkerColors = [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [0, 0, 0],
          ].map((rgb) =>
            rgb.map((encoded, channel) =>
              linearToSrgb8(
                srgb8ToLinear(encoded) * (CUBE_MATERIAL[channel] ?? 0),
              ),
            ),
          );
          const checkerEvidence = await invoke<ColorEvidence>(
            scenario.page,
            moduleUrl,
            "analyzeCapturedColors",
            ["builtin-checker-cube", checkerColors, 3],
          );
          await verifySamplerImage(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "builtin-checker-cube",
            false,
            () => {
              requireVisible(checker.summary, "built-in checker cube");
              assert.equal(checker.drawCalls, 1);
              assert.ok(
                ingressNumber(checker.backend, "sourceBytes") >= 25_165_840,
              );
              assert.ok(
                ingressNumber(checker.backend, "sourcePeakBufferedBytes") <=
                  2 * 65_536,
                "one HTTP texture retains at most one JS chunk and one Rust pipe",
              );
              assert.equal(checker.triangles, 12);
              assertProgramCounts(checker.backend, 1, 1);
              for (const [index, count] of checkerEvidence.counts.entries()) {
                assert.ok(
                  count > 250,
                  `checker RGB/black class ${index} is absent`,
                );
              }
              assert.ok(checkerEvidence.adjacentTransitions > 100);
              assert.ok(
                checkerEvidence.unmatchedForegroundPixels <
                  checkerEvidence.foregroundPixels * 0.02,
              );
            },
          );

          const untexturedInspection = await invoke<WorldInspection>(
            scenario.page,
            moduleUrl,
            "setCubeTextureEnabled",
            [false],
          );
          assert.equal(untexturedInspection.entityExists, true);
          assert.equal(untexturedInspection.texture, null);
          const untextured = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "cube-without-texture-declaration",
          );
          const untexturedColor = CUBE_MATERIAL.map(linearToSrgb8);
          const untexturedEvidence = await invoke<ColorEvidence>(
            scenario.page,
            moduleUrl,
            "analyzeCapturedColors",
            ["cube-without-texture-declaration", [untexturedColor], 3],
          );
          const declarationDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["builtin-checker-cube", "cube-without-texture-declaration"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "cube-without-texture-declaration",
            "builtin-checker-cube",
            () => {
              requireVisible(untextured.summary, "untextured retained cube");
              assert.equal(untextured.drawCalls, 1);
              assert.equal(untextured.triangles, 12);
              assertProgramCounts(untextured.backend, 2, 1);
              assert.ok(declarationDifference.changedFraction > 0.03);
              assert.ok(
                (untexturedEvidence.counts[0] ?? 0) >
                  untexturedEvidence.foregroundPixels * 0.98,
              );
            },
          );

          const restoredInspection = await invoke<WorldInspection>(
            scenario.page,
            moduleUrl,
            "setCubeTextureEnabled",
            [true],
          );
          assert.deepEqual(
            restoredInspection.texture,
            setup.inspection.texture,
          );
          const restored = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "restored-checker-declaration",
          );
          const restoredDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["builtin-checker-cube", "restored-checker-declaration"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "restored-checker-declaration",
            "builtin-checker-cube",
            () => {
              assert.equal(restoredDifference.changedPixels, 0);
              assertProgramCounts(restored.backend, 3, 1);
            },
          );

          const optionalSetup = await invoke<OptionalLayoutSetupReport>(
            scenario.page,
            moduleUrl,
            "mountOptionalLayoutScene",
          );
          assert.equal(optionalSetup.resources.length, 5);
          assert.ok(
            optionalSetup.resources.every(({ status }) => status === "loaded"),
          );
          const optional = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "optional-layouts",
          );
          const optionalSamples = await invoke<OptionalLayoutSample[]>(
            scenario.page,
            moduleUrl,
            "sampleOptionalLayouts",
            ["optional-layouts"],
          );
          verifyOptionalLayoutSamples(optionalSamples);
          assert.equal(optional.drawCalls, 4);
          assert.equal(optional.triangles, 14);
          assert.equal(
            backendNumber(optional.backend, "totalUploadedBytes") -
              backendNumber(restored.backend, "totalUploadedBytes"),
            836 + 3 * 2 * 4,
            "optional layouts must upload only present streams, indices, and the demanded texture",
          );
          assertLivePrograms(optional.backend, 3);

          const layoutOrder = await invoke<OptionalLayoutOrderReport>(
            scenario.page,
            moduleUrl,
            "reverseOptionalLayoutScene",
          );
          assert.equal(layoutOrder.before.length, 4);
          assert.deepEqual(
            layoutOrder.after.map(({ entity }) => entity),
            layoutOrder.before.map(({ entity }) => entity),
            "layout reversal must preserve entity slots",
          );
          assert.deepEqual(
            layoutOrder.after.map(({ source }) => source),
            layoutOrder.before.map(({ source }) => source).reverse(),
            "effective mesh order must reverse in the core draw list",
          );
          const optionalReversed = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "optional-layouts-reversed",
          );
          const reversedDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["optional-layouts", "optional-layouts-reversed"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "optional-layouts-reversed",
            "optional-layouts",
            () => {
              assert.equal(reversedDifference.changedPixels, 0);
              assert.equal(
                backendNumber(optionalReversed.backend, "totalUploadedBytes"),
                backendNumber(optional.backend, "totalUploadedBytes"),
              );
              assertProgramCounts(
                optionalReversed.backend,
                backendNumber(optional.backend, "shaderProgramsCreated"),
                3,
              );
            },
          );

          const quadSetup = await invoke<QuadSetupReport>(
            scenario.page,
            moduleUrl,
            "mountSamplerQuad",
          );
          assert.equal(quadSetup.resources.length, 2);
          assert.ok(
            quadSetup.resources.every(({ status }) => status === "loaded"),
          );
          assert.deepEqual(quadSetup.inspection.texture, {
            source: `${assetBaseUrl}/asymmetric.texture`,
            variant: 0,
          });
          const sampler = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "asymmetric-repeat-quad",
          );
          const observations = await invoke<SamplerObservation[]>(
            scenario.page,
            moduleUrl,
            "sampleAsymmetricTexture",
            ["asymmetric-repeat-quad"],
          );
          await verifySamplerImage(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "asymmetric-repeat-quad",
            true,
            () => {
              requireVisible(sampler.summary, "asymmetric sampler quad");
              assert.equal(sampler.drawCalls, 1);
              assert.equal(sampler.triangles, 2);
              assertLivePrograms(sampler.backend, 1);
              assert.equal(observations.length, 24);
              for (const observation of observations) {
                const source = ASYMMETRIC_RGB[observation.sourceIndex];
                assert.ok(
                  source,
                  `missing source texel ${observation.sourceIndex}`,
                );
                const expected = source.map((encoded, channel) =>
                  linearToSrgb8(
                    srgb8ToLinear(encoded) *
                      (QUAD_VERTEX_COLOR[channel] ?? 0) *
                      (QUAD_MATERIAL[channel] ?? 0),
                  ),
                );
                for (let channel = 0; channel < 3; channel += 1) {
                  assert.ok(
                    Math.abs(
                      (observation.rgba[channel] ?? 0) -
                        (expected[channel] ?? 0),
                    ) <= 3,
                    `UV ${observation.uv.join(",")} channel ${channel} expected ${expected[channel]} from source ${source[channel]}, got ${observation.rgba[channel]} at ${observation.coordinate.join(",")}`,
                  );
                }
                assert.equal(
                  observation.rgba[3],
                  255,
                  "Opaque material must still produce an opaque RGBA8 capture",
                );
              }
              for (let index = 0; index < 12; index += 1) {
                assert.deepEqual(
                  observations[index]?.rgba,
                  observations[index + 12]?.rgba,
                  `vertical repeat sample ${index}`,
                );
              }
              for (const row of [0, 6, 12, 18]) {
                for (let column = 0; column < 3; column += 1) {
                  assert.deepEqual(
                    observations[row + column]?.rgba,
                    observations[row + column + 3]?.rgba,
                    `horizontal repeat row ${row / 6}, column ${column}`,
                  );
                }
              }
            },
          );

          const beforeRejection = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "before-texture-rejections",
          );
          const rejection = await invoke<RejectionReport>(
            scenario.page,
            moduleUrl,
            "rejectTextureMutations",
          );
          assert.equal(rejection.malformed.status, "failed");
          assert.match(receiptError(rejection.malformed), /InvalidAsset/);
          assert.equal(rejection.duplicate.status, "failed");
          assert.match(
            receiptError(rejection.duplicate),
            /already registered|DuplicateAsset/,
          );
          assert.equal(rejection.weightWithoutUv.status, "failed");
          assert.match(receiptError(rejection.weightWithoutUv), /InvalidAsset/);
          assert.equal(rejection.texturedWithoutUv, "InvalidAsset");
          assert.equal(rejection.failedSource.status, "failed");
          assert.equal(
            rejection.failedSource.source,
            `${assetBaseUrl}/legacy-v1.texture`,
          );
          assert.match(
            rejection.failedSource.error ?? "",
            /InvalidAsset|texture|IPPT/i,
          );
          assert.deepEqual(consoleErrors, [
            `IPP 2 resource failed: ${rejection.malformed.source}: ${receiptError(rejection.malformed)}`,
            `IPP 1 resource failed: ${rejection.weightWithoutUv.source}: ${receiptError(rejection.weightWithoutUv)}`,
            `IPP 2 resource failed: ${assetBaseUrl}/legacy-v1.texture: ${rejection.failedSource.error}`,
          ]);
          assert.deepEqual(rejection.after, rejection.before);
          const afterRejection = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "after-texture-rejections",
          );
          const rejectionDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["before-texture-rejections", "after-texture-rejections"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "after-texture-rejections",
            "before-texture-rejections",
            () => {
              assert.equal(rejectionDifference.changedPixels, 0);
              assert.deepEqual(afterRejection.summary, beforeRejection.summary);
            },
          );

          const texturedPlaneInspection = await invoke<WorldInspection>(
            scenario.page,
            moduleUrl,
            "setPlaneTextureEnabled",
            [true],
          );
          assert.deepEqual(texturedPlaneInspection.texture, {
            source: `${assetBaseUrl}/asymmetric.texture`,
            variant: 0,
          });
          const texturedPlane = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "weighted-plane-asymmetric",
          );
          const texturedPlaneColors = expectedTexturedPlaneColors();
          const texturedPlaneEvidence = await invoke<ColorEvidence>(
            scenario.page,
            moduleUrl,
            "analyzeCapturedColors",
            ["weighted-plane-asymmetric", texturedPlaneColors, 4],
          );
          requireVisible(texturedPlane.summary, "weighted asymmetric plane");
          assert.equal(texturedPlane.drawCalls, 1);
          assert.equal(texturedPlane.triangles, 50);
          for (const [index, count] of texturedPlaneEvidence.counts.entries()) {
            assert.ok(
              count > 75,
              `weighted plane color ${index} is not visible`,
            );
          }
          assert.ok(
            texturedPlaneEvidence.unmatchedForegroundPixels <
              texturedPlaneEvidence.foregroundPixels * 0.05,
          );
          assertLivePrograms(texturedPlane.backend, 1);

          const solidPlaneInspection = await invoke<WorldInspection>(
            scenario.page,
            moduleUrl,
            "setPlaneTextureEnabled",
            [false],
          );
          assert.equal(solidPlaneInspection.texture, null);
          const solidPlane = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "weighted-plane-without-texture",
          );
          const solidPlaneEvidence = await invoke<ColorEvidence>(
            scenario.page,
            moduleUrl,
            "analyzeCapturedColors",
            ["weighted-plane-without-texture", expectedSolidPlaneColors(), 4],
          );
          assert.ok((solidPlaneEvidence.counts[0] ?? 0) > 1_000);
          assert.ok((solidPlaneEvidence.counts[1] ?? 0) > 100);
          const planeRemovalDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["weighted-plane-asymmetric", "weighted-plane-without-texture"],
          );
          assert.ok(planeRemovalDifference.changedFraction > 0.01);
          assertLivePrograms(solidPlane.backend, 1);

          await invoke(scenario.page, moduleUrl, "setPlaneTextureEnabled", [
            true,
          ]);
          const restoredPlane = await capture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "weighted-plane-restored",
          );
          const restoredPlaneDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareCaptured",
            ["weighted-plane-asymmetric", "weighted-plane-restored"],
          );
          assert.equal(restoredPlaneDifference.changedPixels, 0);
          assertLivePrograms(restoredPlane.backend, 1);

          const recovery = await invoke<RecoveryReport>(
            scenario.page,
            moduleUrl,
            "recoverTextureContext",
            ["weighted-plane-restored"],
          );
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
            ["weighted-plane-restored", "after-context-restore"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "after-context-restore",
            "weighted-plane-restored",
            () => {
              assert.ok(
                recovery.after.contextGeneration > recovery.beforeGeneration,
              );
              assert.equal(recoveryDifference.changedPixels, 0);
              assert.equal(
                recovery.resourceCountAfter,
                recovery.resourceCountBefore,
                "context restore preserves resource objects",
              );
              assert.equal(
                backendNumber(recovery.after.backend, "totalUploadedBytes") -
                  backendNumber(restoredPlane.backend, "totalUploadedBytes"),
                3_405 + 2 * 3 * 2 * 4,
                "weighted plane streams and retained producer texture must be reuploaded",
              );
              assert.ok(
                ingressNumber(recovery.after.backend, "sourceBytes") >
                  ingressNumber(restoredPlane.backend, "sourceBytes"),
                "source reload transfers the immutable content again",
              );
              assertProgramCounts(
                recovery.after.backend,
                backendNumber(restoredPlane.backend, "shaderProgramsCreated") +
                  1,
                1,
              );
            },
          );

          return {
            session: sampler.session,
            firstTick: checker.tick,
            finalTick: recovery.after.tick,
            captures: [...captured],
            resourceCount: recovery.resourceCountAfter,
          };
        } catch (error) {
          scenarioFailure = error;
          throw error;
        } finally {
          try {
            await invoke(scenario.page, moduleUrl, "closeTextures");
          } catch (cleanupError) {
            if (scenarioFailure !== undefined) {
              throw new AggregateError(
                [scenarioFailure, cleanupError],
                "texture scenario and fixture cleanup failed",
              );
            }
            throw cleanupError;
          }
        }
      },
    );
    assert.ok(result.value.session > 0n);
    assert.ok(result.value.finalTick > result.value.firstTick);
    assert.equal(result.value.captures.length, 12);
    assert.equal(result.value.resourceCount, 5);
    assert.deepEqual(pageErrors, []);
    // The two retained malformed producer assets report failure again during
    // explicit context recovery; the released legacy source does not return.
    assert.equal(consoleErrors.length, 5);
    assert.equal(new Set(consoleErrors).size, 3);
    await assertLoopbackClosed(result.origin);
  });
}

test("changed HTTP content cannot refresh an unloaded resource", {
  timeout: 30_000,
}, async (context) => {
  const result = await runBrowserEnvironment(
    "immutable texture recovery failure",
    {
      workspace,
      build: render,
      mismatchBuild: overlays,
      operationTimeoutMs: 12_000,
      evidenceParent: resolve(
        workspace,
        "target/integration-artifacts/textures",
      ),
    },
    context.signal,
    async (scenario) => {
      const moduleUrl = `${scenario.urls.origin}/target/texture-build/fixture-production.js`;
      try {
        await invoke(scenario.page, moduleUrl, "initializeTextures", [
          {
            generatedModuleUrl: scenario.urls.generated,
            workerScriptUrl: scenario.urls.workerScript,
            wasmUrl: scenario.urls.wasm,
            timeoutMs: 10_000,
            assetBaseUrl: `${scenario.urls.origin}/target/texture-build`,
          },
        ]);
        let conditionalRead = false;
        await scenario.page.route("**/checker.texture", async (route) => {
          conditionalRead =
            typeof route.request().headers()["if-match"] === "string";
          const response = await route.fetch();
          await route.fulfill({
            response,
            headers: { ...response.headers(), etag: '"changed-content"' },
          });
        });
        const recovery = await invoke<{
          before: { id: bigint };
          after: { id: bigint; status: string; error: string };
          drawCalls: number;
        }>(scenario.page, moduleUrl, "rejectedTextureRecovery");
        assert.ok(
          conditionalRead,
          "recovery must request the original HTTP entity",
        );
        assert.equal(recovery.after.id, recovery.before.id);
        assert.equal(recovery.after.status, "failed");
        assert.match(recovery.after.error, /source changed/);
        assert.equal(recovery.drawCalls, 0);
        await saveCapture(
          scenario.page,
          moduleUrl,
          scenario.evidence.directory,
          new Set(),
          "rejected-recovery",
        );
        return recovery.after.id;
      } finally {
        await invoke(scenario.page, moduleUrl, "closeTextures");
      }
    },
  );
  assert.ok(result.value > 0n);
  await assertLoopbackClosed(result.origin);
});

function verifyOptionalLayoutSamples(
  samples: readonly OptionalLayoutSample[],
): void {
  assert.equal(samples.length, 7);
  const single = (layout: OptionalLayoutSample["layout"]) => {
    const matching = samples.filter((sample) => sample.layout === layout);
    assert.equal(matching.length, 1, `${layout} sample count`);
    return matching[0]!;
  };
  assertRgbaClose(
    single("position").rgba,
    [255, 255, 255, 255],
    2,
    "position-only default color",
  );
  assertRgbaClose(
    single("color").rgba,
    [...LAYOUT_COLOR.map(linearToSrgb8), 255],
    3,
    "explicit color without UV",
  );
  assertRgbaClose(
    single("uv").rgba,
    [...ASYMMETRIC_RGB[0], 255],
    3,
    "UV-only defaults to white color and full texture contribution",
  );

  const weighted = samples.filter((sample) => sample.layout === "weight");
  assert.deepEqual(
    weighted.map(({ weightByte }) => weightByte),
    [0, 85, 170, 255],
  );
  for (const sample of weighted) {
    const weightByte = sample.weightByte;
    assert.notEqual(weightByte, null);
    const weight = (weightByte ?? 0) / 255;
    const expected = ASYMMETRIC_RGB[0].map((encoded, channel) =>
      linearToSrgb8(
        (WEIGHT_COLOR[channel] ?? 0) *
          (1 - weight + srgb8ToLinear(encoded) * weight),
      ),
    );
    assertRgbaClose(
      sample.rgba,
      [...expected, 255],
      3,
      `normalized texture weight ${weightByte}`,
    );
  }
}

function expectedTexturedPlaneColors(): readonly (readonly [
  number,
  number,
  number,
])[] {
  return [
    ...ASYMMETRIC_RGB.map(
      (rgb) =>
        rgb.map((encoded, channel) =>
          linearToSrgb8(
            srgb8ToLinear(encoded) * 0.25 * (PLANE_MATERIAL[channel] ?? 0),
          ),
        ) as [number, number, number],
    ),
    PLANE_MATERIAL.map(linearToSrgb8) as [number, number, number],
  ];
}

function expectedSolidPlaneColors(): readonly (readonly [
  number,
  number,
  number,
])[] {
  return [
    PLANE_MATERIAL.map((factor) => linearToSrgb8(factor * 0.25)) as [
      number,
      number,
      number,
    ],
    PLANE_MATERIAL.map(linearToSrgb8) as [number, number, number],
  ];
}

function assertRgbaClose(
  actual: readonly number[],
  expected: readonly number[],
  tolerance: number,
  label: string,
): void {
  for (let channel = 0; channel < 4; channel += 1) {
    assert.ok(
      Math.abs((actual[channel] ?? 0) - (expected[channel] ?? 0)) <= tolerance,
      `${label} channel ${channel}: expected ${expected[channel]}, got ${actual[channel]}`,
    );
  }
}

// Private recipes follow current World demand. Replacing the scene releases
// unused programs; only unchanged demanded recipes promise compilation reuse.
function assertLivePrograms(
  backend: Readonly<Record<string, unknown>>,
  live: number,
): void {
  assert.equal(backendNumber(backend, "shaderProgramsLive"), live);
}

function assertProgramCounts(
  backend: Readonly<Record<string, unknown>>,
  created: number,
  live: number,
): void {
  assert.equal(backendNumber(backend, "shaderProgramsCreated"), created);
  assert.equal(backendNumber(backend, "shaderProgramsLive"), live);
}

function browserBuild(name: "render" | "headless"): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    name,
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
  const report = await invoke<CaptureReport>(
    page,
    moduleUrl,
    "captureTextureFrame",
    [label],
  );
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
    canvasSelector: "#ipp-texture-canvas",
  });
}

async function verifySamplerImage(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  actual: string,
  exactSampler: boolean,
  verify: () => void,
): Promise<void> {
  try {
    verify();
  } catch (error) {
    if (exactSampler) {
      const [actualImage, expectedImage, differenceImage] = await Promise.all([
        invoke<string>(page, moduleUrl, "captureDataUrl", [actual]),
        invoke<string>(page, moduleUrl, "samplerExpectedDataUrl", [actual]),
        invoke<string>(page, moduleUrl, "samplerDifferenceDataUrl", [actual]),
      ]);
      await writeFailureImages(
        evidenceDirectory,
        actualImage,
        expectedImage,
        differenceImage,
      );
    }
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

function srgb8ToLinear(value: number): number {
  const encoded = value / 255;
  return encoded <= 0.04045
    ? encoded / 12.92
    : ((encoded + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb8(value: number): number {
  const encoded =
    value <= 0.003_130_8 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, encoded)) * 255);
}

function backendNumber(
  backend: Readonly<Record<string, unknown>>,
  field: string,
): number {
  const value = backend[field];
  if (typeof value !== "number") {
    throw new Error(`capture backend.${field} is not numeric`);
  }
  return value;
}

function ingressNumber(
  backend: Readonly<Record<string, unknown>>,
  field: string,
): number {
  const ingress = backend.ingress;
  if (!ingress || typeof ingress !== "object") {
    throw new Error("capture backend omitted ingress counters");
  }
  return backendNumber(ingress as Readonly<Record<string, unknown>>, field);
}

function receiptError(receipt: {
  readonly status: string;
  readonly error?: string;
}): string {
  return receipt.status === "loaded"
    ? "success"
    : (receipt.error ?? "missing error");
}

function recordConsoleError(message: ConsoleMessage, errors: string[]): void {
  if (message.type() === "error") errors.push(message.text());
}

interface ColorEvidence {
  readonly counts: readonly number[];
  readonly foregroundPixels: number;
  readonly unmatchedForegroundPixels: number;
  readonly adjacentTransitions: number;
}

interface ImageDifference {
  readonly changedPixels: number;
  readonly changedFraction: number;
  readonly meanAbsoluteChannelDifference: number;
}

interface QuadSetupReport {
  readonly resources: readonly {
    readonly status: "unloaded" | "start" | "progress" | "loaded" | "failed";
  }[];
  readonly inspection: WorldInspection;
}

interface RecoveryReport {
  readonly beforeGeneration: number;
  readonly after: CaptureReport;
  readonly resourceCountBefore: number;
  readonly resourceCountAfter: number;
}
