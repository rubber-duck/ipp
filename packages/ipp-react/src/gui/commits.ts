/** Acknowledgement-aware GUI commits.
 *
 * Tracks desired, submitted and acknowledged state separately per GuiRoot:
 * React keys (retained instance identities) map to monotonically allocated
 * node IDs which are never reused. Ordinary rerenders never emit control
 * value writes and never touch nodes owned by another writer.
 *
 * Root ownership is producer-side. The reconciler describes no overlay for
 * GuiRoot (core rejects overlay declarations that would create one); this
 * class creates the producer component with insertComponent when inspection
 * reports no live root, drives the node tree only through incremental GUI
 * edits, and removes a reconciler-created producer with removeComponent on
 * release. Adopted pre-existing empty producers are used but never removed.
 * Handles stay fenced by session, entity, root incarnation and node
 * lifetime; a replaced incarnation re-initializes instead of retargeting
 * old handles.
 *
 * All transport happens here, in the commit phase. Description, validation
 * and diffing stay pure.
 */
import type {
  BatchOutcome,
  Command,
  DynamicValue,
  GuiEdit,
  GuiEditBatchOutcome,
  GuiInspectedNode,
  GuiInspectResponse,
  GuiNodeHandle,
} from "@ipp/client";
import type { ReactWorldClient } from "../contract.js";
import type { GuiDeclarationStyle, GuiNodeRef } from "./components.js";
import type { GuiDeclarationPatchStyle } from "./diff.js";
import {
  controlKindForContent,
  GuiEffectSubscriptions,
  isCommittedEffect,
  nameForContent,
  type GuiCommittedEffect,
  type GuiObservationBatch,
  type GuiObservationSink,
  type GuiObservationSummary,
} from "./callbacks.js";
import {
  equalGuiContent,
  normalizeGuiStyle,
  type GuiDescribedNode,
  type GuiDescribedRoot,
} from "./description.js";
import { diffGuiTree, type GuiAcknowledgedNode } from "./diff.js";
import { retainedNodeCallbacks } from "../tree.js";
import { guiThemeProperties } from "./theme.js";

export interface GuiCommitOptions {
  checkSession(): void;
  report(error: unknown): Error;
  /** Reporting sink for conflicts, cancellations and scene-bound unhandled
   * inputs. The viewer wires `onUnhandled` to scene fallback. */
  observationSink?: GuiObservationSink | undefined;
}

/** Client observation source, guarded at runtime: the reconciler contract
 * does not require it, so older or narrower clients simply never feed. */
interface GuiObservationSource {
  subscribeGuiObservations(
    listener: (batch: GuiObservationBatch) => void,
  ): () => void;
}

interface GuiRootState {
  entity: bigint;
  incarnation: bigint;
  session: bigint;
  ids: Map<number, number>;
  acked: Map<number, GuiAcknowledgedNode>;
  order: Map<number | undefined, readonly number[]>;
  nextId: number;
  boundRefs: Map<number, GuiNodeRef>;
  /** True when this reconciler created the producer root and must remove it
   * on release. Adopted pre-existing producers are used but never removed. */
  producerOwned: boolean;
  /** Acknowledged ordinary GuiRoot named-part properties. */
  themeProperties: Map<string, DynamicValue>;
  /** Retained teardown progress. A successful node removal must not be
   * replayed merely because the later producer removal was rejected. */
  cleanup?:
    | {
        nodesReleased: boolean;
        producerReleased: boolean;
      }
    | undefined;
}

function isRejected(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code: unknown }).code === "IPP_REQUEST_REJECTED"
  );
}

function hasGui(client: ReactWorldClient): client is ReactWorldClient & {
  editGuiBatch: NonNullable<ReactWorldClient["editGuiBatch"]>;
  inspectGui: NonNullable<ReactWorldClient["inspectGui"]>;
} {
  return (
    typeof client.editGuiBatch === "function" &&
    typeof client.inspectGui === "function"
  );
}

function setRef(target: GuiNodeRef, handle: GuiNodeHandle | null): void {
  if (typeof target === "function") target(handle);
  else target.current = handle;
}

export class GuiCommits {
  private readonly states = new Map<number, GuiRootState>();
  private readonly pendingCleanup = new Set<GuiRootState>();
  private localRoots = new Map<number, GuiDescribedRoot>();
  private highWater = new Map<string, number>();
  /** Lifetime-fenced control records fed by committed client observations. */
  private readonly subscriptions = new GuiEffectSubscriptions();
  private observationDetach: (() => void) | null = null;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly options: GuiCommitOptions,
  ) {}

  private handleFor(
    state: GuiRootState,
    nodeId: number,
    lifetime: number,
  ): GuiNodeHandle {
    if (typeof this.client.createGuiNodeHandle === "function")
      return this.client.createGuiNodeHandle(
        state.entity,
        state.incarnation,
        nodeId,
        lifetime,
      );
    return Object.freeze({
      session: this.client.session,
      entity: state.entity,
      rootIncarnation: state.incarnation,
      nodeId,
      nodeLifetime: lifetime,
    });
  }

  private nullRefs(state: GuiRootState): void {
    for (const target of state.boundRefs.values()) {
      try {
        setRef(target, null);
      } catch {
        // Ref cleanup must not fail unmount.
      }
    }
    state.boundRefs.clear();
  }

  private waterKey(entity: bigint, incarnation: bigint): string {
    return `${entity}:${incarnation}`;
  }

  /** Release every acknowledged root before forgetting its handles. Failed
   * phases remain pending and a later reset, apply or dispose retries them. */
  async reset(): Promise<void> {
    this.localRoots.clear();
    for (const [identity, state] of [...this.states])
      this.stageCleanup(identity, state);
    await this.retryPendingCleanup();
    this.subscriptions.clear();
    this.detachObservationBridge();
  }

  /** Replace the scene-fallback/reporting sink for subsequent batches. */
  setObservationSink(sink: GuiObservationSink | undefined): void {
    this.options.observationSink = sink;
  }

  /** Retain the acknowledged record for one node from its JS-only listeners.
   * Controls subscribe under their kind; containers subscribe for the
   * `onAction` path only and never consume control effects. */
  private observeNode(
    state: GuiRootState,
    root: GuiDescribedRoot,
    identity: number,
  ): void {
    root = this.localRoots.get(root.identity) ?? root;
    const ack = state.acked.get(identity);
    const node = root.nodes.find((entry) => entry.identity === identity);
    if (!ack || !node) return;
    const callbacks = retainedNodeCallbacks(node);
    this.subscriptions.subscribe(state.entity, state.incarnation, ack.nodeId, {
      lifetime: ack.lifetime,
      kind: controlKindForContent(node.content) ?? "container",
      name: nameForContent(node.content),
      onPress: callbacks.onPress,
      onToggle: callbacks.onToggle,
      onScalarCommit: callbacks.onScalarCommit,
      onTextCommit: callbacks.onTextCommit,
      onAction: callbacks.onAction,
      onActionCapture: callbacks.onActionCapture,
    });
  }

  /** Acknowledged ancestry per live node identity for action dispatch. */
  private parentOf(
    entity: bigint,
    rootIncarnation: bigint,
  ): Map<number, number | undefined> {
    const parentOf = new Map<number, number | undefined>();
    for (const state of this.states.values()) {
      if (state.entity !== entity || state.incarnation !== rootIncarnation) {
        continue;
      }
      const live = new Map<number, number>();
      for (const [identity, ack] of state.acked) live.set(identity, ack.nodeId);
      for (const ack of state.acked.values())
        parentOf.set(
          ack.nodeId,
          ack.parent === undefined ? undefined : live.get(ack.parent),
        );
    }
    return parentOf;
  }

  /** Dispatch one client observation batch through retained subscriptions.
   * Unknown effects fail closed with a report; conflicts, cancellations and
   * unhandled inputs reach the construction sink (or the per-call override),
   * whose `onUnhandled` is the scene fallback. */
  feedObservations(
    batch: GuiObservationBatch,
    sink: GuiObservationSink = {},
  ): GuiObservationSummary {
    const onError = sink.onError ?? this.options.observationSink?.onError;
    const effects: GuiCommittedEffect[] = [];
    for (const effect of batch.effects) {
      if (isCommittedEffect(effect)) effects.push(effect);
      else
        onError?.(
          new Error(
            `Unknown GUI effect for node ${(effect as { node?: unknown }).node}`,
          ),
        );
    }
    return this.subscriptions.feedObservations(
      { ...batch, effects },
      (entity, rootIncarnation) => this.parentOf(entity, rootIncarnation),
      {
        ...this.options.observationSink,
        ...sink,
        ...(onError === undefined ? {} : { onError }),
      },
    );
  }

  private observationSource(): GuiObservationSource | null {
    const candidate = this.client as unknown as Partial<GuiObservationSource>;
    return typeof candidate.subscribeGuiObservations === "function"
      ? (candidate as GuiObservationSource)
      : null;
  }

  /** Feed committed client observations into retained subscriptions once any
   * node is acknowledged. Older or narrower clients simply never feed. */
  private ensureObservationBridge(): void {
    if (this.observationDetach) return;
    const source = this.observationSource();
    if (!source) return;
    let live = false;
    for (const state of this.states.values())
      if (state.acked.size > 0) {
        live = true;
        break;
      }
    if (!live) return;
    this.observationDetach = source.subscribeGuiObservations((batch) => {
      try {
        this.feedObservations(batch);
      } catch (error) {
        this.options.report(error);
      }
    });
  }

  private detachObservationBridge(): void {
    this.observationDetach?.();
    this.observationDetach = null;
  }

  async apply(
    roots: readonly GuiDescribedRoot[],
    resolveEntity: (identity: number) => bigint | undefined,
  ): Promise<void> {
    this.reconcileLocal(roots);
    const desiredEntities = new Map(
      roots.map((root) => [root.identity, resolveEntity(root.entity)]),
    );
    for (const [identity, state] of [...this.states]) {
      const desired = desiredEntities.get(identity);
      if (desired === undefined || desired !== state.entity)
        this.stageCleanup(identity, state);
    }
    await this.retryPendingCleanup();
    if (roots.length === 0) return;
    if (!hasGui(this.client))
      throw new Error("GUI declarations require a GUI-capable client");
    for (const root of roots) {
      const entity = desiredEntities.get(root.identity);
      const known = this.states.get(root.identity);
      if (entity === undefined) continue;
      this.options.checkSession();
      let state = known;
      if (state && state.session !== this.client.session) {
        throw new Error("Session replacement requires a new React root");
      }
      state ??= await this.initialize(root.identity, entity);
      await this.flush(root, state);
    }
    this.ensureObservationBridge();
  }

  /** Whether this committer still owns or has adopted a root on the entity. */
  hasAcknowledgedRoot(entity: bigint): boolean {
    return [...this.states.values()].some((state) => state.entity === entity);
  }

  /** Refresh callbacks and refs for every committed React snapshot without
   * coupling those local bindings to transport equality. */
  reconcileLocal(roots: readonly GuiDescribedRoot[]): void {
    this.localRoots = new Map(roots.map((root) => [root.identity, root]));
    for (const [identity, state] of this.states) {
      const root = this.localRoots.get(identity);
      if (!root) continue;
      for (const nodeIdentity of state.acked.keys()) {
        this.observeNode(state, root, nodeIdentity);
      }
      this.syncRefs(root, state);
    }
  }

  private async initialize(
    identity: number,
    entity: bigint,
  ): Promise<GuiRootState> {
    const client = this.client;
    if (!hasGui(client))
      throw new Error("GUI declarations require a GUI-capable client");
    try {
      const inspection = await client.inspectGui({ entity });
      return this.adopt(identity, entity, inspection, false);
    } catch (error) {
      // A rejection means no producer root is live for this entity: create
      // it through the supported producer lifecycle, never through overlays.
      // Any other failure is ambiguous transport; rethrow it without guessing
      // ownership.
      if (!isRejected(error)) throw error;
      const descriptor = client.components["GuiRoot"];
      if (!descriptor)
        throw this.options.report(
          new Error("GUI declarations require a GUI-capable client"),
        );
      this.options.checkSession();
      await this.submitProducer([
        {
          kind: "insertComponent",
          entity: { kind: "handle", id: entity },
          component: descriptor.id,
          fields: [],
        },
      ]);
      const inspection = await client.inspectGui({ entity });
      try {
        return this.adopt(identity, entity, inspection, true);
      } catch (adoptError) {
        // A foreign writer raced our creation: remove the producer we just
        // created so the refused mount leaves no orphan behind.
        const refused: GuiRootState = {
          entity,
          incarnation: inspection.rootIncarnation,
          session: client.session,
          ids: new Map(),
          acked: new Map(),
          order: new Map(),
          nextId: 1,
          boundRefs: new Map(),
          producerOwned: true,
          themeProperties: new Map(),
        };
        this.stageCleanup(undefined, refused);
        await this.retryPendingCleanup();
        throw adoptError;
      }
    }
  }

  /** Adopt an inspected producer root: refuse live foreign trees, resume the
   * allocation high-water mark for the incarnation, and record whether this
   * reconciler created the producer (and must remove it on release). */
  private adopt(
    identity: number,
    entity: bigint,
    inspection: GuiInspectResponse,
    producerOwned: boolean,
  ): GuiRootState {
    if (inspection.nodes.length > 0)
      throw this.options.report(
        new Error(
          "GuiRoot already has live nodes from another writer; refusing to adopt them",
        ),
      );
    const restored = this.highWater.get(
      this.waterKey(entity, inspection.rootIncarnation),
    );
    const state: GuiRootState = {
      entity,
      incarnation: inspection.rootIncarnation,
      session: this.client.session,
      ids: new Map(),
      acked: new Map(),
      order: new Map(),
      nextId: restored ?? 1,
      boundRefs: new Map(),
      producerOwned,
      themeProperties: new Map(),
    };
    this.states.set(identity, state);
    return state;
  }

  /** Submit producer-lifecycle commands for root create/remove. Rejections
   * carry the core scope/operation/reason; ambiguous transport failures
   * propagate without guessing ownership. */
  private async submitProducer(commands: Command[]): Promise<void> {
    this.options.checkSession();
    let outcome: BatchOutcome;
    try {
      outcome = await this.client.batch(commands);
    } catch (error) {
      throw error instanceof Error ? error : new Error(String(error));
    }
    if (!outcome.ok) {
      throw this.options.report(
        new Error(
          `GUI root producer batch rejected at ${outcome.error.scope}${outcome.error.operation === null ? "" : ` ${outcome.error.operation}`}: ${outcome.error.reason}`,
        ),
      );
    }
  }

  /** Move one active root into retained cleanup without losing any server
   * identity. Local callbacks/refs stop immediately; transport ownership is
   * released only after acknowledgement. */
  private stageCleanup(
    identity: number | undefined,
    state: GuiRootState,
  ): void {
    if (identity !== undefined && this.states.get(identity) === state)
      this.states.delete(identity);
    this.nullRefs(state);
    this.highWater.set(
      this.waterKey(state.entity, state.incarnation),
      state.nextId,
    );
    for (const nodeId of state.ids.values())
      this.subscriptions.unsubscribe(state.entity, state.incarnation, nodeId);
    state.cleanup ??= {
      nodesReleased: state.acked.size === 0,
      producerReleased: !state.producerOwned,
    };
    this.pendingCleanup.add(state);
  }

  /** Retry every retained teardown in order. A failed phase throws without
   * clearing the state, so a later apply/reset/dispose resumes exactly there. */
  private async retryPendingCleanup(): Promise<void> {
    for (const state of [...this.pendingCleanup]) await this.releaseRoot(state);
  }

  /** Remove acknowledged nodes before their producer. Each completed phase
   * is recorded independently so rejected later work cannot resurrect or
   * duplicate an earlier command. */
  private async releaseRoot(state: GuiRootState): Promise<void> {
    const cleanup = state.cleanup;
    if (!cleanup) throw new Error("GUI cleanup was not staged");
    const client = this.client;
    if (client.session !== state.session)
      throw new Error("Session replacement prevented GUI root cleanup");
    if (!hasGui(client))
      throw new Error("GUI root cleanup requires a GUI-capable client");
    if (!cleanup.nodesReleased) {
      const root = [...state.acked].find(([, ack]) => ack.parent === undefined);
      if (root) {
        const outcome = await client.editGuiBatch([
          {
            action: "remove",
            handle: this.handleFor(state, root[1].nodeId, root[1].lifetime),
          },
        ]);
        if (!outcome.ok) {
          const inspection = await client
            .inspectGui({ entity: state.entity })
            .catch((error: unknown) => {
              if (isRejected(error)) return null;
              throw error;
            });
          if (
            inspection === null ||
            inspection.rootIncarnation !== state.incarnation
          ) {
            cleanup.nodesReleased = true;
            cleanup.producerReleased = true;
            this.pendingCleanup.delete(state);
            return;
          }
          if (inspection.nodes.length !== 0) this.requireBatchSuccess(outcome);
        }
      }
      cleanup.nodesReleased = true;
    }
    if (!cleanup.producerReleased) {
      const descriptor = client.components["GuiRoot"];
      if (!descriptor)
        throw new Error("GUI root cleanup requires the GuiRoot descriptor");
      const outcome = await client.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: state.entity },
          component: descriptor.id,
        },
      ]);
      if (!outcome.ok) {
        const inspection = await client
          .inspectGui({ entity: state.entity })
          .catch((error: unknown) => {
            if (isRejected(error)) return null;
            throw error;
          });
        if (inspection !== null)
          throw new Error(
            `GUI root producer cleanup rejected at ${outcome.error.scope}${outcome.error.operation === null ? "" : ` ${outcome.error.operation}`}: ${outcome.error.reason}`,
          );
      }
      cleanup.producerReleased = true;
    }
    this.pendingCleanup.delete(state);
  }

  private async flush(
    root: GuiDescribedRoot,
    state: GuiRootState,
  ): Promise<void> {
    const client = this.client;
    if (!hasGui(client))
      throw new Error("GUI declarations require a GUI-capable client");
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const makeHandle = (nodeId: number, lifetime: number): GuiNodeHandle =>
        this.handleFor(state, nodeId, lifetime);
      const plan = diffGuiTree(
        root.nodes,
        state.acked,
        state.ids,
        state.nextId,
        state.order,
        makeHandle,
      );
      if (plan.edits.length === 0) {
        await this.flushTheme(root, state);
        this.syncRefs(root, state);
        return;
      }
      // Adopt allocations before submission so records resolve by node ID.
      // A deterministically rejected flush rolls unacknowledged allocations
      // back; an ambiguously failed flush retains them until inspection
      // resolves each one.
      state.ids = new Map(plan.ids);
      const edits = plan.edits.map((edit) => this.withScope(state, edit));
      let outcome: GuiEditBatchOutcome;
      try {
        outcome = await client.editGuiBatch(edits);
      } catch (error) {
        if (isRejected(error)) {
          // A deterministic rejection landed nothing new: roll back
          // unacknowledged allocations so the next attempt reuses the exact
          // expected identities instead of skipping ahead. The runtime
          // consumes IDs only for successful inserts in order. The
          // allocation high-water mark never retreats, so retired IDs are
          // never silently reused for new content.
          state.ids = new Map(
            [...state.acked].map(([identity, ack]) => [identity, ack.nodeId]),
          );
          state.nextId = this.nextAfter(state);
          throw error instanceof Error ? error : new Error(String(error));
        }
        // Ambiguous transport failure: retain submitted identities until
        // inspection resolves each one, adopting landed inserts and freeing
        // only proven-missing ones. Refusing foreign work keeps them, and
        // recovery advances the high-water mark past adopted content.
        await this.resync(root, state);
        if (attempt === 1)
          throw error instanceof Error ? error : new Error(String(error));
        continue;
      }
      for (const edit of edits.slice(0, outcome.applied))
        this.record(state, root, edit);
      if (!outcome.ok) {
        await this.resync(root, state);
        throw this.options.report(
          new Error(
            `GUI edit batch rejected after ${outcome.applied} edits: ${outcome.error.reason}`,
          ),
        );
      }
      state.nextId = plan.nextId;
      this.highWater.set(
        this.waterKey(state.entity, state.incarnation),
        state.nextId,
      );
      this.rebuildOrder(root, state);
      await this.flushTheme(root, state);
      this.syncRefs(root, state);
      return;
    }
  }

  private nextAfter(state: GuiRootState): number {
    // The allocation high-water mark is independent of live nodes: removals
    // must never let recovery reuse a retired ID for new content, and
    // submitted-but-unacknowledged IDs stay reserved until inspection frees
    // them. Recovery advances past adopted content; it never retreats.
    let next = Math.max(
      state.nextId,
      this.highWater.get(this.waterKey(state.entity, state.incarnation)) ?? 1,
    );
    for (const ack of state.acked.values())
      if (ack.nodeId >= next) next = ack.nodeId + 1;
    for (const id of state.ids.values()) if (id >= next) next = id + 1;
    return next;
  }

  /** Author theme declarations through the ordinary GuiRoot dynamic-property
   * path after structural acknowledgement. Core, not React, selects the
   * active state/variant and owns transition playback. */
  private async flushTheme(
    root: GuiDescribedRoot,
    state: GuiRootState,
  ): Promise<void> {
    const descriptor = this.client.components["GuiRoot"];
    if (!descriptor)
      throw new Error("GUI themes require the GuiRoot component descriptor");
    const desired = new Map<string, DynamicValue>();
    const liveNodeIds = new Set<number>();
    for (const node of root.nodes) {
      const ack = state.acked.get(node.identity);
      if (!ack) continue;
      liveNodeIds.add(ack.nodeId);
      for (const [name, value] of Object.entries(
        guiThemeProperties(ack.nodeId, node.theme),
      ))
        desired.set(name, value);
    }
    const belongsToLiveNode = (name: string): boolean => {
      const match = /^node_([1-9][0-9]*)_part_/.exec(name);
      return match !== null && liveNodeIds.has(Number(match[1]));
    };
    const previous = new Map(
      [...state.themeProperties].filter(([name]) => belongsToLiveNode(name)),
    );
    const commands: Command[] = [];
    for (const [name, value] of desired) {
      if (JSON.stringify(previous.get(name)) === JSON.stringify(value))
        continue;
      commands.push({
        kind: "setDynamicProperty",
        entity: { kind: "handle", id: state.entity },
        component: descriptor.id,
        name,
        value,
      });
    }
    for (const name of previous.keys())
      if (!desired.has(name))
        commands.push({
          kind: "removeDynamicProperty",
          entity: { kind: "handle", id: state.entity },
          component: descriptor.id,
          name,
        });
    if (commands.length > 0) await this.submitProducer(commands);
    state.themeProperties = desired;
  }

  private withScope(state: GuiRootState, edit: GuiEdit): GuiEdit {
    return edit.action === "insert"
      ? {
          ...edit,
          entity: state.entity,
          rootIncarnation: state.incarnation,
        }
      : edit;
  }

  private requireBatchSuccess(outcome: GuiEditBatchOutcome): void {
    if (!outcome.ok)
      throw this.options.report(
        new Error(
          `GUI edit batch rejected after ${outcome.applied} edits: ${outcome.error.reason}`,
        ),
      );
  }

  private record(
    state: GuiRootState,
    root: GuiDescribedRoot,
    edit: GuiEdit,
  ): void {
    const declared = (identity: number): GuiDescribedNode => {
      const node = root.nodes.find((entry) => entry.identity === identity);
      if (!node) throw new Error("Missing GUI declaration");
      return node;
    };
    const identityOf = (nodeId: number): number => {
      for (const [identity, id] of state.ids)
        if (id === nodeId) return identity;
      throw new Error("Missing GUI node identity");
    };
    switch (edit.action) {
      case "insert": {
        const node = declared(identityOf(edit.id));
        state.ids.set(node.identity, edit.id);
        state.acked.set(node.identity, {
          nodeId: edit.id,
          parent: node.parent,
          parentId:
            node.parent === undefined ? undefined : state.ids.get(node.parent),
          lifetime: 1,
          content: node.content,
          style: node.style,
        });
        this.observeNode(state, root, node.identity);
        break;
      }
      case "update": {
        const identity = identityOf(edit.handle.nodeId);
        const ack = state.acked.get(identity);
        if (!ack) throw new Error("Missing acknowledged GUI node");
        const style: GuiDeclarationStyle = { ...ack.style };
        const patch = edit.patch.style;
        if (patch) {
          if (patch.width !== undefined)
            if (patch.width === null) delete style.width;
            else style.width = patch.width;
          if (patch.height !== undefined)
            if (patch.height === null) delete style.height;
            else style.height = patch.height;
          if (patch.minWidth !== undefined)
            if (patch.minWidth === null) delete style.minWidth;
            else style.minWidth = patch.minWidth;
          if (patch.minHeight !== undefined)
            if (patch.minHeight === null) delete style.minHeight;
            else style.minHeight = patch.minHeight;
          if (patch.maxWidth !== undefined)
            if (patch.maxWidth === null) delete style.maxWidth;
            else style.maxWidth = patch.maxWidth;
          if (patch.maxHeight !== undefined)
            if (patch.maxHeight === null) delete style.maxHeight;
            else style.maxHeight = patch.maxHeight;
          if (patch.padding !== undefined)
            if (patch.padding === null) delete style.padding;
            else style.padding = [...patch.padding];
          if (patch.margin !== undefined)
            if (patch.margin === null) delete style.margin;
            else style.margin = [...patch.margin];
          if (patch.flex !== undefined)
            if (patch.flex === null) delete style.flex;
            else style.flex = patch.flex;
          if (patch.alignX !== undefined)
            if (patch.alignX === null) delete style.alignX;
            else style.alignX = patch.alignX;
          if (patch.alignY !== undefined)
            if (patch.alignY === null) delete style.alignY;
            else style.alignY = patch.alignY;
          if (patch.backgroundColor !== undefined)
            if (patch.backgroundColor === null) delete style.backgroundColor;
            else style.backgroundColor = [...patch.backgroundColor];
          if (patch.color !== undefined) style.color = [...patch.color];
          if (patch.opacity !== undefined) style.opacity = patch.opacity;
          if (patch.fontSize !== undefined) style.fontSize = patch.fontSize;
          if (patch.asset !== undefined) {
            if (patch.asset === null) delete style.asset;
            else style.asset = patch.asset;
          }
          const declaredEnabled = (patch as GuiDeclarationPatchStyle).enabled;
          if (declaredEnabled !== undefined) style.enabled = declaredEnabled;
        }
        state.acked.set(identity, {
          ...ack,
          content: edit.patch.content ?? ack.content,
          style,
        });
        this.observeNode(state, root, identity);
        break;
      }
      case "move": {
        const identity = identityOf(edit.handle.nodeId);
        const ack = state.acked.get(identity);
        if (!ack) throw new Error("Missing acknowledged GUI node");
        state.acked.set(identity, {
          ...ack,
          parent: declared(identity).parent,
          parentId: edit.parent,
        });
        break;
      }
      case "remove": {
        const identity = identityOf(edit.handle.nodeId);
        const doomed = [identity];
        for (let index = 0; index < doomed.length; index += 1) {
          const current = doomed[index]!;
          for (const [candidate, ack] of state.acked)
            if (ack.parent === current) doomed.push(candidate);
        }
        for (const gone of doomed) {
          const ack = state.acked.get(gone);
          if (ack)
            this.subscriptions.unsubscribe(
              state.entity,
              state.incarnation,
              ack.nodeId,
            );
          state.acked.delete(gone);
          state.ids.delete(gone);
        }
        break;
      }
      case "setControlValue":
        // Structural authoring never emits revision-gated value writes.
        throw new Error("Unexpected GUI control write");
    }
  }

  private rebuildOrder(root: GuiDescribedRoot, state: GuiRootState): void {
    const order = new Map<number | undefined, number[]>();
    for (const node of root.nodes) {
      if (!state.acked.has(node.identity)) continue;
      const list = order.get(node.parent);
      if (list) list.push(node.identity);
      else order.set(node.parent, [node.identity]);
    }
    state.order = order;
  }

  private syncRefs(root: GuiDescribedRoot, state: GuiRootState): void {
    root = this.localRoots.get(root.identity) ?? root;
    const next = new Map<number, GuiNodeRef>();
    const bind = (
      key: number,
      target: GuiNodeRef,
      handle: GuiNodeHandle,
    ): void => {
      try {
        setRef(target, handle);
      } catch (error) {
        this.options.report(error);
        return;
      }
      next.set(key, target);
    };
    if (root.nodeRef) {
      const top = root.nodes.find((node) => node.parent === undefined);
      const ack = top ? state.acked.get(top.identity) : undefined;
      if (top && ack)
        bind(
          root.identity,
          root.nodeRef,
          this.handleFor(state, ack.nodeId, ack.lifetime),
        );
    }
    for (const node of root.nodes) {
      const ack = state.acked.get(node.identity);
      const target = node.nodeRef ?? null;
      if (ack && target)
        bind(
          node.identity,
          target,
          this.handleFor(state, ack.nodeId, ack.lifetime),
        );
    }
    for (const [identity, target] of state.boundRefs)
      if (!next.has(identity)) {
        try {
          setRef(target, null);
        } catch {
          // Ref cleanup must not fail commits.
        }
      }
    state.boundRefs = next;
  }

  /** Reconcile against the authoritative tree after an ambiguous failure. */
  private async resync(
    root: GuiDescribedRoot,
    state: GuiRootState,
  ): Promise<void> {
    const client = this.client;
    if (!hasGui(client))
      throw new Error("GUI declarations require a GUI-capable client");
    const inspection = await client.inspectGui({ entity: state.entity });
    if (inspection.rootIncarnation !== state.incarnation) {
      // A replaced incarnation owns a different tree; never retarget old handles.
      this.nullRefs(state);
      this.states.delete(root.identity);
      const fresh = await this.initialize(root.identity, state.entity);
      state.entity = fresh.entity;
      state.incarnation = fresh.incarnation;
      state.session = fresh.session;
      state.ids = fresh.ids;
      state.acked = fresh.acked;
      state.order = fresh.order;
      state.nextId = fresh.nextId;
      state.boundRefs = fresh.boundRefs;
      state.producerOwned = fresh.producerOwned;
      this.states.set(root.identity, state);
      return;
    }
    const seen = new Map<number, GuiInspectedNode>();
    for (const node of inspection.nodes) seen.set(node.id, node);
    const known = new Set(state.ids.values());
    for (const id of seen.keys()) {
      if (!known.has(id)) {
        // Another writer changed the structure; never adopt or overwrite it.
        // Refusing keeps every submitted identity and the high-water mark,
        // so produced IDs survive the refusal for later recovery.
        throw this.options.report(
          new Error(
            "GUI tree changed beneath the reconciler; refusing to proceed",
          ),
        );
      }
    }
    const declared = new Map<number, GuiDescribedNode>();
    for (const node of root.nodes) declared.set(node.identity, node);
    const byId = new Map<number, number>();
    for (const [identity, id] of state.ids) byId.set(id, identity);
    for (const [identity, id] of [...state.ids]) {
      if (state.acked.has(identity)) continue;
      const node = seen.get(id);
      const want = declared.get(identity);
      if (node && want) {
        // An ambiguously completed insert landed after all.
        state.acked.set(identity, {
          nodeId: id,
          parent: want.parent,
          parentId: node.parent,
          lifetime: node.lifetime,
          content: node.content,
          style: node.style,
        });
        this.observeNode(state, root, identity);
      } else {
        // It did not land; free its identity for the retry.
        state.ids.delete(identity);
      }
    }
    for (const [identity, ack] of [...state.acked]) {
      const node = seen.get(ack.nodeId);
      if (!node) {
        this.subscriptions.unsubscribe(
          state.entity,
          state.incarnation,
          ack.nodeId,
        );
        state.acked.delete(identity);
        state.ids.delete(identity);
        continue;
      }
      state.acked.set(identity, {
        nodeId: node.id,
        parent: ack.parent,
        parentId: node.parent,
        lifetime: node.lifetime,
        content: equalGuiContent(node.content, ack.content)
          ? ack.content
          : node.content,
        style: normalizeGuiStyle(node.style),
      });
      this.observeNode(state, root, identity);
    }
    state.nextId = this.nextAfter(state);
    // Recovery resolves only to new inspected content or a forced high-water
    // advance: adopted inserts join acknowledged state with the allocator
    // moved past them, and the mark persists independently of live nodes.
    this.highWater.set(
      this.waterKey(state.entity, state.incarnation),
      state.nextId,
    );
    // Order may have shifted beneath us; drop it so the next diff re-emits
    // placement moves rather than assuming stale positions.
    state.order = new Map();
  }

  /** Release acknowledged trees and producer roots. Failed cleanup remains
   * retained on this instance and another dispose call retries it. */
  async dispose(): Promise<void> {
    this.localRoots.clear();
    for (const [identity, state] of [...this.states])
      this.stageCleanup(identity, state);
    await this.retryPendingCleanup();
    this.subscriptions.clear();
    this.detachObservationBridge();
  }
}
