import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import {
  contractRefused,
  protocolRejected,
} from "../integration/assertions.js";
import {
  malformedRequestsFailExplicitly,
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
  runBrowserEnvironment,
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

const headless = browserBuild("headless");
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

test("headless: autonomous frames advance without client requests", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "headless autonomous frames",
    configuration(headless),
    AbortSignal.any([cancellation.signal, context.signal]),
    autonomousFramesAdvance,
  );
});

test("headless: 64 concurrent RPC responses stay correlated", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "headless concurrent RPC correlation",
    configuration(headless),
    AbortSignal.any([cancellation.signal, context.signal]),
    concurrentRpcResponsesStayCorrelated,
  );
});

for (const [name, scenario] of commonScenarios) {
  test(`headless: ${name}`, { timeout: 30_000 }, async (context) => {
    await runBrowserScenario(
      `headless ${name}`,
      configuration(headless),
      AbortSignal.any([cancellation.signal, context.signal]),
      scenario,
    );
  });
}

for (const [name, scenario] of [
  ["base and effective scalar driver", scalarBaseAndEffectiveValues],
  ["driver source deletion", sourceDeletionDoesNotReconnect],
] as const) {
  test(`constraints: ${name}`, { timeout: 30_000 }, async (context) => {
    await runBrowserScenario(
      `constraints ${name}`,
      configuration(headless),
      AbortSignal.any([cancellation.signal, context.signal]),
      scenario,
    );
  });
}

test("headless: owned UTF-8 metadata survives WASM allocation growth", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "headless owned metadata wasm growth",
    configuration(headless),
    AbortSignal.any([cancellation.signal, context.signal]),
    ownedMetadataSurvivesWasmGrowth,
  );
});

test("a client generated for another target refuses the worker Host, which keeps serving", {
  timeout: 30_000,
}, async (context) => {
  const result = await runBrowserScenario(
    "contract refusal",
    configuration(headless),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const { factory, signal, urls } = scenarioContext;
      const hostHash = await factory.contractHash(urls.generated, signal);
      const clientHash = await factory.contractHash(
        urls.mismatchGenerated,
        signal,
      );
      const refusal = await scenarioContext.execute(
        "refuse the worker Host",
        { contract: urls.mismatchGenerated },
        () =>
          factory.refuseMismatchedHost({
            signal,
            record: scenarioContext.evidence.record.bind(
              scenarioContext.evidence,
            ),
          }),
      );
      contractRefused(refusal, hostHash, clientHash);
      await scenarioContext.execute(
        "use matching client after refusal",
        0,
        () => scenarioContext.driver.waitForFrame(undefined, { signal }),
      );
    },
  );
  await assertLoopbackClosed(result.origin);
});

test("stale worker session fails before mutation", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "stale worker session",
    configuration(headless),
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
    configuration(headless),
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
      configuration(headless),
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
    configuration(headless),
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
    configuration(headless),
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

test("final WASM ships its contract and matches only its own target client", {
  timeout: 30_000,
}, async (context) => {
  await runBrowserScenario(
    "final wasm exports",
    configuration(headless),
    AbortSignal.any([cancellation.signal, context.signal]),
    async (scenarioContext) => {
      const { wasm, generated, mismatchGenerated } = scenarioContext.urls;
      const generatedHash = await scenarioContext.factory.contractHash(
        generated,
        scenarioContext.signal,
      );
      const runtimeHash = await scenarioContext.factory.wasmSchemaHash(
        wasm,
        scenarioContext.signal,
      );
      assert.equal(runtimeHash, generatedHash);
      assert.notEqual(
        await scenarioContext.factory.contractHash(
          mismatchGenerated,
          scenarioContext.signal,
        ),
        runtimeHash,
      );
      const exports = await scenarioContext.execute(
        "inspect final WASM exports",
        { wasmUrl: wasm },
        () => scenarioContext.factory.wasmExports(wasm, scenarioContext.signal),
      );
      for (const required of [
        "ipp_schema_hash",
        "ipp_contract_ptr",
        "ipp_contract_len",
      ])
        assert.ok(exports.includes(required), `final WASM lacks ${required}`);
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
      configuration(headless),
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
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

function configuration(
  build: BrowserBuildConfiguration,
): BrowserHarnessConfiguration {
  return {
    workspace,
    build,
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
          ...configuration(headless),
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

test("a Chromium that dies fails the running scenario promptly and clearly", {
  timeout: 30_000,
}, async (context) => {
  let failure: BrowserHarnessRunError | undefined;
  let crashedAt = 0;
  await assert.rejects(
    runBrowserEnvironment(
      "browser dies during scenario",
      { ...configuration(headless), operationTimeoutMs: 20_000 },
      AbortSignal.any([cancellation.signal, context.signal]),
      async (environment) => {
        const cdp = await environment.page
          .context()
          .newCDPSession(environment.page);
        crashedAt = performance.now();
        void cdp.send("Browser.crash").catch(() => undefined);
        await environment.execute("wait on a page whose browser dies", {}, () =>
          environment.page.waitForFunction(() => false, undefined, {
            timeout: 0,
          }),
        );
      },
    ),
    (error: unknown) => {
      assert.ok(error instanceof BrowserHarnessRunError);
      failure = error;
      return true;
    },
  );
  // Well before the 20 s operation timeout the scenario would otherwise wait for.
  assert.ok(performance.now() - crashedAt < 5_000);
  assert.match(
    String((failure?.cause as Error | undefined)?.message),
    /Chromium disconnected during the scenario/,
  );
});
