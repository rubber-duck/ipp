import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserScenario,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

const workspace = process.cwd();
const overlays = build("headless");
const minimal = build("headless-builtins");

function build(
  name: "headless-builtins" | "headless",
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

for (const variant of ["development", "production"] as const) {
  for (const [name, fixtureExport, assertReport] of [
    [
      "Animation declarations own ready bindings and imperative playback",
      "animationComponents",
      (report: string[]) => assert.equal(report.length, 10),
    ],
    [
      "named assets prepare, regenerate, validate references and feed animation",
      "namedAssets",
      (report: string[]) =>
        assert.deepEqual(report, [
          "initial consumers acknowledge with an empty selection before readiness",
          "forward named references bind after real resource loading",
          "equal encoded content reuses the resource",
          "previous selection survives a pending replacement",
          "superseded completion cannot replace the newest loaded asset",
          "changed content switches the existing consumer",
          "roots sharing a client have isolated asset scopes",
          "duplicate asset ids reject before registration",
          "missing named references reject before submission",
          "AnimationAsset feeds the real controller through the generated client",
          "shader references require explicit declared types",
          "unmount releases declarations and producer ownership",
        ]),
    ],
    [
      "custom material named properties reconcile and release owned state",
      "customMaterialProperties",
      (report: unknown[]) =>
        assert.deepEqual(report, [
          ...[
            [0.25, [1, 0, 0, 1]],
            [0.75, [0, 0, 1, 1]],
            [0.5, [0, 1, 0, 1]],
          ].map(([amount, tint]) => ({
            amount: { kind: "f32", value: amount },
            tint: { kind: "vec4", value: tint },
            basis: { kind: "mat2", value: [1, 0, 0, 1] },
            count: { kind: "i32", value: 3 },
            limit: { kind: "u32", value: 4 },
            image: {
              kind: "asset",
              value: { kind: 2, source: "file:///texture.png", variant: 2 },
            },
            geometry: {
              kind: "asset",
              value: { kind: 1, source: "file:///model.mesh", variant: 3 },
            },
          })),
        ]),
    ],
    [
      "Children declares nested hierarchy and preserves acknowledged ownership",
      "childrenHierarchy",
      (report: string[]) => {
        assert.deepEqual(report, [
          "Children assigns each enclosing parent through fragments and function components",
          "plain nesting leaves parenting explicit and preserves producer base",
          "keyed reordering preserves owned generations",
          "parent replacement updates retained child relationships",
          "unmount deletes owned descendants and reveals latest bound parent",
          "explicit entity reference props update by handle value",
          "Children rejects two declarations bound to the same actual entity",
          "Children rejects an explicit Hierarchy on another binding to its child",
          "a rejected parent relationship retains acknowledged partial ownership",
          "corrected rendering cleans partial ownership and rebuilds relationships",
          "final cleanup preserves only producer entities",
        ]);
      },
    ],
    [
      "owned, bound, and automatic declarations follow real core lifecycle",
      "ownershipAndAutomaticLifecycle",
      (report: OwnershipReport) => {
        assert.deepEqual(
          report.boundValues.map(({ base, effective }) => [base, effective]),
          [
            [10, 20],
            [30, 20],
            [30, 30],
            [null, 0],
            [40, 40],
            [45, 50],
          ],
        );
        assert.deepEqual(
          report.sharedFallbackValues.map(({ base, effective }) => [
            base,
            effective,
          ]),
          [
            [null, 11],
            [null, 22],
            [null, 22],
            [null, 33],
            [null, 0],
            [null, null],
          ],
        );
        assert.match(report.boundMissingRejection, /MissingComponent/);
        assert.deepEqual(report.boundMissingAfterRejection, scalar(null, null));
        assert.deepEqual(report.ownedComponentValues, [
          scalar(0, 13),
          scalar(17, 17),
          scalar(17, 17),
        ]);
        assert.deepEqual(report.autoEntityValues, [
          scalar(4, 91),
          { entityExists: false, base: null, effective: null },
          scalar(27, 27),
          scalar(27, 27),
        ]);
        assert.deepEqual(report.ownedBeforeClear, {
          entityExists: true,
          base: 0,
          effective: 7,
        });
        assert.equal(report.ownedExistsAfterClear, false);
        assert.equal(report.boundExistsAfterClear, true);
      },
    ],
    [
      "strict mode rejects mismatches and stale cleanup preserves replacements",
      "strictBindingAndCorrection",
      (report: StrictReport) => {
        assert.match(report.rejection, /ComponentExists/);
        assert.deepEqual(report.afterRejection, scalar(5, 5));
        assert.deepEqual(report.corrected, scalar(5, 8));
        assert.match(report.unsentRejection, /nonfinite/);
        assert.deepEqual(report.afterUnsentRejection, scalar(5, 8));
        assert.deepEqual(report.afterUnsentCorrection, scalar(5, 10));
        assert.deepEqual(report.afterLargeBatch, scalar(5, 256));
        assert.deepEqual(report.afterLargeBatchCleanup, scalar(5, 11));
        assert.ok(report.diagnostics.includes("ComponentRemoved"));
        assert.deepEqual(report.afterStrictLoss, scalar(null, null));
        assert.deepEqual(report.afterReplacement, scalar(9, 9));
        assert.deepEqual(report.afterStaleCleanup, scalar(9, 9));
        assert.deepEqual(report.afterExplicitRecovery, scalar(9, 12));
      },
    ],
    [
      "React hooks and StrictMode commit one owned declaration",
      "hooksAndStrictMode",
      (report: HooksReport) => {
        assert.deepEqual(report.observation, scalar(0, 42));
        assert.equal(report.matchingEntities, 1);
        assert.equal(report.existsAfterUnmount, false);
      },
    ],
    [
      "unmount before acknowledgement drains the real attached resource",
      "pendingUnmountUsesRealAcknowledgement",
      (report: PendingUnmountReport) => {
        assert.ok(report.gate.bufferedResponses >= 2);
        assert.ok(report.gate.bufferedFrames >= 1);
        assert.equal(report.gate.renderPendingBeforeRelease, true);
        assert.equal(report.gate.unmountPendingBeforeRelease, true);
        assert.equal(report.entityExistsAfterUnmount, false);
      },
    ],
  ] as const) {
    test(`${variant}: ${name}`, { timeout: 30_000 }, async (context) => {
      await runBrowserScenario(
        `react ${variant} ${fixtureExport}`,
        { workspace, build: overlays, mismatchBuild: minimal },
        context.signal,
        async (scenario) => {
          const report = await scenario.execute(name, { fixtureExport }, () =>
            scenario.page.evaluate(
              async ({ moduleUrl, fixtureExport, configuration }) => {
                const fixture = (await import(moduleUrl)) as Record<
                  string,
                  (input: typeof configuration) => Promise<unknown>
                >;
                const run = fixture[fixtureExport];
                if (run === undefined) {
                  throw new Error(
                    `missing React fixture export ${fixtureExport}`,
                  );
                }
                return await run(configuration);
              },
              {
                moduleUrl: `${scenario.urls.origin}/target/react-build/${variant === "production" ? "fixture-production.js" : "fixture.js"}`,
                fixtureExport,
                configuration: {
                  generatedModuleUrl: scenario.urls.generated,
                  workerScriptUrl: scenario.urls.workerScript,
                  wasmUrl: scenario.urls.wasm,
                  timeoutMs: 5_000,
                },
              },
            ),
          );
          assertReport(report as never);
        },
      );
    });
  }
}

interface ScalarObservation {
  readonly entityExists: boolean;
  readonly base: number | null;
  readonly effective: number | null;
}

interface OwnershipReport {
  readonly boundValues: readonly ScalarObservation[];
  readonly sharedFallbackValues: readonly ScalarObservation[];
  readonly boundMissingRejection: string;
  readonly boundMissingAfterRejection: ScalarObservation;
  readonly ownedComponentValues: readonly ScalarObservation[];
  readonly autoEntityValues: readonly ScalarObservation[];
  readonly ownedBeforeClear: ScalarObservation;
  readonly ownedExistsAfterClear: boolean;
  readonly boundExistsAfterClear: boolean;
}

interface StrictReport {
  readonly rejection: string;
  readonly afterRejection: ScalarObservation;
  readonly corrected: ScalarObservation;
  readonly unsentRejection: string;
  readonly afterUnsentRejection: ScalarObservation;
  readonly afterUnsentCorrection: ScalarObservation;
  readonly afterLargeBatch: ScalarObservation;
  readonly afterLargeBatchCleanup: ScalarObservation;
  readonly diagnostics: readonly string[];
  readonly afterStrictLoss: ScalarObservation;
  readonly afterReplacement: ScalarObservation;
  readonly afterStaleCleanup: ScalarObservation;
  readonly afterExplicitRecovery: ScalarObservation;
}

interface HooksReport {
  readonly observation: ScalarObservation;
  readonly matchingEntities: number;
  readonly existsAfterUnmount: boolean;
}

interface PendingUnmountReport {
  readonly gate: {
    readonly bufferedResponses: number;
    readonly bufferedFrames: number;
    readonly renderPendingBeforeRelease: boolean;
    readonly unmountPendingBeforeRelease: boolean;
  };
  readonly entityExistsAfterUnmount: boolean;
}

function scalar(base: number | null, effective: number | null) {
  return { entityExists: true, base, effective };
}
