import { ReactControlRefs } from "./control_refs.js";
import { ReactGuiCallbacks } from "./gui_callbacks.js";
import { fieldIdentity, type DeclarationFieldValue } from "./field_values.js";
import { ReactAnimationRegistry } from "./animation_state.js";
import { ReactAssetRegistry } from "./asset_state.js";
import type { ReactCompositionHost } from "./attached-world.js";
import {
  orderEntityLinks,
  siblingChains,
  siblingsInPlace,
  type ResolvedEntityLink,
} from "./entity_links.js";
import type { ReactEntityReference } from "./entity_references.js";
import type {
  BatchOutcome,
  Command,
  ComponentDescriptor,
  DynamicValue,
  EntityRef,
  FieldWrite,
} from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import type {
  ReactComponentDescription,
  ReactEntityDescription,
  ReactWorldDescription,
  ReactWorldFieldValue,
} from "./tree.js";

export interface ReactWorldRootOptions {
  host?: ReactCompositionHost;
  onError?: (error: Error) => void;
}

export class ReactWorldBatchRejectedError extends Error {
  constructor(readonly outcome: Extract<BatchOutcome, { ok: false }>) {
    super(
      `React commit rejected at ${outcome.error.scope}${outcome.error.operation === null ? "" : ` ${outcome.error.operation}`}: ${outcome.error.reason}`,
    );
    this.name = "ReactWorldBatchRejectedError";
  }
}

/**
 * An entity this root reaches through an Entity declaration. A declared
 * entity (`<Entity id>`) is created or adopted by the root and deleted when
 * its declaration disappears; a bound entity (`<Entity bindTo>`) is only
 * referred to by its symbolic id and never deleted.
 */
interface EntityRecord {
  description: ReactEntityDescription;
  /** The handle; a bound entity's is learned when a command resolves it. */
  entity: bigint | undefined;
}

/**
 * A component declaration whose component the root inserted or adopted; the
 * component is removed when its declaration disappears.
 */
interface ComponentRecord {
  description: ReactComponentDescription;
  readonly entity: EntityRecord;
  /** Changes when the root inserts or adopts the component again. */
  readonly serial: number;
  /** Acknowledged declared values, with batch aliases replaced by handles. */
  readonly fields: Map<number, DeclarationFieldValue>;
  readonly properties: Record<string, DynamicValue>;
  /** The resolved field map whose every value is acknowledged. */
  declared: ReadonlyMap<number, ReactWorldFieldValue> | undefined;
}

/** The placement the root last applied for a link declaration. */
interface LinkRecord {
  description: ResolvedEntityLink;
  readonly entity: EntityRecord;
}

/** One command and the acknowledged state it establishes once applied. */
interface PlannedCommand {
  readonly command: Command;
  readonly applied?: (batch: AppliedBatch, operation: number) => void;
}

/** What one batch outcome reported about aliases and symbols. */
class AppliedBatch {
  private readonly aliases = new Map<number, bigint>();
  readonly symbols = new Map<string, bigint>();

  constructor(outcome: BatchOutcome) {
    for (const { alias, id } of outcome.aliases) this.aliases.set(alias, id);
    for (const { symbol, id } of outcome.symbols) this.symbols.set(symbol, id);
  }

  /** The entity a same-batch alias named. */
  entity(alias: number): bigint {
    const entity = this.aliases.get(alias);
    if (entity === undefined)
      throw new Error(`Missing entity of alias ${alias}`);
    return entity;
  }
}

/**
 * How many leading commands of `count` an outcome applied, or `undefined`
 * when the outcome does not say. Batches apply in order and stop at the
 * failing operation; a commit failure follows every operation, except a
 * faulted World, which applies nothing.
 */
function appliedCommands(
  outcome: BatchOutcome,
  count: number,
): number | undefined {
  if (outcome.ok) return count;
  const { scope, operation, reason } = outcome.error;
  if (scope === "operation")
    return operation !== null &&
      Number.isInteger(operation) &&
      operation >= 0 &&
      operation < count
      ? operation
      : undefined;
  return reason === "NonConvergentCommit" ? undefined : count;
}

/** Rejections of a removal whose target is already gone. */
const alreadyRemoved = new Set([
  "InvalidEntity",
  "MissingComponent",
  "MissingSymbolicId",
]);

/** Whether a failed removal only found its target already gone. */
function removedAlready(command: Command, outcome: BatchOutcome): boolean {
  return (
    !outcome.ok &&
    outcome.error.scope === "operation" &&
    (command.kind === "delete" || command.kind === "removeComponent") &&
    alreadyRemoved.has(outcome.error.reason)
  );
}

function equalField(
  left: DeclarationFieldValue | undefined,
  right: DeclarationFieldValue,
): boolean {
  return left !== undefined && fieldIdentity(left) === fieldIdentity(right);
}

function writes(
  fields: ReadonlyMap<number, DeclarationFieldValue>,
): FieldWrite[] {
  return [...fields].map(([offset, value]) => ({ offset, value }));
}

/** A value with a same-batch entity alias replaced by the entity it named. */
function acknowledgedValue(
  value: DeclarationFieldValue,
  batch: AppliedBatch,
): DeclarationFieldValue {
  return value.kind === "entity" && value.value.kind === "alias"
    ? {
        kind: "entity",
        value: { kind: "handle", id: batch.entity(value.value.alias) },
      }
    : value;
}

export class ReactWorldCommits {
  private unknownOutcome: unknown;
  private readonly entities = new Map<number, EntityRecord>();
  /** Records whose declaration disappeared and whose cleanup is pending. */
  private readonly orphans = new Set<EntityRecord>();
  /**
   * Symbolic ids of entities a batch of unknown applied extent may have
   * created. They have no record, so cleanup deletes them by symbol.
   */
  private readonly uncertain = new Set<string>();
  private readonly components = new Map<number, ComponentRecord>();
  /**
   * Component declarations whose acknowledgement changed since controls were
   * last published, so publication rechecks only these.
   */
  private readonly reacknowledged = new Set<number>();
  private readonly links = new Map<number, LinkRecord>();
  /** Per resolved parent, each link's position in the last fully applied
   * sibling chain; a partially applied placement forgets its group. */
  private linkOrders = new Map<bigint | null, ReadonlyMap<number, number>>();
  private nextSerial = 1;
  private componentsById: Map<number, ComponentDescriptor> | undefined;
  private readonly session: bigint;
  private tail: Promise<void> = Promise.resolve();
  private latest: Promise<void> = Promise.resolve();
  private pendingRender:
    | { description: ReactWorldDescription; result: Promise<void> }
    | undefined;
  private signature: string | undefined;
  private fatal: Error | undefined;
  private needsReset = false;
  readonly assets: ReactAssetRegistry;
  readonly animations: ReactAnimationRegistry;
  private readonly controls: ReactControlRefs;
  private readonly callbacks: ReactGuiCallbacks;
  private desired: ReactWorldDescription | undefined;
  private assetCommitQueued = false;
  private controlRefreshQueued = false;
  private localFailureGeneration = 0;
  private closing = false;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly options: ReactWorldRootOptions,
  ) {
    this.session = client.session;
    this.controls = new ReactControlRefs(
      client,
      (error) => this.report(error),
      () => {
        if (this.closing || client.closure || this.controlRefreshQueued) return;
        this.controlRefreshQueued = true;
        void this.enqueue(async () => {
          this.controlRefreshQueued = false;
          if (!this.closing && this.desired) await this.publishControls();
        }).catch(() => {});
      },
    );
    this.callbacks = new ReactGuiCallbacks(
      client,
      this.controls,
      (work) => this.scheduleCallback(work),
      (error) => this.report(error),
    );
    void client.closed?.then(({ reason }) => {
      this.fatal = reason;
      this.closing = true;
      this.assets.close();
      this.animations.close();
      this.controls.close();
      this.callbacks.fence();
    });
    this.animations = new ReactAnimationRegistry(
      client,
      (work) => this.enqueue(work),
      (error) => this.report(error),
    );
    this.assets = new ReactAssetRegistry(
      client,
      () => {
        if (this.closing || client.closure || this.assetCommitQueued) return;
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

  /** Forget a record and every component and link declared on it. */
  private forgetEntity(record: EntityRecord): void {
    this.orphans.delete(record);
    for (const [identity, entity] of this.entities)
      if (entity === record) this.entities.delete(identity);
    for (const [identity, component] of this.components)
      if (component.entity === record) this.forgetComponent(identity);
    for (const [identity, link] of this.links)
      if (link.entity === record) this.links.delete(identity);
  }

  /** Forget one component record; its control is acknowledged anew. */
  private forgetComponent(identity: number): void {
    this.components.delete(identity);
    this.reacknowledged.add(identity);
  }

  /** Forget bound records whose declared components are all removed. */
  private pruneOrphans(): void {
    for (const record of this.orphans)
      if (
        record.description.kind === "bound" &&
        ![...this.components.values()].some(
          (component) => component.entity === record,
        )
      )
        this.orphans.delete(record);
  }

  /** Forget every record after its World state was deleted. */
  private forgetRecords(): void {
    this.entities.clear();
    this.orphans.clear();
    this.components.clear();
    this.links.clear();
    this.linkOrders.clear();
    this.publishEntities();
  }

  private publishEntities(): void {
    const acknowledged: {
      description: ReactEntityDescription;
      entity: bigint;
    }[] = [];
    for (const record of this.entities.values())
      if (record.entity !== undefined)
        acknowledged.push({
          description: record.description,
          entity: record.entity,
        });
    this.callbacks.acknowledge(acknowledged);
  }

  private enqueue(work: () => Promise<void>): Promise<void> {
    // Explicit work is an ordering boundary for a replaceable render slot.
    this.pendingRender = undefined;
    const result = this.tail.then(work);
    this.tail = result.catch((error: unknown) => {
      this.report(error);
    });
    this.latest = result;
    return result;
  }

  private scheduleCallback(work: () => Promise<void>): Promise<void> {
    const result = this.tail.then(work);
    this.tail = result.catch((error: unknown) => {
      this.report(error);
    });
    return result;
  }

  capture(description: ReactWorldDescription): Promise<void> {
    // Unmount fences before React clears the tree: the empty tree is never
    // committed, so unmount deletes nothing.
    if (this.closing) return this.latest;
    if (this.client.closure) return this.failed(this.client.closure.reason);
    this.desired = description;
    this.assets.setDesired(description.assets);
    this.animations.setDesired(description.animations);
    this.callbacks.setDesired(description);
    this.controls.setDesired(description);
    if (description.signature === this.signature) {
      if (
        !this.controls.needsPublication() &&
        !this.callbacks.needsPreparation()
      )
        return this.latest;
      const acknowledged = this.latest;
      return this.enqueue(async () => {
        await acknowledged;
        await this.publishControls();
      });
    }
    this.signature = description.signature;
    // A rejected commit keeps its signature: an identical retry dedups to the
    // cached rejection without transport ("an unchanged rejected tree does
    // not retry"), while a corrected tree carries a new signature and
    // re-enters apply against the acknowledged and partially applied state.
    if (this.pendingRender) {
      this.pendingRender.description = description;
      this.latest = this.pendingRender.result;
      return this.latest;
    }
    const pending = { description, result: Promise.resolve() };
    const result = this.enqueue(() => {
      if (this.pendingRender === pending) this.pendingRender = undefined;
      return this.apply(pending.description);
    });
    pending.result = result;
    this.pendingRender = pending;
    return result;
  }

  failed(error: unknown): Promise<void> {
    if (this.closing) return this.latest;
    // Snapshot validation has no transport effects. React error boundaries
    // still determine local tree recovery after errors thrown during rendering.
    // Stop resource notifications from reapplying the last valid description
    // while React's committed host tree is invalid. Queue the reset marker so
    // an older apply cannot observe and consume it before acknowledging the
    // records that the corrected render must delete.
    this.signature = undefined;
    this.desired = undefined;
    this.localFailureGeneration++;
    return this.enqueue(async () => {
      this.needsReset = true;
      this.controls.reset();
      throw error;
    });
  }

  settled(): Promise<void> {
    return this.latest;
  }

  checkpoint(): Promise<void> {
    this.pendingRender = undefined;
    return this.latest;
  }

  /**
   * Stop authoring: later descriptions are not committed, and refs,
   * callbacks, assets and animations are fenced.
   */
  fence(): void {
    this.closing = true;
    this.assets.close();
    this.animations.close();
    this.controls.close();
    this.callbacks.fence();
  }

  /**
   * Fence and release this root's subscriptions after earlier commits
   * settle. Unmount deletes nothing: entities, components, animation
   * controllers and assets stay in the World. `remove` first deletes what the
   * records hold, for an attached-World boundary whose declaration was
   * removed.
   */
  dispose(remove = false): Promise<void> {
    this.fence();
    return this.enqueue(async () => {
      const errors: unknown[] = [];
      try {
        for (const cleanup of [
          () => this.animations.dispose(remove),
          () => this.controls.dispose(),
          () => this.callbacks.dispose(),
          ...(remove ? [() => this.deleteRecords()] : []),
        ]) {
          try {
            await cleanup();
          } catch (error) {
            errors.push(error);
          }
        }
        if (remove && this.unknownOutcome) errors.push(this.unknownOutcome);
        if (errors.length)
          throw new AggregateError(
            errors,
            "React declaration cleanup is incomplete",
          );
      } finally {
        await this.assets.dispose(remove);
      }
    });
  }

  private checkSession(): void {
    if (this.client.closure) throw this.client.closure.reason;
    if (this.fatal) throw this.fatal;
    if (this.client.session !== this.session) {
      this.fatal = new Error("Session replacement requires a new React root");
      throw this.fatal;
    }
  }

  /** Send one batch; an outcome, successful or not, returns normally. */
  private async send(operations: Command[]): Promise<BatchOutcome> {
    this.checkSession();
    try {
      return await this.client.batch(operations);
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
      // Without an outcome we cannot know what applied. Never guess records
      // or replay an ambiguously completed batch.
      this.fatal = error instanceof Error ? error : new Error(String(error));
      this.unknownOutcome = this.fatal;
      throw this.fatal;
    }
  }

  /** The record of the one entity a symbolic reference names. */
  private namedEntity(reference: string): EntityRecord {
    let match: EntityRecord | undefined;
    for (const record of this.entities.values()) {
      if (
        record.description.symbolicId !== reference ||
        record.entity === undefined
      )
        continue;
      if (match && match.entity !== record.entity)
        throw new Error(
          `Reference must identify one acknowledged Entity: ${reference}`,
        );
      match = record;
    }
    if (!match)
      throw new Error(
        `Reference must identify one acknowledged Entity: ${reference}`,
      );
    return match;
  }

  resolveEntity(reference: string | bigint): bigint {
    this.checkSession();
    if (typeof reference === "bigint") return reference;
    return this.namedEntity(reference).entity!;
  }

  /**
   * A token that stays equal while the acknowledged Camera declaration of
   * `reference` is unchanged: the same entity and the same inserted or
   * adopted Camera component. Undefined when this root does not declare that
   * component, so callers must bind the output again.
   */
  outputWitness(reference: string | bigint): string | undefined {
    if (typeof reference === "bigint") return undefined;
    let record: EntityRecord;
    try {
      record = this.namedEntity(reference);
    } catch {
      return undefined;
    }
    const component = this.client.components.Camera?.id;
    for (const declared of this.components.values())
      if (
        declared.entity === record &&
        declared.description.component === component
      )
        return `${record.entity}:${declared.serial}`;
    return undefined;
  }

  private async apply(description: ReactWorldDescription): Promise<void> {
    this.checkSession();
    if (this.needsReset) this.controls.reset(true);
    const failures: unknown[] = [];
    try {
      await this.controls.releasePending();
    } catch (error) {
      failures.push(error);
    }
    try {
      if (this.needsReset) {
        await this.animations.removeExcept(new Set());
        // The recommit adopts the components on bound entities again,
        // so the reset keeps them, with their last written values.
        await this.deleteRecords(false);
        this.needsReset = false;
      }
      await this.applyAttempt(description);
    } catch (error) {
      failures.push(error);
    }
    // Callbacks follow the acknowledged tree: a failed commit publishes no
    // registration until a later commit succeeds.
    if (failures.length) this.callbacks.suspend();
    else this.callbacks.resume();
    if (failures.length === 1) throw failures[0];
    if (failures.length)
      throw new AggregateError(failures, "React declaration commit incomplete");
  }

  private component(id: number): ComponentDescriptor | undefined {
    this.componentsById ??= new Map(
      Object.values(this.client.components).map((component) => [
        component.id,
        component,
      ]),
    );
    return this.componentsById.get(id);
  }

  /** Replace asset references with the prepared asset each names. */
  private resolveAssets(
    declared: ReactComponentDescription,
  ): ReadonlyMap<number, ReactWorldFieldValue> {
    let fields: Map<number, ReactWorldFieldValue> | undefined;
    for (const [offset, value] of declared.fields) {
      if (value.kind !== "row-asset" && value.kind !== "asset") continue;
      fields ??= new Map(declared.fields);
      const current = this.assets.get(value.value)?.current;
      if (value.kind === "row-asset") {
        fields.set(
          offset,
          current
            ? { kind: "dynamic", value: { kind: "asset", value: current } }
            : { kind: "unset" },
        );
        continue;
      }
      fields.set(offset, { kind: "string", value: current?.source ?? "" });
      const variant = this.component(declared.component)?.fields.variant;
      if (variant)
        fields.set(variant.offset, {
          kind: "u32",
          value: current?.variant ?? 0,
        });
    }
    return fields ?? declared.fields;
  }

  /**
   * Plan and submit one render: remove components whose declarations
   * disappeared, create or adopt new declared entities, place links, insert
   * or adopt new components and write changed fields, then delete entities
   * whose declarations disappeared.
   */
  private async applyAttempt(
    description: ReactWorldDescription,
  ): Promise<void> {
    this.checkSession();
    await this.animations.removeExcept(
      new Set(description.animations.map((animation) => animation.identity)),
    );
    await this.assets.prepare(description.assets);
    this.checkSession();
    const plan: PlannedCommand[] = [];
    let nextAlias = 1;

    // Entity records: keep a declaration's record while its symbolic id is
    // unchanged. An id that moved to another node (a keyed remount or a moved
    // declaration) takes over the record whose declaration disappeared, so
    // its entity is not deleted. A description has at most one `<Entity id>`
    // declaration per symbolic id.
    const desiredEntities = new Map(
      description.entities.map((entity) => [entity.identity, entity]),
    );
    for (const [identity, record] of this.entities) {
      const next = desiredEntities.get(identity);
      if (next?.symbolicId === record.description.symbolicId)
        record.description = next;
      else {
        this.entities.delete(identity);
        this.orphans.add(record);
      }
    }
    const created = new Map<number, number>();
    for (const entity of description.entities) {
      if (this.entities.has(entity.identity)) continue;
      const orphan = [...this.orphans].find(
        (record) => record.description.symbolicId === entity.symbolicId,
      );
      if (orphan) {
        this.orphans.delete(orphan);
        orphan.description = entity;
        this.entities.set(entity.identity, orphan);
      } else if (entity.kind === "bound") {
        const known = [...this.entities.values()].find(
          (record) =>
            record.description.symbolicId === entity.symbolicId &&
            record.entity !== undefined,
        );
        this.entities.set(entity.identity, {
          description: entity,
          entity: known?.entity,
        });
      } else created.set(entity.identity, nextAlias++);
    }
    this.pruneOrphans();

    // Links compare entities by handle, so two bound declarations that name
    // one entity by different symbolic ids conflict. Resolve the bound
    // entities links name before planning, so such a conflict rejects the
    // render before any placement. A bound declaration of an entity this
    // render creates has no handle yet; its symbol resolves at the Host once
    // the creation applies.
    const creating = new Set(
      [...created.keys()].map(
        (identity) => desiredEntities.get(identity)!.symbolicId,
      ),
    );
    const linked = (reference: ReactEntityReference | null) => {
      if (reference === null || typeof reference === "bigint") return false;
      const record = this.entities.get(reference.entity);
      return (
        record?.entity === undefined &&
        record?.description.kind === "bound" &&
        !creating.has(record.description.symbolicId)
      );
    };
    if (
      description.links.some(
        (link) =>
          linked({ entity: link.entity }) ||
          linked(link.parent) ||
          linked(link.before),
      )
    )
      await this.resolveBoundEntities(creating);

    const recordRef = (record: EntityRecord): EntityRef =>
      record.description.kind === "bound" || record.entity === undefined
        ? { kind: "symbol", symbol: record.description.symbolicId }
        : { kind: "handle", id: record.entity };
    const entityRef = (identity: number): EntityRef => {
      const alias = created.get(identity);
      if (alias !== undefined) return { kind: "alias", alias };
      const record = this.entities.get(identity);
      if (!record) throw new Error("Missing declaration entity");
      return recordRef(record);
    };

    // Component declarations that disappeared or moved. Another declaration
    // of the same component on the same entity takes over its component;
    // entity deletion removes the components of declared entities.
    const desiredComponents = new Map(
      description.components.map((component) => [
        component.identity,
        component,
      ]),
    );
    const retained = new Map<number, ComponentRecord>();
    for (const [identity, record] of this.components) {
      const next = desiredComponents.get(identity);
      const entity = record.entity;
      const component = record.description.component;
      if (
        next &&
        next.component === component &&
        this.entities.get(next.entity) === entity
      ) {
        retained.set(identity, record);
        continue;
      }
      // Another declaration of the component on the same entity keeps it,
      // including one under a `bindTo` reference to its symbolic id.
      const covered = description.components.some(
        (other) =>
          other.identity !== identity &&
          other.component === component &&
          desiredEntities.get(other.entity)?.symbolicId ===
            entity.description.symbolicId,
      );
      const deleted =
        entity.description.kind === "declared" && this.orphans.has(entity);
      if (covered || deleted) {
        this.forgetComponent(identity);
        continue;
      }
      plan.push({
        command: {
          kind: "removeComponent",
          entity: recordRef(entity),
          component,
        },
        applied: () => {
          if (this.components.get(identity) === record)
            this.forgetComponent(identity);
        },
      });
    }
    this.pruneOrphans();

    for (const entity of description.entities) {
      const alias = created.get(entity.identity);
      if (alias === undefined) continue;
      plan.push({
        command: {
          kind: "create",
          alias,
          metadata: { symbolicId: entity.symbolicId, classes: [] },
          adopt: true,
        },
        applied: (batch) => {
          this.entities.set(entity.identity, {
            description: entity,
            entity: batch.entity(alias),
          });
        },
      });
    }

    // Link ordering compares entities by handle; an entity whose handle this
    // batch reveals has a negative placeholder key until it applies.
    const placeholders = new Map<bigint, number>();
    let nextPlaceholder = -1n;
    const keyOf = (identity: number): bigint => {
      const record = created.has(identity)
        ? undefined
        : this.entities.get(identity);
      if (record?.entity !== undefined) return record.entity;
      const key = nextPlaceholder--;
      placeholders.set(key, identity);
      return key;
    };
    const entityKeys = new Map<number, bigint>();
    const resolveKey = (
      reference: ReactEntityReference | null,
    ): bigint | null => {
      if (reference === null || typeof reference === "bigint") return reference;
      let key = entityKeys.get(reference.entity);
      if (key === undefined) {
        if (!desiredEntities.has(reference.entity))
          throw new Error("Missing link declaration entity");
        key = keyOf(reference.entity);
        entityKeys.set(reference.entity, key);
      }
      return key;
    };
    const keyRef = (key: bigint): EntityRef =>
      key < 0n
        ? entityRef(placeholders.get(key)!)
        : { kind: "handle", id: key };
    /** The handle a placeholder key stands for, once a batch revealed it. */
    const acknowledgedKey = (
      key: bigint | null,
      batch: AppliedBatch | undefined,
    ): bigint | null | undefined => {
      if (key === null || key >= 0n) return key;
      const identity = placeholders.get(key)!;
      const alias = created.get(identity);
      if (alias !== undefined) return batch?.entity(alias);
      return batch?.symbols.get(desiredEntities.get(identity)!.symbolicId);
    };
    const acknowledgedLink = (
      link: ResolvedEntityLink,
      batch: AppliedBatch | undefined,
    ): ResolvedEntityLink => {
      const target = acknowledgedKey(link.target, batch);
      const parent = acknowledgedKey(link.parent, batch);
      const before = acknowledgedKey(link.before, batch);
      if (target == null || parent === undefined || before === undefined)
        throw new Error("Missing entity of an applied placement");
      return { ...link, target, parent, before };
    };

    const desiredLinks = new Map(
      description.links.map((link) => [link.identity, link]),
    );
    const keptLinks = new Map<number, LinkRecord>();
    for (const [identity, link] of this.links) {
      const next = desiredLinks.get(identity);
      if (
        next &&
        next.entity === link.description.entity &&
        !created.has(next.entity) &&
        this.entities.get(next.entity) === link.entity
      )
        keptLinks.set(identity, link);
      // A removed link leaves its entity where it was.
      else this.links.delete(identity);
    }
    const resolvedLinks = orderEntityLinks(
      description.links.map((link) => ({
        ...link,
        target: resolveKey({ entity: link.entity })!,
        parent: resolveKey(link.parent),
        before: resolveKey(link.before),
      })),
    );
    // Placement labels are fixed when a link is placed, so a sibling keeps
    // its label until moved. Within one sibling chain, keep the largest set of
    // retained links whose previous order already agrees and place every
    // other link before its successor, last sibling first. Groups that are
    // not one chain re-place any link whose declaration or anchor moved.
    const chains = siblingChains(resolvedLinks);
    const chained = new Set<number>();
    const inPlace = new Set<number>();
    for (const [parent, order] of chains) {
      if (!order) continue;
      for (const identity of order) chained.add(identity);
      const previous = this.linkOrders.get(parent);
      if (!previous) continue;
      const candidates = order.filter(
        (identity) => keptLinks.get(identity)?.description.parent === parent,
      );
      for (const identity of siblingsInPlace(candidates, previous))
        inPlace.add(identity);
    }
    const placedGroups = new Set<bigint | null>();
    const unplaced: { record: LinkRecord; link: ResolvedEntityLink }[] = [];
    const movedLinks = new Set<bigint>();
    for (const link of resolvedLinks) {
      const kept = keptLinks.get(link.identity);
      if (kept) {
        const moves = chained.has(link.identity)
          ? !inPlace.has(link.identity)
          : kept.description.parent !== link.parent ||
            kept.description.before !== link.before ||
            (link.before !== null && movedLinks.has(link.before));
        if (!moves) {
          unplaced.push({ record: kept, link });
          continue;
        }
      }
      placedGroups.add(link.parent);
      movedLinks.add(link.target);
      plan.push({
        command: {
          kind: "placeEntity",
          entity: keyRef(link.target),
          placement: {
            parent: link.parent === null ? null : keyRef(link.parent),
            before: link.before === null ? null : keyRef(link.before),
          },
        },
        applied: (batch) => {
          const entity = this.entities.get(link.entity);
          if (entity)
            this.links.set(link.identity, {
              description: acknowledgedLink(link, batch),
              entity,
            });
        },
      });
    }

    for (const declared of description.components) {
      const record = retained.get(declared.identity);
      const resolved = this.resolveAssets(declared);
      const fields = new Map<number, DeclarationFieldValue>();
      for (const [offset, value] of resolved) {
        if (value.kind === "asset" || value.kind === "row-asset")
          throw new Error("Unresolved asset reference");
        if (value.kind !== "entity-reference") {
          fields.set(offset, value);
          continue;
        }
        if (typeof value.value === "string")
          throw new Error("Unresolved entity reference");
        fields.set(offset, {
          kind: "entity",
          value:
            typeof value.value === "bigint"
              ? { kind: "handle", id: value.value }
              : entityRef(value.value.entity),
        });
      }
      const properties = declared.properties ?? {};
      if (record) {
        record.description = declared;
        // An unchanged field map needs no comparison unless it names
        // entities, whose handles may have changed.
        if (
          resolved === record.declared &&
          declared.properties === undefined &&
          ![...resolved.values()].some(
            (value) => value.kind === "entity-reference",
          )
        )
          continue;
        // A removed prop leaves its last value in place; declaring it again
        // writes it again.
        for (const offset of record.fields.keys())
          if (!fields.has(offset)) record.fields.delete(offset);
        const commands: PlannedCommand[] = [];
        for (const [offset, value] of fields) {
          if (equalField(record.fields.get(offset), value)) continue;
          commands.push({
            command: {
              kind: "setField",
              entity: entityRef(declared.entity),
              component: declared.component,
              field: { offset, value },
            },
            applied: (batch) =>
              record.fields.set(offset, acknowledgedValue(value, batch)),
          });
        }
        for (const [name, value] of Object.entries(properties)) {
          if (
            Object.hasOwn(record.properties, name) &&
            JSON.stringify(record.properties[name]) === JSON.stringify(value)
          )
            continue;
          commands.push({
            command: {
              kind: "setDynamicProperty",
              entity: entityRef(declared.entity),
              component: declared.component,
              name,
              value,
            },
            applied: () => {
              record.properties[name] = value;
            },
          });
        }
        const last = commands.at(-1);
        if (!last) record.declared = resolved;
        else
          commands[commands.length - 1] = {
            command: last.command,
            applied: (batch, operation) => {
              last.applied!(batch, operation);
              record.declared = resolved;
            },
          };
        plan.push(...commands);
        continue;
      }
      let inserted: ComponentRecord | undefined;
      plan.push({
        command: {
          kind: "insertComponent",
          entity: entityRef(declared.entity),
          component: declared.component,
          fields: writes(fields),
          adopt: true,
        },
        applied: (batch) => {
          const entity = this.entities.get(declared.entity);
          if (!entity) throw new Error("Missing declaration entity");
          inserted = {
            description: declared,
            entity,
            serial: this.nextSerial++,
            fields: new Map(
              [...fields].map(([offset, value]) => [
                offset,
                acknowledgedValue(value, batch),
              ]),
            ),
            properties: {},
            declared: Object.keys(properties).length ? undefined : resolved,
          };
          this.components.set(declared.identity, inserted);
          this.reacknowledged.add(declared.identity);
        },
      });
      const names = Object.keys(properties);
      names.forEach((name, index) =>
        plan.push({
          command: {
            kind: "setDynamicProperty",
            entity: entityRef(declared.entity),
            component: declared.component,
            name,
            value: properties[name]!,
          },
          applied: () => {
            if (!inserted) return;
            inserted.properties[name] = properties[name]!;
            if (index === names.length - 1) inserted.declared = resolved;
          },
        }),
      );
    }

    // Entities whose declarations disappeared are deleted last, after the
    // commands that may still refer to them.
    const deletions: PlannedCommand[] = [];
    for (const record of this.orphans)
      if (record.description.kind === "declared")
        deletions.push({
          command: { kind: "delete", entity: recordRef(record) },
          applied: () => this.forgetEntity(record),
        });

    // Once every placement applied, retained links that kept their labels
    // adopt their declared anchors and each sibling chain becomes the order
    // later commits compare against.
    const placed = (batch: AppliedBatch | undefined) => {
      for (const { record, link } of unplaced)
        if (this.links.get(link.identity) === record)
          record.description = acknowledgedLink(link, batch);
      const orders = new Map<bigint | null, ReadonlyMap<number, number>>();
      for (const [parent, order] of chains) {
        const key = acknowledgedKey(parent, batch);
        if (!order || key === undefined) continue;
        orders.set(
          key,
          new Map(order.map((identity, index) => [identity, index])),
        );
      }
      this.linkOrders = orders;
    };
    // Deleting an entity invalidates animations that target it, so deletions
    // wait for a separate batch while animations may still retarget away
    // from those entities to newly acknowledged ones.
    const separateDeletions =
      deletions.length > 0 && description.animations.length > 0;
    const batch = separateDeletions ? plan : [...plan, ...deletions];
    let retry = false;
    if (batch.length)
      retry = await this.commit(batch, placed, (applied) => {
        for (const command of applied)
          if (command.kind === "placeEntity")
            for (const parent of placedGroups) this.forgetOrder(parent);
      });
    else placed(undefined);
    if (retry) return this.applyAttempt(description);
    await this.resolveBoundEntities();
    await this.animations.apply(
      description.animations,
      this.assets,
      (identity) => this.entities.get(identity)?.entity,
    );
    if (separateDeletions && (await this.commit(deletions)))
      return this.applyAttempt(description);
    await this.assets.releaseUnused();
    await this.publishControls();
  }

  /** Forget a sibling order whose placement a rejected batch interrupted. */
  private forgetOrder(parent: bigint | null): void {
    if (parent === null || parent >= 0n) this.linkOrders.delete(parent);
  }

  /**
   * Find the handles of bound entities that no command has resolved yet,
   * from the World's entity records, except those whose symbolic id is in
   * `pending`. Without an inspection client their handles stay unknown until
   * a command refers to them.
   */
  private async resolveBoundEntities(
    pending: ReadonlySet<string> = new Set(),
  ): Promise<void> {
    const unresolved = new Map<string, EntityRecord[]>();
    for (const record of this.entities.values())
      if (
        record.entity === undefined &&
        !pending.has(record.description.symbolicId)
      ) {
        const symbol = record.description.symbolicId;
        unresolved.set(symbol, [...(unresolved.get(symbol) ?? []), record]);
      }
    if (!unresolved.size || !this.client.inspectPage) return;
    let after = 0n;
    do {
      this.checkSession();
      const page = await this.client.inspectPage({
        collection: "entities",
        after,
      });
      for (const entity of page.entities) {
        const symbol = entity.metadata.symbolicId;
        if (symbol === null) continue;
        for (const record of unresolved.get(symbol) ?? [])
          record.entity = entity.id;
        unresolved.delete(symbol);
      }
      after = page.next;
    } while (after !== 0n && unresolved.size);
    this.publishEntities();
    if (unresolved.size)
      throw new Error(
        `Bound entities are missing: ${[...unresolved.keys()].join(", ")}`,
      );
  }

  private async publishControls(): Promise<void> {
    const reacknowledged = [...this.reacknowledged];
    this.reacknowledged.clear();
    await this.controls.publish((identity) => {
      const record = this.components.get(identity);
      const entity = record?.entity;
      const handle = entity?.entity;
      if (!record || !entity || handle === undefined) return;
      return {
        entity: handle,
        component: record.description.component,
        valid: () =>
          this.components.get(identity) === record &&
          entity.entity === handle &&
          !this.orphans.has(entity),
      };
    }, reacknowledged);
    await this.callbacks.prepare();
  }

  /**
   * Delete what the records hold: the entities of declared records, the
   * components declared on bound entities (unless `boundComponents` is
   * false) and the entities an unknown-extent batch may have created.
   * Removals that find their target already gone count as done.
   */
  private async deleteRecords(boundComponents = true): Promise<void> {
    const operations: Command[] = [];
    const bound = (record: EntityRecord) => record.description.kind === "bound";
    for (const component of this.components.values())
      if (boundComponents && bound(component.entity))
        operations.push({
          kind: "removeComponent",
          entity: {
            kind: "symbol",
            symbol: component.entity.description.symbolicId,
          },
          component: component.description.component,
        });
    const deleted = new Set<bigint>();
    const symbols = new Set<string>();
    for (const record of [...this.entities.values(), ...this.orphans])
      if (
        !bound(record) &&
        record.entity !== undefined &&
        !deleted.has(record.entity)
      ) {
        deleted.add(record.entity);
        symbols.add(record.description.symbolicId);
        operations.push({
          kind: "delete",
          entity: { kind: "handle", id: record.entity },
        });
      }
    for (const symbol of this.uncertain)
      if (!symbols.has(symbol))
        operations.push({ kind: "delete", entity: { kind: "symbol", symbol } });
    if (operations.length) await this.submitCleanup(operations);
    this.uncertain.clear();
    this.forgetRecords();
  }

  /** Cleanup must remain possible after an unrelated fatal commit error. The
   * original session fence still applies; a removal whose target is already
   * gone is skipped and the rest resubmitted. */
  private async submitCleanup(operations: Command[]): Promise<void> {
    while (operations.length) {
      if (this.client.session !== this.session)
        throw new Error("Session replacement prevented React root cleanup");
      const outcome = await this.client.batch(operations);
      if (outcome.ok) return;
      const count = appliedCommands(outcome, operations.length);
      if (count === undefined || !removedAlready(operations[count]!, outcome))
        throw new ReactWorldBatchRejectedError(outcome);
      operations = operations.slice(count + 1);
    }
  }

  /**
   * Submit planned commands and acknowledge exactly the commands the outcome
   * reports applied, so a corrected render continues from actual state. A
   * fully applied batch also runs `completed`; a partially applied one tells
   * `interrupted` which commands applied. Returns true when the batch stopped
   * at a removal whose target was already gone: that removal counts as done
   * and the caller plans again.
   */
  private async commit(
    plan: readonly PlannedCommand[],
    completed?: (batch: AppliedBatch) => void,
    interrupted?: (applied: readonly Command[]) => void,
  ): Promise<boolean> {
    try {
      const commands = plan.map((planned) => planned.command);
      const outcome = await this.send(commands);
      const batch = new AppliedBatch(outcome);
      this.learnSymbols(batch);
      const count = appliedCommands(outcome, commands.length);
      const rejected = outcome.ok
        ? undefined
        : new ReactWorldBatchRejectedError(outcome);
      if (count === undefined) {
        this.resetUncertain(commands);
        throw rejected;
      }
      try {
        for (let index = 0; index < count; index++)
          plan[index]!.applied?.(batch, index);
      } catch (error) {
        if (rejected) {
          this.resetUncertain(commands);
          throw rejected;
        }
        this.fatal = error instanceof Error ? error : new Error(String(error));
        throw this.fatal;
      }
      if (count === commands.length) completed?.(batch);
      else interrupted?.(commands.slice(0, count));
      if (rejected && removedAlready(commands[count]!, outcome)) {
        plan[count]!.applied?.(batch, count);
        return true;
      }
      if (rejected) throw rejected;
      return false;
    } finally {
      this.pruneOrphans();
      this.publishEntities();
    }
  }

  /**
   * A rejected batch whose applied extent is unknown: the next commit deletes
   * what the records hold and every entity the batch may have created, by
   * symbol, then commits again.
   */
  private resetUncertain(commands: readonly Command[]): void {
    for (const command of commands)
      if (command.kind === "create" && command.metadata.symbolicId !== null)
        this.uncertain.add(command.metadata.symbolicId);
    this.needsReset = true;
    this.controls.reset();
  }

  /** Record the handles an outcome reports for bound entities' symbols. */
  private learnSymbols(batch: AppliedBatch): void {
    for (const record of [...this.entities.values(), ...this.orphans]) {
      if (record.description.kind !== "bound") continue;
      const entity = batch.symbols.get(record.description.symbolicId);
      if (entity === undefined || entity === record.entity) continue;
      // Controls acknowledged on the previous handle are rechecked.
      if (record.entity !== undefined)
        for (const [identity, component] of this.components)
          if (component.entity === record) this.reacknowledged.add(identity);
      record.entity = entity;
    }
  }
}
