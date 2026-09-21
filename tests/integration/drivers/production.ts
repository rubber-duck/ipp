import {
  Entity,
  IppClient,
  LinearDriver,
  MAX_MESSAGE_BYTES,
  SCHEMA_HASH,
  Scalar,
  acceptBootstrap,
  bootstrap,
  components,
  encodeRequest,
  type BatchOutcome as SdkBatchOutcome,
  type Command,
  type EntityRef as SdkEntityRef,
  type EntitySnapshot,
} from "../../../target/integration-artifacts/client/generated.js";
import type {
  BatchOutcome,
  ConcurrentRpcCorrelation,
  DriverConnectOptions,
  EntityId,
  EntityObservation,
  EntityReference,
  FrameObservation,
  HarnessDriver,
  HarnessDriverFactory,
  MalformedCase,
  ProtocolRejection,
  SceneOperation,
} from "../driver.js";

type RecordEvidence = DriverConnectOptions["record"];

class ProductionSdkDriver implements HarnessDriver {
  readonly #client: IppClient;
  readonly #record: RecordEvidence;

  constructor(client: IppClient, record: RecordEvidence) {
    this.#client = client;
    this.#record = record;
  }

  async submit(
    batchId: bigint,
    operations: readonly SceneOperation[],
    options: { readonly signal: AbortSignal },
  ): Promise<BatchOutcome> {
    options.signal.throwIfAborted();
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
          Entity.create(numericAlias, {
            symbolicId: operation.symbolicId ?? null,
            classes:
              operation.classes === undefined ? [] : [...operation.classes],
          }),
        );
      } else if (operation.kind === "delete") {
        commands.push(Entity.delete(sdkReference(operation.entity, aliases)));
      } else if (operation.kind === "updateMetadata") {
        commands.push(
          Entity.setMetadata(sdkReference(operation.entity, aliases), {
            symbolicId: operation.symbolicId ?? null,
            classes:
              operation.classes === undefined ? [] : [...operation.classes],
          }),
        );
      } else if (operation.kind === "insertScalar") {
        commands.push(
          Scalar.insert(sdkReference(operation.entity, aliases), {
            value: operation.value,
          }),
        );
      } else if (operation.kind === "setScalar") {
        commands.push(
          Scalar.setValue(
            sdkReference(operation.entity, aliases),
            operation.value,
          ),
        );
      } else {
        commands.push(
          LinearDriver.insert(sdkReference(operation.entity, aliases), {
            source: sdkReference(operation.source, aliases),
            scale: operation.scale,
            bias: operation.bias,
          }),
        );
      }
    }

    await this.#record("sdk_batch_request", {
      session: this.#client.session,
      batchId,
      operations: commands.length,
    });
    const outcome = await this.#client.batch(commands, batchId);
    await this.#record("sdk_batch_response", outcome);
    return normalizeBatchOutcome(outcome, aliases);
  }

  async waitForFrame(
    afterTick: bigint | undefined,
    options: { readonly signal: AbortSignal },
  ): Promise<FrameObservation> {
    options.signal.throwIfAborted();
    const result = await abortable(
      this.#client.waitForFrame(afterTick),
      options.signal,
    );
    await this.#record("sdk_frame_observed", {
      session: this.#client.session,
      afterTick,
      requestSent: false,
      result,
    });
    return { tick: result.tick, time: result.time };
  }

  async publicStepAvailable(options: {
    readonly signal: AbortSignal;
  }): Promise<boolean> {
    options.signal.throwIfAborted();
    const available = "step" in this.#client;
    await this.#record("sdk_public_step_presence", { available });
    return available;
  }

  async correlateConcurrentRequests(
    marker: EntityId,
    batchIds: readonly bigint[],
    options: { readonly signal: AbortSignal },
  ): Promise<ConcurrentRpcCorrelation> {
    options.signal.throwIfAborted();
    if (batchIds.length * 2 !== 64) {
      throw new Error("concurrent correlation probe requires exactly 64 RPCs");
    }
    await this.#record("sdk_concurrent_rpc_burst", {
      session: this.#client.session,
      batches: batchIds.length,
      inspections: batchIds.length,
    });

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
        this.#client
          .batch(
            [
              Entity.create(1, {
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
        this.#client.inspect().then((inspection) => ({
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

    const results = await abortable(Promise.all(pending), options.signal);
    const correlation: ConcurrentRpcCorrelation = {
      batches: results.flatMap((result) =>
        result.kind === "batch" ? [result.value] : [],
      ),
      inspections: results.flatMap((result) =>
        result.kind === "inspect" ? [result.value] : [],
      ),
    };
    await this.#record("sdk_concurrent_rpc_results", correlation);
    return correlation;
  }

  async inspect(
    entity: EntityId,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null> {
    const entities = await this.#inspectAll(options.signal);
    const snapshot = entities.find((candidate) => candidate.id === entity.bits);
    return snapshot === undefined ? null : normalizeObservation(snapshot);
  }

  async findBySymbolicId(
    symbolicId: string,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null> {
    const entities = await this.#inspectAll(options.signal);
    const snapshot = entities.find(
      (candidate) => candidate.metadata.symbolicId === symbolicId,
    );
    return snapshot === undefined ? null : normalizeObservation(snapshot);
  }

  async close(): Promise<void> {
    await this.#client.close();
  }

  async #inspectAll(signal: AbortSignal): Promise<readonly EntitySnapshot[]> {
    signal.throwIfAborted();
    await this.#record("sdk_inspect_request", {
      session: this.#client.session,
    });
    const result = await this.#client.inspect();
    await this.#record("sdk_inspect_response", result);
    return result.entities;
  }
}

export class ProductionDriverFactory implements HarnessDriverFactory {
  async connect(
    url: string,
    options: DriverConnectOptions,
  ): Promise<HarnessDriver> {
    const client = await IppClient.connectWebSocket(url, {
      timeoutMs: 5_000,
      signal: options.signal,
    });
    try {
      await options.record("sdk_connected", {
        schemaHash: SCHEMA_HASH,
        session: client.session,
      });
      options.signal.throwIfAborted();
      return new ProductionSdkDriver(client, options.record);
    } catch (error) {
      await client.close();
      throw error;
    }
  }

  async rejectMismatchedSchema(
    url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    const bytes = bootstrap().slice();
    const finalByte = bytes.byteLength - 1;
    bytes[finalByte] = (bytes[finalByte] ?? 0) ^ 1;
    return await rejectionProbe(url, bytes, "handshake", options);
  }

  async rejectStaleSession(
    url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    const first = await openSocket(url, options.signal);
    let firstSession: bigint;
    try {
      firstSession = await handshake(first, options.signal);
    } finally {
      await closeSocket(first);
    }
    const second = await openSocket(url, options.signal);
    try {
      const secondSession = await handshake(second, options.signal);
      if (secondSession === firstSession) {
        throw new Error("server reused a live session identity");
      }
      const staleRequest = encodeRequest({
        session: firstSession,
        requestId: 1n,
        body: {
          kind: "batch",
          batch: {
            id: 1n,
            operations: [
              Entity.create(1, { symbolicId: "stale-must-not-apply" }),
            ],
          },
        },
      });
      return await expectRejection(second, staleRequest, "request", options);
    } finally {
      await closeSocket(second);
    }
  }

  async rejectMalformed(
    url: string,
    malformedCase: MalformedCase,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    if (malformedCase === "no-bootstrap") {
      const bytes = encodeRequest({
        session: 1n,
        requestId: 1n,
        body: { kind: "inspect", collection: "entities" },
      });
      return await rejectionProbe(url, bytes, "handshake", options);
    }

    const socket = await openSocket(url, options.signal);
    try {
      const session = await handshake(socket, options.signal);
      let bytes: Uint8Array;
      if (malformedCase === "oversized-message") {
        bytes = new Uint8Array(MAX_MESSAGE_BYTES + 1);
      } else {
        const valid = encodeRequest({
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
      return await expectRejection(socket, bytes, "request", options);
    } finally {
      await closeSocket(socket);
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

function sdkReference(
  reference: EntityReference,
  aliases: ReadonlyMap<string, number>,
): SdkEntityRef {
  if (reference.kind === "entity") {
    return Entity.handle(reference.entity.bits);
  }
  const numericAlias = aliases.get(reference.alias);
  if (numericAlias === undefined) {
    throw new Error(
      `alias '${reference.alias}' has not been created in this batch`,
    );
  }
  return Entity.alias(numericAlias);
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

function normalizeObservation(snapshot: EntitySnapshot): EntityObservation {
  const scalarBase = snapshot.base.find(
    (component) => component.component === components.Scalar.id,
  );
  const scalarEffective = snapshot.effective.find(
    (component) => component.component === components.Scalar.id,
  );
  const driver = snapshot.base.find(
    (component) => component.component === components.LinearDriver.id,
  );
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

async function rejectionProbe(
  url: string,
  bytes: Uint8Array,
  stage: ProtocolRejection["stage"],
  options: DriverConnectOptions,
): Promise<ProtocolRejection> {
  const socket = await openSocket(url, options.signal);
  try {
    return await expectRejection(socket, bytes, stage, options);
  } finally {
    await closeSocket(socket);
  }
}

async function expectRejection(
  socket: WebSocket,
  bytes: Uint8Array,
  stage: ProtocolRejection["stage"],
  options: DriverConnectOptions,
): Promise<ProtocolRejection> {
  await options.record("transport_invalid_input", {
    stage,
    bytes: bytes.byteLength,
  });
  const closed = waitForClose(socket, options.signal);
  socket.send(bytes.slice().buffer);
  const event = await closed;
  return {
    rejected: event.code !== 1000,
    stage,
    code: `websocket-close-${event.code}`,
    detail: event.reason === "" ? "connection rejected" : event.reason,
  };
}

async function handshake(
  socket: WebSocket,
  signal: AbortSignal,
): Promise<bigint> {
  const reply = receiveOne(socket, signal);
  socket.send(bootstrap().slice().buffer);
  return acceptBootstrap(await reply);
}

async function openSocket(
  url: string,
  signal: AbortSignal,
): Promise<WebSocket> {
  signal.throwIfAborted();
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  return await new Promise((resolve, reject) => {
    const cleanup = (): void => {
      socket.removeEventListener("open", onOpen);
      socket.removeEventListener("error", onError);
      socket.removeEventListener("close", onClose);
      signal.removeEventListener("abort", onAbort);
    };
    const onOpen = (): void => {
      cleanup();
      resolve(socket);
    };
    const onError = (): void => {
      cleanup();
      reject(new Error("WebSocket connection failed"));
    };
    const onClose = (event: CloseEvent): void => {
      cleanup();
      reject(
        new Error(
          `WebSocket closed during connect (code=${event.code}, reason=${event.reason})`,
        ),
      );
    };
    const onAbort = (): void => {
      cleanup();
      socket.close();
      reject(signal.reason ?? new Error("WebSocket connection cancelled"));
    };
    socket.addEventListener("open", onOpen, { once: true });
    socket.addEventListener("error", onError, { once: true });
    socket.addEventListener("close", onClose, { once: true });
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

async function receiveOne(
  socket: WebSocket,
  signal: AbortSignal,
): Promise<Uint8Array> {
  signal.throwIfAborted();
  return await new Promise((resolve, reject) => {
    const cleanup = (): void => {
      socket.removeEventListener("message", onMessage);
      socket.removeEventListener("error", onError);
      socket.removeEventListener("close", onClose);
      signal.removeEventListener("abort", onAbort);
    };
    const onMessage = (event: MessageEvent): void => {
      cleanup();
      void binaryData(event.data).then(resolve, reject);
    };
    const onError = (): void => {
      cleanup();
      reject(new Error("WebSocket failed while awaiting a message"));
    };
    const onClose = (event: CloseEvent): void => {
      cleanup();
      reject(
        new Error(
          `WebSocket closed while awaiting a message (code=${event.code}, reason=${event.reason})`,
        ),
      );
    };
    const onAbort = (): void => {
      cleanup();
      reject(signal.reason ?? new Error("WebSocket receive cancelled"));
    };
    socket.addEventListener("message", onMessage, { once: true });
    socket.addEventListener("error", onError, { once: true });
    socket.addEventListener("close", onClose, { once: true });
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

async function waitForClose(
  socket: WebSocket,
  signal: AbortSignal,
): Promise<CloseEvent> {
  signal.throwIfAborted();
  return await new Promise((resolve, reject) => {
    const cleanup = (): void => {
      socket.removeEventListener("close", onClose);
      signal.removeEventListener("abort", onAbort);
    };
    const onClose = (event: CloseEvent): void => {
      cleanup();
      resolve(event);
    };
    const onAbort = (): void => {
      cleanup();
      reject(signal.reason ?? new Error("WebSocket close wait cancelled"));
    };
    socket.addEventListener("close", onClose, { once: true });
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

async function closeSocket(socket: WebSocket): Promise<void> {
  if (socket.readyState === WebSocket.CLOSED) {
    return;
  }
  const closed = new Promise<void>((resolve) => {
    const finish = (): void => {
      clearTimeout(timer);
      socket.removeEventListener("close", finish);
      resolve();
    };
    const timer = setTimeout(finish, 1_000);
    timer.unref();
    socket.addEventListener("close", finish, { once: true });
  });
  socket.close();
  await closed;
}

async function binaryData(data: unknown): Promise<Uint8Array> {
  if (data instanceof ArrayBuffer) {
    return new Uint8Array(data);
  }
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(
      data.buffer,
      data.byteOffset,
      data.byteLength,
    ).slice();
  }
  if (data instanceof Blob) {
    return new Uint8Array(await data.arrayBuffer());
  }
  throw new Error(
    `expected a binary WebSocket message, received ${typeof data}`,
  );
}

async function abortable<T>(
  promise: Promise<T>,
  signal: AbortSignal,
): Promise<T> {
  signal.throwIfAborted();
  return await new Promise<T>((resolve, reject) => {
    const cleanup = (): void => signal.removeEventListener("abort", onAbort);
    const onAbort = (): void => {
      cleanup();
      reject(signal.reason ?? new Error("operation cancelled"));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    void promise.then(
      (value) => {
        cleanup();
        resolve(value);
      },
      (error: unknown) => {
        cleanup();
        reject(error);
      },
    );
  });
}
