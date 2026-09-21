import { invoke, writeDataUrl, bigintJson, recordCapture } from "./evidence.js";
import assert from "node:assert/strict";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import type { ConsoleMessage, Page } from "playwright";
import type { ComponentFieldValue } from "@ipp/client";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  runBrowserEnvironment,
} from "../browser/environment.js";
import { requireVisible } from "./image-assertions.js";
import {
  ARROW_GEOMETRY_CASES,
  type analyzeArrowGeometryCapture,
  CHECKER_SOURCE,
  type InvalidRecipeReport,
  ROTATED_PLANE_Y,
  SHAPE_IDS,
  SHAPES,
  type ShapeCaptureReport,
  type ShapeId,
  type ShapeImageEvidence,
  type ShapeInspection,
  type ShapeSetupReport,
  type analyzePlanePointerCapture,
} from "./shapes-fixture.js";

const workspace = resolve(process.cwd());
const render = browserBuild("render");
const overlays = browserBuild("headless");
const EXPECTED_GEOMETRY: Readonly<
  Record<
    ShapeId,
    {
      readonly vertices: number;
      readonly triangles: number;
      readonly sourceBytes?: number;
      readonly vertexBytes?: number;
    }
  >
> = Object.freeze({
  cube: { vertices: 24, triangles: 12 },
  sphere: { vertices: 559, triangles: 960 },
  pill: { vertices: 592, triangles: 1_024 },
  cone: { vertices: 99, triangles: 64 },
  "cone-outline": { vertices: 746, triangles: 1152 },
  "cone-outline-rings": { vertices: 1340, triangles: 2176 },
  plane: {
    vertices: 69,
    triangles: 50,
    sourceBytes: 3_465,
    vertexBytes: 3_105,
  },
  "cube-outline": { vertices: 456, triangles: 384 },
  "sphere-outline": { vertices: 1_755, triangles: 3_072 },
  "pill-outline": { vertices: 2_376, triangles: 4_160 },
  "plane-outline": { vertices: 217, triangles: 176, sourceBytes: 10_656 },
});

for (const variant of ["development", "production"] as const) {
  test(`${variant}: built-in solids and contours survive the real React WASM WebGL pipeline`, {
    timeout: 90_000,
  }, async (context) => {
    const pageErrors: string[] = [];
    const consoleErrors: string[] = [];
    const result = await runBrowserEnvironment(
      `${variant} React WASM built-in shapes`,
      {
        workspace,
        build: render,
        mismatchBuild: overlays,
        operationTimeoutMs: 15_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/shapes",
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
        const moduleUrl = `${scenario.urls.origin}/target/shapes-build/${
          variant === "production" ? "fixture-production.js" : "fixture.js"
        }`;
        const captured = new Set<string>();
        let scenarioFailure: unknown;
        try {
          const setup = await invoke<ShapeSetupReport>(
            scenario.page,
            moduleUrl,
            "initializeShapes",
            [
              {
                generatedModuleUrl: scenario.urls.generated,
                workerScriptUrl: scenario.urls.workerScript,
                wasmUrl: scenario.urls.wasm,
                timeoutMs: 12_000,
              },
            ],
          );
          assert.equal(setup.resources.length, 2);
          assert.deepEqual(
            setup.resources.map(({ source, status }) => ({ source, status })),
            [
              { source: SHAPES.cube.recipe, status: "loaded" },
              { source: CHECKER_SOURCE, status: "loaded" },
            ],
          );

          const originalCaptures = new Map<ShapeId, ShapeCaptureReport>();
          const originalEvidence = new Map<ShapeId, ShapeImageEvidence>();
          for (const shape of SHAPE_IDS) {
            const inspection = await invoke<ShapeInspection>(
              scenario.page,
              moduleUrl,
              "selectShape",
              [shape],
            );
            verifyInspection(inspection, shape);
            const capture = await captureShape(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              captured,
              `original-${shape}`,
            );
            originalCaptures.set(shape, capture);
            assert.equal(
              backendNumber(capture.backend, "shaderProgramsLive"),
              1,
              "Only the selected shape recipe remains resident",
            );
            const unchanged = await captureShape(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              captured,
              `unchanged-${shape}`,
            );
            assert.equal(
              backendNumber(unchanged.backend, "shaderProgramsCreated"),
              backendNumber(capture.backend, "shaderProgramsCreated"),
              "Unchanged draws must reuse their resident program",
            );
            const evidence = await invoke<ShapeImageEvidence>(
              scenario.page,
              moduleUrl,
              "analyzeShapeCapture",
              [`original-${shape}`, shape],
            );
            originalEvidence.set(shape, evidence);
            await writeFile(
              join(
                scenario.evidence.directory,
                `original-${shape}-oracle.json`,
              ),
              `${JSON.stringify(
                {
                  recipe: SHAPES[shape].recipe,
                  expectedGeometry: EXPECTED_GEOMETRY[shape],
                  material: SHAPES[shape].material,
                  evidence,
                },
                null,
                2,
              )}\n`,
            );
            await verifyShapeImage(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              `original-${shape}`,
              shape,
              capture,
              evidence,
            );
          }

          const rotatedInspection = await invoke<ShapeInspection>(
            scenario.page,
            moduleUrl,
            "selectRotatedPlane",
          );
          verifyInspection(rotatedInspection, "plane");
          verifyRotatedPlaneInspection(rotatedInspection);
          const rotatedPlane = await captureShape(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "rotated-plane",
          );
          const rotatedEvidence = await invoke<ShapeImageEvidence>(
            scenario.page,
            moduleUrl,
            "analyzeShapeCapture",
            ["rotated-plane", "plane"],
          );
          await writeFile(
            join(scenario.evidence.directory, "rotated-plane-oracle.json"),
            `${JSON.stringify(
              {
                recipe: SHAPES.plane.recipe,
                rotationY: ROTATED_PLANE_Y,
                transform: rotatedInspection.transform,
                evidence: rotatedEvidence,
              },
              bigintJson,
              2,
            )}\n`,
          );
          await verifyShapeImage(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "rotated-plane",
            "plane",
            rotatedPlane,
            rotatedEvidence,
          );
          verifyPlaneDirections(
            requiredMapValue(originalEvidence, "plane"),
            rotatedEvidence,
          );

          for (const [length, offset] of [
            [1.25, 0],
            [1.25, 0.4],
            [0.75, 0.4],
          ] as const) {
            const label = `plane-pointer-${length}-${offset}`;
            const inspection = await invoke<ShapeInspection>(
              scenario.page,
              moduleUrl,
              "selectPlanePointer",
              [length, offset],
            );
            verifyInspection(
              inspection,
              "plane",
              `ipp://mesh/plane?size=2&normalLength=${length}&stroke=0.05&normalOffset=${offset}`,
            );
            await captureShape(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              captured,
              label,
            );
            const pointer = await invoke<
              ReturnType<typeof analyzePlanePointerCapture>
            >(scenario.page, moduleUrl, "analyzePlanePointerCapture", [
              label,
              length,
              offset,
            ]);
            await writeFile(
              join(scenario.evidence.directory, `${label}-oracle.json`),
              JSON.stringify(pointer, null, 2),
            );
            assert.ok(
              pointer.bounds && pointer.expectedBounds,
              `${label} has a visible pointer`,
            );
            assertBoundsClose(pointer.bounds, pointer.expectedBounds, 2, label);
            if (offset > 0)
              assert.equal(pointer.gapWhitePixels, 0, `${label} leaves a gap`);
          }

          await invoke(scenario.page, moduleUrl, "selectShape", [
            "plane-outline",
          ]);

          const beforeInvalid = await captureShape(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "before-invalid-recipes",
          );
          const invalid = await invoke<InvalidRecipeReport>(
            scenario.page,
            moduleUrl,
            "rejectInvalidShapeRecipes",
          );
          assert.equal(invalid.outcomes.length, 16);
          assert.equal(invalid.failedResources.length, 16);
          for (const resource of invalid.failedResources) {
            assert.equal(resource.status, "failed");
            assert.ok(resource.error);
          }
          assert.deepEqual(
            [...consoleErrors].sort(),
            invalid.failedResources
              .map(
                ({ source, error }) =>
                  `IPP 1 resource failed: ${source}: ${error ?? "unknown failure"}`,
              )
              .sort(),
          );
          assert.deepEqual(invalid.after, invalid.before);
          assert.equal(invalid.resourcesAfterCleanup, 2);
          const afterInvalid = await captureShape(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "after-invalid-recipes",
          );
          const invalidDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareShapeCaptures",
            ["before-invalid-recipes", "after-invalid-recipes"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "after-invalid-recipes",
            "before-invalid-recipes",
            () => {
              assert.equal(invalidDifference.changedPixels, 0);
              assert.deepEqual(afterInvalid.summary, beforeInvalid.summary);
              assert.equal(
                afterInvalid.triangles,
                EXPECTED_GEOMETRY["plane-outline"].triangles,
              );
            },
          );

          const transferredBeforeRecovery = ingressNumber(
            afterInvalid.backend,
            "transferredAssetBytes",
          );
          const recovery = await invoke<RecoveryReport>(
            scenario.page,
            moduleUrl,
            "recoverShapeContext",
            ["after-invalid-recipes"],
          );
          await saveCapture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "after-context-restore",
          );
          const restoredDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareShapeCaptures",
            ["after-invalid-recipes", "after-context-restore"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "after-context-restore",
            "after-invalid-recipes",
            () => {
              assert.ok(
                recovery.after.contextGeneration > recovery.beforeGeneration,
              );
              assert.equal(restoredDifference.changedPixels, 0);
              assert.equal(
                recovery.resourceCountAfter,
                recovery.resourceCountBefore,
              );
              assert.equal(
                ingressNumber(recovery.after.backend, "transferredAssetBytes"),
                transferredBeforeRecovery,
                "built-in recovery must not require producer transfers",
              );
              assert.ok(
                backendNumber(recovery.after.backend, "totalUploadedBytes") >
                  backendNumber(afterInvalid.backend, "totalUploadedBytes"),
                "context recovery must rebuild visible GPU resources",
              );
              assert.equal(
                backendNumber(recovery.after.backend, "shaderProgramsCreated"),
                backendNumber(afterInvalid.backend, "shaderProgramsCreated") +
                  1,
              );
              assert.equal(
                backendNumber(recovery.after.backend, "shaderProgramsLive"),
                1,
              );
            },
          );

          for (const shape of SHAPE_IDS) {
            await invoke(scenario.page, moduleUrl, "selectShape", [shape]);
            const recovered = await captureShape(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              captured,
              `recovered-${shape}`,
            );
            const difference = await invoke<ImageDifference>(
              scenario.page,
              moduleUrl,
              "compareShapeCaptures",
              [`original-${shape}`, `recovered-${shape}`],
            );
            await verifyPair(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              `recovered-${shape}`,
              `original-${shape}`,
              () => {
                assert.equal(
                  difference.changedPixels,
                  0,
                  `${shape} changed after recovery`,
                );
                assert.deepEqual(
                  recovered.summary,
                  originalCaptures.get(shape)?.summary,
                );
                assert.equal(recovered.resourceCount, 2);
              },
            );
          }

          for (const [index, definition] of ARROW_GEOMETRY_CASES.entries()) {
            const inspection = await invoke<ShapeInspection>(
              scenario.page,
              moduleUrl,
              "selectArrowGeometry",
              [index],
            );
            assert.equal(inspection.selected, definition.shape);
            assert.equal(inspection.entityExists, true);
            assert.deepEqual(inspection.mesh, {
              source: definition.source,
              variant: 0,
            });
            assert.equal(inspection.texture, null);
            const capture = await captureShape(
              scenario.page,
              moduleUrl,
              scenario.evidence.directory,
              captured,
              definition.label,
            );
            assert.equal(capture.drawCalls, 1);
            assert.equal(capture.triangles, definition.axes.length * 48);
            assert.equal(capture.resourceCount, 1);
            const oracle = await invoke<
              ReturnType<typeof analyzeArrowGeometryCapture>
            >(scenario.page, moduleUrl, "analyzeArrowGeometryCapture", [
              definition.label,
              index,
            ]);
            await scenario.evidence.writeJson(
              `${definition.label}-oracle.json`,
              oracle,
            );
            assert.ok(
              oracle.unmatched < 10,
              `${definition.label} uses only authored colors`,
            );
            for (const axis of oracle.axes) {
              assert.ok(
                axis.pixels > 25,
                `${definition.label} axis ${axis.axis} is visible`,
              );
              assert.ok(
                axis.hits / axis.samples > 0.9,
                `${definition.label} axis ${axis.axis} follows its projected direction`,
              );
              assert.ok(axis.bounds && axis.expectedBounds);
              assertBoundsClose(
                axis.bounds,
                axis.expectedBounds,
                3,
                `${definition.label} axis ${axis.axis}`,
              );
            }
          }
          const colorDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareShapeCaptures",
            ["axis-default", "axis-custom"],
          );
          assert.ok(
            colorDifference.changedPixels > 100,
            "color-only source replacement changes rendered vertices",
          );
          const axisRecovery = await invoke<RecoveryReport>(
            scenario.page,
            moduleUrl,
            "recoverShapeContext",
            ["axis-partial", "axis-restored"],
          );
          await saveCapture(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            captured,
            "axis-restored",
          );
          const axisDifference = await invoke<ImageDifference>(
            scenario.page,
            moduleUrl,
            "compareShapeCaptures",
            ["axis-partial", "axis-restored"],
          );
          await verifyPair(
            scenario.page,
            moduleUrl,
            scenario.evidence.directory,
            "axis-restored",
            "axis-partial",
            () => {
              assert.equal(axisDifference.changedPixels, 0);
              assert.ok(
                axisRecovery.after.contextGeneration >
                  axisRecovery.beforeGeneration,
              );
              assert.equal(
                axisRecovery.resourceCountBefore,
                axisRecovery.resourceCountAfter,
              );
              assert.equal(axisRecovery.after.triangles, 144);
            },
          );

          await scenario.evidence.writeJson("shape-scenario.json", {
            variant,
            invalidRecipeOutcomes: invalid.outcomes,
            contextGeneration: {
              before: recovery.beforeGeneration,
              after: recovery.after.contextGeneration,
            },
            axisContextGeneration: {
              before: axisRecovery.beforeGeneration,
              after: axisRecovery.after.contextGeneration,
            },
            resourceCount: axisRecovery.resourceCountAfter,
            captures: [...captured],
          });

          return {
            session: beforeInvalid.session,
            firstTick: originalCaptures.get("cube")?.tick ?? 0n,
            finalTick: axisRecovery.after.tick,
            captures: [...captured],
            resourceCount: axisRecovery.resourceCountAfter,
          };
        } catch (error) {
          scenarioFailure = error;
          throw error;
        } finally {
          try {
            await invoke(scenario.page, moduleUrl, "closeShapes");
          } catch (cleanupError) {
            if (scenarioFailure !== undefined) {
              throw new AggregateError(
                [scenarioFailure, cleanupError],
                "shape scenario and fixture cleanup failed",
              );
            }
            throw cleanupError;
          }
        }
      },
    );
    assert.ok(result.value.session > 0n);
    assert.ok(result.value.finalTick > result.value.firstTick);
    assert.equal(
      result.value.captures.length,
      SHAPE_IDS.length * 3 + 8 + ARROW_GEOMETRY_CASES.length,
    );
    assert.deepEqual(pageErrors, []);
    assert.equal(consoleErrors.length, 16);
    assert.equal(result.value.resourceCount, 1);
    await assertLoopbackClosed(result.origin);
  });
}

function verifyInspection(
  inspection: ShapeInspection,
  shape: ShapeId,
  source = SHAPES[shape].recipe,
): void {
  assert.equal(inspection.selected, shape);
  assert.equal(inspection.entityExists, true);
  assert.deepEqual(inspection.mesh, {
    source,
    variant: 0,
  });
  assert.deepEqual(inspection.texture, {
    source: CHECKER_SOURCE,
    variant: 0,
  });
  const material = inspection.material;
  assert.ok(material);
  for (const [field, expected] of [
    ["r", SHAPES[shape].material[0]],
    ["g", SHAPES[shape].material[1]],
    ["b", SHAPES[shape].material[2]],
  ] as const) {
    const actual: ComponentFieldValue | undefined = material[field];
    assert.equal(typeof actual, "number");
    assert.ok(
      Math.abs(Number(actual) - expected) < 1e-6,
      `${shape} material ${field}`,
    );
  }
}

function verifyRotatedPlaneInspection(inspection: ShapeInspection): void {
  const transform = inspection.transform;
  assert.ok(transform, "rotated plane is missing its effective Transform");
  for (const [field, expected] of [
    ["qy", Math.sin(ROTATED_PLANE_Y / 2)],
    ["qw", Math.cos(ROTATED_PLANE_Y / 2)],
  ] as const) {
    const actual: ComponentFieldValue | undefined = transform[field];
    assert.equal(typeof actual, "number");
    assert.ok(
      Math.abs(Number(actual) - expected) < 1e-6,
      `rotated plane Transform.${field}`,
    );
  }
}

function verifyPlaneDirections(
  original: ShapeImageEvidence,
  rotated: ShapeImageEvidence,
): void {
  assert.ok(original.normalOrigin && original.normalTip);
  assert.ok(rotated.normalOrigin && rotated.normalTip);
  const originalDirection = subtractPoint(
    original.normalTip,
    original.normalOrigin,
  );
  const rotatedDirection = subtractPoint(
    rotated.normalTip,
    rotated.normalOrigin,
  );
  assert.ok(
    originalDirection[0] < -25 && originalDirection[1] > 12,
    `+Z normal projected in unexpected direction ${originalDirection.join(",")}`,
  );
  assert.ok(
    rotatedDirection[0] > 12 && rotatedDirection[1] > 15,
    `React-rotated normal did not follow +Y quaternion ${rotatedDirection.join(",")}`,
  );
}

async function verifyShapeImage(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  label: string,
  shape: ShapeId,
  capture: ShapeCaptureReport,
  evidence: ShapeImageEvidence,
): Promise<void> {
  try {
    assert.equal(capture.shape, shape);
    assert.equal(capture.drawCalls, 1);
    assert.equal(capture.triangles, EXPECTED_GEOMETRY[shape].triangles);
    assert.ok(capture.summary.bounds, `${shape} has no rendered bounds`);
    assert.ok(evidence.expectedBounds, `${shape} has no analytic bounds`);
    assert.ok(
      evidence.foregroundPixels > 250,
      `${shape} has too little visible geometry`,
    );

    if (SHAPES[shape].base === "plane") {
      verifyPlaneImage(shape, capture, evidence);
      return;
    }

    for (const [index, count] of evidence.checkerCounts.entries()) {
      assert.ok(
        count > 10,
        `${shape} does not visibly sample checker class ${index}`,
      );
    }
    assert.ok(
      evidence.unmatchedForegroundPixels / evidence.foregroundPixels < 0.035,
      `${shape} foreground does not match its material-modulated checker`,
    );

    if (!SHAPES[shape].outline) {
      requireVisible(capture.summary, shape);
      assert.ok(evidence.analyticInteriorPixels > 500);
      assert.ok(
        evidence.analyticInteriorForegroundPixels /
          evidence.analyticInteriorPixels >
          0.985,
        `${shape} leaves holes inside its analytic solid projection`,
      );
      assert.ok(evidence.analyticExteriorPixels > 20_000);
      assert.ok(
        evidence.analyticExteriorForegroundPixels /
          evidence.analyticExteriorPixels <
          0.001,
        `${shape} extends beyond its analytic projection`,
      );
      assertBoundsClose(
        capture.summary.bounds,
        evidence.expectedBounds,
        3,
        shape,
      );
      if (SHAPES[shape].base !== "cube") {
        assert.ok(evidence.longitudeSeamSamples > 20);
        assert.ok(
          evidence.longitudeSeamSampleHits / evidence.longitudeSeamSamples >
            0.97,
          `${shape} has visible holes along its duplicated non-pole longitude seam`,
        );
      }
    } else {
      assert.ok(
        capture.summary.coverage > 0.003 && capture.summary.coverage < 0.15,
      );
      assert.ok(evidence.analyticInteriorPixels > 500);
      assert.ok(
        evidence.analyticInteriorForegroundPixels /
          evidence.analyticInteriorPixels <
          0.012,
        `${shape} closes its intended open interior`,
      );
      assert.ok(evidence.contourSamples > 100);
      assert.ok(
        evidence.contourSampleHits / evidence.contourSamples > 0.88,
        `${shape} misses characteristic projected centerlines`,
      );
      assert.ok(
        evidence.foregroundNearContours / evidence.foregroundPixels > 0.94,
        `${shape} draws pixels outside the 2.5px characteristic-contour corridor`,
      );
    }
  } catch (error) {
    const [actual, expected, difference] = await Promise.all([
      invoke<string>(page, moduleUrl, "shapeCaptureDataUrl", [label]),
      invoke<string>(page, moduleUrl, "shapeOracleDataUrl", [label, shape]),
      invoke<string>(page, moduleUrl, "shapeOracleDifferenceDataUrl", [
        label,
        shape,
      ]),
    ]);
    await writeFailureImages(evidenceDirectory, actual, expected, difference);
    throw error;
  }
}

function verifyPlaneImage(
  shape: ShapeId,
  capture: ShapeCaptureReport,
  evidence: ShapeImageEvidence,
): void {
  assert.ok(evidence.normalOrigin && evidence.normalTip);
  assert.ok(evidence.normalSamples > 10);
  assert.ok(
    evidence.normalSampleHits / evidence.normalSamples > 0.82,
    `${shape} misses its analytically projected normal arrow`,
  );
  assert.ok(evidence.planeSurfaceExpectedPixels > 5_000);
  assert.ok(capture.summary.bounds && evidence.expectedBounds);
  assertBoundsClose(capture.summary.bounds, evidence.expectedBounds, 4, shape);

  if (shape === "plane") {
    assert.ok(evidence.expectedArrowBounds && evidence.actualArrowBounds);
    assertBoundsClose(
      evidence.actualArrowBounds,
      evidence.expectedArrowBounds,
      5,
      `${shape} arrow`,
    );
    for (const [index, count] of evidence.planeSurfaceColorCounts.entries()) {
      assert.ok(
        count > 100,
        `plane does not preserve checker class ${index} through its linear RGB 0.25 surface`,
      );
    }
    assert.ok(
      evidence.planeSurfaceMatchedPixels / evidence.planeSurfaceExpectedPixels >
        0.965,
      "plane does not match its checker square and solid white arrow",
    );
    assert.ok(
      evidence.solidArrowPixels > 100,
      "plane normal does not remain solid white under the checker texture",
    );
    return;
  }

  assert.equal(shape, "plane-outline");
  assert.ok(
    capture.summary.coverage > 0.003 && capture.summary.coverage < 0.08,
  );
  assert.ok(evidence.planePerimeterSamples > 100);
  assert.ok(
    evidence.planePerimeterSampleHits / evidence.planePerimeterSamples > 0.9,
    "plane outline misses its four expected perimeter edges",
  );
  assert.ok(evidence.emptyProbePixels > 50);
  assert.ok(
    evidence.emptyProbeForegroundPixels / evidence.emptyProbePixels < 0.02,
    "plane outline fills an analytically selected empty interior region",
  );
  assert.ok(
    evidence.checkerCounts.every((count) => count > 10),
    "plane outline does not visibly retain all four checker colors",
  );
  assert.ok(
    evidence.unmatchedForegroundPixels / evidence.foregroundPixels < 0.04,
    "plane outline contains colors outside its white textured geometry",
  );
}

function subtractPoint(
  a: readonly [number, number],
  b: readonly [number, number],
): readonly [number, number] {
  return [a[0] - b[0], a[1] - b[1]];
}

function requiredMapValue<K, V>(map: ReadonlyMap<K, V>, key: K): V {
  const value = map.get(key);
  if (value === undefined)
    throw new Error(`missing map value for ${String(key)}`);
  return value;
}

function assertBoundsClose(
  actual: NonNullable<ShapeCaptureReport["summary"]["bounds"]>,
  expected: NonNullable<ShapeImageEvidence["expectedBounds"]>,
  tolerance: number,
  label: string,
): void {
  for (const edge of ["left", "top", "right", "bottom"] as const) {
    assert.ok(
      Math.abs(actual[edge] - expected[edge]) <= tolerance,
      `${label} ${edge} bound ${actual[edge]} differs from analytic ${expected[edge]}`,
    );
  }
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

async function captureShape(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  captured: Set<string>,
  label: string,
): Promise<ShapeCaptureReport> {
  const report = await invoke<ShapeCaptureReport>(
    page,
    moduleUrl,
    "captureShapeFrame",
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
    canvasSelector: "#ipp-shapes-canvas",
    dataUrlExport: "shapeCaptureDataUrl",
    metadataExport: "shapeCaptureMetadata",
  });
}

async function verifyPair(
  page: Page,
  moduleUrl: string,
  evidenceDirectory: string,
  actualLabel: string,
  expectedLabel: string,
  verify: () => void,
): Promise<void> {
  try {
    verify();
  } catch (error) {
    const [actual, expected, difference] = await Promise.all([
      invoke<string>(page, moduleUrl, "shapeCaptureDataUrl", [actualLabel]),
      invoke<string>(page, moduleUrl, "shapeCaptureDataUrl", [expectedLabel]),
      invoke<string>(page, moduleUrl, "shapeDifferenceDataUrl", [
        expectedLabel,
        actualLabel,
      ]),
    ]);
    await writeFailureImages(evidenceDirectory, actual, expected, difference);
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

function backendNumber(
  backend: Readonly<Record<string, unknown>>,
  field: string,
): number {
  const value = backend[field];
  if (typeof value !== "number")
    throw new Error(`capture backend.${field} is not numeric`);
  return value;
}

function ingressNumber(
  backend: Readonly<Record<string, unknown>>,
  field: string,
): number {
  const ingress = backend.ingress;
  if (!ingress || typeof ingress !== "object")
    throw new Error("capture omitted ingress counters");
  return backendNumber(ingress as Readonly<Record<string, unknown>>, field);
}

function recordConsoleError(message: ConsoleMessage, errors: string[]): void {
  if (message.type() === "error") errors.push(message.text());
}

interface ImageDifference {
  readonly changedPixels: number;
  readonly changedFraction: number;
  readonly meanAbsoluteChannelDifference: number;
}

interface RecoveryReport {
  readonly beforeGeneration: number;
  readonly after: ShapeCaptureReport;
  readonly resourceCountBefore: number;
  readonly resourceCountAfter: number;
}
