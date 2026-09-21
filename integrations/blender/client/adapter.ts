import {
  FieldKind,
  type AnimationWorldClient,
  type AnimationClipSource,
  type AnimationControllerDescription,
  type AnimationDriverTarget,
  type CameraWorldClient,
  type ClientAssetSource,
  type BatchOutcome,
  type Request,
  type EntityRef,
  type FieldValue,
  type FieldWrite,
} from "@ipp/client";
import {
  applyBlenderBatches,
  BlenderCommandError,
  type BlenderCommand,
} from "./ordered-batches.js";
import type { BlenderClip, BlenderEntity, BlenderSnapshot } from "./types.js";

export type BlenderClient = AnimationWorldClient & CameraWorldClient;
export interface BlenderContract {
  encodeRequest(request: Request): Uint8Array<ArrayBuffer>;
  encodeAnimationClip(clip: AnimationClipSource): Uint8Array<ArrayBuffer>;
  WIRE: { ASSET_ANIMATION: number };
}

type Values = Record<string, number | string | boolean>;
type Components = Map<string, Values>;
interface RetainedEntity {
  handle: bigint;
  name: string | null;
  components: Components;
}
export interface ImportedClip {
  duration: number;
  source: string;
  properties: AnimationDriverTarget[];
}
interface RetainedController {
  autoplayAcknowledged: boolean;
  handle: bigint;
  target: string;
  clip: ImportedClip;
  description: AnimationControllerDescription;
}

export interface BlenderAssetIO {
  /** Immutable runtime source for a detached Blender source. */
  resolve(source: string): string;
  /** Fetch location for authoring JSON, when it differs from the runtime source. */
  read(source: string): string;
  publishAnimation(bytes: Uint8Array<ArrayBuffer>): Promise<string>;
}

export interface AppliedRevision {
  clips: { id: string; name: string; target: string; clip: ImportedClip }[];
  session: string;
  revision: number;
  tick: bigint;
  entities: ReadonlyMap<string, bigint>;
  diagnostics: NonNullable<BlenderSnapshot["scene"]["diagnostics"]>;
}

const VIEW_CAMERA = "$view-camera";
const MAX_QUEUED_REVISIONS = 4;
const defaultTransform = {
  x: 0,
  y: 0,
  z: 6,
  qx: 0,
  qy: 0,
  qz: 0,
  qw: 1,
  sx: 1,
  sy: 1,
  sz: 1,
};
const defaultCamera = {
  projection: 0,
  fov_y: Math.PI / 4,
  near: 0.01,
  far: 1000,
  ortho_height: 4,
  focus_distance: 6,
};

/** Detached revisions and React declarations share the existing generated client. */
export class BlenderAdapter {
  private readonly retained = new Map<string, RetainedEntity>();
  private readonly clips = new Map<string, ImportedClip>();
  private readonly controllers = new Map<string, RetainedController>();
  private readonly ownedAssets: ClientAssetSource[] = [];
  private session: string | undefined;
  private revision = -1;
  private tail: Promise<unknown> = Promise.resolve();
  private queued = 0;
  private closed = false;
  private playbackRequest = 0;
  private socket: WebSocket | undefined;
  private readonly abort = new AbortController();
  private readonly runtimeSession: bigint;
  private stopBatchListener = () => {};
  private stream:
    | {
        transfer: string;
        session: string;
        revision: number;
        sequence: number;
        failed: boolean;
        batchId?: bigint | undefined;
        nextAlias: number;
        batchTick?: bigint | undefined;
      }
    | undefined;
  /** Import measurements relative to connect(), in milliseconds. */
  readonly importProfile = {
    started: 0,
    firstChunk: 0,
    finalSnapshot: 0,
    applied: 0,
    chunks: 0,
    commandBatches: 0,
    logicalBatches: 0,
    entityBatches: 0,
    firstEntityBatch: 0,
    maxBatchCommands: 0,
    exportSeconds: 0,
    clipsReady: 0,
    entitiesApplied: 0,
    controllersReady: 0,
    controllers: 0,
    stream: null as unknown,
  };

  constructor(
    readonly client: BlenderClient,
    private readonly contract: BlenderContract,
    private readonly endpoint: URL,
    private readonly token: string,
    private readonly updated: (result: AppliedRevision) => void = () => {},
    private readonly failed: (error: Error) => void = () => {},
    private readonly assets?: BlenderAssetIO,
    private readonly activateViewCamera = true,
  ) {
    this.runtimeSession = client.session;
    if (endpoint.protocol !== "https:")
      throw new Error("Blender requires HTTPS");
    for (const name of [
      "Transform",
      "Camera",
      "MeshInstance",
      "MeshPose",
      "Hierarchy",
      "UnlitMaterial",
      "BaseColorTexture",
      "PbrMaterial",
      "Light",
      "Skeleton",
      "Skin",
    ])
      if (!client.components[name])
        throw new Error(`Viewer is missing ${name}`);
    this.stopBatchListener = client.onBatchAborted((failure) => {
      if (this.stream?.batchId === failure.batchId) {
        this.stream.batchId = undefined;
        this.stream.failed = true;
      }
    });
  }

  /** Token only goes to the configured addon origin, never arbitrary scene URLs. */
  source(source: string): string {
    if (this.assets) return this.assets.resolve(source);
    const url = new URL(source, this.endpoint);
    if (
      url.origin !== this.endpoint.origin ||
      !/^\/assets\/[A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*$/.test(url.pathname)
    )
      throw new Error(
        "Blender asset must be an immutable resource on the addon origin",
      );
    url.search = "";
    url.searchParams.set("token", this.token);
    return url.href;
  }

  connect(options: { streamBatchSize?: number; refresh?: boolean } = {}): void {
    if (this.closed || this.socket)
      throw new Error("Blender adapter already started");
    const url = new URL("/v1/updates", this.endpoint);
    url.protocol = "wss:";
    url.searchParams.set("token", this.token);
    if (options.streamBatchSize !== undefined) {
      if (
        !Number.isInteger(options.streamBatchSize) ||
        options.streamBatchSize < 0 ||
        options.streamBatchSize > 1000
      )
        throw new Error("Stream batch size must be 0..1000");
      url.searchParams.set("stream", String(options.streamBatchSize));
    }
    if (options.refresh) url.searchParams.set("refresh", "1");
    this.importProfile.started = performance.now();
    const socket = new WebSocket(url);
    this.socket = socket;
    socket.onmessage = (event: MessageEvent<unknown>) => {
      if (typeof event.data !== "string") {
        this.fail(new Error("Invalid Blender update"));
        return;
      }
      try {
        const input: unknown = JSON.parse(event.data);
        void this.enqueue(() => this.receive(input))
          .then((result) => {
            if (result && socket.readyState === WebSocket.OPEN && !this.closed)
              socket.send(
                JSON.stringify({
                  type: "applied",
                  session: result.session,
                  revision: result.revision,
                  runtime_session: this.client.session.toString(),
                  tick: result.tick.toString(),
                }),
              );
          })
          .catch((error: unknown) => this.failed(asError(error)));
      } catch (error) {
        this.fail(asError(error));
      }
    };
    socket.onerror = () =>
      this.fail(
        new Error("Blender connection failed; open its HTTPS page first"),
      );
    socket.onclose = () => {
      if (!this.closed)
        this.fail(
          new Error("Blender disconnected; reconnect to start a fresh viewer"),
        );
    };
  }

  apply(input: unknown): Promise<AppliedRevision> {
    return this.enqueue(() => {
      if (this.stream) throw new Error("A Blender stream is still active");
      return this.applyRevision(input);
    });
  }

  private enqueue<T>(operation: () => Promise<T>): Promise<T> {
    if (this.closed)
      return Promise.reject(new Error("Blender adapter is closed"));
    if (this.queued >= MAX_QUEUED_REVISIONS)
      return Promise.reject(new Error("Blender revision queue is full"));
    this.queued++;
    const next = this.tail.then(operation);
    this.tail = next.catch(() => {});
    return next.finally(() => {
      this.queued--;
    });
  }

  private async receive(input: unknown): Promise<AppliedRevision | undefined> {
    if (!input || typeof input !== "object")
      throw new Error("Invalid Blender update");
    const message = input as {
      type: string;
      transfer: string;
      session: string;
      revision: number;
      sequence: number;
      entities?: BlenderEntity[];
      clips?: string[];
      assets?: { source: string }[];
      snapshot?: unknown;
      error?: unknown;
    };
    if (message.type === "error")
      throw new Error(`Blender export failed: ${message.error}`);
    if (message.type === "snapshot") {
      if (this.stream) throw new Error("Snapshot interrupted a Blender stream");
      return this.completeImport(message);
    }
    if (message.type === "abort") {
      if (!this.stream || message.transfer !== this.stream.transfer)
        throw new Error("Unknown Blender stream abort");
      await this.endEntityBatch();
      this.stream = undefined;
      throw new Error(`Blender export aborted: ${message.error}`);
    }
    let ok = false;
    try {
      if (message.type === "begin") {
        validateSnapshot({
          type: "snapshot",
          session: message.session,
          revision: message.revision,
          scene: { entities: [] },
        });
        if (
          this.stream ||
          this.revision !== -1 ||
          this.retained.size ||
          typeof message.transfer !== "string" ||
          !message.transfer
        )
          throw new Error("Streaming requires a fresh Blender adapter");
        this.stream = {
          transfer: message.transfer,
          session: message.session,
          revision: message.revision,
          sequence: 0,
          failed: false,
          nextAlias: 1,
        };
      }
      const stream = this.stream;
      if (
        !stream ||
        stream.failed ||
        message.transfer !== stream.transfer ||
        message.session !== stream.session ||
        message.revision !== stream.revision ||
        message.sequence !== stream.sequence++
      )
        throw new Error("Out-of-order or stale Blender stream chunk");
      let result: AppliedRevision | undefined;
      if (message.type === "chunk") {
        if (
          Array.isArray(message.entities) &&
          message.clips === undefined &&
          message.entities.length <= 1000
        ) {
          await this.applyRevision(
            {
              type: "snapshot",
              session: stream.session,
              revision: stream.revision,
              scene: { entities: message.entities },
            },
            true,
          );
        } else if (
          Array.isArray(message.clips) &&
          message.entities === undefined &&
          message.clips.length <= 1000
        ) {
          for (const source of message.clips) {
            if (typeof source !== "string")
              throw new Error("Invalid streamed clip");
            await this.clip(source);
          }
        } else throw new Error("Invalid Blender stream chunk");
        this.importProfile.chunks++;
        this.importProfile.firstChunk ||=
          performance.now() - this.importProfile.started;
      } else if (message.type === "entities-end") {
        await this.endEntityBatch();
      } else if (message.type === "commit") {
        if (stream.batchId !== undefined)
          throw new Error(
            "Blender commit is missing an entity-batch terminator",
          );
        const snapshot = validateSnapshot(message.snapshot);
        if (
          snapshot.session !== stream.session ||
          snapshot.revision !== stream.revision
        )
          throw new Error("Blender stream commit identity changed");
        // The authoritative snapshot removes early identities whose object
        // extraction was unsupported or whose ancestor was pruned.
        result = await this.completeImport(message.snapshot);
        this.stream = undefined;
      } else if (message.type === "asset-index") {
        if (!Array.isArray(message.assets))
          throw new Error("Invalid asset index");
        for (const asset of message.assets) this.source(asset.source);
      } else if (message.type !== "begin")
        throw new Error("Invalid Blender stream message");
      ok = true;
      return result;
    } finally {
      if (!ok && this.stream) {
        this.stream.failed = true;
        await this.endEntityBatch();
      }
      if (this.socket?.readyState === WebSocket.OPEN)
        this.socket.send(
          JSON.stringify({
            type: "chunk-applied",
            transfer: message.transfer,
            session: message.session,
            revision: message.revision,
            sequence: message.sequence,
            ok,
          }),
        );
    }
  }

  private async endEntityBatch(): Promise<void> {
    const stream = this.stream;
    if (stream?.batchId === undefined) return;
    const id = stream.batchId;
    stream.batchId = undefined;
    stream.batchTick = undefined;
    stream.nextAlias = 1;
    await this.client.endBatch(id);
    this.importProfile.entityBatches++;
    this.importProfile.firstEntityBatch ||=
      performance.now() - this.importProfile.started;
  }

  private async completeImport(input: unknown): Promise<AppliedRevision> {
    this.importProfile.finalSnapshot =
      performance.now() - this.importProfile.started;
    const result = await this.applyRevision(input);
    this.importProfile.applied = performance.now() - this.importProfile.started;
    const profile = input as {
      exportSeconds?: number;
      streamProfile?: unknown;
    };
    this.importProfile.exportSeconds = profile.exportSeconds ?? 0;
    this.importProfile.stream = profile.streamProfile ?? null;
    return result;
  }

  async flush(): Promise<void> {
    await this.tail;
  }

  async playback(action: "play" | "pause" | "stop"): Promise<void> {
    const request = ++this.playbackRequest;
    await this.flush();
    this.checkLive();
    if (request !== this.playbackRequest) return;
    for (const [id, controller] of this.controllers) {
      if (action === "play") await this.startController(id, request);
      else this.client.playback(controller.handle, { action });
    }
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    void this.endEntityBatch().catch(() => {});
    this.stopBatchListener();
    this.abort.abort();
    for (const controller of this.controllers.values()) {
      void this.client
        .deleteAnimationController(controller.handle)
        .catch(() => {});
    }
    this.controllers.clear();
    for (const source of this.ownedAssets)
      void this.client.releaseAsset(source).catch(() => {});
    this.ownedAssets.length = 0;
    if (this.socket) {
      this.socket.onopen =
        this.socket.onmessage =
        this.socket.onerror =
        this.socket.onclose =
          null;
      this.socket.close();
    }
  }

  private fail(error: Error): void {
    if (this.closed) return;
    this.close();
    this.failed(error);
  }

  private checkLive(): void {
    if (this.closed) throw new Error("Blender adapter is closed");
    if (this.client.session !== this.runtimeSession)
      throw new Error("Runtime session changed; reconnect the Blender viewer");
  }

  private async recoverEntities(
    refs: Map<string, EntityRef>,
    aliases: Map<number, bigint>,
  ): Promise<void> {
    const inspection = await this.client.inspect();
    this.checkLive();
    const identities = new Map(
      [...this.retained].map(([id, record]) => [id, record.handle]),
    );
    for (const [id, ref] of refs) {
      const handle = ref.kind === "handle" ? ref.id : aliases.get(ref.alias);
      if (handle !== undefined) identities.set(id, handle);
    }
    const entities = new Map(
      inspection.entities.map((entity) => [entity.id, entity]),
    );
    this.retained.clear();
    for (const [id, handle] of identities) {
      const entity = entities.get(handle);
      if (entity)
        this.retained.set(id, {
          handle,
          name: entity.metadata.symbolicId,
          // Presence comes from the runtime. Empty field records make the next
          // revision rewrite desired values after an indeterminate partial edit.
          components: new Map(
            Object.entries(this.client.components)
              .filter(([, descriptor]) =>
                entity.base.some(
                  (component) => component.component === descriptor.id,
                ),
              )
              .map(([name]) => [name, {}]),
          ),
        });
    }
  }

  /** Generate only the next buffer's commands; acknowledged aliases outlive buffers. */
  private *commands(
    desired: Map<string, Components>,
    names: Map<string, string | null>,
    refs: Map<string, EntityRef>,
    partial: boolean,
  ): Generator<BlenderCommand> {
    // Release names before assigning replacements, within the same ordered batch.
    // Swaps and name reuse preserve entity handles. Large revisions use ordered
    // frames; observers may see intermediate state and failure retains prior effects.
    for (const [id, previous] of this.retained)
      if (
        (!partial || desired.has(id)) &&
        previous.name !== null &&
        previous.name !== (names.get(id) ?? null)
      )
        yield {
          kind: "setMetadata",
          entity: { kind: "handle", id: previous.handle },
          metadata: { symbolicId: null, classes: ["blender"] },
        };
    let alias = partial && this.stream ? this.stream.nextAlias : 1;
    for (const id of desired.keys()) {
      const previous = this.retained.get(id);
      const name = names.get(id) ?? null;
      if (previous) {
        refs.set(id, { kind: "handle", id: previous.handle });
        if (previous.name !== name)
          yield {
            kind: "setMetadata",
            entity: { kind: "handle", id: previous.handle },
            metadata: { symbolicId: name, classes: ["blender"] },
          };
      } else {
        if (partial && this.stream) this.stream.nextAlias = alias + 1;
        refs.set(id, { kind: "alias", alias });
        yield {
          kind: "create",
          alias: alias++,
          metadata: { symbolicId: name, classes: ["blender"] },
        };
      }
    }
    // Detach changed relationships first: reversing a valid chain must not
    // introduce a temporary cycle while the ordered batch applies new parents.
    const detached = new Set<string>();
    for (const [id, previous] of this.retained) {
      const old = previous.components.get("Hierarchy");
      if (
        (!partial || desired.has(id)) &&
        old &&
        old.parent !== desired.get(id)?.get("Hierarchy")?.parent
      ) {
        yield {
          kind: "removeComponent",
          entity: { kind: "handle", id: previous.handle },
          component: this.client.components.Hierarchy!.id,
        };
        detached.add(id);
      }
    }
    // A cached old target may be incompatible with a newly authored base.
    // Disable interpolation while switching the pair, preserving component
    // incarnations and weight bindings across ordinary geometry edits.
    const clearedPoses = new Set<string>();
    for (const [id, components] of desired) {
      const previous = this.retained.get(id)?.components;
      if (
        previous?.has("MeshPose") &&
        previous.get("MeshInstance")?.source !==
          components.get("MeshInstance")?.source
      ) {
        yield {
          kind: "setField",
          entity: refs.get(id)!,
          component: this.client.components.MeshPose!.id,
          field: this.writes("MeshPose", { source: "" }, refs)[0]!,
        };
        clearedPoses.add(id);
      }
    }
    for (const [id, components] of desired) {
      const entity = refs.get(id)!;
      const previous = this.retained.get(id)?.components;
      for (const [name, values] of components) {
        const descriptor = this.client.components[name]!;
        const old =
          name === "Hierarchy" && detached.has(id)
            ? undefined
            : previous?.get(name);
        if (!old) {
          yield {
            kind: "insertComponent",
            entity,
            component: descriptor.id,
            fields: this.writes(name, values, refs),
          };
        } else {
          for (const [field, value] of Object.entries(values))
            if (
              old[field] !== value ||
              (name === "MeshPose" &&
                field === "source" &&
                clearedPoses.has(id))
            )
              yield {
                kind: "setField",
                entity,
                component: descriptor.id,
                field: this.writes(name, { [field]: value }, refs)[0]!,
              };
        }
      }
      for (const name of previous?.keys() ?? [])
        if (
          !components.has(name) &&
          !(name === "Hierarchy" && detached.has(id))
        )
          yield {
            kind: "removeComponent",
            entity,
            component: this.client.components[name]!.id,
          };
    }
    for (const [id, previous] of this.retained)
      if (!partial && !desired.has(id))
        yield {
          kind: "delete",
          entity: { kind: "handle", id: previous.handle },
        };
  }

  private async applyRevision(
    input: unknown,
    partial = false,
  ): Promise<AppliedRevision> {
    this.checkLive();
    const snapshot = validateSnapshot(input);
    if (this.session !== undefined && snapshot.session !== this.session)
      throw new Error(
        "Blender export session changed; reconnect to start a fresh viewer",
      );
    if (snapshot.revision <= this.revision)
      throw new Error("Stale Blender revision");

    const desired = new Map<string, Components>();
    const names = new Map<string, string | null>();
    const uniqueNames = new Set<string>();
    for (const entity of snapshot.scene.entities) {
      if (entity.id.startsWith("$") || desired.has(entity.id))
        throw new Error("Duplicate or reserved Blender identity");
      desired.set(entity.id, this.components(entity));
      const name = entity.name ?? entity.id;
      if (typeof name !== "string" || !name || uniqueNames.has(name))
        throw new Error("Duplicate or invalid Blender object name");
      uniqueNames.add(name);
      names.set(entity.id, name);
    }
    const selected =
      snapshot.scene.active_camera === undefined
        ? snapshot.scene.entities.find((entity) => entity.camera)
        : snapshot.scene.entities.find(
            (entity) => entity.id === snapshot.scene.active_camera,
          );
    if (snapshot.scene.active_camera !== undefined && !selected?.camera)
      throw new Error("Selected Blender camera is missing");
    if (!partial)
      desired.set(
        VIEW_CAMERA,
        new Map<string, Values>([
          ["Transform", { ...defaultTransform, ...(selected ? { z: 0 } : {}) }],
          ["Camera", { ...(selected?.camera ?? defaultCamera) }],
          ...(selected
            ? [["Hierarchy", { parent: selected.id }] as [string, Values]]
            : []),
        ]),
      );

    const library: AppliedRevision["clips"] = [];
    for (const entry of snapshot.scene.clips ?? []) {
      if (!desired.has(entry.target) || !entry.name || !entry.id)
        throw new Error("Invalid reusable animation association");
      if (library.some((clip) => clip.id === entry.id))
        throw new Error("Duplicate reusable animation identity");
      library.push({ ...entry, clip: await this.clip(entry.source) });
    }
    const desiredControllers = new Map<
      string,
      {
        target: string;
        clip: ImportedClip;
        speed: number;
        looping: boolean;
        autoplay: boolean;
      }
    >();
    for (const animation of snapshot.scene.animations ?? []) {
      if (!desired.has(animation.target))
        throw new Error("Animation target is missing");
      if (desiredControllers.has(animation.id))
        throw new Error("Duplicate animation identity");
      const clip = await this.clip(animation.source);
      this.checkLive();
      desiredControllers.set(animation.id, {
        target: animation.target,
        clip,
        speed: animation.speed ?? 1,
        looping: animation.looping ?? false,
        autoplay: animation.autoplay ?? false,
      });
    }
    this.checkLive();

    if (!partial)
      this.importProfile.clipsReady =
        performance.now() - this.importProfile.started;
    const refs = new Map<string, EntityRef>(
      partial
        ? [...this.retained].map(([id, entity]) => [
            id,
            { kind: "handle", id: entity.handle },
          ])
        : [],
    );
    this.session = snapshot.session;
    let outcome: BatchOutcome;
    try {
      outcome = await applyBlenderBatches(
        {
          beginBatch: async () => {
            if (partial && this.stream?.batchId !== undefined)
              return this.stream.batchId;
            const id = await this.client.beginBatch();
            this.importProfile.logicalBatches++;
            if (partial && this.stream) this.stream.batchId = id;
            return id;
          },
          endBatch: async (id) => {
            if (!partial) await this.client.endBatch(id);
          },
          batchChunk: async (id, commands) => {
            this.importProfile.commandBatches++;
            this.importProfile.maxBatchCommands = Math.max(
              this.importProfile.maxBatchCommands,
              commands.length,
            );
            const outcome = await this.client.batchChunk(id, commands);
            if (partial && this.stream) {
              if (
                this.stream.batchTick !== undefined &&
                outcome.tick !== this.stream.batchTick
              )
                throw new Error(
                  "World evaluated before the Blender entity-batch terminator",
                );
              this.stream.batchTick = outcome.tick;
              if (!outcome.ok) this.stream.batchId = undefined;
            }
            return outcome;
          },
        },
        this.commands(desired, names, refs, partial),
        this.contract.encodeRequest,
      );
    } catch (error) {
      if (error instanceof BlenderCommandError) {
        await this.endEntityBatch();
        // Local generation/encoding failed with no buffer in flight. Preserve
        // earlier acknowledged identities for a corrected revision.
        await this.recoverEntities(refs, error.aliases);
        throw error;
      }
      // Earlier buffers may have applied. An unacknowledged transport failure
      // cannot safely reconstruct their aliases; reconnect with a fresh adapter.
      this.close();
      throw error;
    }
    this.checkLive();
    const aliases = new Map(
      outcome.aliases.map((entry) => [entry.alias, entry.id]),
    );
    if (!outcome.ok) {
      await this.recoverEntities(refs, aliases);
      throw new Error(
        `Blender revision failed at ${outcome.error.operation}: ${outcome.error.reason}`,
      );
    }
    const initial = this.revision === -1;
    if (!partial) this.retained.clear();
    for (const [id, components] of desired) {
      const ref = refs.get(id)!;
      const handle = ref.kind === "handle" ? ref.id : aliases.get(ref.alias)!;
      this.retained.set(id, {
        handle,
        name: names.get(id) ?? null,
        components,
      });
    }
    if (partial)
      return {
        clips: [],
        session: snapshot.session,
        revision: snapshot.revision,
        tick: outcome.tick,
        entities: new Map(
          [...this.retained].map(([id, record]) => [id, record.handle]),
        ),
        diagnostics: [],
      };
    this.importProfile.entitiesApplied =
      performance.now() - this.importProfile.started;
    this.client.sendCommand({
      type: "RenderStateUpdateCommand",
      changes: { ambientLight: snapshot.scene.ambient_light ?? [0, 0, 0] },
    });
    this.session = snapshot.session;
    if (initial && this.activateViewCamera)
      this.client.sendCommand({
        type: "CameraActivateCommand",
        entity: this.retained.get(VIEW_CAMERA)!.handle,
      });
    for (const [id, controller] of this.controllers) {
      if (!desiredControllers.has(id)) {
        await this.client.deleteAnimationController(controller.handle);
        this.controllers.delete(id);
      }
    }
    for (const [id, desired] of desiredControllers) {
      const target = this.retained.get(desired.target)!.handle;
      const description: AnimationControllerDescription = {
        speed: desired.speed,
        looping: desired.looping,
        drivers: desired.clip.properties.map((property, track) => ({
          source: desired.clip.source,
          track,
          target,
          property,
        })),
      };
      const previous = this.controllers.get(id);
      const changed =
        !previous ||
        previous.clip !== desired.clip ||
        previous.target !== desired.target ||
        previous.description.drivers[0]?.target !== target ||
        previous.description.speed !== desired.speed ||
        previous.description.looping !== desired.looping;
      if (previous && changed) {
        await this.client.updateAnimationController(
          previous.handle,
          description,
        );
      }
      const handle =
        previous?.handle ??
        (await this.client.createAnimationController(description));
      this.controllers.set(id, {
        handle,
        target: desired.target,
        clip: desired.clip,
        description,
        autoplayAcknowledged:
          !changed && (previous?.autoplayAcknowledged ?? false),
      });
      this.importProfile.controllers = this.controllers.size;
      if (desired.autoplay && !this.controllers.get(id)!.autoplayAcknowledged)
        await this.startController(id);
    }
    this.checkLive();
    this.importProfile.controllersReady =
      performance.now() - this.importProfile.started;
    const appliedState = await this.client.inspectPage({
      collection: "summary",
    });
    this.checkLive();
    this.revision = snapshot.revision;
    const result = {
      clips: library,
      session: snapshot.session,
      revision: snapshot.revision,
      tick: appliedState.tick,
      entities: new Map(
        [...this.retained].map(([id, value]) => [id, value.handle]),
      ),
      diagnostics: snapshot.scene.diagnostics ?? [],
    };
    this.updated(result);
    return result;
  }

  private components(entity: BlenderEntity): Components {
    const result: Components = new Map();
    for (const [name, values] of [
      ["ParticleEmitter", entity.particle_emitter],
      ["ParticlePlayback", entity.particle_playback],
      ["ParticleSprite", entity.particle_sprite],
      ["ParticleMesh", entity.particle_mesh],
    ] as const) {
      if (!values) continue;
      if (!this.client.components[name])
        throw new Error(
          `Particle export requires the particles capability (${name})`,
        );
      result.set(name, {
        ...values,
        ...(typeof values.source === "string" && values.source
          ? { source: this.source(values.source) }
          : {}),
        variant: 0,
      });
    }
    if (entity.transform) result.set("Transform", { ...entity.transform });
    if (entity.parent !== undefined)
      result.set("Hierarchy", {
        parent: entity.parent,
        parent_bone: entity.parent_bone ?? 0xffff_ffff,
      });
    if (entity.mesh)
      result.set("MeshInstance", {
        source: this.source(entity.mesh.source),
        variant: 0,
      });
    if (entity.mesh_pose)
      result.set("MeshPose", {
        source: this.source(entity.mesh_pose.source),
        variant: 0,
        weight: entity.mesh_pose.weight,
      });
    if (entity.texture)
      result.set("BaseColorTexture", {
        source: this.source(entity.texture.source),
        variant: 0,
      });
    if (entity.material) {
      const m = entity.material;
      if (m.type !== "unlit" && m.type !== "pbr")
        throw new Error("Unknown material mapping");
      result.set(m.type === "pbr" ? "PbrMaterial" : "UnlitMaterial", {
        r: m.r,
        g: m.g,
        b: m.b,
        ...(m.type === "pbr"
          ? {
              metallic: m.metallic ?? 0,
              roughness: m.roughness ?? 0.5,
              cast_shadows: m.cast_shadows ?? true,
              receive_shadows: m.receive_shadows ?? true,
            }
          : {}),
      });
    }
    if (entity.camera) result.set("Camera", { ...entity.camera });
    if (entity.light)
      result.set("Light", {
        cast_shadows: false,
        shadow_near: 0.1,
        shadow_bias: 0.001,
        shadow_radius: 0,
        ...entity.light,
      });
    if (entity.skeleton)
      result.set("Skeleton", {
        source: this.source(entity.skeleton.source),
        variant: 0,
        pose_source: entity.skeleton.pose_source
          ? this.source(entity.skeleton.pose_source)
          : "",
        pose_variant: 0,
      });
    if (entity.skin)
      result.set("Skin", {
        source: this.source(entity.skin.source),
        variant: 0,
        skeleton: entity.skin.skeleton,
      });
    return result;
  }

  private writes(
    name: string,
    values: Values,
    refs: Map<string, EntityRef>,
  ): FieldWrite[] {
    const descriptor = this.client.components[name]!;
    return Object.entries(values).map(([name, value]) => {
      const field = descriptor.fields[name];
      if (!field) throw new Error(`Unsupported field ${name}`);
      let typed: FieldValue;
      switch (field.kind) {
        case FieldKind.F32:
        case FieldKind.U32:
          if (typeof value !== "number" || !Number.isFinite(value))
            throw new Error(`Invalid numeric field ${name}`);
          typed = { kind: field.kind === FieldKind.F32 ? "f32" : "u32", value };
          break;
        case FieldKind.String:
          if (typeof value !== "string")
            throw new Error(`Invalid text field ${name}`);
          typed = { kind: "string", value };
          break;
        case FieldKind.Bool:
          if (typeof value !== "boolean")
            throw new Error(`Invalid boolean field ${name}`);
          typed = { kind: "bool", value };
          break;
        case FieldKind.Entity: {
          const entity =
            typeof value === "string" ? refs.get(value) : undefined;
          if (!entity) throw new Error(`Missing referenced entity ${value}`);
          typed = { kind: "entity", value: entity };
          break;
        }
        default:
          throw new Error(`Unsupported authoring field ${name}`);
      }
      return { offset: field.offset, value: typed };
    });
  }

  private async clip(source: string): Promise<ImportedClip> {
    this.checkLive();
    const url = this.source(source);
    const cached = this.clips.get(url);
    if (cached) return cached;
    const response = await fetch(this.assets?.read(source) ?? url, {
      signal: AbortSignal.any([this.abort.signal, AbortSignal.timeout(15_000)]),
    });
    this.checkLive();
    if (!response.ok || !response.body)
      throw new Error(`Animation fetch failed: ${response.status}`);
    const reader = response.body.getReader();
    let total = 0;
    const chunks: Uint8Array[] = [];
    try {
      for (;;) {
        const result = await reader.read();
        this.checkLive();
        if (result.done) break;
        total += result.value.byteLength;
        chunks.push(result.value);
      }
    } finally {
      await reader.cancel();
    }
    const bytes = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    const clip: BlenderClip = JSON.parse(
      new TextDecoder("utf-8", { fatal: true }).decode(bytes),
    );
    const tracks: AnimationClipSource["tracks"] = clip.tracks.map((track) => {
      if ("joints" in track) return track;
      const component = this.client.components[track.property.component];
      if (!component) throw new Error("Animation component is unavailable");
      return {
        keys: track.keys,
        property: {
          component: component.id,
          offsets: track.property.fields.map((name) => {
            const field = component.fields[name];
            if (!field)
              throw new Error(`Animation field ${name} is unavailable`);
            return field.offset;
          }),
        },
      };
    });
    const encoded = this.contract.encodeAnimationClip({
      duration: clip.duration,
      tracks,
    });
    const publishedSource = await this.publishAnimation(encoded);
    this.checkLive();
    const imported = {
      duration: clip.duration,
      source: publishedSource,
      properties: tracks.map((track) =>
        "joints" in track ? { joints: track.joints } : track.property,
      ),
    };
    this.clips.set(url, imported);
    return imported;
  }

  /** Authoring hosts may persist encoded clips and return an immutable HTTP source. */
  protected async publishAnimation(
    encoded: Uint8Array<ArrayBuffer>,
  ): Promise<string> {
    if (this.assets) return this.assets.publishAnimation(encoded);
    const source = await this.client.createAsset(
      this.contract.WIRE.ASSET_ANIMATION,
      encoded.buffer,
    );
    this.ownedAssets.push(source);
    try {
      this.checkLive();
    } catch (error) {
      this.ownedAssets.pop();
      await this.client.releaseAsset(source).catch(() => {});
      throw error;
    }
    return source.source;
  }

  private async startController(
    id: string,
    request = this.playbackRequest,
  ): Promise<void> {
    const controller = this.controllers.get(id)!;
    const target = this.retained.get(controller.target);
    const skeleton = target?.components.get("Skeleton");
    const mesh = target?.components.get("MeshInstance");
    const meshPose = target?.components.get("MeshPose");
    const needed = new Set(
      [
        controller.clip.source,
        skeleton?.source,
        skeleton?.pose_source,
        mesh?.source,
        meshPose?.source,
      ].filter(Boolean),
    );
    const deadline = performance.now() + 15_000;
    for (;;) {
      this.checkLive();
      if (request !== this.playbackRequest) return;
      const state = await this.client.inspect();
      this.checkLive();
      const resources = state.resources.filter((resource) =>
        needed.has(resource.source),
      );
      const failed = resources.find((resource) => resource.status === "failed");
      if (failed) throw new Error(`Blender resource failed: ${failed.error}`);
      if (
        resources.length === needed.size &&
        resources.every((resource) => resource.status === "loaded")
      )
        break;
      if (performance.now() >= deadline)
        throw new Error("Blender resources did not become ready");
      await this.client.waitForFrame(state.tick);
    }
    this.checkLive();
    if (
      request !== this.playbackRequest ||
      this.controllers.get(id) !== controller
    )
      return;
    this.client.playback(controller.handle, { action: "play" });
    controller.autoplayAcknowledged = true;
  }
}

function validateSnapshot(value: unknown): BlenderSnapshot {
  if (!value || typeof value !== "object")
    throw new Error("Invalid Blender snapshot");
  const snapshot = value as BlenderSnapshot;
  if (
    snapshot.type !== "snapshot" ||
    typeof snapshot.session !== "string" ||
    !snapshot.session.length ||
    !Number.isSafeInteger(snapshot.revision) ||
    snapshot.revision < 0 ||
    !snapshot.scene ||
    !Array.isArray(snapshot.scene.entities)
  )
    throw new Error("Invalid Blender snapshot");
  const ambient = snapshot.scene.ambient_light;
  if (
    ambient !== undefined &&
    (!Array.isArray(ambient) ||
      ambient.length !== 3 ||
      ambient.some(
        (channel) =>
          typeof channel !== "number" ||
          !Number.isFinite(Math.fround(channel)) ||
          channel < 0,
      ))
  )
    throw new Error("Invalid Blender ambient light");
  for (const entity of snapshot.scene.entities)
    if (
      !entity ||
      typeof entity.id !== "string" ||
      !entity.id.length ||
      entity.id.length > 256
    )
      throw new Error("Invalid Blender entity identity");
  if (snapshot.scene.clips && !Array.isArray(snapshot.scene.clips))
    throw new Error("Invalid Blender reusable clips");
  if (snapshot.scene.animations && !Array.isArray(snapshot.scene.animations))
    throw new Error("Invalid Blender animation bounds");
  return snapshot;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
