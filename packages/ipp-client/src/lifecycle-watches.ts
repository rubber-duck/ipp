import type { Response, WorldReference } from "./types.js";
import type {
  LifecycleBaseline,
  LifecycleMemberId,
  LifecycleMembershipCut,
  LifecycleTarget,
  LifecycleTargetEvent,
  LifecycleTargetSelection,
  LifecycleTargetWatch,
  LifecycleWatchClosure,
  LifecycleWatchRemoveError as LifecycleWatchRemoveFailure,
  LifecycleWatchEvent,
  LifecycleWatchRequest,
} from "./lifecycle-types.js";

interface Adapter {
  nextId(): bigint;
  send(request: bigint, control: LifecycleWatchRequest): Promise<Response>;
  definitelyUnapplied(error: unknown): boolean;
  fail(error: Error): void;
}

interface Group {
  world: WorldReference;
  listener(event: LifecycleWatchEvent): void;
  baselines: LifecycleBaseline[];
  cuts: LifecycleMembershipCut[];
  removalCuts: LifecycleMembershipCut[];
  activeGenerations: Set<bigint>;
  members: Map<bigint, Member>;
  closed: Promise<LifecycleWatchClosure>;
  finish(closure: LifecycleWatchClosure): void;
  ended: boolean;
  callbacks: boolean;
  closing?: Promise<readonly LifecycleMembershipCut[]>;
  endCuts?: readonly LifecycleMembershipCut[];
  pageSize: number;
}

interface Active {
  group: Group;
  baseline: LifecycleBaseline;
  kinds: number;
  sequence: bigint;
  /** Tick of the member's last value record; value members only. */
  valueTick?: bigint;
}

interface Control {
  group: Group;
  request: LifecycleWatchRequest;
  cut?: LifecycleMembershipCut;
}

interface Member {
  id: LifecycleMemberId;
  removal?: { cut: LifecycleMembershipCut; order: number };
  pending?: RemovalPage;
}

interface RemovalPage {
  members: Member[];
  settled: Promise<Error | undefined>;
  settle(error?: Error): void;
}

class MembershipRejected extends Error {}

/** Failed paged starts expose every acknowledged identity for exact cleanup. */
export class LifecycleWatchStartError extends Error {
  constructor(
    readonly partial: LifecycleTargetWatch,
    cause: unknown,
  ) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
    this.name = "LifecycleWatchStartError";
  }
}

/** Only ACKed removals are confirmed; unconfirmed members may have unknown effects. */
class LifecycleWatchRemoveError
  extends Error
  implements LifecycleWatchRemoveFailure
{
  override readonly name = "LifecycleWatchRemoveError";
  declare readonly cause: unknown;

  constructor(
    readonly cuts: readonly LifecycleMembershipCut[],
    readonly unconfirmedMembers: readonly LifecycleMemberId[],
    cause: unknown,
  ) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
  }
}

/** Recognize the owned error shape across independently generated SDK modules. */
export function isLifecycleWatchRemoveError(
  value: unknown,
): value is LifecycleWatchRemoveFailure {
  try {
    if (
      !isRecord(value) ||
      value.name !== "LifecycleWatchRemoveError" ||
      typeof value.message !== "string" ||
      (value.stack !== undefined && typeof value.stack !== "string") ||
      !Object.hasOwn(value, "cause") ||
      !Array.isArray(value.cuts) ||
      !Array.isArray(value.unconfirmedMembers) ||
      value.unconfirmedMembers.length === 0
    )
      return false;
    for (let index = 0; index < value.cuts.length; index++) {
      const cut: unknown = value.cuts[index];
      if (
        !isRecord(cut) ||
        !isRecord(cut.world) ||
        !isU64(cut.world.id) ||
        !isU64(cut.world.incarnation) ||
        !isU64(cut.session) ||
        !isU64(cut.request) ||
        !isU64(cut.sequence) ||
        !isU64(cut.tick)
      )
        return false;
    }
    for (let index = 0; index < value.unconfirmedMembers.length; index++) {
      const member: unknown = value.unconfirmedMembers[index];
      if (
        !isRecord(member) ||
        !isU64(member.output) ||
        !isU64(member.generation)
      )
        return false;
    }
    return true;
  } catch {
    return false;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object";
}

function isU64(value: unknown): boolean {
  return (
    typeof value === "bigint" && value >= 0n && value <= 0xffffffffffffffffn
  );
}

export class LifecycleWatches {
  private controls = new Map<bigint, Control>();
  private active = new Map<bigint, Active>();
  private groups = new Set<Group>();
  private output?: bigint;
  private world?: WorldReference;
  private stopped?: Error;

  constructor(private readonly adapter: Adapter) {}

  async watch(
    world: WorldReference,
    targets: readonly LifecycleTargetSelection[],
    listener: (event: LifecycleWatchEvent) => void,
    pageSize: number,
  ): Promise<LifecycleTargetWatch> {
    if (this.stopped) throw this.stopped;
    if (!Number.isSafeInteger(pageSize) || pageSize < 1 || targets.length === 0)
      throw new RangeError("Invalid lifecycle membership page");
    const selections = targets.map((selection) =>
      immutable({ ...selection, target: { ...selection.target } }),
    );
    let finish!: (closure: LifecycleWatchClosure) => void;
    const closed = new Promise<LifecycleWatchClosure>((resolve) => {
      finish = resolve;
    });
    const group: Group = {
      world: immutable({ ...world }),
      listener,
      baselines: [],
      cuts: [],
      removalCuts: [],
      activeGenerations: new Set(),
      members: new Map(),
      closed,
      finish,
      ended: false,
      callbacks: true,
      pageSize,
    };
    if (this.world && !sameWorld(world, this.world))
      throw new Error("Foreign lifecycle World");
    this.world = group.world;
    this.groups.add(group);
    try {
      for (let offset = 0; offset < selections.length; offset += pageSize) {
        await this.control(group, {
          kind: "add",
          world: group.world,
          targets: selections.slice(offset, offset + pageSize),
        });
      }
      return this.handle(group);
    } catch (error) {
      group.callbacks = false;
      if (!group.baselines.length)
        this.finish(group, { kind: "closed", reason: asError(error) });
      throw new LifecycleWatchStartError(this.handle(group), error);
    }
  }

  private handle(group: Group): LifecycleTargetWatch {
    return Object.freeze({
      world: group.world,
      baselines: Object.freeze([...group.baselines]),
      cuts: Object.freeze([...group.cuts]),
      closed: group.closed,
      removeMembers: (members: readonly LifecycleMemberId[]) =>
        this.removeMembers(group, members),
      remove: () => this.remove(group),
    });
  }

  private remove(group: Group): Promise<readonly LifecycleMembershipCut[]> {
    if (group.endCuts) return Promise.resolve(group.endCuts);
    if (group.closing) return group.closing;
    if (this.stopped) return Promise.reject(this.stopped);
    if (group.ended)
      return Promise.reject(new Error("Lifecycle tracking ended"));
    const operation = this.removePages(group);
    group.closing = operation;
    void operation.catch(() => {
      if (group.closing === operation) delete group.closing;
    });
    return operation;
  }

  private async removePages(
    group: Group,
  ): Promise<readonly LifecycleMembershipCut[]> {
    const members = [...group.activeGenerations].map(
      (generation) => group.members.get(generation)!.id,
    );
    try {
      await this.removeMembers(group, members);
    } catch (error) {
      throw error instanceof LifecycleWatchRemoveError ? error.cause : error;
    }
    if (group.ended)
      throw this.stopped ?? new Error("Lifecycle tracking ended");
    group.endCuts = Object.freeze([...group.removalCuts]);
    this.finish(group, { kind: "removed", cuts: group.endCuts });
    return group.endCuts;
  }

  private async removeMembers(
    group: Group,
    ids: readonly LifecycleMemberId[],
  ): Promise<readonly LifecycleMembershipCut[]> {
    const selected = new Map<bigint, Member>();
    for (const id of ids) {
      const member = id && group.members.get(id.generation);
      if (!member || member.id.output !== id.output)
        throw new RangeError("Lifecycle member does not belong to this group");
      selected.set(member.id.generation, member);
    }
    const members = [...selected.values()].sort((left, right) =>
      left.id.generation < right.id.generation ? -1 : 1,
    );
    const unsettled = members.filter((member) => !member.removal);
    const needed = unsettled.filter((member) => !member.pending);
    const pages: RemovalPage[] = [];
    const unavailable =
      this.stopped ??
      (group.ended ? new Error("Lifecycle tracking ended") : undefined);
    if (!unavailable) {
      for (let offset = 0; offset < needed.length; offset += group.pageSize) {
        let settle!: RemovalPage["settle"];
        const page: RemovalPage = {
          members: needed.slice(offset, offset + group.pageSize),
          settled: new Promise((resolve) => {
            settle = resolve;
          }),
          settle: (error) => settle(error),
        };
        for (const member of page.members) member.pending = page;
        pages.push(page);
      }
    }
    const waiting = new Set(
      unsettled.flatMap((member) => (member.pending ? [member.pending] : [])),
    );
    void this.removeSelectedPages(group, pages);
    const failures = await Promise.all(
      [...waiting].map((page) => page.settled),
    );
    const cuts = this.removalCuts(members);
    const unconfirmed = members.filter((member) => !member.removal);
    if (unconfirmed.length) {
      throw new LifecycleWatchRemoveError(
        cuts,
        Object.freeze(unconfirmed.map((member) => member.id)),
        failures.find((error) => error !== undefined) ??
          unavailable ??
          new Error("Lifecycle removal was not confirmed"),
      );
    }
    return cuts;
  }

  private removalCuts(
    members: readonly Member[],
  ): readonly LifecycleMembershipCut[] {
    const completed = new Map<bigint, NonNullable<Member["removal"]>>();
    for (const member of members) {
      if (member.removal)
        completed.set(member.removal.cut.request, member.removal);
    }
    return Object.freeze(
      [...completed.values()]
        .sort((left, right) => left.order - right.order)
        .map((removal) => removal.cut),
    );
  }

  private async removeSelectedPages(
    group: Group,
    pages: readonly RemovalPage[],
  ): Promise<void> {
    let failure: Error | undefined;
    for (const page of pages) {
      if (!failure) {
        try {
          await this.control(group, {
            kind: "remove",
            world: group.world,
            output: page.members[0]!.id.output,
            generations: page.members.map((member) => member.id.generation),
          });
        } catch (error) {
          failure = asError(error);
        }
      }
      for (const member of page.members) {
        if (member.pending === page) delete member.pending;
      }
      page.settle(failure);
    }
  }

  private async control(
    group: Group,
    request: LifecycleWatchRequest,
  ): Promise<LifecycleMembershipCut> {
    if (this.stopped) throw this.stopped;
    if (group.ended) throw new MembershipRejected("Lifecycle tracking ended");
    const id = this.adapter.nextId();
    const control: Control = { group, request };
    this.controls.set(id, control);
    try {
      const response = await this.adapter.send(id, request);
      if (
        response.body.kind !== "lifecycleWatch" ||
        response.body.record.kind !== "ack"
      )
        throw new Error("Invalid membership reply");
      const record = response.body.record;
      if (record.result.kind !== "applied")
        throw new MembershipRejected(
          `Lifecycle membership ${record.result.kind}`,
        );
      if (!control.cut)
        throw new Error("Missing synchronous membership activation");
      if (group.ended) throw new MembershipRejected("Lifecycle tracking ended");
      return control.cut;
    } catch (error) {
      if (
        !this.stopped &&
        !this.adapter.definitelyUnapplied(error) &&
        !(error instanceof MembershipRejected)
      )
        this.adapter.fail(asError(error));
      throw error;
    } finally {
      this.controls.delete(id);
    }
  }

  receive(response: Response): boolean {
    if (this.stopped) return false;
    const pending = this.controls.get(response.requestId);
    if (response.body.kind !== "lifecycleWatch") {
      if (pending) {
        if (response.body.kind !== "error" || response.body.code !== 1)
          throw new Error("Unknown lifecycle control outcome");
        this.controls.delete(response.requestId);
      }
      return false;
    }
    const record = response.body.record;
    if (
      response.tick !== 0n ||
      !this.world ||
      !sameWorld(record.world, this.world) ||
      record.output <= 0n ||
      (this.output !== undefined && record.output !== this.output)
    )
      throw new Error("Foreign lifecycle endpoint");
    if (record.kind === "ack") {
      if (
        !pending ||
        response.requestId === 0n ||
        record.action !== pending.request.kind
      )
        throw new Error("Invalid lifecycle ACK correlation");
      const { group, request } = pending;
      if ((record.result.kind === "cancelled") !== (record.cut === null))
        throw new Error("Invalid lifecycle apply cut");
      this.output = record.output;
      if (record.result.kind === "applied") {
        const expected =
          request.kind === "add" ? request.targets : request.generations;
        const baselines = record.result.baselines;
        if (baselines.length !== expected.length || !record.cut)
          throw new Error("Invalid lifecycle baseline count");
        let previous = 0n;
        for (let index = 0; index < baselines.length; index++) {
          const baseline = baselines[index]!;
          const generation = baseline.member.generation;
          if (
            baseline.member.output !== record.output ||
            generation <= previous
          )
            throw new Error("Invalid lifecycle member generation");
          previous = generation;
          if (request.kind === "add") {
            if (
              !sameTarget(baseline.target, request.targets[index]!.target) ||
              baseline.lifetime.kind !== lifetimeKind(baseline.target) ||
              this.active.has(generation)
            )
              throw new Error("Invalid lifecycle acquisition baseline");
          } else {
            const active = this.active.get(generation);
            if (
              generation !== request.generations[index] ||
              !active ||
              active.group !== group ||
              !sameTarget(active.baseline.target, baseline.target) ||
              baseline.lifetime.kind !== "removed"
            )
              throw new Error("Foreign lifecycle removal");
          }
        }
        const cut: LifecycleMembershipCut = immutable({
          world: { ...record.world },
          session: response.session,
          request: response.requestId,
          ...record.cut,
        });
        pending.cut = cut;
        for (let index = 0; index < baselines.length; index++) {
          const baseline = immutable(baselines[index]!);
          if (request.kind === "add") {
            group.baselines.push(baseline);
            group.members.set(baseline.member.generation, {
              id: baseline.member,
            });
            if (!group.ended) {
              group.activeGenerations.add(baseline.member.generation);
              this.active.set(baseline.member.generation, {
                group,
                baseline,
                kinds: request.targets[index]!.kinds,
                sequence: cut.sequence,
              });
            }
          } else {
            group.members.get(baseline.member.generation)!.removal = {
              cut,
              order: group.removalCuts.length,
            };
            this.active.delete(baseline.member.generation);
            group.activeGenerations.delete(baseline.member.generation);
          }
        }
        if (request.kind === "add") group.cuts.push(cut);
        else group.removalCuts.push(cut);
      }
      this.controls.delete(response.requestId);
      return false;
    }
    if (response.requestId !== 0n || this.output === undefined)
      throw new Error("Lifecycle event before membership ACK");
    if (record.kind === "value") {
      const entry = this.active.get(record.member.generation);
      const target = entry?.baseline.target;
      if (
        !entry ||
        target?.kind !== "value" ||
        record.member.output !== this.output ||
        (entry.valueTick !== undefined && record.tick <= entry.valueTick) ||
        (record.values !== null && !sameOffsets(record.values, target.fields))
      )
        throw new Error("Unexpected lifecycle value record");
      entry.valueTick = record.tick;
      this.deliver(entry.group, record);
      return true;
    }
    const entry = this.active.get(record.member.generation);
    if (
      !entry ||
      record.member.output !== this.output ||
      record.sequence <= entry.sequence ||
      !sameTarget(record.observation, entry.baseline.target) ||
      !(entry.kinds & changeBit(record))
    )
      throw new Error("Unexpected lifecycle target observation");
    entry.sequence = record.sequence;
    this.deliver(entry.group, record);
    return true;
  }

  private deliver(group: Group, event: LifecycleWatchEvent): void {
    if (!group.callbacks || group.ended) return;
    try {
      group.listener(immutable(event));
    } catch (error) {
      globalThis.reportError?.(error);
    }
  }

  private finish(group: Group, closure: LifecycleWatchClosure): void {
    if (group.ended) return;
    group.ended = true;
    group.callbacks = false;
    group.activeGenerations.clear();
    this.groups.delete(group);
    group.finish(closure);
  }

  stop(reason: Error): void {
    if (this.stopped) return;
    this.stopped = reason;
    for (const group of [...this.groups])
      this.finish(group, { kind: "closed", reason });
    this.active.clear();
    this.controls.clear();
  }
}

function sameWorld(left: WorldReference, right: WorldReference): boolean {
  return left.id === right.id && left.incarnation === right.incarnation;
}

function sameTarget(left: LifecycleTarget, right: LifecycleTarget): boolean {
  if (left.kind !== right.kind || left.entity !== right.entity) return false;
  if (left.kind === "entity") return true;
  if (left.component !== (right as typeof left).component) return false;
  if (left.kind === "component") return true;
  const fields = (right as typeof left).fields;
  return (
    left.fields.length === fields.length &&
    left.fields.every((offset, index) => offset === fields[index])
  );
}

/** Value members report the lifetime of their component. */
function lifetimeKind(target: LifecycleTarget): "entity" | "component" {
  return target.kind === "entity" ? "entity" : "component";
}

/** Whether a value record reports exactly the target's fields, in order. */
function sameOffsets(
  values: readonly { offset: number }[],
  fields: readonly number[],
): boolean {
  return (
    values.length === fields.length &&
    values.every((value, index) => value.offset === fields[index])
  );
}

function changeBit(event: LifecycleTargetEvent): number {
  const change = event.observation;
  return change.kind === "entity"
    ? { created: 1, metadataChanged: 2, deleted: 4 }[change.change]
    : { inserted: 8, updated: 16, replaced: 32, removed: 64 }[change.change];
}

function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}

function immutable<Value>(value: Value): Value {
  if (value && typeof value === "object") {
    for (const field of Object.values(value)) immutable(field);
    Object.freeze(value);
  }
  return value;
}
