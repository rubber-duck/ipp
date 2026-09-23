import assert from "node:assert/strict";
import { join, resolve } from "node:path";
import test from "node:test";
import type { RenderStatePatch, RenderStateUpdatedEvent } from "@ipp/client";
import {
  runBrowserEnvironment,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";
import { invoke, recordCapture, writeDataUrl } from "./evidence.js";
import { requireBlank, type ImageDifference } from "./image-assertions.js";
import type * as Fixture from "./debug-geometry-fixture.js";

type Observation = Awaited<ReturnType<typeof Fixture.observe>>;
type Capture = Awaited<ReturnType<typeof Fixture.capture>>;
type Declaration = Fixture.DebugDeclaration;

const GLOBAL_COLOR = [0.25, 0.5, 1] as const;
const SHAPES = ["box", "sphere", "pill"] as const;

for (const name of ["render-baseline", "render"] as const) {
  test(`${name}: debug geometry uses private uniform WebGL draws`, {
    timeout: 90_000,
  }, async (context) => {
    const workspace = resolve(process.cwd());
    const directory = resolve(workspace, "target/browser-build", name);
    const build: BrowserBuildConfiguration = {
      name,
      generatedModule: resolve(directory, "generated.js"),
      runtimeWasm: resolve(directory, "runtime.wasm"),
      exportWasm: resolve(directory, "export.wasm"),
      contractArtifact: resolve(directory, "contract.bin"),
    };
    await runBrowserEnvironment(
      `private debug geometry ${name}`,
      {
        workspace,
        build,
        mismatchBuild: build,
        operationTimeoutMs: 20_000,
        closeTimeoutMs: 5_000,
        evidenceParent: resolve(
          workspace,
          "target/integration-artifacts/debug-geometry",
        ),
      },
      context.signal,
      async (environment) => {
        const module = `${environment.urls.origin}/dist/tests/render/debug-geometry-fixture.js`;
        const captured = new Set<string>();
        const call = <T>(operation: string, args: readonly unknown[] = []) =>
          environment.execute(operation, args, () =>
            invoke<T>(environment.page, module, operation, args),
          );
        const capture = async (label: string) => {
          const result = await call<Capture>("capture", [label]);
          await recordCapture(
            environment.page,
            module,
            environment.evidence.directory,
            captured,
            label,
            { canvasSelector: "#debug-geometry-canvas" },
          );
          assertPrivate(result.observation);
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
        const color = async (label: string, expected: readonly number[]) => {
          const result = await call<ReturnType<typeof Fixture.uniformColor>>(
            "uniformColor",
            [label, expected],
          );
          assert.ok(
            result.foreground > 100,
            `${label} has measurable shape pixels`,
          );
          assert.ok(
            result.matching / result.foreground > 0.995,
            `${label} must use one uniform sRGB color ${result.expected}; matched ${result.matching}/${result.foreground}`,
          );
        };
        const patch = async (changes: RenderStatePatch) => {
          const result = await call<{
            session: bigint;
            tick: bigint;
            notifications: RenderStateUpdatedEvent[];
          }>("patch", [changes]);
          assert.equal(result.notifications.length, 1);
          const notification = result.notifications[0]!;
          assert.deepEqual(notification.changes, changes);
          assert.equal(notification.requestId, 0n);
          assert.equal(notification.session, result.session);
          assert.ok(notification.tick <= result.tick);
          assert.ok(!("ok" in notification) && !("error" in notification));
          return notification;
        };
        try {
          const initialized = await call<Observation>("initialize", [
            {
              generatedModuleUrl: environment.urls.generated,
              workerScriptUrl: environment.urls.workerScript,
              wasmUrl: environment.urls.wasm,
            },
          ]);
          assert.equal(initialized.debugEnabled, true);
          assertPrivate(initialized);
          await patch({ debugGeometryColor: [...GLOBAL_COLOR] });

          for (const [shape, name] of SHAPES.entries()) {
            let solid: Capture | undefined;
            for (const outline of [false, true]) {
              const label = `${name}-${outline ? "outline" : "solid"}`;
              await call("replaceScene", [[shapeDeclaration(shape, outline)]]);
              const result = await capture(label);
              assert.equal(
                result.drawCalls,
                1,
                `${label} contributes one debug draw without MeshInstance`,
              );
              assert.ok(result.triangles > 0);
              assert.ok(
                result.summary.foregroundPixels > 100 &&
                  result.summary.coverage < 0.65,
                `${label} has bounded visible geometry`,
              );
              assert.ok(
                result.summary.bounds &&
                  result.summary.bounds.left > 1 &&
                  result.summary.bounds.top > 1 &&
                  result.summary.bounds.right < result.width - 2 &&
                  result.summary.bounds.bottom < result.height - 2,
                `${label} fits inside the camera`,
              );
              await color(label, GLOBAL_COLOR);
              if (!outline) solid = result;
              else {
                assert.ok(solid);
                assert.ok(
                  result.summary.coverage < solid.summary.coverage * 0.8,
                  `${name} contour must leave its interior visibly open`,
                );
                assert.ok(
                  (await compare(`${name}-solid`, label)).changedFraction >
                    0.01,
                );
              }
            }
          }

          const selected = await call<Observation>("replaceScene", [
            [
              {
                name: "selected",
                geometry: {
                  shape: 0,
                  outline: true,
                  is_rendered: true,
                  stroke: 0.065,
                },
                transform: { x: -0.8, sx: 0.6, sy: 0.6, sz: 0.6 },
              },
              {
                name: "unselected",
                geometry: {
                  shape: 1,
                  outline: true,
                  is_rendered: false,
                  stroke: 0.065,
                },
                transform: { x: 0.8, sx: 0.6, sy: 0.6, sz: 0.6 },
              },
            ],
          ]);
          const initial = await capture("selected-only");
          assert.equal(initial.drawCalls, 1);
          const allChanged = await patch({ showAllDebugGeometries: true });
          const all = await capture("show-all");
          assert.equal(all.session, allChanged.session);
          assert.ok(all.tick > allChanged.tick);
          assert.equal(all.drawCalls, 2);
          assert.deepEqual(
            all.observation.entities,
            selected.entities,
            "Show-all preserves both authored and effective component fields",
          );
          assert.ok(
            (await compare("selected-only", "show-all")).changedFraction >
              0.003,
          );
          await patch({ showAllDebugGeometries: false });
          const restored = await capture("selection-restored");
          assert.equal(restored.drawCalls, 1);
          assert.deepEqual(restored.observation.entities, selected.entities);
          assert.ok(
            (await compare("selected-only", "selection-restored"))
              .changedFraction < 0.0005,
          );

          await patch({ debugGeometryColor: [1, 0, 0] });
          const global = await capture("global-red");
          await color("global-red", [1, 0, 0]);
          assert.deepEqual(
            global.observation.entities,
            selected.entities,
            "Global color never writes component state",
          );
          await call("updateGeometry", [
            0,
            { has_color_override: true, r: 0, g: 1, b: 0 },
          ]);
          await capture("component-green");
          await color("component-green", [0, 1, 0]);
          await patch({ debugGeometryColor: [0, 0, 1] });
          await capture("override-keeps-green");
          assert.ok(
            (await compare("component-green", "override-keeps-green"))
              .changedFraction < 0.0005,
          );
          await call("updateGeometry", [0, { has_color_override: false }]);
          const blue = await capture("global-blue-restored");
          await color("global-blue-restored", [0, 0, 1]);
          await call("recoverContext");
          const recovered = await capture("context-restored");
          assert.ok(recovered.contextGeneration > blue.contextGeneration);
          assert.equal(recovered.session, blue.session);
          assert.equal(recovered.drawCalls, 1);
          assert.deepEqual(
            recovered.observation.entities,
            blue.observation.entities,
          );
          assert.ok(
            (await compare("global-blue-restored", "context-restored"))
              .changedFraction < 0.0005,
          );
          await call("replaceScene", [[]]);
          const empty = await capture("debug-entities-removed");
          assert.equal(empty.drawCalls, 0);
          requireBlank(
            empty.summary,
            "Removing the final debug entity clears its pixels",
          );
        } catch (error) {
          await capture("failure").catch((captureError: unknown) =>
            environment.evidence.record("failure_capture_error", {
              captureError,
            }),
          );
          throw error;
        } finally {
          await call("close");
        }
      },
    );
  });
}

function shapeDeclaration(shape: number, outline: boolean): Declaration {
  return {
    name: `${SHAPES[shape]}-${outline ? "outline" : "solid"}`,
    geometry: {
      shape,
      outline,
      is_rendered: true,
      stroke: 0.055,
      ...(shape === 2 ? { radius: 0.6, height: 2.5 } : {}),
    },
  };
}

function assertPrivate(observation: Observation) {
  assert.deepEqual(
    observation.resources,
    [],
    "Debug declarations expose no World resource records",
  );
  assert.deepEqual(
    observation.resourceEvents,
    [],
    "Debug generation, changes and recovery produce no client asset events",
  );
  assert.deepEqual(observation.renderDiagnostics, []);
}
