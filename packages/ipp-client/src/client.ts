import { GuiObservations } from "./gui-observations.js";
import { LifecycleWatches } from "./lifecycle-watches.js";
import type {
  LifecycleTargetSelection,
  LifecycleTargetWatch,
  LifecycleWatchEvent,
} from "./lifecycle-types.js";
import {
  BatchIdentities,
  type CommandBatchSink,
  type CommandBatchWriter,
  CommandPageCodec,
  type CommandPageLimits,
  type EncodedCommandPage,
  encodeCommandPages,
  openCommandBatch,
  submitCommandPages,
} from "./command-pages.js";
import { ClientAssetSources, clientAssetSource } from "./asset-sources.js";
import type { ResourceUrlMapping } from "./resource-urls.js";
export type { ResourceUrlMapping } from "./resource-urls.js";
/** Target-independent session base for generated concrete clients. */
import type {
  LifecycleFilter,
  LifecycleNotification,
  LifecycleSubscription,
  AnimationControllerCommand,
  AnimationControllerDescription,
  AnimationControllerTransition,
  AnimationPlaybackControl,
  AnimationPlaybackEvent,
  BatchOutcome,
  Command,
  ComponentDescriptor,
  Inspection,
  Request,
  RequestBody,
  Response,
  AssetResourceSnapshot,
  ClientAssetSource,
  GeometryPickQuery,
  GeometryPickResultEvent,
  CameraProjectQuery,
  CameraProjectResultEvent,
  SystemQuery,
  SystemQueryResult,
  SystemCommand,
  RenderStateUpdatedEvent,
} from "./types.js";
import type { WorldDescriptor, WorldManifest } from "./host-protocol.js";
import type { WorldReference } from "./types.js";
import type { BatchIdentitySource, MessageTransport } from "./transport.js";
import { DiagnosticLogger, logLevelValue, type LogLevel } from "./logging.js";

export type { LogLevel } from "./logging.js";

/**
 * Root-independent GUI effect observation. Control values are ordinary
 * component fields: read them through inspection, observe them through
 * lifecycle value watches and change them conditionally with `setFieldIf`.
 * Semantic actions are `guiAction` batch commands (`Entity.guiAction`).
 */
export interface GuiWorldClient extends CanvasWorldClient {
  subscribeGuiEffects(
    listener: (effect: import("./gui-types.js").GuiObservedEffect) => void,
    options?: import("./gui-types.js").GuiObservationOptions,
  ): Promise<import("./gui-types.js").GuiEffectSubscription>;
}

/** A World that selects the Canvas System takes `CanvasStateUpdateCommand`s:
 * sparse, uncorrelated updates of its canvas extent and density; one that
 * selects GUI also takes `GuiPreferencesUpdateCommand`s. */
export interface CanvasWorldClient extends Client {
  sendCommand(command: SystemCommand): void;
}

/** Terminal local session state; it does not acknowledge remote declaration cleanup. */
export interface ClientClosure {
  readonly reason: Error;
}

/** Public asynchronous session contract implemented by each generated client. */
export interface Client {
  watchLifecycle(
    targets: readonly LifecycleTargetSelection[],
    listener: (event: LifecycleWatchEvent) => void,
  ): Promise<LifecycleTargetWatch>;
  readonly session: bigint;
  readonly closure: ClientClosure | undefined;
  readonly closed: Promise<ClientClosure>;
  readonly world?: WorldDescriptor | undefined;
  readonly worldReference?: WorldReference | undefined;
  readonly manifest?: WorldManifest | undefined;
  readonly schemaHash: bigint;
  readonly components: Readonly<Record<string, ComponentDescriptor>>;
  subscribeLifecycle(
    filter: LifecycleFilter,
    listener: (event: LifecycleNotification) => void,
  ): Promise<LifecycleSubscription>;
  onRuntimeFailure(
    listener: (
      failure: import("./types.js").RuntimeFailure & { tick: bigint },
    ) => void,
  ): () => void;
  onBatchAborted(
    listener: (failure: {
      batchId: bigint;
      message: string;
      tick: bigint;
    }) => void,
  ): () => void;
  /**
   * Apply one logical batch. Large batches travel as several pages sent back to
   * back; the Host applies the whole batch at its final page, whose reply
   * resolves this promise.
   */
  batch(operations: Command[]): Promise<BatchOutcome>;
  /** Open a batch for a producer whose commands arrive over time. */
  openBatch(): CommandBatchWriter;
  attachmentRetirement(receipt: bigint): Promise<"pending" | "retired">;
  releaseAttachmentReceipt(receipt: bigint): Promise<void>;
  inspectPage(
    query?: import("./types.js").InspectionQuery,
  ): Promise<import("./types.js").InspectionPage>;
  inspectTreePage(
    query?: import("./types.js").EntityTreeQuery,
  ): Promise<import("./types.js").EntityTreePage>;
  inspect(): Promise<Inspection>;
  waitForFrame(afterTick?: bigint): Promise<{ tick: bigint; time: number }>;
  close(): Promise<void>;
}

/** Generic data-plane and resource observations, independent of rendering. */
export interface AssetWorldClient extends Client {
  /** Register and prepare immutable named bytes; resolves on registration, not readiness. */
  registerAsset(
    resource: import("./types.js").ClientAssetSource,
    bytes: ArrayBuffer,
  ): Promise<void>;
  /** Release preparation/source ownership while preserving actual consumers. */
  releaseAsset(resource: import("./types.js").ClientAssetSource): Promise<void>;
  /** Publish immutable bytes through the provider data plane and return their source. */
  createAsset(
    kind: number,
    bytes: ArrayBuffer,
    variant?: number,
  ): Promise<ClientAssetSource>;
  onResourceChange(
    listener: (resource: AssetResourceSnapshot) => void,
  ): () => void;
}

/** Resource operations retained by the world client API. */
export interface SpatialWorldClient extends AssetWorldClient {}

/** World-owned controller operations and shared playback observations. */
export interface AnimationWorldClient extends AssetWorldClient {
  encodeAnimationClip(
    clip: import("./types.js").AnimationClipSource,
  ): Uint8Array<ArrayBuffer>;
  createAnimationController(
    description: AnimationControllerDescription,
  ): Promise<bigint>;
  updateAnimationController(
    id: bigint,
    description: AnimationControllerDescription,
  ): Promise<void>;
  transitionAnimationController(
    id: bigint,
    transition: AnimationControllerTransition,
  ): Promise<void>;
  deleteAnimationController(id: bigint): Promise<void>;
  controlAnimationController(
    id: bigint,
    control: AnimationPlaybackControl,
  ): Promise<void>;
  playback(controller: bigint, control: AnimationPlaybackControl): void;
  onPlaybackEvent(
    listener: (event: AnimationPlaybackEvent) => void,
  ): () => void;
}

/** Generated System command submission; camera view selection belongs to the Host. */
export interface CameraWorldClient extends SpatialWorldClient {
  sendCommand(command: SystemCommand): void;
  navigateCamera(
    request: import("./types.js").CameraNavigateRequest,
  ): Promise<void>;
}

/** Optional geometry picking is available without a renderer. */
export interface PickingWorldClient extends CameraWorldClient {
  query(query: GeometryPickQuery): Promise<GeometryPickResultEvent>;
  query(query: CameraProjectQuery): Promise<CameraProjectResultEvent>;
  query(query: SystemQuery): Promise<SystemQueryResult>;
}

/** Session rendering settings, independently observable after commit. */
export interface RenderWorldClient extends CameraWorldClient {
  onRenderStateUpdated(
    listener: (event: RenderStateUpdatedEvent) => void,
  ): () => void;
}

export function validateOptions(options: ConnectOptions): void {
  logLevelValue(options.logLevel);
  const timeoutMs = options.timeoutMs ?? 10_000;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || timeoutMs > 60_000) {
    throw new RangeError("timeoutMs must be in (0, 60000]");
  }
  options.signal?.throwIfAborted();
}

export interface ConnectOptions {
  timeoutMs?: number;
  signal?: AbortSignal;
  /** Console verbosity; worker connections also configure their runtime host. */
  logLevel?: LogLevel;
}

/** Worker connection transfers the optional canvas to its owning runtime host. */
export interface WorkerConnectOptions extends ConnectOptions {
  canvas?: OffscreenCanvas;
  /**
   * Soft target in bytes for completed assets the Host keeps after their last
   * consumer. Omitted keeps the Host default (64 MiB); 0 evicts on release.
   */
  assetCacheBytes?: number;
  /** HTTP fetch locations; retained resource identities are unchanged. */
  resourceUrls?: readonly ResourceUrlMapping[];
}

/** Convenience connection that creates and owns a temporary World. */
export interface WorldConnectOptions extends ConnectOptions {
  /** Registered System names the temporary World instantiates, exactly.
   * Required: there is no default selection. */
  selectedSystems: readonly string[];
}

/** Worker convenience connection that creates and owns a temporary World. */
export interface WorkerWorldConnectOptions
  extends WorkerConnectOptions,
    WorldConnectOptions {}

/** A local rejection with no bytes submitted; corrected work can reuse the session. */
export class RequestNotSentError extends Error {
  readonly code = "IPP_REQUEST_NOT_SENT";

  constructor(message: string, cause?: unknown) {
    super(message, { cause });
    this.name = "RequestNotSentError";
  }
}

/** The host rejected ingress before queueing or applying the request. */
export class RequestRejectedError extends Error {
  readonly code = "IPP_REQUEST_REJECTED";

  constructor(message: string) {
    super(message);
    this.name = "RequestRejectedError";
  }
}

interface Pending {
  expectedEvent: string | undefined;
  /** A final batch page is answered only by its batch outcome or an error. */
  expectsBatch: boolean;
  resolve: (response: Response) => void;
  reject: (error: Error) => void;
  /** The reply deadline, from when the request goes on the wire. */
  timer: ReturnType<typeof setTimeout> | undefined;
}

interface FrameWaiter {
  afterTick: bigint;
  resolve: (frame: { tick: bigint; time: number }) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

let lifecycleStatisticsExchange: (
  client: Client,
  output: bigint,
) => Promise<import("./lifecycle-diagnostics.js").LifecycleDiagnosticSample>;

/**
 * The lifecycle counter exchange of a generated client. Reached through
 * `lifecycleDiagnostics` in `@ipp/client/diagnostics`; the package root and
 * generated clients expose no method for it.
 */
export function requestLifecycleStatistics(
  client: Client,
  output: bigint,
): Promise<import("./lifecycle-diagnostics.js").LifecycleDiagnosticSample> {
  return lifecycleStatisticsExchange(client, output);
}

/** One fresh world session. Commands and responses are always session-fenced. */
export abstract class ClientBase implements Client {
  static {
    // Generated clients load their own copy of this module, so the exchange
    // is found by its member rather than by class identity.
    lifecycleStatisticsExchange = (client, output) => {
      const base = client as ClientBase;
      if (typeof base.submitLifecycleStatistics !== "function")
        throw new TypeError(
          "Lifecycle diagnostics require a generated IPP client",
        );
      return base.submitLifecycleStatistics(output);
    };
  }

  private readonly batchFailures = new Set<
    (failure: { batchId: bigint; message: string; tick: bigint }) => void
  >();

  onBatchAborted(
    listener: (failure: {
      batchId: bigint;
      message: string;
      tick: bigint;
    }) => void,
  ): () => void {
    this.batchFailures.add(listener);
    return () => this.batchFailures.delete(listener);
  }

  private readonly runtimeFailures = new Set<
    (failure: import("./types.js").RuntimeFailure & { tick: bigint }) => void
  >();

  onRuntimeFailure(
    listener: (
      failure: import("./types.js").RuntimeFailure & { tick: bigint },
    ) => void,
  ): () => void {
    this.runtimeFailures.add(listener);
    return () => {
      this.runtimeFailures.delete(listener);
    };
  }

  abstract readonly schemaHash: bigint;
  abstract readonly components: Readonly<Record<string, ComponentDescriptor>>;
  private sessionId = 0n;
  private readonly assetSources: ClientAssetSources;
  private worldDescriptor?: WorldDescriptor;
  private attachedWorldReference?: WorldReference;
  private worldManifest?: WorldManifest;
  private nextId = 1n;
  private observedTick = 0n;
  private pending = new Map<bigint, Pending>();
  private readonly batchIdentities: BatchIdentitySource;
  private latestFrame?: { tick: bigint; time: number };
  private frameWaiters = new Set<FrameWaiter>();
  private resourceListeners = new Set<
    (resource: AssetResourceSnapshot) => void
  >();
  private renderStateListeners = new Set<
    (event: RenderStateUpdatedEvent) => void
  >();
  private playbackListeners = new Set<
    (event: AnimationPlaybackEvent) => void
  >();
  private nextLifecycleSubscription = 1n;
  private lifecycleListeners = new Map<
    bigint,
    (event: LifecycleNotification) => void
  >();
  private readonly guiObservations = new GuiObservations({
    nextId: () => this.nextId++,
    send: (request, control) =>
      this.request({ kind: "guiObservation", control }, request),
    definitelyUnapplied: (error) =>
      error instanceof RequestNotSentError ||
      error instanceof RequestRejectedError,
    fail: (error) => this.stop(error),
  });
  private stopped = false;
  private readonly lifecycleWatches = new LifecycleWatches({
    nextId: () => this.nextId++,
    send: (request, control) =>
      this.request({ kind: "lifecycleWatch", control }, request),
    definitelyUnapplied: (error) =>
      error instanceof RequestNotSentError ||
      error instanceof RequestRejectedError,
    fail: (error) => this.stop(error),
  });
  private terminalClosure: ClientClosure | undefined;
  private resolveClosed!: (closure: ClientClosure) => void;
  readonly closed = new Promise<ClientClosure>((resolve) => {
    this.resolveClosed = resolve;
  });
  private readonly timeoutMs: number;
  private readonly logger: DiagnosticLogger;

  protected constructor(
    private readonly transport: MessageTransport,
    options: ConnectOptions = {},
  ) {
    this.timeoutMs = options.timeoutMs ?? 10_000;
    this.batchIdentities = transport.batchIdentities ?? new BatchIdentities();
    this.logger = new DiagnosticLogger("client", options.logLevel);
    this.assetSources = new ClientAssetSources(
      () => this.sessionId,
      (bytes) => this.transport.send(bytes) ?? undefined,
      this.timeoutMs,
      (error) => this.stop(error),
    );
  }

  protected abstract readonly lifecyclePageMembers: number;
  /** The target contract's batch page bounds. */
  protected abstract readonly commandPageLimits: CommandPageLimits;
  protected abstract encodeRequest(request: Request): Uint8Array<ArrayBuffer>;
  protected abstract decodeResponse(
    bytes: Uint8Array,
    session: bigint,
  ): Response;

  get session(): bigint {
    return this.sessionId;
  }

  get closure(): ClientClosure | undefined {
    return this.terminalClosure;
  }

  get world(): WorldDescriptor | undefined {
    return this.worldDescriptor;
  }

  get worldReference(): WorldReference | undefined {
    return this.attachedWorldReference;
  }

  get manifest(): WorldManifest | undefined {
    return this.worldManifest;
  }

  /** The Host connection accepted the Host's contract and opened this fresh World session. */
  protected initializeAttached(
    session: bigint,
    world: WorldDescriptor,
    manifest: WorldManifest,
    reference: WorldReference,
  ): this {
    if (session === 0n || this.sessionId !== 0n)
      throw new Error("Invalid World session");
    this.sessionId = session;
    this.worldDescriptor = world;
    this.attachedWorldReference = reference;
    this.worldManifest = manifest;
    this.transport.start({
      ready: () => {},
      error: (error) => this.stop(error, true),
      closed: () => this.stop(new Error("World session closed"), true),
      message: (bytes) => {
        if (!this.stopped) {
          try {
            this.receive(bytes);
          } catch (error) {
            this.stop(asError(error));
          }
        }
      },
    });
    this.logger.log("info", "session.connected", () => ({
      session: this.sessionId,
    }));
    return this;
  }

  private receive(bytes: Uint8Array): void {
    if (this.assetSources.receive(bytes)) return;
    const response = this.decodeResponse(bytes, this.sessionId);
    if (this.guiObservations.receive(response)) return;
    if (this.lifecycleWatches.receive(response)) return;
    if (
      response.body.kind === "guiObservation" ||
      response.body.kind === "lifecycleWatch" ||
      response.body.kind === "lifecycleDiagnostics"
    ) {
      if (response.tick !== 0n)
        throw new Error("Invalid GUI observation outer tick");
    } else {
      if (response.tick < this.observedTick)
        throw new Error("Response tick moved backwards");
      this.observedTick = response.tick;
    }
    if (response.requestId === 0n) {
      if (response.body.kind === "batchAborted") {
        const failure = response.body;
        this.logger.log("warn", "command.batch_aborted", () => ({
          session: this.sessionId,
          batch: failure.batchId,
          reason: failure.message,
        }));
        for (const listener of [...this.batchFailures]) {
          try {
            listener({ ...failure, tick: response.tick });
          } catch (error) {
            this.logger.log("warn", "batch failure listener threw", () => ({
              error: String(error),
            }));
          }
        }
        return;
      }
      if (response.body.kind === "lifecycleEvents") {
        for (const event of response.body.events) {
          const listener = this.lifecycleListeners.get(event.subscription);
          if (listener)
            this.notifyLifecycle(listener, {
              session: response.session,
              requestId: 0n,
              kind: "change",
              ...event,
            });
        }
        return;
      }
      if (response.body.kind === "runtimeFailure") {
        for (const listener of [...this.runtimeFailures]) {
          try {
            listener({ ...response.body, tick: response.tick });
          } catch (error) {
            this.logger.log("warn", "runtime failure listener threw", () => ({
              error: String(error),
            }));
          }
        }
        return;
      }
      if (response.body.kind === "event") {
        if (response.body.event.type === "RenderStateUpdatedEvent") {
          this.publishRenderState(response);
          return;
        }
      }
      if (response.body.kind === "playback") {
        for (const payload of response.body.events)
          for (const listener of [...this.playbackListeners]) {
            try {
              listener({
                session: response.session,
                requestId: response.requestId,
                tick: response.tick,
                ...payload,
                controller: { ...payload.controller },
              });
            } catch (error) {
              try {
                globalThis.reportError?.(error);
              } catch {}
            }
          }
        return;
      }
      if (response.body.kind === "resources") {
        for (const resource of response.body.resources) {
          // Lifecycle observations are emitted by the resource through its manager.
          if (resource.status === "failed") {
            this.logger.message(
              "error",
              () =>
                `IPP ${resource.kind} resource failed: ${resource.source}: ${resource.error}`,
            );
          } else if (
            resource.status === "loaded" ||
            resource.status === "unloaded"
          ) {
            this.logger.log("info", `resource.${resource.status}`, () => ({
              session: this.sessionId,
              resource: resource.id,
              kind: resource.kind,
              source: resource.source,
            }));
          }
          for (const listener of [...this.resourceListeners]) {
            try {
              listener({ ...resource });
            } catch (error) {
              // Error reporting itself must not tear down the session either.
              try {
                globalThis.reportError?.(error);
              } catch {}
            }
          }
        }
        return;
      }
      if (response.body.kind !== "frame")
        throw new Error("Expected unsolicited frame event");
      if (
        this.latestFrame &&
        (response.tick <= this.latestFrame.tick ||
          response.body.time < this.latestFrame.time)
      )
        throw new Error("Frame tick or time moved backwards");
      const frame = { tick: response.tick, time: response.body.time };
      this.latestFrame = frame;
      for (const waiter of this.frameWaiters) {
        if (frame.tick <= waiter.afterTick) continue;
        this.frameWaiters.delete(waiter);
        clearTimeout(waiter.timer);
        waiter.resolve({ ...frame });
      }
      return;
    }
    if (response.body.kind === "frame" || response.body.kind === "resources")
      throw new Error("Reserved frame event identity");
    const pending = this.pending.get(response.requestId);
    if (!pending) throw new Error("Unexpected response request identity");
    if (
      pending.expectedEvent &&
      response.body.kind !== "error" &&
      (response.body.kind !== "event" ||
        response.body.event.type !== pending.expectedEvent)
    )
      throw new Error("Invalid query response correlation");
    if (
      pending.expectsBatch &&
      response.body.kind !== "error" &&
      response.body.kind !== "batch"
    )
      throw new Error("Batch response correlation mismatch");
    this.pending.delete(response.requestId);
    clearTimeout(pending.timer);
    if (response.body.kind === "error") {
      const message = `Host ${response.body.code}: ${response.body.message}`;
      this.logger.log("warn", "command.rejected", () => ({
        session: this.sessionId,
        request: response.requestId,
        reason: message,
      }));
      // Code 1 is the host's enqueue rejection. Code 3 can mean an outcome
      // could not be encoded after commit, so it cannot authorize a retry.
      pending.reject(
        response.body.code === 1
          ? new RequestRejectedError(message)
          : new Error(message),
      );
    } else {
      pending.resolve(response);
    }
  }

  /**
   * Correlated request. The connection's transport holds it until its flow
   * control lets it leave, so a busy Host delays requests instead of refusing them.
   */
  private request(
    body: RequestBody,
    requestId = this.nextId++,
  ): Promise<Response> {
    if (this.stopped) return Promise.reject(new Error("Client is closed"));
    let bytes: Uint8Array<ArrayBuffer>;
    try {
      bytes = this.encodeRequest({
        session: this.sessionId,
        requestId,
        body,
      });
    } catch (error) {
      return Promise.reject(
        new RequestNotSentError(asError(error).message, error),
      );
    }
    return this.sendRequest(body, requestId, bytes);
  }

  /** Correlate and send one encoded request. */
  private sendRequest(
    body: RequestBody,
    requestId: bigint,
    bytes: Uint8Array<ArrayBuffer>,
  ): Promise<Response> {
    const expectedEvent =
      body.kind === "query"
        ? body.query.type === "GeometryPickQuery"
          ? "GeometryPickResultEvent"
          : "CameraProjectResultEvent"
        : undefined;
    // Worker transport transfers and detaches the buffer once it sends it.
    const byteLength = bytes.byteLength;
    const result = this.send(
      requestId,
      bytes,
      expectedEvent,
      body.kind === "submitBatch",
    );
    this.logRequest(body, requestId, byteLength);
    return result;
  }

  private logRequest(
    body: RequestBody,
    requestId: bigint,
    byteLength: number,
  ): void {
    if (body.kind === "submitBatch" && !this.stopped) {
      this.logger.log("debug", "command.sent", () => ({
        session: this.sessionId,
        request: requestId,
        batch: body.batchId,
        last: body.last,
        operations: body.operations.length,
        bytes: byteLength,
      }));
      this.logger.log("trace", "command.kinds", () => ({
        session: this.sessionId,
        request: requestId,
        kinds: body.operations.map((operation) => operation.kind).join(","),
      }));
    }
  }

  private send(
    requestId: bigint,
    bytes: Uint8Array<ArrayBuffer>,
    expectedEvent?: string,
    expectsBatch = false,
  ): Promise<Response> {
    return new Promise<Response>((resolve, reject) => {
      const pending: Pending = {
        resolve,
        reject,
        timer: undefined,
        expectedEvent,
        expectsBatch,
      };
      this.pending.set(requestId, pending);
      // Waiting for connection credit is not a Host delay: the reply deadline
      // starts when the request goes on the wire.
      const deadline = () => {
        if (this.pending.get(requestId) === pending)
          pending.timer = setTimeout(
            () => this.stop(new Error("Request timed out")),
            this.timeoutMs,
          );
      };
      try {
        const leaving = this.transport.send(bytes);
        if (leaving) void leaving.then(deadline);
        else deadline();
      } catch (error) {
        this.stop(asError(error));
      }
    });
  }

  /** Fire-and-forget system inputs allocate no identity, waiter or timer. */
  protected submitCommand(command: SystemCommand): void {
    if (this.stopped) throw new Error("Client is closed");
    let bytes: Uint8Array<ArrayBuffer>;
    try {
      bytes = this.encodeRequest({
        session: this.sessionId,
        requestId: 0n,
        body: { kind: "command", command },
      });
    } catch (error) {
      throw new RequestNotSentError(asError(error).message, error);
    }
    try {
      this.transport.send(bytes);
    } catch (error) {
      this.stop(asError(error));
      throw error;
    }
  }

  /** Edits the exact bound Camera and rejects stale/gated gesture work without replay. */
  async navigateCamera(
    request: import("./types.js").CameraNavigateRequest,
  ): Promise<void> {
    const response = await this.request({ kind: "cameraNavigate", request });
    if (response.body.kind !== "cameraNavigated")
      throw new Error("Invalid camera navigation response correlation");
  }

  /** Queries retain a single correlated terminal result while the session lives. */
  protected async submitQuery(query: SystemQuery): Promise<SystemQueryResult> {
    const response = await this.request({ kind: "query", query });
    if (
      response.body.kind !== "event" ||
      (response.body.event.type !== "GeometryPickResultEvent" &&
        response.body.event.type !== "CameraProjectResultEvent")
    )
      throw new Error("Invalid query response correlation");
    return {
      session: response.session,
      requestId: response.requestId,
      tick: response.tick,
      ...response.body.event,
    };
  }

  /** Correlated mutations share the World’s ordered entity/controller boundary. */
  protected async submitAnimationController(
    command: AnimationControllerCommand,
  ): Promise<bigint | null> {
    const response = await this.request({
      kind: "animationController",
      command,
    });
    if (
      response.body.kind !== "animationController" ||
      (command.action === "create") !== (response.body.id !== null)
    )
      throw new Error("Invalid controller response correlation");
    return response.body.id;
  }

  protected submitGuiSubscription(
    listener: (effect: import("./gui-types.js").GuiObservedEffect) => void,
    options: import("./gui-types.js").GuiObservationOptions = {},
  ): Promise<import("./gui-types.js").GuiEffectSubscription> {
    const world = this.worldReference;
    if (!world)
      return Promise.reject(
        new RequestNotSentError(
          "GUI subscriptions require an exact World session",
        ),
      );
    return this.guiObservations.subscribe(world, listener, options);
  }

  protected async submitClientAsset(
    resource: ClientAssetSource,
    bytes?: ArrayBuffer,
  ): Promise<void> {
    if (bytes === undefined) return this.assetSources.release(resource);
    this.issuedAssetSources.add(resource.source);
    return this.assetSources.register(resource, bytes);
  }

  private nextGeneratedAsset = 1n;
  private readonly issuedAssetSources = new Set<string>();

  /** Allocate independently of author-provided names, shared across React roots. */
  protected async submitNewAsset(
    kind: number,
    bytes: ArrayBuffer,
    variant = 0,
  ): Promise<ClientAssetSource> {
    let source: ClientAssetSource;
    do {
      source = clientAssetSource(
        this.session,
        kind,
        `generated-${this.nextGeneratedAsset++}`,
        variant,
      );
    } while (this.issuedAssetSources.has(source.source));
    await this.submitClientAsset(source, bytes);
    return source;
  }

  async attachmentRetirement(receipt: bigint): Promise<"pending" | "retired"> {
    const response = await this.request({
      kind: "attachmentReceipt",
      receipt,
      release: false,
    });
    if (
      response.body.kind !== "attachmentReceipt" ||
      response.body.receipt !== receipt ||
      response.body.state === "released"
    )
      throw new Error("Invalid attachment retirement response");
    return response.body.state;
  }

  async releaseAttachmentReceipt(receipt: bigint): Promise<void> {
    const response = await this.request({
      kind: "attachmentReceipt",
      receipt,
      release: true,
    });
    if (
      response.body.kind !== "attachmentReceipt" ||
      response.body.receipt !== receipt ||
      response.body.state !== "released"
    )
      throw new Error("Invalid attachment receipt release response");
  }

  batch(operations: Command[]): Promise<BatchOutcome> {
    if (this.stopped) return Promise.reject(new Error("Client is closed"));
    let pages: EncodedCommandPage[];
    try {
      pages = encodeCommandPages(operations, this.pageCodec());
    } catch (error) {
      return Promise.reject(
        new RequestNotSentError(asError(error).message, error),
      );
    }
    try {
      return submitCommandPages(this.batchIdentities, this.batchSink(), pages);
    } catch (error) {
      return Promise.reject(asError(error));
    }
  }

  openBatch(): CommandBatchWriter {
    if (this.stopped) throw new Error("Client is closed");
    return openCommandBatch(
      this.batchIdentities,
      this.batchSink(),
      this.pageCodec(),
    );
  }

  private commandPageCodec?: CommandPageCodec;

  private pageCodec(): CommandPageCodec {
    this.commandPageCodec ??= new CommandPageCodec(
      (request) => this.encodeRequest(request),
      this.commandPageLimits,
    );
    return this.commandPageCodec;
  }

  /** Pages of one batch; only the final page is correlated with a reply. */
  private batchSink(): CommandBatchSink {
    return {
      page: (batchId, page) => {
        if (this.stopped) throw new Error("Client is closed");
        const bytes = this.pageCodec().message(
          this.sessionId,
          0n,
          batchId,
          false,
          page,
        );
        this.logRequest(
          {
            kind: "submitBatch",
            batchId,
            last: false,
            operations: page.operations,
          },
          0n,
          bytes.byteLength,
        );
        try {
          this.transport.send(bytes);
        } catch (error) {
          this.stop(asError(error));
          throw error;
        }
      },
      finish: async (batchId, page) => {
        if (this.stopped) throw new Error("Client is closed");
        const requestId = this.nextId++;
        let bytes: Uint8Array<ArrayBuffer>;
        try {
          bytes = this.pageCodec().message(
            this.sessionId,
            requestId,
            batchId,
            true,
            page,
          );
        } catch (error) {
          throw new RequestNotSentError(asError(error).message, error);
        }
        const response = await this.sendRequest(
          {
            kind: "submitBatch",
            batchId,
            last: true,
            operations: page.operations,
          },
          requestId,
          bytes,
        );
        if (
          response.body.kind !== "batch" ||
          response.body.outcome.batchId !== BigInt(batchId) ||
          response.body.outcome.tick !== response.tick
        ) {
          this.stop(new Error("Batch response correlation mismatch"));
          throw new Error("Batch response correlation mismatch");
        }
        const outcome = response.body.outcome;
        this.logger.log(
          outcome.ok ? "debug" : "warn",
          outcome.ok ? "command.completed" : "command.rejected",
          () => ({
            session: this.sessionId,
            request: response.requestId,
            batch: batchId,
            tick: outcome.tick,
          }),
        );
        return outcome;
      },
    };
  }

  /**
   * Observe host progress without sending a request or advancing simulation.
   * With no threshold, wait for a newly received frame after the last observed tick.
   * An explicit threshold can reuse the latest newer frame already received.
   * A timeout rejects this wait only; the session remains connected.
   */
  async waitForFrame(
    afterTick?: bigint,
  ): Promise<{ tick: bigint; time: number }> {
    if (this.stopped) throw new Error("Client is closed");
    const threshold = afterTick ?? this.observedTick;
    if (
      typeof threshold !== "bigint" ||
      threshold < 0n ||
      threshold > 0xffffffffffffffffn
    )
      throw new RangeError("afterTick must be a u64 bigint");
    if (
      afterTick !== undefined &&
      this.latestFrame &&
      this.latestFrame.tick > threshold
    )
      return { ...this.latestFrame };
    if (this.frameWaiters.size >= 64) throw new Error("Frame waiter limit");
    return new Promise((resolve, reject) => {
      const waiter: FrameWaiter = {
        afterTick: threshold,
        resolve,
        reject,
        timer: setTimeout(() => {
          this.frameWaiters.delete(waiter);
          reject(new Error("Waiting for a frame timed out"));
        }, this.timeoutMs),
      };
      this.frameWaiters.add(waiter);
    });
  }

  async inspectPage(
    query: import("./types.js").InspectionQuery = { collection: "summary" },
  ): Promise<import("./types.js").InspectionPage> {
    const response = await this.request({ kind: "inspect", ...query });
    if (response.body.kind !== "inspect") {
      this.stop(new Error("Expected inspect response"));
      throw new Error("Expected inspect response");
    }
    return {
      next: response.body.next,
      tick: response.tick,
      time: response.body.time,
      entities: response.body.entities,
      resources: response.body.resources,
      renderDiagnostics: response.body.renderDiagnostics,
      ...(response.body.controllers
        ? { controllers: response.body.controllers }
        : {}),
      ...(response.body.guiFocus ? { guiFocus: response.body.guiFocus } : {}),
      ...(response.body.guiPointers
        ? { guiPointers: response.body.guiPointers }
        : {}),
      ...(response.body.guiActiveItems
        ? { guiActiveItems: response.body.guiActiveItems }
        : {}),
      ...(response.body.canvas !== undefined
        ? { canvas: response.body.canvas }
        : {}),
      ...(response.body.guiPreferences !== undefined
        ? { guiPreferences: response.body.guiPreferences }
        : {}),
    };
  }

  async inspectTreePage(
    query: import("./types.js").EntityTreeQuery = {},
  ): Promise<import("./types.js").EntityTreePage> {
    const response = await this.request({ kind: "inspectTree", ...query });
    if (response.body.kind !== "entityTree") {
      this.stop(new Error("Expected entity tree response"));
      throw new Error("Expected entity tree response");
    }
    return {
      tick: response.tick,
      time: response.body.time,
      next: response.body.next,
      nodes: response.body.nodes,
    };
  }

  /** Collect independent pages. Their ticks may differ; this is not an atomic snapshot. */
  async inspect(): Promise<Inspection> {
    const manifest = this.manifest;
    if (!manifest) throw new Error("World manifest unavailable");
    const result: Inspection = {
      tick: 0n,
      time: 0,
      entities: [],
      resources: [],
      renderDiagnostics: [],
      controllers: [],
    };
    for (const collection of [
      "entities",
      "resources",
      "controllers",
      "renderDiagnostics",
    ] as const) {
      if (
        (collection === "controllers" &&
          !manifest.operations.includes("animation")) ||
        (collection === "renderDiagnostics" &&
          !manifest.operations.includes("rendering"))
      )
        continue;
      let after = 0n;
      do {
        const page = await this.inspectPage({ collection, after });
        result.tick = page.tick;
        result.time = page.time;
        result.entities.push(...page.entities);
        result.resources = [...result.resources, ...page.resources];
        result.controllers = [
          ...(result.controllers ?? []),
          ...(page.controllers ?? []),
        ];
        result.renderDiagnostics = [
          ...result.renderDiagnostics,
          ...page.renderDiagnostics,
        ];
        after = page.next;
      } while (after !== 0n);
    }
    return result;
  }

  protected addPlaybackListener(
    listener: (event: AnimationPlaybackEvent) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    this.playbackListeners.add(listener);
    return () => this.playbackListeners.delete(listener);
  }

  /** Generated world clients expose committed settings notifications. */
  protected addRenderStateListener(
    listener: (event: RenderStateUpdatedEvent) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    this.renderStateListeners.add(listener);
    return () => this.renderStateListeners.delete(listener);
  }

  private publishRenderState(response: Response): void {
    if (
      response.body.kind !== "event" ||
      response.body.event.type !== "RenderStateUpdatedEvent"
    )
      return;
    for (const listener of [...this.renderStateListeners]) {
      const payload = response.body.event;
      const event: RenderStateUpdatedEvent = {
        session: response.session,
        requestId: response.requestId,
        tick: response.tick,
        ...payload,
      };
      event.changes = {
        ...event.changes,
        ...(event.changes.ambientLight
          ? {
              ambientLight: [...event.changes.ambientLight] as [
                number,
                number,
                number,
              ],
            }
          : {}),
        ...(event.changes.debugGeometryColor
          ? {
              debugGeometryColor: [...event.changes.debugGeometryColor] as [
                number,
                number,
                number,
              ],
            }
          : {}),
      };
      try {
        listener(event);
      } catch (error) {
        try {
          globalThis.reportError?.(error);
        } catch {}
      }
    }
  }

  /** Subscribe to subsequent applied effects through the ordered production protocol. */
  watchLifecycle(
    targets: readonly LifecycleTargetSelection[],
    listener: (event: LifecycleWatchEvent) => void,
  ): Promise<LifecycleTargetWatch> {
    const world = this.worldReference;
    if (!world)
      return Promise.reject(
        new RequestNotSentError(
          "Lifecycle targets require an exact World session",
        ),
      );
    return this.lifecycleWatches.watch(
      world,
      targets,
      listener,
      this.lifecyclePageMembers,
    );
  }

  private async submitLifecycleStatistics(
    output: bigint,
  ): Promise<import("./lifecycle-diagnostics.js").LifecycleDiagnosticSample> {
    const world = this.worldReference;
    if (!world || output <= 0n)
      throw new RequestNotSentError(
        "Lifecycle diagnostics require an acknowledged exact endpoint",
      );
    const response = await this.request({
      kind: "lifecycleDiagnostics",
      query: { world, output },
    });
    if (
      response.body.kind !== "lifecycleDiagnostics" ||
      response.tick !== 0n ||
      response.body.sample.world.id !== world.id ||
      response.body.sample.world.incarnation !== world.incarnation ||
      response.body.sample.output !== output
    ) {
      const error = new Error("Invalid lifecycle diagnostic provenance");
      this.stop(error);
      throw error;
    }
    const sample = response.body.sample;
    return Object.freeze({
      ...sample,
      world: Object.freeze({ ...sample.world }),
      work: Object.freeze({ ...sample.work }),
      traffic: Object.freeze({ ...sample.traffic }),
    });
  }

  async subscribeLifecycle(
    filter: LifecycleFilter,
    listener: (event: LifecycleNotification) => void,
  ): Promise<LifecycleSubscription> {
    if (this.stopped) throw new Error("Client is closed");
    const id = this.nextLifecycleSubscription++;
    this.lifecycleListeners.set(id, listener);
    try {
      const response = await this.request({
        kind: "subscribeLifecycle",
        subscription: id,
        filter,
      });
      if (response.body.kind !== "lifecycleSubscription")
        throw new Error("Invalid lifecycle subscription response");
    } catch (error) {
      this.lifecycleListeners.delete(id);
      throw error;
    }
    let closing: Promise<void> | undefined;
    return {
      id,
      unsubscribe: () => {
        closing ??= (async () => {
          if (!this.lifecycleListeners.has(id)) return;
          const response = await this.request({
            kind: "unsubscribeLifecycle",
            subscription: id,
          });
          if (response.body.kind !== "lifecycleSubscription")
            throw new Error("Invalid lifecycle unsubscribe response");
          this.lifecycleListeners.delete(id);
        })();
        return closing;
      },
    };
  }

  private notifyLifecycle(
    listener: (event: LifecycleNotification) => void,
    event: LifecycleNotification,
  ): void {
    try {
      listener(event);
    } catch (error) {
      try {
        globalThis.reportError?.(error);
      } catch {}
    }
  }

  /** Generated world clients expose this when resource events are decodable. */
  protected addResourceListener(
    listener: (resource: AssetResourceSnapshot) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    this.resourceListeners.add(listener);
    return () => {
      this.resourceListeners.delete(listener);
    };
  }

  private stop(error: Error, expected = false): void {
    if (this.stopped) return;
    this.stopped = true;
    this.terminalClosure = Object.freeze({ reason: error });
    this.resolveClosed(this.terminalClosure);
    this.guiObservations.close(error);
    this.lifecycleWatches.stop(error);
    this.assetSources.close(error);
    this.runtimeFailures.clear();
    this.batchFailures.clear();
    this.playbackListeners.clear();
    this.lifecycleListeners.clear();
    this.logger.log(
      expected ? "info" : "error",
      expected ? "session.closing" : "session.failed",
      () => ({ session: this.sessionId, reason: error.message }),
    );
    for (const p of this.pending.values()) {
      clearTimeout(p.timer);
      p.reject(error);
    }
    this.pending.clear();
    for (const waiter of this.frameWaiters) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.frameWaiters.clear();
    this.resourceListeners.clear();
    this.renderStateListeners.clear();
    void this.transport.close().catch(() => {});
  }

  async close(): Promise<void> {
    const active = !this.stopped;
    this.stop(new Error("Client closed"), true);
    try {
      await this.transport.close();
      if (active)
        this.logger.log("info", "session.closed", () => ({
          session: this.sessionId,
        }));
    } catch (error) {
      if (active)
        this.logger.log("error", "session.close_failed", () => ({
          session: this.sessionId,
          reason: asError(error).message,
        }));
      throw error;
    }
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
