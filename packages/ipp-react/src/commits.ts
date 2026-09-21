import { ReactAnimationRegistry } from "./animation_state.js";
import { ReactAssetRegistry } from "./asset_state.js";
import { GuiCommits } from "./gui/commits.js";
import type { FieldWrite, StateOverlayAlias } from "@ipp/client";
import type {
  StateOverlayLifecycleDiagnostic,
  StateOverlayCommand,
  StateOverlayOutcome,
  ReactWorldClient,
  StateOverlayRef,
} from "./contract.js";
import type {
  ReactWorldDescription,
  EntityOverlayBindingDescription,
  ComponentStateOverlayDescription,
  StateOverlayFieldValue,
  ReactWorldFieldValue,
} from "./tree.js";

export interface ReactWorldRootOptions {
  onError?: (error: Error) => void;
  onDiagnostic?: (diagnostic: StateOverlayLifecycleDiagnostic) => void;
}

export class ReactWorldBatchRejectedError extends Error {
  constructor(readonly outcome: Extract<StateOverlayOutcome, { ok: false }>) {
    super(
      `React commit rejected at ${outcome.error.scope}${outcome.error.operation === null ? "" : ` ${outcome.error.operation}`}: ${outcome.error.reason}`,
    );
    this.name = "ReactWorldBatchRejectedError";
  }
}

export class EntityOverlayBindingLostError extends Error {
  constructor() {
    super(
      "The entity binding was lost; explicitly remount or change its target/mode",
    );
    this.name = "EntityOverlayBindingLostError";
  }
}

/** True for GUI adoption refusals: the runtime tree belongs to another
 * writer (see GuiCommits adoption), so this attempt must release what it
 * acquired and leave foreign state alone. */
function isGuiAdoptionRefusal(error: unknown): boolean {
  return error instanceof Error && /another writer/.test(error.message);
}

type AcknowledgedStateOverlay<T> = {
  description: T;
  handle: bigint;
  lost: boolean;
};
type PendingStateOverlay<T> = {
  description: T;
  ref: StateOverlayRef;
  lost: boolean;
};

function handle(id: bigint): StateOverlayRef {
  return { kind: "handle", id };
}

type AcknowledgedEntity =
  AcknowledgedStateOverlay<EntityOverlayBindingDescription> & {
    entity: bigint;
  };

function equalField(
  a: ReactWorldFieldValue | undefined,
  b: StateOverlayFieldValue,
): boolean {
  if (a?.kind !== b.kind) return false;
  if (a.kind === "entity" && b.kind === "entity")
    return (
      a.value.kind === "handle" &&
      b.value.kind === "handle" &&
      a.value.id === b.value.id
    );
  if (a.kind === "bytes" && b.kind === "bytes") {
    if (a.value === b.value) return true;
    if (a.value.byteLength !== b.value.byteLength) return false;
    return a.value.every((value, index) => value === b.value[index]);
  }
  return Object.is(a.value, b.value);
}

function writes(
  fields: ReadonlyMap<number, StateOverlayFieldValue>,
): FieldWrite[] {
  return [...fields].map(([offset, value]) => ({
    offset,
    value,
  }));
}

export class ReactWorldCommits {
  private owner: bigint | undefined;
  private entities = new Map<number, AcknowledgedEntity>();
  private overlays = new Map<
    number,
    AcknowledgedStateOverlay<ComponentStateOverlayDescription>
  >();
  private readonly session: bigint;
  private readonly unsubscribe: () => void;
  private tail: Promise<void> = Promise.resolve();
  private latest: Promise<void> = Promise.resolve();
  private signature: string | undefined;
  private inFlight = false;
  private diagnostics: StateOverlayLifecycleDiagnostic[] = [];
  private fatal: Error | undefined;
  private needsReset = false;
  readonly assets: ReactAssetRegistry;
  readonly animations: ReactAnimationRegistry;
  readonly gui: GuiCommits;
  private desired: ReactWorldDescription | undefined;
  private assetCommitQueued = false;
  private localFailureGeneration = 0;
  private closing = false;
  /** Acknowledged identities at the outermost attempt start. The
   * entity-handle path re-enters apply after its first commit; only the
   * outermost frame snapshots, so a refusal cleans the whole attempt. */
  private attemptBase:
    | {
        owner: bigint | undefined;
        entities: Set<number>;
        overlays: Set<number>;
      }
    | undefined;
  /** Adoption-cleanup interrupted by a rejected batch. Core releases are
   * idempotent, so the next render retries exactly these handles; an owner
   * released elsewhere supersedes the record. */
  private pendingCleanup:
    | {
        owner: bigint;
        bindings: bigint[];
        overlays: bigint[];
        ownerCreated: boolean;
      }
    | undefined;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly options: ReactWorldRootOptions,
  ) {
    this.session = client.session;
    this.animations = new ReactAnimationRegistry(
      client,
      (work) => this.enqueue(work),
      (error) => this.report(error),
    );
    this.assets = new ReactAssetRegistry(
      client,
      () => {
        if (this.closing || this.assetCommitQueued) return;
        this.assetCommitQueued = true;
        const generation = this.localFailureGeneration;
        void this.enqueue(async () => {
          this.assetCommitQueued = false;
          if (
            !this.closing &&
            generation === this.localFailureGeneration &&
            this.desired
          )
            await this.apply(this.desired);
        }).catch(() => {});
      },
      (error) => this.report(error),
    );
    this.unsubscribe = client.onDiagnostic((diagnostic) => {
      if (client.session !== this.session) return;
      if (this.owner !== undefined && diagnostic.owner !== this.owner) return;
      if (this.inFlight) this.diagnostics.push(diagnostic);
      else this.observe(diagnostic);
    });
    this.gui = new GuiCommits(client, {
      checkSession: () => this.checkSession(),
      report: (error) => this.report(error),
    });
  }

  report(error: unknown): Error {
    const result = error instanceof Error ? error : new Error(String(error));
    try {
      if (this.options.onError) this.options.onError(result);
      else console.error(result);
    } catch (callbackError) {
      console.error("React root error callback failed", callbackError);
    }
    return result;
  }

  private observe(diagnostic: StateOverlayLifecycleDiagnostic): void {
    if (this.owner === undefined || diagnostic.owner !== this.owner) return;
    for (const entity of this.entities.values()) {
      if (entity.handle === diagnostic.stateOverlay) {
        entity.lost = true;
        for (const overlay of this.overlays.values()) {
          if (overlay.description.entity === entity.description.identity)
            overlay.lost = true;
        }
      }
    }
    for (const overlay of this.overlays.values()) {
      if (overlay.handle === diagnostic.stateOverlay) overlay.lost = true;
    }
    try {
      this.options.onDiagnostic?.(diagnostic);
    } catch (error) {
      this.report(error);
    }
  }

  private enqueue(work: () => Promise<void>): Promise<void> {
    const result = this.tail.then(work);
    this.tail = result.catch((error: unknown) => {
      this.report(error);
    });
    this.latest = result;
    return result;
  }

  capture(description: ReactWorldDescription): Promise<void> {
    this.desired = description;
    this.assets.setDesired(description.assets);
    this.animations.setDesired(description.animations);
    this.gui.reconcileLocal(description.gui);
    if (description.signature === this.signature) return this.latest;
    this.signature = description.signature;
    // A rejected commit keeps its signature: an identical retry dedups to the
    // cached rejection without transport ("an unchanged rejected tree does
    // not retry"), while a corrected tree carries a new signature and
    // re-enters apply. Forgetting the signature here would re-enter apply
    // with needsReset set and submit an owner release the caller never
    // settles, hanging the retry.
    return this.enqueue(() => this.apply(description));
  }

  failed(error: unknown): Promise<void> {
    // Snapshot validation has no transport effects. React error boundaries
    // still determine local tree recovery after errors thrown during rendering.
    // Stop resource notifications from reapplying the last valid description
    // while React's committed host tree is invalid. Queue the reset marker so
    // an older apply cannot observe and consume it before acknowledging the
    // ownership that the corrected render must release.
    this.signature = undefined;
    this.desired = undefined;
    this.localFailureGeneration++;
    return this.enqueue(async () => {
      this.needsReset = true;
      throw error;
    });
  }

  settled(): Promise<void> {
    return this.latest;
  }

  dispose(): Promise<void> {
    this.closing = true;
    this.assets.close();
    this.animations.close();
    return this.enqueue(async () => {
      try {
        await this.animations.dispose();
        await this.gui.dispose();
        await this.retryPendingCleanup();
        if (this.owner !== undefined) {
          await this.submitCleanup([
            { kind: "releaseStateOverlayOwner", owner: handle(this.owner) },
          ]);
          this.owner = undefined;
          this.entities.clear();
          this.overlays.clear();
        }
      } finally {
        try {
          await this.assets.dispose();
        } finally {
          this.unsubscribe();
        }
      }
    });
  }

  private checkSession(): void {
    if (this.fatal) throw this.fatal;
    if (this.client.session !== this.session) {
      this.fatal = new Error("Session replacement requires a new React root");
      throw this.fatal;
    }
  }

  private async submit(
    operations: StateOverlayCommand[],
  ): Promise<Extract<StateOverlayOutcome, { ok: true }>> {
    this.checkSession();
    let outcome: StateOverlayOutcome;
    try {
      outcome = await this.client.batch(operations);
    } catch (error) {
      if (
        error instanceof Error &&
        "code" in error &&
        (error.code === "IPP_REQUEST_NOT_SENT" ||
          error.code === "IPP_REQUEST_REJECTED")
      ) {
        // The generated client may come from another module instance. Match
        // the structural code, not its constructor. Corrected commits can retry.
        throw error;
      }
      // Without an outcome we cannot know whether an attachment committed.
      // Never guess ownership or replay an ambiguously completed batch.
      this.fatal = error instanceof Error ? error : new Error(String(error));
      throw this.fatal;
    }
    if (!outcome.ok) {
      // The core retained partial work. Keep its owner for unmount cleanup and
      // rebuild this root only when the caller supplies a corrected render.
      this.owner ??= outcome.stateOverlays.find(
        (alias) => alias.kind === "owner",
      )?.id;
      this.needsReset = true;
      throw new ReactWorldBatchRejectedError(outcome);
    }
    return outcome;
  }

  private async apply(description: ReactWorldDescription): Promise<void> {
    this.checkSession();
    if (this.attemptBase !== undefined) return this.applyAttempt(description);
    if (this.needsReset) {
      await this.animations.removeExcept(new Set());
      await this.gui.reset();
      if (this.owner !== undefined)
        await this.submit([
          { kind: "releaseStateOverlayOwner", owner: handle(this.owner) },
        ]);
      this.owner = undefined;
      this.entities.clear();
      this.overlays.clear();
      this.needsReset = false;
    }
    await this.retryPendingCleanup();
    this.attemptBase = {
      owner: this.owner,
      entities: new Set(this.entities.keys()),
      overlays: new Set(this.overlays.keys()),
    };
    try {
      await this.applyAttempt(description);
    } catch (error) {
      // Overlay and binding additions commit before GUI adoption. An adopt
      // refusal skips the remaining work, so release what this attempt
      // acquired; previously acknowledged declarations and foreign trees
      // are never removed. A failed cleanup retains its handles for the
      // next render, and unmount still releases the owner.
      if (isGuiAdoptionRefusal(error)) await this.cleanupAttemptDelta();
      throw error;
    } finally {
      this.attemptBase = undefined;
    }
  }

  private async applyAttempt(
    description: ReactWorldDescription,
  ): Promise<void> {
    this.checkSession();
    await this.animations.removeExcept(
      new Set(description.animations.map((animation) => animation.identity)),
    );
    await this.assets.prepare(description.assets);
    this.checkSession();
    description = {
      ...description,
      overlays: description.overlays.map((overlay) => {
        const fields = new Map(overlay.fields);
        for (const [offset, value] of fields)
          if (value.kind === "asset") {
            const current = this.assets.get(value.value)?.current;
            fields.set(offset, {
              kind: "string",
              value: current?.source ?? "",
            });
            const component = Object.values(this.client.components).find(
              (component) => component.id === overlay.component,
            );
            const variant = component?.fields.variant;
            if (variant)
              fields.set(variant.offset, {
                kind: "u32",
                value: current?.variant ?? 0,
              });
          }
        return { ...overlay, fields };
      }),
    };
    const operations: StateOverlayCommand[] = [];
    const deferredReleases = new Set<StateOverlayCommand>();
    let nextAlias = 1;
    const alias = (): StateOverlayRef => ({
      kind: "alias",
      alias: nextAlias++,
    });
    let owner: StateOverlayRef;
    if (this.owner === undefined) {
      if (description.entities.length === 0) {
        await this.animations.apply(
          description.animations,
          this.assets,
          () => undefined,
        );
        await this.assets.releaseUnused();
        return;
      }
      const ownerAlias = alias();
      owner = ownerAlias;
      operations.push({
        kind: "createStateOverlayOwner",
        alias: nextAlias - 1,
      });
    } else owner = handle(this.owner);

    const entities = new Map<
      number,
      PendingStateOverlay<EntityOverlayBindingDescription>
    >();
    const overlays = new Map<
      number,
      PendingStateOverlay<ComponentStateOverlayDescription>
    >();
    const desiredEntities = new Map(
      description.entities.map((entity) => [entity.identity, entity]),
    );
    const desiredOverlays = new Map(
      description.overlays.map((overlay) => [overlay.identity, overlay]),
    );

    // Release declarations before bindings and before all new attachments.
    // Retained declarations preserve their successful attachment precedence.
    for (const [identity, entity] of this.entities) {
      const next = desiredEntities.get(identity);
      if (
        next &&
        next.symbolicId === entity.description.symbolicId &&
        next.mode === entity.description.mode
      ) {
        entities.set(identity, {
          description: next,
          ref: handle(entity.handle),
          lost: entity.lost,
        });
      }
    }
    for (const [identity, overlay] of this.overlays) {
      const next = desiredOverlays.get(identity);
      if (
        next &&
        entities.has(next.entity) &&
        next.entity === overlay.description.entity &&
        next.mode === overlay.description.mode &&
        next.component === overlay.description.component
      ) {
        overlays.set(identity, {
          description: overlay.description,
          ref: handle(overlay.handle),
          lost: overlay.lost,
        });
      } else {
        const release: StateOverlayCommand = {
          kind: "releaseComponentStateOverlay",
          owner,
          overlay: handle(overlay.handle),
        };
        operations.push(release);
        const entity = this.entities.get(overlay.description.entity);
        if (
          overlay.description.component ===
            this.client.components.Surface?.id &&
          (description.gui.some(
            (root) => root.entity === overlay.description.entity,
          ) ||
            (entity !== undefined &&
              this.gui.hasAcknowledgedRoot(entity.entity)))
        )
          deferredReleases.add(release);
      }
    }
    for (const [identity, entity] of this.entities) {
      if (!entities.has(identity)) {
        const release: StateOverlayCommand = {
          kind: "releaseEntityOverlayBinding",
          owner,
          binding: handle(entity.handle),
        };
        operations.push(release);
        deferredReleases.add(release);
      }
    }
    for (const entity of description.entities) {
      if (entities.has(entity.identity)) continue;
      const ref = alias();
      operations.push({
        kind: "attachEntityOverlayBinding",
        owner,
        alias: nextAlias - 1,
        symbolicId: entity.symbolicId,
        mode: entity.mode,
      });
      entities.set(entity.identity, { description: entity, ref, lost: false });
    }
    // Entity binding aliases and EntityRef aliases occupy different protocol domains.
    // A new parent must be acknowledged before its handle can be written into Hierarchy.
    const hasChildren = description.overlays.some(
      (overlay) => overlay.identity < 0,
    );
    const needsEntityHandles =
      (hasChildren &&
        [...entities.values()].some((entity) => entity.ref.kind === "alias")) ||
      description.overlays.some((overlay) =>
        [...overlay.fields.values()].some(
          (value) =>
            value.kind === "binding" &&
            entities.get(value.value)?.ref.kind === "alias",
        ),
      );
    if (needsEntityHandles) {
      await this.commit(operations, owner, entities, overlays);
      await this.apply(description);
      return;
    }

    if (hasChildren) {
      const parents = new Map<bigint, number>();
      for (const declaration of description.overlays) {
        if (declaration.component !== this.client.components.Hierarchy?.id)
          continue;
        const target = this.entities.get(declaration.entity);
        if (!target || target.lost) continue;
        const previous = parents.get(target.entity);
        if (
          previous !== undefined &&
          (previous < 0 || declaration.identity < 0)
        )
          throw new Error(
            "Children conflicts with another parenting declaration for the same bound entity",
          );
        parents.set(target.entity, declaration.identity);
      }
    }

    for (const declaration of description.overlays) {
      const overlay = {
        ...declaration,
        fields: new Map(
          [...declaration.fields].map(
            ([offset, value]): [number, StateOverlayFieldValue] => {
              if (value.kind === "asset")
                throw new Error("Unresolved asset reference");
              if (value.kind !== "binding") return [offset, value];
              const parent = this.entities.get(value.value);
              if (!parent)
                throw new Error("Missing acknowledged parent entity");
              // Lost bindings retain their generation. Core owns invalidation; a surviving
              // declaration must never silently follow a replacement with the same name.
              return [offset, { kind: "entity", value: handle(parent.entity) }];
            },
          ),
        ),
      };
      const retained = overlays.get(overlay.identity);
      const entity = entities.get(overlay.entity);
      if (!entity) throw new Error("Missing declaration entity");
      if (retained) {
        // A strict loss is terminal for this attachment. Core diagnostics are
        // observational; changing ordinary fields must never reacquire it.
        if (retained.lost || entity.lost) continue;
        const previous = this.overlays.get(overlay.identity);
        if (!previous) throw new Error("Missing acknowledged overlay");
        const changed = new Map(
          [...overlay.fields].filter(([offset, value]) => {
            const old = previous.description.fields.get(offset);
            return !equalField(old, value);
          }),
        );
        retained.description = overlay;
        const clear = [...previous.description.fields.keys()].filter(
          (offset) => !overlay.fields.has(offset),
        );
        if (changed.size || clear.length)
          operations.push({
            kind: "updateComponentStateOverlay",
            owner,
            overlay: retained.ref,
            fields: writes(changed),
            clear,
          });
        const oldProperties = previous.description.properties ?? {};
        const properties = Object.fromEntries(
          Object.entries(overlay.properties ?? {}).filter(
            ([name, value]) =>
              JSON.stringify(value) !== JSON.stringify(oldProperties[name]),
          ),
        );
        const clearProperties = Object.keys(oldProperties).filter(
          (name) => !Object.hasOwn(overlay.properties ?? {}, name),
        );
        if (Object.keys(properties).length || clearProperties.length)
          operations.push({
            kind: "updateDynamicComponentStateOverlay",
            owner,
            overlay: retained.ref,
            properties,
            clear: clearProperties,
          });
      } else {
        if (entity.lost) throw new EntityOverlayBindingLostError();
        const ref = alias();
        operations.push({
          kind: "attachComponentStateOverlay",
          owner,
          binding: entity.ref,
          alias: nextAlias - 1,
          component: overlay.component,
          mode: overlay.mode,
          fields: writes(overlay.fields),
        });
        if (overlay.properties && Object.keys(overlay.properties).length)
          operations.push({
            kind: "updateDynamicComponentStateOverlay",
            owner,
            overlay: ref,
            properties: { ...overlay.properties },
            clear: [],
          });
        overlays.set(overlay.identity, {
          description: overlay,
          ref,
          lost: false,
        });
      }
    }
    // GUI nodes and reconciler-created producers must be removed while their
    // entity and related overlays still exist. Keep those releases after GUI
    // cleanup. Other component replacements retain their original ordered
    // release-before-attach batch so core validates the desired declaration.
    const releases =
      owner.kind === "handle"
        ? operations.filter((operation) => deferredReleases.has(operation))
        : [];
    const additions =
      releases.length > 0
        ? operations.filter((operation) => !deferredReleases.has(operation))
        : operations;
    if (additions.length)
      await this.commit(additions, owner, entities, overlays);
    // GUI node edits run after overlay acknowledgement so entity identities
    // and the GuiRoot incarnation are known. They never replay control values.
    await this.gui.apply(
      description.gui,
      (identity) => this.entities.get(identity)?.entity,
    );
    await this.animations.apply(
      description.animations,
      this.assets,
      (identity) => {
        const entity = this.entities.get(identity);
        return entity?.lost ? undefined : entity?.entity;
      },
    );
    if (releases.length) await this.commit(releases, owner, entities, overlays);
    await this.assets.releaseUnused();
  }

  /** Retry an adoption cleanup interrupted by a rejected batch. Releases
   * are idempotent, so re-sending converges even after partial
   * application; an owner released elsewhere supersedes the record. */
  private async retryPendingCleanup(): Promise<void> {
    const pending = this.pendingCleanup;
    if (!pending) return;
    const operations: StateOverlayCommand[] = [];
    for (const overlay of pending.overlays)
      operations.push({
        kind: "releaseComponentStateOverlay",
        owner: handle(pending.owner),
        overlay: handle(overlay),
      });
    for (const binding of pending.bindings)
      operations.push({
        kind: "releaseEntityOverlayBinding",
        owner: handle(pending.owner),
        binding: handle(binding),
      });
    if (pending.ownerCreated)
      operations.push({
        kind: "releaseStateOverlayOwner",
        owner: handle(pending.owner),
      });
    await this.submitCleanup(operations);
    this.pendingCleanup = undefined;
    const releasedOverlays = new Set(pending.overlays);
    for (const [identity, overlay] of [...this.overlays])
      if (releasedOverlays.has(overlay.handle)) this.overlays.delete(identity);
    const releasedBindings = new Set(pending.bindings);
    for (const [identity, entity] of [...this.entities])
      if (releasedBindings.has(entity.handle)) this.entities.delete(identity);
    if (pending.ownerCreated && this.owner === pending.owner)
      this.owner = undefined;
  }

  /** Cleanup must remain possible after an unrelated fatal commit error. The
   * original session fence still applies, while idempotent release commands
   * retain their exact owner/resource handles across rejected attempts. */
  private async submitCleanup(
    operations: StateOverlayCommand[],
  ): Promise<void> {
    if (this.client.session !== this.session)
      throw new Error("Session replacement prevented React root cleanup");
    const outcome = await this.client.batch(operations);
    if (!outcome.ok) throw new ReactWorldBatchRejectedError(outcome);
  }

  private rememberCleanup(
    owner: bigint,
    bindings: readonly bigint[],
    overlays: readonly bigint[],
    ownerCreated: boolean,
  ): void {
    const current = this.pendingCleanup;
    if (current && current.owner !== owner)
      throw new Error("Cannot retain cleanup for multiple overlay owners");
    this.pendingCleanup = {
      owner,
      bindings: [...new Set([...(current?.bindings ?? []), ...bindings])],
      overlays: [...new Set([...(current?.overlays ?? []), ...overlays])],
      ownerCreated: (current?.ownerCreated ?? false) || ownerCreated,
    };
  }

  /** Release resources this attempt acquired after a GUI adoption refusal.
   * Only identities absent from the attempt snapshot are released, in
   * reverse creation order (overlays, bindings, then an owner this attempt
   * created). A rejected cleanup reports and retains its handles for the
   * next render; ambiguous transport failures stay fatal instead of
   * guessing ownership. */
  private async cleanupAttemptDelta(): Promise<void> {
    const base = this.attemptBase;
    if (!base) return;
    this.checkSession();
    const overlays: bigint[] = [];
    for (const [identity, overlay] of this.overlays)
      if (!base.overlays.has(identity)) overlays.push(overlay.handle);
    const bindings: bigint[] = [];
    for (const [identity, entity] of this.entities)
      if (!base.entities.has(identity)) bindings.push(entity.handle);
    const ownerCreated = this.owner !== undefined && this.owner !== base.owner;
    if (!overlays.length && !bindings.length && !ownerCreated) return;
    const owner = this.owner;
    if (owner === undefined) return;
    this.rememberCleanup(owner, bindings, overlays, ownerCreated);
    try {
      await this.retryPendingCleanup();
    } catch (error) {
      this.report(error);
      return;
    }
    for (const identity of [...this.overlays.keys()])
      if (!base.overlays.has(identity)) this.overlays.delete(identity);
    for (const identity of [...this.entities.keys()])
      if (!base.entities.has(identity)) this.entities.delete(identity);
    if (ownerCreated && this.owner === owner) this.owner = undefined;
  }

  private async commit(
    operations: StateOverlayCommand[],
    owner: StateOverlayRef,
    entities: ReadonlyMap<
      number,
      PendingStateOverlay<EntityOverlayBindingDescription>
    >,
    overlays: ReadonlyMap<
      number,
      PendingStateOverlay<ComponentStateOverlayDescription>
    >,
  ): Promise<void> {
    this.inFlight = true;
    try {
      const outcome = await this.submit(operations);
      const resolve = (
        ref: StateOverlayRef,
        kind: StateOverlayAlias["kind"],
      ): bigint => {
        if (ref.kind === "handle") return ref.id;
        const resource = outcome.stateOverlays.find(
          (resource) => resource.alias === ref.alias && resource.kind === kind,
        );
        if (!resource) {
          this.fatal = new Error(
            `Successful batch omitted ${kind} resource alias ${ref.alias}`,
          );
          throw this.fatal;
        }
        return resource.id;
      };
      // One apply may commit additions and releases as separate batches so
      // GUI teardown runs while entities still exist. Aliases attached by a
      // sibling commit are never re-resolved here: their records carry over
      // from acknowledged state instead.
      const claimed = new Set<number>();
      for (const operation of operations) {
        if (
          operation.kind === "createStateOverlayOwner" ||
          operation.kind === "attachEntityOverlayBinding" ||
          operation.kind === "attachComponentStateOverlay"
        )
          claimed.add(operation.alias);
      }
      const nextOwner =
        owner.kind === "handle" || claimed.has(owner.alias)
          ? resolve(owner, "owner")
          : this.owner;
      if (nextOwner === undefined)
        throw new Error("Missing acknowledged owner for unclaimed reference");
      const nextEntities = new Map<number, AcknowledgedEntity>();
      const acceptedBindings: bigint[] = [];
      const missingBindingEntities: number[] = [];
      for (const [identity, record] of entities) {
        const ref = record.ref;
        if (ref.kind === "handle" || !claimed.has(ref.alias)) {
          const acknowledged = this.entities.get(identity);
          if (!acknowledged)
            throw new Error(
              "Missing acknowledged entity for unclaimed reference",
            );
          nextEntities.set(identity, {
            description: record.description,
            handle: acknowledged.handle,
            entity: acknowledged.entity,
            lost: record.lost,
          });
          continue;
        }
        const binding = resolve(ref, "entityOverlayBinding");
        acceptedBindings.push(binding);
        const entity = outcome.stateOverlays.find(
          (resource) =>
            resource.alias === ref.alias &&
            resource.kind === "entityOverlayBinding",
        )?.entity;
        if (entity == null) {
          missingBindingEntities.push(identity);
          continue;
        }
        nextEntities.set(identity, {
          description: record.description,
          handle: binding,
          entity,
          lost: record.lost,
        });
      }
      const nextOverlays = new Map<
        number,
        AcknowledgedStateOverlay<ComponentStateOverlayDescription>
      >();
      const acceptedOverlays: bigint[] = [];
      for (const [identity, record] of overlays) {
        const ref = record.ref;
        if (ref.kind === "handle" || !claimed.has(ref.alias)) {
          const acknowledged = this.overlays.get(identity);
          if (!acknowledged)
            throw new Error(
              "Missing acknowledged overlay for unclaimed reference",
            );
          nextOverlays.set(identity, {
            description: record.description,
            handle: acknowledged.handle,
            lost: record.lost,
          });
          continue;
        }
        const overlay = resolve(ref, "componentStateOverlay");
        acceptedOverlays.push(overlay);
        nextOverlays.set(identity, {
          description: record.description,
          handle: overlay,
          lost: record.lost,
        });
      }
      this.owner = nextOwner;
      if (missingBindingEntities.length > 0) {
        const ownerCreated = owner.kind === "alias" && claimed.has(owner.alias);
        this.rememberCleanup(
          nextOwner,
          acceptedBindings,
          acceptedOverlays,
          ownerCreated,
        );
        throw new Error(
          `Successful binding attachment omitted entity identity for declaration ${missingBindingEntities.join(", ")}`,
        );
      }
      this.entities = new Map([...this.entities, ...nextEntities]);
      this.overlays = new Map([...this.overlays, ...nextOverlays]);
      for (const operation of operations) {
        if (operation.kind === "releaseEntityOverlayBinding") {
          for (const [identity, acknowledged] of this.entities) {
            if (
              operation.binding.kind === "handle" &&
              acknowledged.handle === operation.binding.id
            )
              this.entities.delete(identity);
          }
        }
        if (operation.kind === "releaseComponentStateOverlay") {
          for (const [identity, acknowledged] of this.overlays) {
            if (
              operation.overlay.kind === "handle" &&
              acknowledged.handle === operation.overlay.id
            )
              this.overlays.delete(identity);
          }
        }
      }
    } finally {
      this.inFlight = false;
      const diagnostics = this.diagnostics;
      this.diagnostics = [];
      for (const diagnostic of diagnostics) this.observe(diagnostic);
    }
  }
}
