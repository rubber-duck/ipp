import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import { protocolRejected } from "../integration/assertions.js";
import {
  malformedRequestsFailExplicitly,
  schemaMismatchFailsDuringHandshake,
  staleSessionFailsBeforeMutation,
} from "../integration/protocol-failures.js";
import {
  partialFailureThenSuccess,
  autonomousFramesAdvance,
  concurrentRpcResponsesStayCorrelated,
  metadataAndGenerationReuse,
  reconnectStartsWithEmptyWorld,
  scalarBaseAndEffectiveValues,
  sourceDeletionDoesNotReconnect,
} from "../integration/scenarios/index.js";
import {
  assertLoopbackClosed,
  type BrowserBuildConfiguration,
  type BrowserHarnessConfiguration,
  BrowserHarnessRunError,
  runBrowserScenario,
  runtimeConfigurationFor,
} from "./environment.js";
import { ownedMetadataSurvivesWasmGrowth } from "./owned-metadata-growth.js";

const workspace = resolve(process.cwd());
if (
  !existsSync(resolve(workspace, "Cargo.toml")) ||
  !existsSync(resolve(workspace, "package.json"))
) {
  throw new Error(
    `browser tests must run from the repository root: ${workspace}`,
  );
}

const minimal = browserBuild("headless");
const constraints = browserBuild("headless-builtins");
const cancellation = new AbortController();
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () =>
    cancellation.abort(new Error(`browser run cancelled by ${signal}`)),
  );
}

const commonScenarios = [
  ["partial failure then success", partialFailureThenSuccess],
  ["metadata and generation reuse", metadataAndGenerationReuse],
  ["reconnect starts with empty world", reconnectStartsWithEmptyWorld],
] as const;

test("constraints: autonomous frames advance without client requests", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "constraints autonomous frames",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    autonomousFramesAdvance,
  );
});

test("constraints: 64 concurrent RPC responses stay correlated", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "constraints concurrent RPC correlation",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    concurrentRpcResponsesStayCorrelated,
  );
});

for (const build of [minimal, constraints]) {
  for (const [name, scenario] of commonScenarios) {
    test(`${build.name}: ${name}`, { timeout: 30_000 }, async (context) => {
      await runBrowserScenario(
        `${build.name} ${name}`,
        configuration(build),
        AbortSignal.any([cancellation.signal, context.signal]),
        scenario,
      );
    });
  }
}

for (const [name, scenario] of [
  ["base and effective scalar driver", scalarBaseAndEffectiveValues],
  ["driver source deletion", sourceDeletionDoesNotReconnect],
] as const) {
  test(`constraints: ${name}`, { timeout: 30_000 }, async (context) => {
    await runBrowserScenario(
      `constraints ${name}`,
      configuration(constraints),
      AbortSignal.any([cancellation.signal, context.signal]),
      scenario,
    );
  });
}

test("constraints: owned UTF-8 metadata survives WASM allocation growth", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "constraints owned metadata wasm growth",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    ownedMetadataSurvivesWasmGrowth,
  );
});

test("feature mismatch fails during worker bootstrap and cleans up", {
  timeout: 30_000,
}, async (context) => {
  const result = await runBrowserScenario(
    "feature mismatch cleanup",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      await scenarioContext.execute(
        "reject minimal generated contract",
        {},
        () =>
          schemaMismatchFailsDuringHandshake({
            factory: scenarioContext.factory,
            url: scenarioContext.url,
            signal: scenarioContext.signal,
            evidence: scenarioContext.evidence,
          }),
      );
      await scenarioContext.execute(
        "use matching client after mismatch",
        0,
        () =>
          scenarioContext.driver.waitForFrame(undefined, {
            signal: scenarioContext.signal,
          }),
      );
    },
  );
  await assertLoopbackClosed(result.origin);
});

test("native generated contract fails against the constraints WASM runtime", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "native target mismatch",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const result = await scenarioContext.execute(
        "reject native generated contract",
        { contract: scenarioContext.urls.nativeGenerated },
        () =>
          scenarioContext.factory.rejectConnection(
            runtimeConfigurationFor(
              scenarioContext.urls,
              scenarioContext.urls.nativeGenerated,
              scenarioContext.urls.wasm,
            ),
            scenarioContext.signal,
          ),
      );
      protocolRejected(result, "handshake");
      assert.match(result.detail, /schema|mismatch/i);
    },
  );
});

test("stale worker session fails before mutation", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "stale worker session",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) =>
      scenarioContext.execute("reject stale worker session", {}, () =>
        staleSessionFailsBeforeMutation({
          factory: scenarioContext.factory,
          url: scenarioContext.url,
          signal: scenarioContext.signal,
          evidence: scenarioContext.evidence,
        }),
      ),
  );
});

test("malformed worker requests fail explicitly", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "malformed worker requests",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) =>
      scenarioContext.execute("reject malformed worker requests", {}, () =>
        malformedRequestsFailExplicitly({
          factory: scenarioContext.factory,
          url: scenarioContext.url,
          signal: scenarioContext.signal,
          evidence: scenarioContext.evidence,
        }),
      ),
  );
});

for (const [name, selectWasm] of [
  [
    "missing WASM URL",
    (urls: { readonly missingWasm: string }) => urls.missingWasm,
  ],
  [
    "invalid WASM boot",
    (urls: { readonly invalidWasm: string }) => urls.invalidWasm,
  ],
] as const) {
  test(`${name} rejects and leaves the matching worker usable`, {
    timeout: 30_000,
  }, async (context) => {
    await runBrowserScenario(
      name,
      configuration(constraints),
      AbortSignal.any([cancellation.signal, context.signal]),
      async (scenarioContext) => {
        const result = await scenarioContext.execute(name, {}, () =>
          scenarioContext.factory.rejectConnection(
            runtimeConfigurationFor(
              scenarioContext.urls,
              scenarioContext.urls.generated,
              selectWasm(scenarioContext.urls),
            ),
            scenarioContext.signal,
          ),
        );
        protocolRejected(result, "handshake");
        await scenarioContext.execute(
          "wait for frame after failed worker boot",
          {},
          () =>
            scenarioContext.driver.waitForFrame(undefined, {
              signal: scenarioContext.signal,
            }),
        );
      },
    );
  });
}

test("a terminated real worker rejects requests", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "terminated worker request",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const result = await scenarioContext.execute(
        "inspect after worker termination",
        {},
        () =>
          scenarioContext.factory.terminatedPendingRequestRejects(
            scenarioContext.signal,
          ),
      );
      assert.equal(result.rejected, true, result.detail);
      assert.match(result.detail, /closed|timed out|terminated/i);
    },
  );
});

test("worker transport close rejects an error envelope and disposes once", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "worker transport close error",
    configuration(minimal),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const result = await scenarioContext.execute(
        "reject close error envelope",
        {},
        () =>
          scenarioContext.factory.closeErrorRejectsOnce(scenarioContext.signal),
      );
      assert.equal(result.rejected, true, result.detail);
      assert.equal(result.detail, "shutdown failed");
      assert.equal(result.disposals, 1);
    },
  );
});

test("final minimal and constraints WASM omit export-only symbols", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "final wasm exports",
    configuration(constraints),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const identities: bigint[] = [];
      for (const [name, wasmUrl, generatedUrl] of [
        [
          "headless-builtins",
          scenarioContext.urls.wasm,
          scenarioContext.urls.generated,
        ],
        [
          "headless",
          scenarioContext.urls.mismatchWasm,
          scenarioContext.urls.mismatchGenerated,
        ],
      ] as const) {
        const generatedHash = await scenarioContext.factory.contractHash(
          generatedUrl,
          scenarioContext.signal,
        );
        const runtimeHash = await scenarioContext.factory.wasmSchemaHash(
          wasmUrl,
          scenarioContext.signal,
        );
        assert.equal(runtimeHash, generatedHash);
        identities.push(generatedHash);
        const exports = await scenarioContext.execute(
          `inspect ${name} WASM exports`,
          { wasmUrl },
          () =>
            scenarioContext.factory.wasmExports(
              wasmUrl,
              scenarioContext.signal,
            ),
        );
        assert.ok(exports.length > 0, `${name} WASM exports are empty`);
        assert.ok(exports.includes("ipp_schema_hash"));
        for (const forbidden of [
          "ipp_contract_ptr",
          "ipp_contract_len",
          "ipp_fixture_ptr",
          "ipp_fixture_len",
          "ipp_fixture_check",
        ]) {
          assert.equal(
            exports.includes(forbidden),
            false,
            `${name} final WASM exports ${forbidden}`,
          );
        }
      }
      assert.notEqual(identities[0], identities[1]);
    },
  );
});

test("accepted client hash and cleanup evidence survive scenario failure", {
  timeout: 30_000,
}, async (context) => {
  let failure: BrowserHarnessRunError | undefined;
  await assert.rejects(
    runBrowserScenario(
      "expected browser scenario failure",
      configuration(minimal),
      AbortSignal.any([cancellation.signal, context.signal]),
      async (scenarioContext) => {
        await scenarioContext.driver.waitForFrame(undefined, {
          signal: scenarioContext.signal,
        });
        throw new Error("intentional browser harness lifecycle fault");
      },
    ),
    (error: unknown) => {
      assert.ok(error instanceof BrowserHarnessRunError);
      failure = error;
      return true;
    },
  );
  assert.ok(failure);
  await assertLoopbackClosed(failure.origin);
  const events = await readFile(
    resolve(failure.evidenceDirectory, "events.jsonl"),
    "utf8",
  );
  assert.match(events, /"kind":"browser_sdk_connected"/);
  assert.match(events, /"schemaHash":\{\"\$bigint\":\"[0-9]+\"\}/);
  assert.match(events, /"kind":"browser_scenario_failure"/);
  assert.match(events, /"kind":"browser_environment_stopped"/);
  assert.match(events, /"cleanupFailures":\[\]/);
});

function browserBuild(
  name: BrowserBuildConfiguration["name"],
): BrowserBuildConfiguration {
  const directory = resolve(workspace, "target/browser-build", name);
  return {
    name,
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, "runtime.wasm"),
    exportWasm: resolve(directory, "export.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

function configuration(
  build: BrowserBuildConfiguration,
): BrowserHarnessConfiguration {
  return {
    workspace,
    build,
    mismatchBuild: build.name === "headless" ? constraints : minimal,
    evidenceParent: resolve(workspace, "target/integration-artifacts/browser"),
  };
}

for (const abortStage of ["server-ready", "browser-launching"] as const) {
  test(`cancellation during ${abortStage} closes owned startup resources`, {
    timeout: 30_000,
  }, async () => {
    const controller = new AbortController();
    let origin = "";
    let scenarioRan = false;
    let failure: BrowserHarnessRunError | undefined;
    await assert.rejects(
      runBrowserScenario(
        `cancel ${abortStage}`,
        {
          ...configuration(minimal),
          onStartup(stage, endpoint) {
            if (stage === abortStage) {
              origin = endpoint;
              controller.abort(new Error(`cancel at ${stage}`));
            }
          },
        },
        controller.signal,
        async () => {
          scenarioRan = true;
        },
      ),
      (error: unknown) => {
        assert.ok(error instanceof BrowserHarnessRunError);
        failure = error;
        return true;
      },
    );
    assert.equal(scenarioRan, false);
    assert.notEqual(origin, "");
    await assertLoopbackClosed(origin);
    assert.ok(failure);
    const logs = JSON.parse(
      await readFile(
        resolve(failure.evidenceDirectory, "browser-log.json"),
        "utf8",
      ),
    ) as { kind: string }[];
    if (abortStage === "browser-launching") {
      assert.ok(
        logs.some((entry) => entry.kind === "browser_disconnected"),
        "late launch was not closed before handoff",
      );
    }
  });
}
