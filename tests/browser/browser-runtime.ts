import type {
  BatchOutcome,
  ConcurrentRpcCorrelation,
  EntityId,
  EntityObservation,
  EntityReference,
  FrameObservation,
  ProtocolRejection,
  SceneOperation,
} from "../integration/driver.js";
import type {
  BatchOutcome as SdkBatchOutcome,
  Command,
  EntityRef as SdkEntityRef,
  EntitySnapshot,
} from "../../target/integration-artifacts/client/generated.js";
import type { Client, PortTransport } from "@ipp/client";

export interface BrowserRuntimeConfiguration {
  readonly workerScriptUrl: string;
  readonly transportModuleUrl: string;
  readonly contractModuleUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs: number;
  readonly logLevel?: "trace" | "debug" | "info" | "warn" | "error" | "off";
}

type NativeGeneratedModule =
  typeof import("../../target/integration-artifacts/client/generated.js");
type LinearDriverModule = Pick<NativeGeneratedModule, "LinearDriver">;
type GeneratedContract = Omit<NativeGeneratedModule, "LinearDriver"> &
  Partial<LinearDriverModule>;
type TransportModule =
  typeof import("../../packages/ipp-client/src/transport.js");

interface ActiveConnection {
  readonly client: Client;
  readonly contract: GeneratedContract;
}

interface RawWorker {
  readonly worker: Worker;
  readonly transport: PortTransport;
  readonly ready: Promise<void>;
  next(timeoutMs: number): Promise<RawResult>;
  close(): Promise<void>;
}

interface WorkerTransportOwner {
  readonly worker: Worker;
  readonly transport: PortTransport;
  close(): Promise<void>;
}

const connections = new Map<string, ActiveConnection>();

export async function connect(
  connectionId: string,
  configuration: BrowserRuntimeConfiguration,
): Promise<{ readonly schemaHash: bigint; readonly session: bigint }> {
  if (connections.has(connectionId)) {
    throw new Error(`browser connection '${connectionId}' already exists`);
  }
  const contract = await loadContract(configuration.contractModuleUrl);
  const client = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    {
      timeoutMs: configuration.timeoutMs,
      ...(configuration.logLevel === undefined
        ? {}
        : { logLevel: configuration.logLevel }),
    },
  );
  connections.set(connectionId, { client, contract });
  return { schemaHash: contract.SCHEMA_HASH, session: client.session };
}

export async function submit(
  connectionId: string,
  batchId: bigint,
  operations: readonly SceneOperation[],
): Promise<BatchOutcome> {
  const { client, contract } = connection(connectionId);
  const aliases = new Map<string, number>();
  let nextAlias = 1;
  const commands: Command[] = [];
  for (const operation of operations) {
    if (operation.kind === "create") {
      if (aliases.has(operation.alias)) {
        throw new Error(`duplicate harness alias '${operation.alias}'`);
      }
      const numericAlias = nextAlias++;
      aliases.set(operation.alias, numericAlias);
      commands.push(
        contract.Entity.create(numericAlias, {
          symbolicId: operation.symbolicId ?? null,
          classes:
            operation.classes === undefined ? [] : [...operation.classes],
        }),
      );
    } else if (operation.kind === "delete") {
      commands.push(
        contract.Entity.delete(
          sdkReference(contract, operation.entity, aliases),
        ),
      );
    } else if (operation.kind === "updateMetadata") {
      commands.push(
        contract.Entity.setMetadata(
          sdkReference(contract, operation.entity, aliases),
          {
            symbolicId: operation.symbolicId ?? null,
            classes:
              operation.classes === undefined ? [] : [...operation.classes],
          },
        ),
      );
    } else if (operation.kind === "insertScalar") {
      commands.push(
        contract.Scalar.insert(
          sdkReference(contract, operation.entity, aliases),
          { value: operation.value },
        ),
      );
    } else if (operation.kind === "setScalar") {
      commands.push(
        contract.Scalar.setValue(
          sdkReference(contract, operation.entity, aliases),
          operation.value,
        ),
      );
    } else {
      const linearDriver = contract.LinearDriver;
      if (linearDriver === undefined) {
        throw new Error("target contract does not include LinearDriver");
      }
      commands.push(
        linearDriver.insert(sdkReference(contract, operation.entity, aliases), {
          source: sdkReference(contract, operation.source, aliases),
          scale: operation.scale,
          bias: operation.bias,
        }),
      );
    }
  }

  const outcome = await client.batch(commands, batchId);
  return normalizeBatchOutcome(outcome, aliases);
}

export async function waitForFrame(
  connectionId: string,
  afterTick: bigint | undefined,
): Promise<FrameObservation> {
  const result = await connection(connectionId).client.waitForFrame(afterTick);
  return { tick: result.tick, time: result.time };
}

export function publicStepAvailable(connectionId: string): boolean {
  return "step" in connection(connectionId).client;
}

export async function correlateConcurrentRequests(
  connectionId: string,
  marker: EntityId,
  batchIds: readonly bigint[],
): Promise<ConcurrentRpcCorrelation> {
  if (batchIds.length * 2 !== 64) {
    throw new Error("concurrent correlation probe requires exactly 64 RPCs");
  }
  const { client, contract } = connection(connectionId);
  type CorrelatedResult =
    | {
        readonly kind: "batch";
        readonly value: ConcurrentRpcCorrelation["batches"][number];
      }
    | {
        readonly kind: "inspect";
        readonly value: ConcurrentRpcCorrelation["inspections"][number];
      };
  const pending: Promise<CorrelatedResult>[] = [];
  for (const batchId of batchIds) {
    pending.push(
      client
        .batch(
          [
            contract.Entity.create(1, {
              symbolicId: `concurrent-${batchId}`,
              classes: ["concurrent-rpc"],
            }),
          ],
          batchId,
        )
        .then((outcome) => {
          if (!outcome.ok) {
            throw new Error(
              `concurrent batch ${batchId} rejected: ${outcome.error.reason}`,
            );
          }
          return {
            kind: "batch" as const,
            value: {
              requestedBatchId: batchId,
              returnedBatchId: outcome.batchId,
              commitTick: outcome.tick,
              aliasCount: outcome.aliases.length,
            },
          };
        }),
    );
    pending.push(
      client.inspect().then((inspection) => ({
        kind: "inspect" as const,
        value: {
          tick: inspection.tick,
          time: inspection.time,
          sawMarker: inspection.entities.some(
            (snapshot) => snapshot.id === marker.bits,
          ),
        },
      })),
    );
  }

  const results = await Promise.all(pending);
  return {
    batches: results.flatMap((result) =>
      result.kind === "batch" ? [result.value] : [],
    ),
    inspections: results.flatMap((result) =>
      result.kind === "inspect" ? [result.value] : [],
    ),
  };
}

export async function inspect(
  connectionId: string,
  entity: EntityId,
): Promise<EntityObservation | null> {
  const active = connection(connectionId);
  const entities = (await active.client.inspect()).entities;
  const snapshot = entities.find((candidate) => candidate.id === entity.bits);
  return snapshot === undefined
    ? null
    : normalizeObservation(active.contract, snapshot);
}

export async function findBySymbolicId(
  connectionId: string,
  symbolicId: string,
): Promise<EntityObservation | null> {
  const active = connection(connectionId);
  const entities = (await active.client.inspect()).entities;
  const snapshot = entities.find(
    (candidate) => candidate.metadata.symbolicId === symbolicId,
  );
  return snapshot === undefined
    ? null
    : normalizeObservation(active.contract, snapshot);
}

export async function close(connectionId: string): Promise<void> {
  const active = connections.get(connectionId);
  if (active === undefined) {
    return;
  }
  connections.delete(connectionId);
  await active.client.close();
}

export async function rejectConnection(
  configuration: BrowserRuntimeConfiguration,
): Promise<ProtocolRejection> {
  try {
    const contract = await loadContract(configuration.contractModuleUrl);
    const client = await contract.IppClient.connectWorker(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      { timeoutMs: configuration.timeoutMs },
    );
    await client.close();
    return {
      rejected: false,
      stage: "handshake",
      code: "unexpected-connection",
      detail: "worker accepted the incompatible or invalid runtime",
    };
  } catch (error) {
    return {
      rejected: true,
      stage: "handshake",
      code: errorName(error),
      detail: errorMessage(error),
    };
  }
}

export async function rejectStaleSession(
  configuration: BrowserRuntimeConfiguration,
): Promise<ProtocolRejection> {
  const contract = await loadContract(configuration.contractModuleUrl);
  const raw = await openRawWorker(configuration);
  try {
    await raw.ready;
    const session = await rawHandshake(raw, contract, configuration);
    const bytes = contract.encodeRequest({
      session: session + 1n,
      requestId: 1n,
      body: {
        kind: "batch",
        batch: {
          id: 1n,
          operations: [
            contract.Entity.create(1, {
              symbolicId: "stale-must-not-apply",
              classes: [],
            }),
          ],
        },
      },
    });
    return await expectRawRejection(
      raw,
      bytes,
      "request",
      contract,
      session,
      configuration.timeoutMs,
    );
  } finally {
    await raw.close();
  }
}

export async function rejectMalformed(
  configuration: BrowserRuntimeConfiguration,
  malformedCase:
    | "no-bootstrap"
    | "oversized-message"
    | "trailing-bytes"
    | "unknown-tag"
    | "removed-step-tag",
): Promise<ProtocolRejection> {
  const contract = await loadContract(configuration.contractModuleUrl);
  const raw = await openRawWorker(configuration);
  try {
    await raw.ready;
    if (malformedCase === "no-bootstrap") {
      const bytes = contract.encodeRequest({
        session: 1n,
        requestId: 1n,
        body: { kind: "inspect", collection: "entities" },
      });
      return await expectRawRejection(
        raw,
        bytes,
        "handshake",
        contract,
        0n,
        configuration.timeoutMs,
      );
    }

    const session = await rawHandshake(raw, contract, configuration);
    let bytes: Uint8Array<ArrayBuffer>;
    if (malformedCase === "oversized-message") {
      bytes = new Uint8Array(contract.MAX_MESSAGE_BYTES + 1);
    } else {
      const valid = contract.encodeRequest({
        session,
        requestId: 1n,
        body: { kind: "inspect", collection: "entities" },
      });
      if (malformedCase === "trailing-bytes") {
        bytes = new Uint8Array(valid.byteLength + 1);
        bytes.set(valid);
      } else if (malformedCase === "removed-step-tag") {
        bytes = removedStepRequest(valid);
      } else {
        bytes = valid.slice();
        bytes[bytes.byteLength - 1] = 0xff;
      }
    }
    return await expectRawRejection(
      raw,
      bytes,
      "request",
      contract,
      session,
      configuration.timeoutMs,
    );
  } finally {
    await raw.close();
  }
}

export async function terminatedPendingRequestRejects(
  configuration: BrowserRuntimeConfiguration,
): Promise<{ readonly rejected: boolean; readonly detail: string }> {
  const contract = await loadContract(configuration.contractModuleUrl);
  // The dead worker cannot acknowledge graceful close. Keep teardown within the
  // probe's operation budget while leaving ordinary frame shutdown unchanged.
  const timeoutMs = Math.min(configuration.timeoutMs, 2_000);
  const owner = await createWorkerTransport(configuration, 1_000);
  let client: Client | undefined;
  try {
    client = await contract.IppClient.connectTransport(owner.transport, {
      timeoutMs,
    });
    owner.worker.terminate();
    const pending = client.inspect();
    try {
      await pending;
      return {
        rejected: false,
        detail: "request resolved after its worker was terminated",
      };
    } catch (error) {
      return { rejected: true, detail: errorMessage(error) };
    }
  } finally {
    await client?.close().catch(() => undefined);
    await owner.close();
  }
}

export async function wasmExports(wasmUrl: string): Promise<readonly string[]> {
  const response = await fetch(wasmUrl);
  if (!response.ok) {
    throw new Error(
      `WASM fetch failed: ${response.status} ${response.statusText}`,
    );
  }
  const module = await WebAssembly.compile(await response.arrayBuffer());
  return WebAssembly.Module.exports(module).map((item) => item.name);
}

export async function wasmSchemaHash(wasmUrl: string): Promise<bigint> {
  const response = await fetch(wasmUrl);
  if (!response.ok) {
    throw new Error(
      `WASM fetch failed: ${response.status} ${response.statusText}`,
    );
  }
  const result = await WebAssembly.instantiate(
    await response.arrayBuffer(),
    {},
  );
  const schemaHash = result.instance.exports.ipp_schema_hash;
  if (typeof schemaHash !== "function") {
    throw new Error("final WASM does not export ipp_schema_hash");
  }
  const value: unknown = schemaHash();
  if (typeof value !== "bigint") {
    throw new Error("ipp_schema_hash did not return a bigint");
  }
  return BigInt.asUintN(64, value);
}

export async function contractHash(contractModuleUrl: string): Promise<bigint> {
  return (await loadContract(contractModuleUrl)).SCHEMA_HASH;
}

export async function closeErrorRejectsOnce(
  transportModuleUrl: string,
): Promise<{
  readonly rejected: boolean;
  readonly detail: string;
  readonly disposals: number;
}> {
  const transportModule = await loadTransportModule(transportModuleUrl);
  const channel = new MessageChannel();
  let disposals = 0;
  const transport = new transportModule.PortTransport(channel.port1, () => {
    disposals += 1;
  });
  transport.start({
    ready: () => undefined,
    message: () => undefined,
    error: () => undefined,
    closed: () => undefined,
  });
  channel.port2.onmessage = (event: MessageEvent<unknown>) => {
    if (
      typeof event.data === "object" &&
      event.data !== null &&
      "type" in event.data &&
      event.data.type === "close"
    ) {
      channel.port2.postMessage({
        type: "error",
        message: "shutdown failed",
      });
    }
  };
  channel.port2.start();
  try {
    await transport.close();
    return { rejected: false, detail: "close resolved", disposals };
  } catch (error) {
    return { rejected: true, detail: errorMessage(error), disposals };
  } finally {
    channel.port2.close();
  }
}

function connection(connectionId: string): ActiveConnection {
  const active = connections.get(connectionId);
  if (active === undefined) {
    throw new Error(`browser connection '${connectionId}' is closed`);
  }
  return active;
}

function sdkReference(
  contract: GeneratedContract,
  reference: EntityReference,
  aliases: ReadonlyMap<string, number>,
): SdkEntityRef {
  if (reference.kind === "entity") {
    return contract.Entity.handle(reference.entity.bits);
  }
  const numericAlias = aliases.get(reference.alias);
  if (numericAlias === undefined) {
    throw new Error(
      `alias '${reference.alias}' has not been created in this batch`,
    );
  }
  return contract.Entity.alias(numericAlias);
}

function normalizeBatchOutcome(
  outcome: SdkBatchOutcome,
  aliases: ReadonlyMap<string, number>,
): BatchOutcome {
  const resolved: Record<string, EntityId> = Object.create(null) as Record<
    string,
    EntityId
  >;
  const namesByAlias = new Map(
    [...aliases].map(([name, numericAlias]) => [numericAlias, name]),
  );
  for (const item of outcome.aliases) {
    const name = namesByAlias.get(item.alias);
    if (name === undefined) {
      throw new Error(`response returned unknown alias ${item.alias}`);
    }
    resolved[name] = normalizeEntity(item.id);
  }
  if (!outcome.ok) {
    return {
      status: "rejected",
      aliases: resolved,
      batchId: outcome.batchId,
      operationIndex: outcome.error.operation,
      scope: outcome.error.scope,
      code: outcome.error.reason,
      reason: outcome.error.reason,
    };
  }
  return {
    status: "committed",
    batchId: outcome.batchId,
    commitTick: outcome.tick,
    aliases: resolved,
  };
}

function normalizeObservation(
  contract: GeneratedContract,
  snapshot: EntitySnapshot,
): EntityObservation {
  const scalarId = contract.components.Scalar.id;
  const driverId = contract.components.LinearDriver?.id;
  const scalarBase = snapshot.base.find(
    (component) => component.component === scalarId,
  );
  const scalarEffective = snapshot.effective.find(
    (component) => component.component === scalarId,
  );
  const driver =
    driverId === undefined
      ? undefined
      : snapshot.base.find((component) => component.component === driverId);
  return {
    entity: normalizeEntity(snapshot.id),
    symbolicId: snapshot.metadata.symbolicId,
    classes: [...snapshot.metadata.classes],
    scalar:
      scalarBase === undefined || scalarEffective === undefined
        ? null
        : {
            base: requiredNumber(scalarBase.fields.value, "Scalar base value"),
            effective: requiredNumber(
              scalarEffective.fields.value,
              "Scalar effective value",
            ),
          },
    linearDriver:
      driver === undefined
        ? null
        : {
            source: normalizeEntity(
              requiredBigInt(driver.fields.source, "LinearDriver source"),
            ),
            scale: requiredNumber(driver.fields.scale, "LinearDriver scale"),
            bias: requiredNumber(driver.fields.bias, "LinearDriver bias"),
          },
  };
}

function normalizeEntity(bits: bigint): EntityId {
  return {
    bits,
    index: Number(bits & 0xffff_ffffn),
    generation: Number((bits >> 32n) & 0xffff_ffffn),
  };
}

function requiredNumber(value: unknown, label: string): number {
  if (typeof value !== "number") {
    throw new Error(`${label} is missing or has the wrong type`);
  }
  return value;
}

function requiredBigInt(value: unknown, label: string): bigint {
  if (typeof value !== "bigint") {
    throw new Error(`${label} is missing or has the wrong type`);
  }
  return value;
}

async function loadTransportModule(url: string): Promise<TransportModule> {
  const value: unknown = await import(url);
  if (
    typeof value !== "object" ||
    value === null ||
    !("PortTransport" in value) ||
    typeof value.PortTransport !== "function"
  ) {
    throw new Error("transport module does not export PortTransport");
  }
  return value as TransportModule;
}

async function loadContract(url: string): Promise<GeneratedContract> {
  const value: unknown = await import(url);
  if (
    typeof value !== "object" ||
    value === null ||
    !("SCHEMA_HASH" in value) ||
    typeof value.SCHEMA_HASH !== "bigint" ||
    !("IppClient" in value) ||
    typeof value.IppClient !== "function" ||
    !("bootstrap" in value) ||
    typeof value.bootstrap !== "function" ||
    !("Scalar" in value) ||
    typeof value.Scalar !== "object" ||
    value.Scalar === null
  ) {
    throw new Error("generated module is not a target client contract");
  }
  return value as GeneratedContract;
}

async function openRawWorker(
  configuration: BrowserRuntimeConfiguration,
): Promise<RawWorker> {
  const owner = await createWorkerTransport(configuration);
  const queued: RawResult[] = [];
  const waiters: ((result: RawResult) => void)[] = [];
  const deliver = (result: RawResult): void => {
    const waiter = waiters.shift();
    if (waiter === undefined) queued.push(result);
    else waiter(result);
  };
  let resolveReady = (): void => undefined;
  let rejectReady = (_error: Error): void => undefined;
  const ready = new Promise<void>((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  owner.transport.start({
    ready: resolveReady,
    message: (bytes) => deliver({ kind: "message", bytes }),
    error: (error) => {
      rejectReady(error);
      deliver({ kind: "error", detail: error.message });
    },
    closed: () => {
      const error = new Error("Worker closed");
      rejectReady(error);
      deliver({ kind: "closed", detail: error.message });
    },
  });
  return {
    worker: owner.worker,
    transport: owner.transport,
    ready,
    next(timeoutMs) {
      const result = queued.shift();
      if (result !== undefined) return Promise.resolve(result);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          const index = waiters.indexOf(finish);
          if (index >= 0) waiters.splice(index, 1);
          reject(new Error(`raw worker probe timed out after ${timeoutMs}ms`));
        }, timeoutMs);
        const finish = (value: RawResult): void => {
          clearTimeout(timer);
          resolve(value);
        };
        waiters.push(finish);
      });
    },
    close: owner.close,
  };
}

async function createWorkerTransport(
  configuration: BrowserRuntimeConfiguration,
  closeTimeoutMs?: number,
): Promise<WorkerTransportOwner> {
  const transportModule = await loadTransportModule(
    configuration.transportModuleUrl,
  );
  const worker = new Worker(configuration.workerScriptUrl, {
    type: "module",
    name: "ipp-runtime-probe",
  });
  const channel = new MessageChannel();
  const transport = new transportModule.PortTransport(
    channel.port1,
    () => {
      worker.terminate();
    },
    false,
    closeTimeoutMs,
  );
  worker.addEventListener("error", (event) => {
    transport.fail(new Error(event.message || "Worker failed"));
  });
  worker.addEventListener("messageerror", () => {
    transport.fail(new Error("Worker message error"));
  });
  worker.postMessage(
    {
      type: "init",
      wasmUrl: new URL(configuration.wasmUrl, globalThis.location.href).href,
      port: channel.port2,
    },
    [channel.port2],
  );
  return {
    worker,
    transport,
    async close() {
      await transport.close().catch(() => undefined);
      worker.terminate();
    },
  };
}

async function rawHandshake(
  raw: RawWorker,
  contract: GeneratedContract,
  configuration: BrowserRuntimeConfiguration,
): Promise<bigint> {
  raw.transport.send(contract.bootstrap());
  const response = await raw.next(configuration.timeoutMs);
  if (response.kind !== "message") {
    throw new Error(`bootstrap rejected: ${response.detail}`);
  }
  return contract.acceptBootstrap(response.bytes);
}

async function expectRawRejection(
  raw: RawWorker,
  bytes: Uint8Array<ArrayBuffer>,
  stage: ProtocolRejection["stage"],
  contract: GeneratedContract,
  session: bigint,
  timeoutMs: number,
): Promise<ProtocolRejection> {
  raw.transport.send(bytes);
  const deadline = performance.now() + timeoutMs;
  while (true) {
    const remaining = deadline - performance.now();
    if (remaining <= 0) {
      throw new Error(`raw worker rejection timed out after ${timeoutMs}ms`);
    }
    const response = await raw.next(remaining);
    if (response.kind !== "message") {
      return {
        rejected: true,
        stage,
        code: response.kind,
        detail: response.detail,
      };
    }
    try {
      const decoded = contract.decodeResponse(response.bytes, session);
      if (decoded.body.kind === "frame") {
        if (decoded.requestId !== 0n) {
          return {
            rejected: false,
            stage,
            code: "invalid-frame-correlation",
            detail: `frame used request identity ${decoded.requestId}`,
          };
        }
        continue;
      }
      if (decoded.body.kind === "error") {
        return {
          rejected: true,
          stage,
          code: String(decoded.body.code),
          detail: decoded.body.message,
        };
      }
      return {
        rejected: false,
        stage,
        code: "unexpected-response",
        detail: `worker returned ${decoded.body.kind}`,
      };
    } catch (error) {
      return {
        rejected: true,
        stage,
        code: errorName(error),
        detail: errorMessage(error),
      };
    }
  }
}

function removedStepRequest(
  inspectRequest: Uint8Array<ArrayBuffer>,
): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(inspectRequest.byteLength + 8);
  bytes.set(inspectRequest);
  bytes[inspectRequest.byteLength - 1] = 2;
  return bytes;
}

type RawResult =
  | { readonly kind: "message"; readonly bytes: Uint8Array }
  | { readonly kind: "error" | "closed"; readonly detail: string };

function errorName(error: unknown): string {
  return error instanceof Error ? error.name : "Error";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
