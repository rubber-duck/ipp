import assert from "node:assert/strict";
import { resolve } from "node:path";
import test from "node:test";
import {
  runBrowserScenario,
  type BrowserBuildConfiguration,
} from "../browser/environment.js";

const workspace = process.cwd();
const overlays = build("headless");

function build(name: "headless"): BrowserBuildConfiguration {
  const base = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(base, "generated.js"),
    runtimeWasm: resolve(base, "runtime.wasm"),
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
          "unmount during a pending asset load deletes nothing",
        ]),
    ],
    [
      "custom material named properties reconcile and keep the last written value",
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
          "plain nesting leaves parenting explicit",
          "keyed reordering changes core sibling order and preserves generations",
          "parent replacement updates retained child relationships",
          "declared parent deletion leaves an unrelated client child alive as a root",
          "unmount deletes declared descendants and leaves the bound child where a client placed it",
          "explicit entity reference props update by handle value",
          "all keyed permutations and insertion-removal preserve explicit order and retained identities",
          "Children rejects two declarations bound to the same actual entity",
          "Children rejects an EntityLink on another binding to its child",
          "a keyed remount of an Entity id keeps its entity and placement",
          "two Entity declarations of one id reject the render and send nothing",
          "multiple roots place one entity and the last placement wins",
          "withdrawing a root's link leaves the entity where it was last placed",
          "typed structural animation resolves scene entityBindings after acknowledgement",
          "callback-only animation rendering submits no structural operations and retains playback",
          "structural animation accepts mixed scene names and runtime entity bindings",
          "stopping structural animation leaves its last placement",
          "stable keyed parent reversal orders acknowledged handle updates without reattachment",
          "all four-entity chain permutations preserve generations and only move them",
          "parent dependencies traverse unchanged declarations and preserve before ordering",
          "reparent prefixes survive partial rejection and corrected rendering reconciles the keyed entities",
          "a rejected parent relationship keeps its applied creation",
          "corrected rendering deletes the partial entity and rebuilds relationships",
          "unmount deletes nothing",
          "removing the declarations preserves only client entities",
        ]);
      },
    ],
    [
      "plain field declarations adopt, write and clean up through the real core",
      "plainFieldLifecycle",
      (report: PlainFieldReport) => {
        assert.deepEqual(
          report.boundValues.map(({ value }) => value),
          [20, 30, 30, null, 40, 50, 45],
          "adopt, client write, prop removal keeps, removal, insert, rewrite, replacement",
        );
        assert.deepEqual(
          report.sharedValues.map(({ value }) => value),
          [11, 22, 33, null, null, null],
          "last write wins; removing either root's declaration removes the component",
        );
        assert.match(report.boundMissingRejection, /MissingSymbolicId/);
        assert.deepEqual(report.boundMissingAfterRejection, absent);
        assert.deepEqual(report.insertedComponentValues, [
          scalar(13),
          scalar(17),
          scalar(null),
        ]);
        assert.deepEqual(report.adoptedEntityValues, [
          scalar(91),
          absent,
          scalar(27),
          scalar(null),
        ]);
        assert.deepEqual(report.declaredBeforeClear, scalar(7));
        assert.equal(report.declaredExistsAfterClear, false);
        assert.equal(report.boundExistsAfterClear, true);
      },
    ],
    [
      "rejected and unsent commits keep records consistent with the World",
      "rejectionAndCorrection",
      (report: RejectionReport) => {
        assert.match(report.rejection, /MissingSymbolicId/);
        assert.deepEqual(report.afterRejection, scalar(5));
        assert.deepEqual(report.corrected, scalar(8));
        assert.match(report.unsentRejection, /nonfinite/);
        assert.deepEqual(report.afterUnsentRejection, scalar(8));
        assert.deepEqual(report.afterUnsentCorrection, scalar(10));
        assert.deepEqual(report.afterLargeBatch, scalar(256));
        assert.deepEqual(report.afterLargeBatchCleanup, scalar(11));
        assert.deepEqual(report.afterLoss, scalar(null));
        assert.deepEqual(report.afterReplacement, scalar(9));
        assert.deepEqual(report.afterRemoval, scalar(null));
        assert.deepEqual(report.afterRecovery, scalar(12));
      },
    ],
    [
      "React hooks and StrictMode commit one owned declaration",
      "hooksAndStrictMode",
      (report: HooksReport) => {
        assert.deepEqual(report.observation, scalar(42));
        assert.equal(report.matchingEntities, 1);
        assert.equal(report.existsAfterUnmount, true);
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
        assert.equal(report.entityExistsAfterUnmount, true);
      },
    ],
  ] as const) {
    test(`${variant}: ${name}`, { timeout: 30_000 }, async (context) => {
      await runBrowserScenario(
        `react ${variant} ${fixtureExport}`,
        {
          workspace,
          build: overlays,
          operationTimeoutMs:
            fixtureExport === "childrenHierarchy" ? 15_000 : 5_000,
        },
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
  readonly value: number | null;
}

interface PlainFieldReport {
  readonly boundValues: readonly ScalarObservation[];
  readonly sharedValues: readonly ScalarObservation[];
  readonly boundMissingRejection: string;
  readonly boundMissingAfterRejection: ScalarObservation;
  readonly insertedComponentValues: readonly ScalarObservation[];
  readonly adoptedEntityValues: readonly ScalarObservation[];
  readonly declaredBeforeClear: ScalarObservation;
  readonly declaredExistsAfterClear: boolean;
  readonly boundExistsAfterClear: boolean;
}

interface RejectionReport {
  readonly rejection: string;
  readonly afterRejection: ScalarObservation;
  readonly corrected: ScalarObservation;
  readonly unsentRejection: string;
  readonly afterUnsentRejection: ScalarObservation;
  readonly afterUnsentCorrection: ScalarObservation;
  readonly afterLargeBatch: ScalarObservation;
  readonly afterLargeBatchCleanup: ScalarObservation;
  readonly afterLoss: ScalarObservation;
  readonly afterReplacement: ScalarObservation;
  readonly afterRemoval: ScalarObservation;
  readonly afterRecovery: ScalarObservation;
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

function scalar(value: number | null) {
  return { entityExists: true, value };
}

const absent = { entityExists: false, value: null };
