import {
  isLifecycleWatchRemoveError,
  type LifecycleBaseline,
  type LifecycleMemberId,
  type LifecycleTargetEvent,
  type LifecycleTargetSelection,
  type LifecycleTargetWatch,
  type LifecycleValueRecord,
  type LifecycleWatchEvent,
} from "@ipp/client";
import type { ReactWorldClient } from "./world-client.js";

type TrackingClient = Pick<
  ReactWorldClient,
  "session" | "closure" | "watchLifecycle"
>;

export interface ControlLifetime {
  entityLive: boolean;
  incarnation: bigint | null;
}

interface Listener {
  /** The tracked lifetime changed. */
  changed(): void;
  failed(error: Error): void;
  /** A value record of the tracked fields, in delivery order. */
  value?(record: LifecycleValueRecord): void;
}

export interface ControlTrackingLease {
  ready: Promise<void>;
  lifetime(): ControlLifetime | undefined;
  /** The latest value record of the tracked fields, if any arrived. */
  value(): LifecycleValueRecord | undefined;
  release(): Promise<void>;
}

function completion() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  void promise.catch(() => {});
  return { promise, resolve, reject };
}

interface TargetEntry {
  entity: bigint;
  component: number;
  /** Value-watched field offsets, ascending; absent for lifetime tracking only. */
  fields: readonly number[] | undefined;
  users: Set<Listener>;
  ready: ReturnType<typeof completion>;
  removal?: ReturnType<typeof completion>;
  group?: TrackingGroup;
  members: LifecycleMemberId[];
  failure?: Error;
  lifetime?: ControlLifetime;
  value?: LifecycleValueRecord;
  accepting: boolean;
}

interface TrackingGroup {
  entries: Set<TargetEntry>;
  members: Map<bigint, TargetEntry>;
  watch?: LifecycleTargetWatch;
  ended: boolean;
}

const registries = new WeakMap<TrackingClient, ControlTracking>();

export function unsubmittedTracking(error: unknown): boolean {
  if (!error || typeof error !== "object") return false;
  if (
    "code" in error &&
    (error.code === "IPP_REQUEST_NOT_SENT" ||
      error.code === "IPP_REQUEST_REJECTED")
  )
    return true;
  return (
    "cause" in error &&
    !!error.cause &&
    typeof error.cause === "object" &&
    "code" in error.cause &&
    (error.cause.code === "IPP_REQUEST_NOT_SENT" ||
      error.cause.code === "IPP_REQUEST_REJECTED")
  );
}

export function controlTracking(client: TrackingClient): ControlTracking {
  let registry = registries.get(client);
  if (!registry) {
    registry = new ControlTracking(client);
    registries.set(client, registry);
  }
  return registry;
}

/** Registry key of one component's tracking, with or without value fields. */
function entryKey(
  component: number,
  fields: readonly number[] | undefined,
): string {
  return fields ? `${component}:${fields.join(",")}` : String(component);
}

/** The watch members of one entry: entity, component and optional value. */
function selections(entry: TargetEntry): LifecycleTargetSelection[] {
  const members: LifecycleTargetSelection[] = [
    { target: { kind: "entity", entity: entry.entity }, kinds: 4 },
    {
      target: {
        kind: "component",
        entity: entry.entity,
        component: entry.component,
      },
      kinds: 104,
    },
  ];
  if (entry.fields)
    members.push({
      target: {
        kind: "value",
        entity: entry.entity,
        component: entry.component,
        fields: entry.fields,
      },
      kinds: 128,
    });
  return members;
}

/**
 * Shared per-client lifecycle tracking of control components. One entry per
 * entity, component and value field set holds the watch members all its
 * users share.
 */
export class ControlTracking {
  private readonly session: bigint;
  private entries = new Map<bigint, Map<string, TargetEntry>>();
  private additions = new Set<TargetEntry>();
  private removals = new Set<TargetEntry>();
  private scheduled = false;
  private running = false;

  constructor(private readonly client: TrackingClient) {
    this.session = client.session;
  }

  /**
   * Track the lifetime of `component` on `entity` and, with `fields`, the
   * values of those field offsets (ascending).
   */
  acquire(
    entity: bigint,
    component: number,
    listener: Listener,
    fields?: readonly number[],
  ): ControlTrackingLease {
    if (this.client.closure) throw this.client.closure.reason;
    if (this.client.session !== this.session)
      throw new Error("Control tracking session changed");
    let components = this.entries.get(entity);
    if (!components) this.entries.set(entity, (components = new Map()));
    const key = entryKey(component, fields);
    let entry = components.get(key);
    if (!entry || !entry.accepting) {
      const predecessor = entry?.removal?.promise;
      entry = {
        entity,
        component,
        fields: fields && Object.freeze([...fields]),
        users: new Set(),
        accepting: true,
        ready: completion(),
        members: [],
      };
      components.set(key, entry);
      const owned = entry;
      void Promise.resolve(predecessor).then(
        () => {
          this.additions.add(owned);
          this.schedule();
        },
        (error: unknown) => {
          owned.failure =
            error instanceof Error ? error : new Error(String(error));
          owned.ready.reject(error);
        },
      );
    }
    const owned = entry;
    owned.users.add(listener);
    let released: Promise<void> | undefined;
    const ready = owned.ready.promise.then(() => {
      if (owned.failure) throw owned.failure;
    });
    void ready.catch(() => {});
    return {
      ready,
      lifetime: () => (owned.failure ? undefined : owned.lifetime),
      value: () => (owned.failure ? undefined : owned.value),
      release: () => {
        if (released) return released;
        owned.users.delete(listener);
        if (owned.users.size) return (released = Promise.resolve());
        owned.accepting = false;
        const removal = completion();
        owned.removal = removal;
        released = removal.promise;
        void owned.ready.promise
          .catch(() => {})
          .then(() => {
            this.removals.add(owned);
            this.schedule();
          });
        void released.catch(() => {
          released = undefined;
        });
        return released;
      },
    };
  }

  private schedule(): void {
    if (this.scheduled || this.running) return;
    this.scheduled = true;
    void Promise.resolve().then(async () => {
      this.scheduled = false;
      this.running = true;
      const removing = [...this.removals];
      const adding = [...this.additions];
      this.removals.clear();
      this.additions.clear();
      try {
        await this.remove(removing);
        await this.start(adding);
      } finally {
        this.running = false;
        if (this.removals.size || this.additions.size) this.schedule();
      }
    });
  }

  private async start(pending: TargetEntry[]): Promise<void> {
    const entries = pending.filter((entry) => {
      if (entry.accepting) return true;
      entry.ready.resolve();
      return false;
    });
    if (!entries.length) return;
    const group: TrackingGroup = {
      entries: new Set(entries),
      members: new Map(),
      ended: false,
    };
    for (const entry of entries) entry.group = group;
    const members = entries.map(selections);
    // Records that arrive before the watch resolves wait, in delivery order,
    // until its baselines identify their members.
    const early: LifecycleWatchEvent[] = [];
    let active = false;
    const changed = (event: LifecycleWatchEvent) => {
      if (!active) {
        early.push(event);
        return;
      }
      if (event.kind === "event") this.changed(group, event);
      else this.valueChanged(group, event);
    };
    try {
      const watch = await this.client.watchLifecycle!(members.flat(), changed);
      this.retain(group, entries, members, watch);
      if (
        watch.baselines.length !==
        members.reduce((count, selected) => count + selected.length, 0)
      )
        throw new Error("Control tracking omitted membership baselines");
      active = true;
      for (const event of early) changed(event);
      early.length = 0;
      await Promise.resolve();
      for (const entry of entries) {
        if (this.client.closure) throw this.client.closure.reason;
        if (entry.failure) entry.ready.reject(entry.failure);
        else entry.ready.resolve();
      }
    } catch (error) {
      if (
        !group.watch &&
        error &&
        typeof error === "object" &&
        "partial" in error
      ) {
        const partial = error.partial;
        if (
          partial &&
          typeof partial === "object" &&
          "removeMembers" in partial &&
          typeof partial.removeMembers === "function" &&
          "baselines" in partial &&
          Array.isArray(partial.baselines)
        )
          this.retain(group, entries, members, partial as LifecycleTargetWatch);
      }
      for (const entry of entries) {
        if (unsubmittedTracking(error) && !group.watch?.baselines.length)
          entry.failure =
            error instanceof Error ? error : new Error(String(error));
        else
          this.fail(
            entry,
            error instanceof Error ? error : new Error(String(error)),
          );
        entry.ready.reject(error);
      }
    } finally {
      early.length = 0;
    }
  }

  private retain(
    group: TrackingGroup,
    entries: TargetEntry[],
    members: readonly (readonly LifecycleTargetSelection[])[],
    watch: LifecycleTargetWatch,
  ): void {
    group.watch = watch;
    // Baselines follow the requested members: each entry's two or three.
    const owners = entries.flatMap((entry, index) =>
      members[index]!.map(() => entry),
    );
    for (let index = 0; index < watch.baselines.length; index++) {
      const baseline = watch.baselines[index]!;
      const entry = owners[index];
      if (!entry) throw new Error("Unexpected control membership baseline");
      entry.members.push(baseline.member);
      group.members.set(baseline.member.generation, entry);
      this.baseline(entry, baseline);
    }
    if (!watch.baselines.length) {
      group.ended = true;
      return;
    }
    void watch.closed.then((closure) => {
      group.ended = true;
      for (const entry of group.entries) {
        if (!entry.accepting) continue;
        this.fail(
          entry,
          closure.kind === "closed"
            ? closure.reason
            : new Error("Control lifecycle tracking ended"),
        );
      }
    });
  }

  private baseline(entry: TargetEntry, baseline: LifecycleBaseline): void {
    if (
      baseline.target.entity !== entry.entity ||
      (baseline.target.kind !== "entity" &&
        baseline.target.component !== entry.component)
    )
      throw new Error("Foreign control tracking baseline");
    if (
      baseline.target.kind === "component" &&
      baseline.lifetime.kind === "component"
    )
      entry.lifetime = {
        entityLive: baseline.lifetime.entityLive,
        incarnation: baseline.lifetime.incarnation,
      };
  }

  /** The live entry that owns `member` in `group`; fails a foreign member. */
  private member(
    group: TrackingGroup,
    member: LifecycleMemberId,
  ): TargetEntry | undefined {
    const entry = group.members.get(member.generation);
    if (!entry || !entry.accepting || entry.failure || this.client.closure)
      return undefined;
    if (
      !entry.members.some(
        (known) =>
          known.generation === member.generation &&
          known.output === member.output,
      )
    ) {
      this.fail(entry, new Error("Foreign control tracking generation"));
      return undefined;
    }
    return entry;
  }

  private valueChanged(
    group: TrackingGroup,
    record: LifecycleValueRecord,
  ): void {
    const entry = this.member(group, record.member);
    if (!entry) return;
    entry.value = record;
    for (const user of [...entry.users])
      if (entry.users.has(user)) user.value?.(record);
  }

  private changed(group: TrackingGroup, event: LifecycleTargetEvent): void {
    const entry = this.member(group, event.member);
    if (!entry) return;
    const observation = event.observation;
    if (
      observation.entity !== entry.entity ||
      (observation.kind === "component" &&
        observation.component !== entry.component)
    )
      return this.fail(entry, new Error("Foreign control tracking generation"));
    entry.lifetime =
      observation.kind === "entity"
        ? { entityLive: false, incarnation: null }
        : {
            entityLive: entry.lifetime?.entityLive ?? true,
            incarnation: observation.incarnation,
          };
    for (const user of [...entry.users])
      if (entry.users.has(user)) user.changed();
  }

  private fail(entry: TargetEntry, error: Error): void {
    if (entry.failure) return;
    entry.failure = error;
    for (const user of [...entry.users])
      if (entry.users.has(user)) user.failed(error);
  }

  private finishRemoval(entry: TargetEntry, error?: unknown): void {
    if (error !== undefined) {
      entry.removal!.reject(error);
      return;
    }
    const components = this.entries.get(entry.entity);
    const key = entryKey(entry.component, entry.fields);
    if (components?.get(key) === entry) {
      components.delete(key);
      if (!components.size) this.entries.delete(entry.entity);
    }
    entry.removal!.resolve();
  }

  private async remove(entries: TargetEntry[]): Promise<void> {
    const groups = new Map<TrackingGroup, TargetEntry[]>();
    for (const entry of entries) {
      if (!entry.group) {
        this.finishRemoval(
          entry,
          entry.failure &&
            !unsubmittedTracking(entry.failure) &&
            !this.client.closure
            ? entry.failure
            : undefined,
        );
        continue;
      }
      let selected = groups.get(entry.group);
      if (!selected) groups.set(entry.group, (selected = []));
      selected.push(entry);
    }
    for (const [group, selected] of groups) {
      let failure: unknown;
      let unconfirmed: Set<bigint> | undefined;
      const members = selected.flatMap((entry) => entry.members);
      try {
        if (!this.client.closure && !group.ended && group.watch)
          await group.watch.removeMembers(members);
        else if (!group.watch) {
          const failed = selected.find(
            (entry) => entry.failure && !unsubmittedTracking(entry.failure),
          );
          if (failed && !this.client.closure) throw failed.failure;
        }
      } catch (error) {
        if (!this.client.closure && !group.ended) {
          failure = error;
          if (isLifecycleWatchRemoveError(error)) {
            const requested = new Map(
              members.map((member) => [member.generation, member.output]),
            );
            if (
              error.unconfirmedMembers.every(
                (member) => requested.get(member.generation) === member.output,
              )
            )
              unconfirmed = new Set(
                error.unconfirmedMembers.map((member) => member.generation),
              );
          }
        }
      }
      const completed: TargetEntry[] = [];
      for (const entry of selected) {
        entry.members = entry.members.filter((member) => {
          if (
            failure !== undefined &&
            (!unconfirmed || unconfirmed.has(member.generation))
          )
            return true;
          group.members.delete(member.generation);
          return false;
        });
        if (entry.members.length || (failure !== undefined && !unconfirmed))
          this.finishRemoval(entry, failure);
        else {
          group.entries.delete(entry);
          completed.push(entry);
        }
      }
      let closingError: unknown;
      if (
        !group.entries.size &&
        group.watch &&
        !group.ended &&
        !this.client.closure
      ) {
        try {
          await group.watch.remove();
        } catch (error) {
          if (!group.ended && !this.client.closure) closingError = error;
        }
      }
      for (const entry of completed) this.finishRemoval(entry, closingError);
    }
  }
}
