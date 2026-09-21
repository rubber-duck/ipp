import type { Page } from "playwright";
import type {
  BatchOutcome,
  ConcurrentRpcCorrelation,
  DriverConnectOptions,
  EntityId,
  EntityObservation,
  FrameObservation,
  HarnessDriver,
  HarnessDriverFactory,
  MalformedCase,
  ProtocolRejection,
  SceneOperation,
} from "../integration/driver.js";
import type { BrowserRuntimeConfiguration } from "./browser-runtime.js";

type RecordEvidence = DriverConnectOptions["record"];

export class BrowserDriverFactory implements HarnessDriverFactory {
  readonly #page: Page;
  readonly #runtimeModuleUrl: string;
  readonly #configuration: BrowserRuntimeConfiguration;
  readonly #mismatchConfiguration: BrowserRuntimeConfiguration;
  #nextConnection = 1;

  constructor(
    page: Page,
    runtimeModuleUrl: string,
    configuration: BrowserRuntimeConfiguration,
    mismatchConfiguration: BrowserRuntimeConfiguration,
  ) {
    this.#page = page;
    this.#runtimeModuleUrl = runtimeModuleUrl;
    this.#configuration = configuration;
    this.#mismatchConfiguration = mismatchConfiguration;
  }

  async connect(
    _url: string,
    options: DriverConnectOptions,
  ): Promise<HarnessDriver> {
    options.signal.throwIfAborted();
    const connectionId = `browser-client-${this.#nextConnection++}`;
    const connected = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, configuration }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.connect(connectionId, configuration);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId,
        configuration: this.#configuration,
      },
    );
    await options.record("browser_sdk_connected", {
      connectionId,
      schemaHash: connected.schemaHash,
      session: connected.session,
    });
    options.signal.throwIfAborted();
    return new BrowserHarnessDriver(
      this.#page,
      this.#runtimeModuleUrl,
      connectionId,
      options.record,
    );
  }

  async rejectMismatchedSchema(
    _url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    options.signal.throwIfAborted();
    await options.record("browser_schema_mismatch_probe", {
      contractModuleUrl: this.#mismatchConfiguration.contractModuleUrl,
      wasmUrl: this.#mismatchConfiguration.wasmUrl,
    });
    return await this.rejectConnection(
      this.#mismatchConfiguration,
      options.signal,
    );
  }

  async rejectStaleSession(
    _url: string,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    options.signal.throwIfAborted();
    await options.record("browser_stale_session_probe", {});
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, configuration }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.rejectStaleSession(configuration);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        configuration: this.#configuration,
      },
    );
  }

  async rejectMalformed(
    _url: string,
    malformedCase: MalformedCase,
    options: DriverConnectOptions,
  ): Promise<ProtocolRejection> {
    options.signal.throwIfAborted();
    await options.record("browser_malformed_probe", { malformedCase });
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, configuration, malformedCase }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.rejectMalformed(configuration, malformedCase);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        configuration: this.#configuration,
        malformedCase,
      },
    );
  }

  async rejectConnection(
    configuration: BrowserRuntimeConfiguration,
    signal: AbortSignal,
  ): Promise<ProtocolRejection> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, configuration }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.rejectConnection(configuration);
      },
      { runtimeModuleUrl: this.#runtimeModuleUrl, configuration },
    );
  }

  async terminatedPendingRequestRejects(
    signal: AbortSignal,
  ): Promise<{ readonly rejected: boolean; readonly detail: string }> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, configuration }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.terminatedPendingRequestRejects(configuration);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        configuration: this.#configuration,
      },
    );
  }

  async wasmExports(
    wasmUrl: string,
    signal: AbortSignal,
  ): Promise<readonly string[]> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, wasmUrl }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.wasmExports(wasmUrl);
      },
      { runtimeModuleUrl: this.#runtimeModuleUrl, wasmUrl },
    );
  }

  async contractHash(
    contractModuleUrl: string,
    signal: AbortSignal,
  ): Promise<bigint> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, contractModuleUrl }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.contractHash(contractModuleUrl);
      },
      { runtimeModuleUrl: this.#runtimeModuleUrl, contractModuleUrl },
    );
  }

  async wasmSchemaHash(wasmUrl: string, signal: AbortSignal): Promise<bigint> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, wasmUrl }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.wasmSchemaHash(wasmUrl);
      },
      { runtimeModuleUrl: this.#runtimeModuleUrl, wasmUrl },
    );
  }

  async closeErrorRejectsOnce(signal: AbortSignal): Promise<{
    readonly rejected: boolean;
    readonly detail: string;
    readonly disposals: number;
  }> {
    signal.throwIfAborted();
    return await this.#page.evaluate(
      async ({ runtimeModuleUrl, transportModuleUrl }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.closeErrorRejectsOnce(transportModuleUrl);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        transportModuleUrl: this.#configuration.transportModuleUrl,
      },
    );
  }
}

class BrowserHarnessDriver implements HarnessDriver {
  readonly #page: Page;
  readonly #runtimeModuleUrl: string;
  readonly #connectionId: string;
  readonly #record: RecordEvidence;
  #closed = false;

  constructor(
    page: Page,
    runtimeModuleUrl: string,
    connectionId: string,
    record: RecordEvidence,
  ) {
    this.#page = page;
    this.#runtimeModuleUrl = runtimeModuleUrl;
    this.#connectionId = connectionId;
    this.#record = record;
  }

  async submit(
    batchId: bigint,
    operations: readonly SceneOperation[],
    options: { readonly signal: AbortSignal },
  ): Promise<BatchOutcome> {
    options.signal.throwIfAborted();
    await this.#record("browser_sdk_batch_request", {
      connectionId: this.#connectionId,
      batchId,
      operations: operations.length,
    });
    const result = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, batchId, operations }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.submit(connectionId, batchId, operations);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
        batchId,
        operations,
      },
    );
    await this.#record("browser_sdk_batch_response", result);
    return result;
  }

  async waitForFrame(
    afterTick: bigint | undefined,
    options: { readonly signal: AbortSignal },
  ): Promise<FrameObservation> {
    options.signal.throwIfAborted();
    const result = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, afterTick }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.waitForFrame(connectionId, afterTick);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
        afterTick,
      },
    );
    await this.#record("browser_sdk_frame_observed", {
      connectionId: this.#connectionId,
      afterTick,
      requestSent: false,
      result,
    });
    return result;
  }

  async publicStepAvailable(options: {
    readonly signal: AbortSignal;
  }): Promise<boolean> {
    options.signal.throwIfAborted();
    const available = await this.#page.evaluate(
      ({ runtimeModuleUrl, connectionId }) =>
        import(runtimeModuleUrl).then(
          (runtime: typeof import("./browser-runtime.js")) =>
            runtime.publicStepAvailable(connectionId),
        ),
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
      },
    );
    await this.#record("browser_sdk_public_step_presence", {
      connectionId: this.#connectionId,
      available,
    });
    return available;
  }

  async correlateConcurrentRequests(
    marker: EntityId,
    batchIds: readonly bigint[],
    options: { readonly signal: AbortSignal },
  ): Promise<ConcurrentRpcCorrelation> {
    options.signal.throwIfAborted();
    await this.#record("browser_sdk_concurrent_rpc_burst", {
      connectionId: this.#connectionId,
      batches: batchIds.length,
      inspections: batchIds.length,
    });
    const result = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, marker, batchIds }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.correlateConcurrentRequests(
          connectionId,
          marker,
          batchIds,
        );
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
        marker,
        batchIds,
      },
    );
    await this.#record("browser_sdk_concurrent_rpc_results", result);
    return result;
  }

  async inspect(
    entity: EntityId,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null> {
    options.signal.throwIfAborted();
    const result = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, entity }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.inspect(connectionId, entity);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
        entity,
      },
    );
    await this.#record("browser_sdk_inspect_response", {
      connectionId: this.#connectionId,
      result,
    });
    return result;
  }

  async findBySymbolicId(
    symbolicId: string,
    options: { readonly signal: AbortSignal },
  ): Promise<EntityObservation | null> {
    options.signal.throwIfAborted();
    const result = await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId, symbolicId }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        return await runtime.findBySymbolicId(connectionId, symbolicId);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
        symbolicId,
      },
    );
    await this.#record("browser_sdk_find_response", {
      connectionId: this.#connectionId,
      symbolicId,
      result,
    });
    return result;
  }

  async close(): Promise<void> {
    if (this.#closed) return;
    this.#closed = true;
    await this.#page.evaluate(
      async ({ runtimeModuleUrl, connectionId }) => {
        const runtime = (await import(
          runtimeModuleUrl
        )) as typeof import("./browser-runtime.js");
        await runtime.close(connectionId);
      },
      {
        runtimeModuleUrl: this.#runtimeModuleUrl,
        connectionId: this.#connectionId,
      },
    );
    await this.#record("browser_sdk_closed", {
      connectionId: this.#connectionId,
    });
  }
}
