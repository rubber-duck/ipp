import { applyPlannedCommandPages, planCommandPages } from "./command-pages.js";
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
  StateOverlayLifecycleDiagnostic,
  Request,
  RequestBody,
  Response,
  AssetResourceSnapshot,
  ClientAssetSource,
  CameraStateChangedEvent,
  GeometryPickQuery,
  GeometryPickResultEvent,
  CameraProjectQuery,
  CameraProjectResultEvent,
  SystemQuery,
  SystemQueryResult,
  SystemCommand,
  RenderStateUpdatedEvent,
} from "./types.js";
import type { WorldDescriptor } from "./host-protocol.js";
import type { MessageTransport } from "./transport.js";
import type { ClientPresentation } from "./presentation.js";
import { DiagnosticLogger, logLevelValue, type LogLevel } from "./logging.js";

export type { LogLevel } from "./logging.js";

/** Public asynchronous session contract implemented by each generated client. */
export interface SurfaceWorldClient extends Client {
  editSurface(edit: import("./surface-types.js").SurfaceEdit): Promise<void>;
  encodeSurfaceItems(
    collection: import("./surface-types.js").SurfaceCollection,
  ): Uint8Array<ArrayBuffer>;
  decodeSurfaceItems(
    bytes: Uint8Array,
  ): import("./surface-types.js").SurfaceCollection;
}

/** Public asynchronous session contract for GUI trees and incremental edits. */
export interface GuiWorldClient extends Client {
  subscribeGuiObservations(
    listener: (batch: import("./gui-types.js").GuiObservationBatch) => void,
  ): () => void;
  editGui(edit: import("./gui-types.js").GuiEdit): Promise<void>;
  editGuiBatch(
    edits: readonly import("./gui-types.js").GuiEdit[],
  ): Promise<import("./gui-types.js").GuiEditBatchOutcome>;
  editGuiBatchChunk(
    batchId: bigint,
    edits: readonly import("./gui-types.js").GuiEdit[],
  ): Promise<import("./gui-types.js").GuiEditBatchOutcome>;
  submitGuiInput(
    input: import("./gui-types.js").GuiInputCommand,
  ): Promise<import("./gui-types.js").GuiInputRoutingOutcome>;
  inspectGui(
    query: import("./gui-types.js").GuiInspectQuery,
  ): Promise<import("./gui-types.js").GuiInspectResponse>;
  semanticSnapshot(
    query: import("./gui-types.js").GuiSemanticSnapshotQuery,
  ): Promise<import("./gui-types.js").GuiSemanticTree>;
  semanticAction(
    action: import("./gui-types.js").GuiSemanticActionRequest,
  ): Promise<void>;
  encodeGuiTree(
    tree: import("./gui-types.js").GuiTree,
  ): Uint8Array<ArrayBuffer>;
  decodeGuiTree(bytes: Uint8Array): import("./gui-types.js").GuiTree;
  createGuiNodeHandle(
    entity: bigint,
    rootIncarnation: bigint,
    nodeId: import("./gui-types.js").GuiNodeId,
    nodeLifetime: number,
  ): import("./gui-types.js").GuiNodeHandle;
}

/** Public asynchronous session contract implemented by each generated client. */
export interface Client {
  readonly presentation?: ClientPresentation | undefined;
  readonly session: bigint;
  readonly world?: WorldDescriptor | undefined;
  readonly schemaHash: bigint;
  readonly components: Readonly<Record<string, ComponentDescriptor>>;
  readonly capabilities: {
    readonly animation?: boolean;
    readonly assets?: boolean;
    readonly stateOverlays: boolean;
    readonly spatial: boolean;
    readonly textures: boolean;
    readonly builtinAssets: boolean;
    readonly picking: boolean;
    readonly debugGeometry: boolean;
    readonly pbr: boolean;
    readonly shadows: boolean;
    readonly skeletalAnimation: boolean;
    readonly meshPoses: boolean;
    readonly particles?: boolean;
    readonly surfaces?: boolean;
    readonly gui?: boolean;
  };
  subscribeLifecycle(
    filter: LifecycleFilter,
    listener: (event: LifecycleNotification) => void,
  ): Promise<LifecycleSubscription>;
  onDiagnostic(
    listener: (diagnostic: StateOverlayLifecycleDiagnostic) => void,
  ): () => void;
  onRuntimeFailure(
    listener: (
      failure: import("./types.js").RuntimeFailure & { tick: bigint },
    ) => void,
  ): () => void;
  beginBatch(): Promise<bigint>;
  batchChunk(batchId: bigint, operations: Command[]): Promise<BatchOutcome>;
  endBatch(batchId: bigint): Promise<void>;
  onBatchAborted(
    listener: (failure: {
      batchId: bigint;
      message: string;
      tick: bigint;
    }) => void,
  ): () => void;
  batch(operations: Command[], batchId?: bigint): Promise<BatchOutcome>;
  inspectPage(
    query?: import("./types.js").InspectionQuery,
  ): Promise<import("./types.js").InspectionPage>;
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

/** World camera commands and independently observed selection changes. */
export interface CameraWorldClient extends SpatialWorldClient {
  sendCommand(command: SystemCommand): void;
  onCameraStateChanged(
    listener: (event: CameraStateChangedEvent) => void,
  ): () => void;
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
  /** Host idle-resource retention target in bytes; omitted uses the runtime default. */
  assetCacheBytes?: number;
  /** HTTP fetch locations; retained resource identities are unchanged. */
  resourceUrls?: readonly ResourceUrlMapping[];
}

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
  resolve: (response: Response) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface FrameWaiter {
  afterTick: bigint;
  resolve: (frame: { tick: bigint; time: number }) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

/** One fresh world session. Commands and responses are always session-fenced. */
export abstract class ClientBase implements Client {
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

  /** Observe committed GUI effects with their conflicts, cancellations and
   * supplier-private unhandled inputs. Each host chunk arrives as one batch;
   * unhandled inputs naming another session are dropped on receipt. */
  subscribeGuiObservations(
    listener: (batch: import("./gui-types.js").GuiObservationBatch) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    if (this.latestGuiTextFocus !== undefined) {
      try {
        listener({ effects: [], textFocus: this.latestGuiTextFocus });
      } catch (error) {
        try {
          globalThis.reportError?.(error);
        } catch {}
        return () => {};
      }
    }
    this.guiObservationListeners.add(listener);
    return () => this.guiObservationListeners.delete(listener);
  }

  private publishGuiObservations(
    batch: import("./gui-types.js").GuiObservationBatch,
  ): void {
    if (batch.textFocus !== undefined)
      this.latestGuiTextFocus = batch.textFocus;
    for (const listener of [...this.guiObservationListeners]) {
      try {
        listener({
          effects: [...batch.effects],
          conflicts: [...(batch.conflicts ?? [])],
          cancellations: [...(batch.cancellations ?? [])],
          unhandled: [...(batch.unhandled ?? [])],
          ...(batch.textFocus === undefined
            ? {}
            : { textFocus: batch.textFocus }),
        });
      } catch (error) {
        try {
          globalThis.reportError?.(error);
        } catch {}
      }
    }
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
  abstract readonly capabilities: {
    readonly animation?: boolean;
    readonly assets?: boolean;
    readonly stateOverlays: boolean;
    readonly spatial: boolean;
    readonly textures: boolean;
    readonly builtinAssets: boolean;
    readonly picking: boolean;
    readonly debugGeometry: boolean;
    readonly pbr: boolean;
    readonly shadows: boolean;
    readonly skeletalAnimation: boolean;
    readonly meshPoses: boolean;
    readonly particles?: boolean;
    readonly surfaces?: boolean;
  };
  private sessionId = 0n;
  private readonly assetSources: ClientAssetSources;
  private worldDescriptor?: WorldDescriptor;
  private nextId = 1n;
  private observedTick = 0n;
  private pending = new Map<bigint, Pending>();
  private queuedRequests = 0;
  private latestFrame?: { tick: bigint; time: number };
  private frameWaiters = new Set<FrameWaiter>();
  private diagnosticListeners = new Set<
    (diagnostic: StateOverlayLifecycleDiagnostic) => void
  >();
  private resourceListeners = new Set<
    (resource: AssetResourceSnapshot) => void
  >();
  private cameraStateListeners = new Set<
    (event: CameraStateChangedEvent) => void
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
  private guiObservationListeners = new Set<
    (batch: import("./gui-types.js").GuiObservationBatch) => void
  >();
  private latestGuiTextFocus:
    | import("./gui-types.js").GuiTextFocusState
    | null
    | undefined;
  private stopped = false;
  private readonly timeoutMs: number;
  private readonly logger: DiagnosticLogger;

  protected constructor(
    private readonly transport: MessageTransport,
    options: ConnectOptions = {},
  ) {
    this.timeoutMs = options.timeoutMs ?? 10_000;
    this.logger = new DiagnosticLogger("client", options.logLevel);
    this.assetSources = new ClientAssetSources(
      () => this.sessionId,
      (bytes) => this.transport.send(bytes),
      this.timeoutMs,
      (error) => this.stop(error),
    );
  }

  protected abstract bootstrap(): Uint8Array<ArrayBuffer>;
  protected abstract acceptBootstrap(bytes: Uint8Array): bigint;
  protected abstract encodeRequest(request: Request): Uint8Array<ArrayBuffer>;
  protected abstract decodeResponse(
    bytes: Uint8Array,
    session: bigint,
  ): Response;

  get session(): bigint {
    return this.sessionId;
  }

  get presentation(): ClientPresentation | undefined {
    const host = this.transport.presentation;
    if (!host) return undefined;
    return {
      capture: (afterTick = this.observedTick) => {
        if (this.stopped) return Promise.reject(new Error("Client is closed"));
        return host.capture(this.sessionId, afterTick, this.timeoutMs);
      },
      resize: (width, height) => host.resize(width, height),
      loseContext: () => host.loseContext(),
      restoreContext: () => host.restoreContext(),
    };
  }

  get world(): WorldDescriptor | undefined {
    return this.worldDescriptor;
  }

  /** A Host already negotiated compatibility and allocated this fresh World session. */
  protected initializeAttached(session: bigint, world: WorldDescriptor): this {
    if (session === 0n || this.sessionId !== 0n)
      throw new Error("Invalid World session");
    this.sessionId = session;
    this.worldDescriptor = world;
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

  protected async initialize(options: ConnectOptions): Promise<this> {
    const client = this;
    const transport = this.transport;
    const timeoutMs = this.timeoutMs;
    try {
      validateOptions(options);
      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const cleanup = () => {
          clearTimeout(timer);
          options.signal?.removeEventListener("abort", abort);
        };
        const failure = (error: Error, expected = false) => {
          if (!settled) {
            settled = true;
            cleanup();
            reject(error);
          }
          client.stop(error, expected);
        };
        const abort = () => failure(new Error("Connection aborted"), true);
        const timer = setTimeout(
          () => failure(new Error("Bootstrap timed out")),
          timeoutMs,
        );
        options.signal?.addEventListener("abort", abort, { once: true });
        try {
          transport.start({
            ready() {
              try {
                transport.send(client.bootstrap());
              } catch (error) {
                failure(asError(error));
              }
            },
            error: failure,
            closed: () => failure(new Error("Transport closed")),
            message(bytes) {
              if (client.stopped) return;
              try {
                if (client.sessionId === 0n) {
                  client.sessionId = client.acceptBootstrap(bytes);
                  client.logger.log("info", "session.connected", () => ({
                    session: client.sessionId,
                  }));
                  settled = true;
                  cleanup();
                  resolve();
                } else client.receive(bytes);
              } catch (error) {
                failure(asError(error));
              }
            },
          });
        } catch (error) {
          failure(asError(error));
        }
      });
      return client;
    } catch (error) {
      client.stop(asError(error));
      await transport.close().catch(() => {});
      throw error;
    }
  }

  private receive(bytes: Uint8Array): void {
    if (this.assetSources.receive(bytes)) return;
    const response = this.decodeResponse(bytes, this.sessionId);
    if (response.tick < this.observedTick)
      throw new Error("Response tick moved backwards");
    this.observedTick = response.tick;
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
      if (response.body.kind === "lifecycleOverflow") {
        const listeners = [...this.lifecycleListeners];
        this.lifecycleListeners.clear();
        for (const [subscription, listener] of listeners)
          this.notifyLifecycle(listener, {
            session: response.session,
            requestId: 0n,
            tick: response.tick,
            kind: "overflow",
            subscription,
            dropped: response.body.dropped,
          });
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
        if (response.body.event.type === "CameraStateChangedEvent") {
          this.publishCameraState(response);
          return;
        }
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
      if (response.body.kind === "guiObservations") {
        this.publishGuiObservations({
          effects: [...response.body.observations.effects],
          conflicts: [...(response.body.observations.conflicts ?? [])],
          cancellations: [...(response.body.observations.cancellations ?? [])],
          unhandled: [],
          ...(response.body.observations.textFocus === undefined
            ? {}
            : { textFocus: response.body.observations.textFocus }),
        });
        return;
      }
      if (response.body.kind === "guiUnhandledInputs") {
        const unhandled = response.body.inputs.filter((input) => {
          if (input.session !== this.sessionId) {
            this.logger.log("warn", "gui.unhandled.filtered", () => ({
              session: this.sessionId,
            }));
            return false;
          }
          return true;
        });
        if (unhandled.length === 0) return;
        this.publishGuiObservations({ effects: [], unhandled });
        return;
      }
      if (response.body.kind === "lifecycle") {
        for (const diagnostic of response.body.diagnostics) {
          this.logger.log("warn", "ownership.lost", () => ({
            session: this.sessionId,
            owner: diagnostic.owner,
            resource: diagnostic.stateOverlay,
          }));
          for (const listener of [...this.diagnosticListeners]) {
            // Application observers cannot poison the shared transport/session.
            try {
              listener(diagnostic);
            } catch (error) {
              globalThis.reportError?.(error);
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
    if (
      response.body.kind === "frame" ||
      response.body.kind === "lifecycle" ||
      response.body.kind === "resources" ||
      response.body.kind === "guiObservations" ||
      response.body.kind === "guiUnhandledInputs"
    )
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

  private request(
    body: RequestBody,
    requestId = this.nextId++,
    automaticOperation?: symbol,
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
    const expectedEvent =
      body.kind === "query"
        ? body.query.type === "GeometryPickQuery"
          ? "GeometryPickResultEvent"
          : "CameraProjectResultEvent"
        : undefined;
    const dispatch = () => {
      if (this.stopped) return Promise.reject(new Error("Client is closed"));
      // Worker transport transfers and detaches the buffer during send.
      const byteLength = bytes.byteLength;
      const result = this.send(requestId, bytes, expectedEvent);
      this.logRequest(body, requestId, byteLength);
      return result;
    };
    const batchTransport = isBatchTransportBody(body);
    const bypassesAutomaticGate = batchTransport || body.kind === "guiInput";
    const ownsAutomaticGate =
      automaticOperation !== undefined &&
      automaticOperation === this.activeAutomaticWorldOperation;
    const gate =
      bypassesAutomaticGate || ownsAutomaticGate
        ? undefined
        : this.automaticWorldGate;
    if (!gate) {
      const occupied =
        this.pending.size + (batchTransport ? 0 : this.queuedRequests);
      if (occupied >= 64)
        return Promise.reject(new RequestNotSentError("Pending request limit"));
      return dispatch();
    }
    if (this.pending.size + this.queuedRequests >= 64)
      return Promise.reject(new RequestNotSentError("Pending request limit"));
    this.queuedRequests++;
    return gate.then(() => {
      this.queuedRequests--;
      return dispatch();
    });
  }

  private logRequest(
    body: RequestBody,
    requestId: bigint,
    byteLength: number,
  ): void {
    if (body.kind === "batch" && !this.stopped) {
      this.logger.log("debug", "command.sent", () => ({
        session: this.sessionId,
        request: requestId,
        batch: body.batch.id,
        operations: body.batch.operations.length,
        bytes: byteLength,
      }));
      this.logger.log("trace", "command.kinds", () => ({
        session: this.sessionId,
        request: requestId,
        kinds: body.batch.operations
          .map((operation) => operation.kind)
          .join(","),
      }));
    }
  }

  private send(
    requestId: bigint,
    bytes: Uint8Array<ArrayBuffer>,
    expectedEvent?: string,
  ): Promise<Response> {
    return new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(
        () => this.stop(new Error("Request timed out")),
        this.timeoutMs,
      );
      this.pending.set(requestId, { resolve, reject, timer, expectedEvent });
      try {
        this.transport.send(bytes);
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
      const gate = this.automaticWorldGate;
      if (gate) {
        void gate.then(() => {
          if (this.stopped) return;
          try {
            this.transport.send(bytes);
          } catch (error) {
            this.stop(asError(error));
          }
        });
        return;
      }
      this.transport.send(bytes);
    } catch (error) {
      this.stop(asError(error));
      throw error;
    }
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

  protected async submitSurface(
    edit: import("./surface-types.js").SurfaceEdit,
  ): Promise<void> {
    const response = await this.request({ kind: "surface", edit });
    if (response.body.kind !== "surface")
      throw new Error("Invalid Surface response correlation");
  }

  protected async submitGui(
    edits: readonly import("./gui-types.js").GuiEdit[],
    batchId?: bigint,
    automaticOperation?: symbol,
  ): Promise<import("./gui-types.js").GuiEditBatchOutcome> {
    const response = await this.request(
      batchId === undefined
        ? { kind: "gui", edits }
        : { kind: "gui", batchId, edits },
      undefined,
      automaticOperation,
    );
    if (response.body.kind !== "gui")
      throw new Error("Invalid GUI response correlation");
    return response.body.outcome;
  }

  protected async submitGuiInspect(
    query: import("./gui-types.js").GuiInspectQuery,
  ): Promise<import("./gui-types.js").GuiInspectResponse> {
    const response = await this.request({ kind: "guiInspect", query });
    if (response.body.kind !== "guiInspect")
      throw new Error("Invalid GUI inspect response correlation");
    return response.body.response;
  }

  protected async submitGuiInput(
    input: import("./gui-types.js").GuiInputCommand,
  ): Promise<import("./gui-types.js").GuiInputRoutingOutcome> {
    const response = await this.request({ kind: "guiInput", input });
    if (response.body.kind !== "guiInput")
      throw new Error("Invalid GUI input response correlation");
    return response.body.outcome;
  }

  protected async submitGuiSemanticSnapshot(
    query: import("./gui-types.js").GuiSemanticSnapshotQuery,
  ): Promise<import("./gui-types.js").GuiSemanticTree> {
    const response = await this.request({ kind: "guiSemanticSnapshot", query });
    if (response.body.kind !== "guiSemanticSnapshot")
      throw new Error("Invalid GUI semantic snapshot response correlation");
    return response.body.snapshot;
  }

  protected async submitGuiSemanticAction(
    action: import("./gui-types.js").GuiSemanticActionRequest,
  ): Promise<void> {
    const response = await this.request({ kind: "guiSemanticAction", action });
    if (response.body.kind !== "gui" && response.body.kind !== "guiInput")
      throw new Error("Invalid GUI semantic action response correlation");
  }

  createGuiNodeHandle(
    entity: bigint,
    rootIncarnation: bigint,
    nodeId: import("./gui-types.js").GuiNodeId,
    nodeLifetime: number,
  ): import("./gui-types.js").GuiNodeHandle {
    return {
      session: this.session,
      entity,
      rootIncarnation,
      nodeId,
      nodeLifetime,
    };
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

  /** Allocate a session-fenced identity. The first buffer starts the deadline. */
  async beginBatch(): Promise<bigint> {
    const response = await this.request({ kind: "beginBatch" });
    if (response.body.kind !== "batchStarted" || response.body.batchId === 0n)
      throw new Error("Invalid batch allocation response");
    return response.body.batchId;
  }

  /** Apply a buffer without evaluation; size never signals logical completion. */
  async batchChunk(
    batchId: bigint,
    operations: Command[],
  ): Promise<BatchOutcome> {
    const response = await this.request({
      kind: "batchChunk",
      batch: { id: batchId, operations },
    });
    if (
      response.body.kind !== "batch" ||
      response.body.outcome.batchId !== batchId ||
      response.body.outcome.tick !== response.tick
    )
      throw new Error("Batch buffer response correlation mismatch");
    return response.body.outcome;
  }

  /** Explicitly terminate the batch and permit evaluation of its applied effects. */
  async endBatch(batchId: bigint): Promise<void> {
    const response = await this.request({ kind: "endBatch", batchId });
    if (
      response.body.kind !== "batchFinished" ||
      response.body.batchId !== batchId
    )
      throw new Error("Invalid batch terminator response");
  }

  private automaticBatchTail: Promise<void> | undefined;
  private automaticWorldGate: Promise<void> | undefined;
  private activeAutomaticWorldOperation: symbol | undefined;

  batch(operations: Command[], batchId?: bigint): Promise<BatchOutcome> {
    let snapshot: Command[];
    let pages: Command[][];
    try {
      // A queued paged batch may outlive the caller's current turn. Retain an
      // owned command snapshot just as transport encoding owns submitted bytes.
      snapshot = structuredClone(operations);
      pages = planCommandPages(snapshot, (request) =>
        this.encodeRequest(request),
      );
    } catch (error) {
      return Promise.reject(
        new RequestNotSentError(asError(error).message, error),
      );
    }
    const submit = () =>
      pages.length <= 1
        ? this.submitSingleBatch(snapshot, batchId)
        : applyPlannedCommandPages(this, pages).then((outcome) =>
            batchId === undefined ? outcome : { ...outcome, batchId },
          );
    return this.queueAutomaticWorldBatch(pages.length <= 1, submit);
  }

  /** Serialize automatic logical batches and hold unrelated client requests. */
  protected queueAutomaticWorldBatch<T>(
    singleBuffer: boolean,
    submit: (operation?: symbol) => Promise<T>,
  ): Promise<T> {
    const previous = this.automaticBatchTail;
    if (!previous && singleBuffer) return submit();
    const operation = Symbol("automatic World operation");
    const execute = () => {
      if (this.activeAutomaticWorldOperation !== undefined)
        throw new Error("Automatic World operations overlapped");
      this.activeAutomaticWorldOperation = operation;
      let submitted: Promise<T>;
      try {
        submitted = submit(operation);
      } catch (error) {
        this.activeAutomaticWorldOperation = undefined;
        throw error;
      }
      return submitted.finally(() => {
        if (this.activeAutomaticWorldOperation === operation)
          this.activeAutomaticWorldOperation = undefined;
      });
    };
    const result = previous ? previous.then(execute) : execute();
    const completion = result.then(
      () => {},
      () => {},
    );
    this.automaticBatchTail = completion;
    this.automaticWorldGate = completion;
    void completion.then(() => {
      if (this.automaticBatchTail === completion)
        this.automaticBatchTail = undefined;
      if (this.automaticWorldGate === completion)
        this.automaticWorldGate = undefined;
    });
    return result;
  }

  private async submitSingleBatch(
    operations: Command[],
    batchId?: bigint,
  ): Promise<BatchOutcome> {
    const id = batchId ?? this.nextId;
    const response = await this.request({
      kind: "batch",
      batch: { id, operations },
    });
    if (
      response.body.kind !== "batch" ||
      response.body.outcome.batchId !== id ||
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
        batch: id,
        tick: outcome.tick,
      }),
    );
    return response.body.outcome;
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
    };
  }

  /** Collect independent pages. Their ticks may differ; this is not an atomic snapshot. */
  async inspect(): Promise<Inspection> {
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

  protected addCameraStateListener(
    listener: (event: CameraStateChangedEvent) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    this.cameraStateListeners.add(listener);
    return () => this.cameraStateListeners.delete(listener);
  }

  private publishCameraState(response: Response): void {
    if (
      response.body.kind !== "event" ||
      response.body.event.type !== "CameraStateChangedEvent"
    )
      return;
    for (const listener of [...this.cameraStateListeners]) {
      try {
        listener({
          session: response.session,
          requestId: response.requestId,
          tick: response.tick,
          ...response.body.event,
          changes: { ...response.body.event.changes },
        });
      } catch (error) {
        try {
          globalThis.reportError?.(error);
        } catch {}
      }
    }
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

  onDiagnostic(
    listener: (diagnostic: StateOverlayLifecycleDiagnostic) => void,
  ): () => void {
    if (this.stopped) throw new Error("Client is closed");
    this.diagnosticListeners.add(listener);
    return () => {
      this.diagnosticListeners.delete(listener);
    };
  }

  private stop(error: Error, expected = false): void {
    if (this.stopped) return;
    this.stopped = true;
    if (this.latestGuiTextFocus !== undefined)
      this.publishGuiObservations({ effects: [], textFocus: null });
    this.latestGuiTextFocus = undefined;
    this.guiObservationListeners.clear();
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
    this.diagnosticListeners.clear();
    this.resourceListeners.clear();
    this.renderStateListeners.clear();
    this.cameraStateListeners.clear();
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

function isBatchTransportBody(body: RequestBody): boolean {
  return (
    body.kind === "batch" ||
    body.kind === "beginBatch" ||
    body.kind === "batchChunk" ||
    body.kind === "endBatch" ||
    (body.kind === "gui" && body.batchId !== undefined)
  );
}
