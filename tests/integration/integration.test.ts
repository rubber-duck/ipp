import { existsSync } from "node:fs";
import { mkdir, readFile, rename } from "node:fs/promises";
import assert from "node:assert/strict";
import { createConnection } from "node:net";
import { resolve } from "node:path";
import test from "node:test";
import type { NativeServerConfiguration } from "./environment.js";
import { HarnessRunError, runNativeScenario } from "./environment.js";
import {
  malformedRequestsFailExplicitly,
  schemaMismatchFailsDuringHandshake,
  staleSessionFailsBeforeMutation,
} from "./protocol-failures.js";
import { ProductionDriverFactory } from "./drivers/production.js";
import { entity, type EntityId } from "./driver.js";
import {
  partialFailureThenSuccess,
  autonomousFramesAdvance,
  concurrentRpcResponsesStayCorrelated,
  metadataAndGenerationReuse,
  reconnectStartsWithEmptyWorld,
  scalarBaseAndEffectiveValues,
  sourceDeletionDoesNotReconnect,
} from "./scenarios/index.js";

const workspace = resolve(process.cwd());
if (
  !existsSync(resolve(workspace, "Cargo.toml")) ||
  !existsSync(resolve(workspace, "package.json"))
) {
  throw new Error(
    `integration tests must run from the repository root: ${workspace}`,
  );
}
const configuration: NativeServerConfiguration = {
  executable: resolve(
    workspace,
    "target/integration-artifacts/native",
    process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
  ),
  schemaArtifact: resolve(
    workspace,
    "target/integration-artifacts/native.contract",
  ),
  workingDirectory: workspace,
  evidenceParent: resolve(workspace, "target/integration-artifacts"),
};
const factory = new ProductionDriverFactory();
const cancellation = new AbortController();
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.once(signal, () =>
    cancellation.abort(new Error(`integration run cancelled by ${signal}`)),
  );
}

const behaviorScenarios = [
  [
    "autonomous frames advance without client requests",
    autonomousFramesAdvance,
  ],
  [
    "64 concurrent RPC responses stay correlated",
    concurrentRpcResponsesStayCorrelated,
  ],
  ["partial failure then success", partialFailureThenSuccess],
  ["metadata and generation reuse", metadataAndGenerationReuse],
  ["base and effective scalar driver", scalarBaseAndEffectiveValues],
  ["driver source deletion", sourceDeletionDoesNotReconnect],
  ["reconnect starts with empty world", reconnectStartsWithEmptyWorld],
] as const;

for (const [name, scenario] of behaviorScenarios) {
  test(name, { timeout: 30_000 }, async (testContext) => {
    const signal = AbortSignal.any([cancellation.signal, testContext.signal]);
    await runNativeScenario(name, configuration, factory, signal, scenario);
  });
}

test("native WebSocket diagnostics report partial effects and off silence", {
  timeout: 60_000,
}, async (testContext) => {
  for (const logLevel of ["debug", "off"] as const) {
    const result = await runNativeScenario(
      `${logLevel} native diagnostics`,
      { ...configuration, environment: { IPP_LOG: logLevel } },
      factory,
      AbortSignal.any([cancellation.signal, testContext.signal]),
      diagnosticBatchSequence,
    );
    const stderr = await readFile(
      resolve(result.evidenceDirectory, "server-stderr.log"),
      "utf8",
    );
    const lines = stderr.split("\n").filter((line) => line.includes("[IPP "));
    if (logLevel === "off") {
      assert.deepEqual(lines, []);
      continue;
    }

    // Shared Worlds assign internal batch identities before applying commands.
    const batchLines = (boundary: string | undefined) => {
      assert.ok(boundary, "missing diagnostic batch boundary");
      const id = boundary.match(/\bbatch=(\d+)/)?.[1];
      assert.ok(id, "diagnostic boundary omitted its batch identity");
      return lines.filter((line) => line.match(/\bbatch=(\d+)/)?.[1] === id);
    };
    const created = batchLines(
      lines.find((line) => nativeDiagnosticEvent(line) === "entity.create"),
    );
    assertNativeEventOrder(created, [
      "buffer.receive",
      "buffer.queued",
      "buffer.processing",
      "batch.begin",
      "entity.create",
      "batch.commit",
      "buffer.complete",
    ]);
    const rejected = batchLines(
      lines.find((line) => nativeDiagnosticEvent(line) === "batch.reject"),
    );
    assertNativeIncludes(rejected, ["batch.reject"]);
    assertNativeIncludes(rejected, ["entity.create", "entity.delete"]);
    const deleted = batchLines(
      lines.findLast((line) => nativeDiagnosticEvent(line) === "entity.delete"),
    );
    assertNativeEventOrder(deleted, [
      "batch.begin",
      "entity.delete",
      "batch.commit",
      "buffer.complete",
    ]);
    assertNativeOmits(lines, [
      "frame",
      "draw",
      "evaluation",
      "inspect",
      "capture",
      "ack",
    ]);
    for (const line of [...created, ...rejected, ...deleted]) {
      const event = nativeDiagnosticEvent(line);
      if (event?.startsWith("buffer.")) {
        assert.match(line, /\brequest=\d+/);
        assert.match(line, /\bbatch=\d+/);
      } else if (event?.startsWith("batch.") || event?.startsWith("entity.")) {
        assert.match(line, /\bbatch=\d+/);
      }
    }
  }
});

const protocolScenarios = [
  [
    "schema mismatch fails during handshake",
    schemaMismatchFailsDuringHandshake,
  ],
  ["stale session fails before mutation", staleSessionFailsBeforeMutation],
  ["malformed requests fail explicitly", malformedRequestsFailExplicitly],
] as const;

for (const [name, scenario] of protocolScenarios) {
  test(name, { timeout: 30_000 }, async (testContext) => {
    const signal = AbortSignal.any([cancellation.signal, testContext.signal]);
    await runNativeScenario(
      name,
      configuration,
      factory,
      signal,
      async (context) =>
        context.execute(name, {}, () =>
          scenario({
            factory,
            url: context.url,
            signal: context.signal,
            evidence: context.evidence,
          }),
        ),
    );
  });
}

// These exercise the runner's actual failure boundaries with the production host.
// They do not substitute a fake server, client, filesystem, or cleanup response.
for (const evidenceFailure of [false, true]) {
  test(`harness closes the real host after ${evidenceFailure ? "evidence write" : "scenario"} failure`, {
    timeout: 15_000,
  }, async (testContext) => {
    let endpoint = "";
    let evidenceDirectory = "";
    await assert.rejects(
      runNativeScenario(
        evidenceFailure
          ? "expected evidence failure"
          : "expected scenario failure",
        configuration,
        factory,
        testContext.signal,
        async (context) => {
          endpoint = context.url;
          evidenceDirectory = context.evidence.directory;
          await context.driver.waitForFrame(undefined, {
            signal: context.signal,
          });
          if (evidenceFailure) {
            const events = resolve(evidenceDirectory, "events.jsonl");
            await rename(
              events,
              resolve(evidenceDirectory, "events-before-failure.jsonl"),
            );
            await mkdir(events); // Real EISDIR failure on the next evidence append.
          }
          throw new Error("intentional harness lifecycle fault");
        },
      ),
      HarnessRunError,
    );
    assert.notEqual(endpoint, "");
    await assertListenerClosed(endpoint);
    assert.ok(existsSync(resolve(evidenceDirectory, "build-identity.json")));
    if (!evidenceFailure) {
      const events = await readFile(
        resolve(evidenceDirectory, "events.jsonl"),
        "utf8",
      );
      assert.match(events, /"kind":"scenario_failure"/);
      assert.match(events, /"kind":"environment_stopped"/);
      assert.match(events, /"cleanupFailures":\[\]/);
    }
  });
}

test("harness preserves logs when the real host fails before readiness", {
  timeout: 15_000,
}, async (testContext) => {
  let failure: HarnessRunError | undefined;
  await assert.rejects(
    runNativeScenario(
      "expected startup failure",
      { ...configuration, extraArguments: ["--invalid-harness-probe"] },
      factory,
      testContext.signal,
      async () => {
        assert.fail("failed host became ready");
      },
    ),
    (error: unknown) => {
      assert.ok(error instanceof HarnessRunError);
      failure = error;
      return true;
    },
  );
  assert.ok(failure);
  const stderr = await readFile(
    resolve(failure.evidenceDirectory, "server-stderr.log"),
    "utf8",
  );
  assert.match(stderr, /unknown argument: --invalid-harness-probe/);
  const events = await readFile(
    resolve(failure.evidenceDirectory, "events.jsonl"),
    "utf8",
  );
  assert.match(events, /"exit":\{"code":1,"signal":null\}/);
});

async function assertListenerClosed(endpoint: string): Promise<void> {
  const url = new URL(endpoint);
  await new Promise<void>((resolvePromise, reject) => {
    const socket = createConnection({
      host: url.hostname,
      port: Number(url.port),
    });
    socket.setTimeout(1_000);
    socket.once("connect", () => {
      socket.destroy();
      reject(new Error("owned host still accepts connections after cleanup"));
    });
    socket.once("timeout", () => {
      socket.destroy();
      reject(new Error("could not verify host listener shutdown"));
    });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      socket.destroy();
      if (error.code === "ECONNREFUSED") resolvePromise();
      else reject(error);
    });
  });
}

async function diagnosticBatchSequence(
  context: import("./environment.js").ScenarioContext,
): Promise<void> {
  const live = await submitCreated(context, 910n, "live", "diagnostic-live");
  const stale = await submitCreated(context, 920n, "stale", "diagnostic-stale");
  const staleDeletion = await context.driver.submit(
    921n,
    [{ kind: "delete", entity: entity(stale) }],
    { signal: context.signal },
  );
  assert.equal(staleDeletion.status, "committed");

  const rejected = await context.driver.submit(
    911n,
    [
      {
        kind: "create",
        alias: "partial",
        symbolicId: "diagnostic-partial",
      },
      { kind: "delete", entity: entity(live) },
      { kind: "insertScalar", entity: entity(stale), value: 1 },
    ],
    { signal: context.signal },
  );
  assert.equal(rejected.status, "rejected");
  if (rejected.status === "rejected") assert.equal(rejected.operationIndex, 2);
  assert.equal(
    await context.driver.inspect(live, { signal: context.signal }),
    null,
  );
  const partial = await context.driver.findBySymbolicId("diagnostic-partial", {
    signal: context.signal,
  });
  assert.ok(partial);

  let afterTick: bigint | undefined;
  for (let index = 0; index < 3; index += 1) {
    const frame = await context.driver.waitForFrame(afterTick, {
      signal: context.signal,
    });
    if (afterTick !== undefined) assert.ok(frame.tick > afterTick);
    afterTick = frame.tick;
  }

  const deletion = await context.driver.submit(
    912n,
    [{ kind: "delete", entity: entity(partial.entity) }],
    { signal: context.signal },
  );
  assert.equal(deletion.status, "committed");
}

async function submitCreated(
  context: import("./environment.js").ScenarioContext,
  batchId: bigint,
  name: string,
  symbolicId: string,
): Promise<EntityId> {
  const outcome = await context.driver.submit(
    batchId,
    [{ kind: "create", alias: name, symbolicId }],
    { signal: context.signal },
  );
  assert.equal(outcome.status, "committed");
  if (outcome.status !== "committed") throw new Error("unreachable");
  const created = outcome.aliases[name];
  assert.ok(created, `batch ${batchId} omitted alias ${name}`);
  return created;
}

function nativeDiagnosticEvent(line: string): string | undefined {
  return line.match(/\[IPP [^\]]+\]\s+(?:\[session=[^\]]+\]\s+)?([^\s]+)/)?.[1];
}

function assertNativeIncludes(
  lines: readonly string[],
  expected: readonly string[],
): void {
  const events = lines.map(nativeDiagnosticEvent);
  for (const event of expected) {
    assert.ok(events.includes(event), `missing ${event}: ${lines.join("\n")}`);
  }
}

function assertNativeOmits(
  lines: readonly string[],
  forbidden: readonly string[],
): void {
  const events = lines.map(nativeDiagnosticEvent);
  for (const event of forbidden) {
    assert.equal(
      events.includes(event),
      false,
      `unexpected ${event}: ${lines.join("\n")}`,
    );
  }
}

function assertNativeEventOrder(
  lines: readonly string[],
  expected: readonly string[],
): void {
  const events = lines.map(nativeDiagnosticEvent);
  let previous = -1;
  for (const event of expected) {
    const index = events.indexOf(event, previous + 1);
    assert.ok(
      index > previous,
      `${event} missing or out of order: ${events.join(", ")}`,
    );
    previous = index;
  }
}
