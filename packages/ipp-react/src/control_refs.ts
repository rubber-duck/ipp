import {
  FieldKind,
  type ComponentDescriptor,
  type ComponentFieldValue,
  type EntitySnapshot,
  type FieldValue,
  type GuiTarget,
  type LifecycleValueRecord,
} from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import {
  controlValueCallbackNames,
  controlValueLayout,
  type GuiControlValueLayout,
} from "./gui/callbacks.js";
import type {
  ReactComponentDescription,
  ReactEntityDescription,
  ReactWorldDescription,
} from "./tree.js";
import type { GuiControlHandle, GuiControlRef } from "./gui/control-ref.js";
import {
  controlTracking,
  unsubmittedTracking,
  type ControlTrackingLease,
} from "./control_tracking.js";

/** A control component this root tracks, and what it observes. */
interface ControlDeclaration {
  description: ReactComponentDescription;
  symbolicId: string;
  entityKind: ReactEntityDescription["kind"];
  /** The watched value fields, or undefined when values are not observed. */
  values: GuiControlValueLayout | undefined;
}

/** The acknowledged entity and component of one control declaration. */
export interface AcknowledgedControl {
  entity: bigint;
  component: number;
  /** Whether this acknowledgement still describes the declaration's component. */
  valid(): boolean;
}

interface RefAssignment {
  ref: GuiControlRef;
  release?: () => void;
}

/** A published handle of one control incarnation. */
interface ControlBinding {
  handle: GuiControlHandle;
  live: boolean;
  valid(): boolean;
  assignment?: RefAssignment | undefined;
}

/**
 * The lifecycle tracking of one control declaration's component; with value
 * fields it is also the control's value registration.
 */
interface TrackedControl {
  readonly entity: bigint;
  readonly component: number;
  acknowledged: AcknowledgedControl;
  readonly lease: ControlTrackingLease;
}

/** A value record of one observed control, in delivery order. */
export interface ControlValueDelivery {
  readonly identity: number;
  /** The value registration the record belongs to; delivery state is per registration. */
  readonly registration: object;
  /** The binding current when the record arrived; absent for an absent component. */
  readonly binding: object | undefined;
  readonly record: LifecycleValueRecord;
}

/** A value-observing control whose delivery is still current. */
export interface ObservedControl {
  readonly target: GuiTarget;
  readonly description: ReactComponentDescription;
  readonly values: GuiControlValueLayout;
}

type ControlClient = Pick<
  ReactWorldClient,
  | "session"
  | "closure"
  | "worldReference"
  | "components"
  | "batch"
  | "watchLifecycle"
  | "inspectPage"
>;

function sameDeclaration(
  left: ControlDeclaration,
  right: ControlDeclaration,
): boolean {
  return (
    left.symbolicId === right.symbolicId &&
    left.entityKind === right.entityKind &&
    left.description.entity === right.description.entity &&
    left.description.component === right.description.component &&
    left.values === right.values
  );
}

/** The fields of `component` in an inspected entity, by field name. */
function componentFields(
  snapshot: EntitySnapshot,
  component: number,
): Readonly<Record<string, ComponentFieldValue>> | undefined {
  return snapshot.components.find((entry) => entry.component === component)
    ?.fields;
}

/** A compare-and-set operand for a field of kind `kind`. */
function controlFieldValue(
  kind: FieldKind,
  value: boolean | number | bigint | string,
): FieldValue {
  if (kind === FieldKind.Bool && typeof value === "boolean")
    return { kind: "bool", value };
  if (kind === FieldKind.F32 && typeof value === "number")
    return { kind: "f32", value };
  if (kind === FieldKind.U32 && typeof value === "number")
    return { kind: "u32", value };
  if (kind === FieldKind.U64 && typeof value === "bigint")
    return { kind: "u64", value };
  if (kind === FieldKind.String && typeof value === "string")
    return { kind: "string", value };
  throw new TypeError(
    `Control compare-and-set cannot compare a ${typeof value} with a field of kind ${kind}`,
  );
}

/**
 * Control refs and value observation of one root's GUI control declarations.
 * Each tracked control holds one shared lifecycle tracking of its component;
 * a binding (and its ref handle) exists while that tracking shows the entity
 * live with a component incarnation, and a new incarnation publishes a new
 * binding.
 */
export class ReactControlRefs {
  private desired = new Map<number, ControlDeclaration>();
  private bindings = new Map<number, ControlBinding>();
  private tracked = new Map<number, TrackedControl>();
  private dirty = new Set<number>();
  private cleanup = new Set<ControlTrackingLease>();
  private closed = false;
  private trackingFailure: Error | undefined;
  private descriptors: Map<number, ComponentDescriptor> | undefined;
  private layouts = new Map<number, GuiControlValueLayout | undefined>();
  private valueListener: ((delivery: ControlValueDelivery) => void) | undefined;

  constructor(
    private readonly client: ControlClient,
    private readonly report: (error: unknown) => unknown,
    private readonly refresh: () => void,
  ) {}

  /** Receive value records of observed controls, and their seeds. */
  observeValues(listener: (delivery: ControlValueDelivery) => void): void {
    this.valueListener = listener;
  }

  async dispose(): Promise<void> {
    this.close();
    await this.releasePending();
  }

  async releasePending(): Promise<void> {
    const outcomes = await Promise.allSettled(
      [...this.cleanup].map(async (lease) => {
        await lease.release();
        this.cleanup.delete(lease);
      }),
    );
    const failures = outcomes.filter(
      (outcome) => outcome.status === "rejected",
    );
    if (failures.length)
      throw new AggregateError(
        failures.map((outcome) => outcome.reason),
        "Control tracking cleanup incomplete",
      );
  }

  private descriptor(component: number): ComponentDescriptor | undefined {
    this.descriptors ??= new Map(
      Object.values(this.client.components).map((descriptor) => [
        descriptor.id,
        descriptor,
      ]),
    );
    return this.descriptors.get(component);
  }

  private layout(component: number): GuiControlValueLayout | undefined {
    if (!this.layouts.has(component))
      this.layouts.set(
        component,
        controlValueLayout(this.client.components, component),
      );
    return this.layouts.get(component);
  }

  private fail(error: Error): void {
    if (this.closed || this.trackingFailure) return;
    this.trackingFailure = error;
    this.dirty.clear();
    this.reset();
    this.report(error);
  }

  private changed(identity: number, tracked: TrackedControl): void {
    if (
      this.closed ||
      this.trackingFailure ||
      this.tracked.get(identity) !== tracked
    )
      return;
    const lifetime = tracked.lease.lifetime();
    if (!this.desired.has(identity) || !lifetime) return;
    const binding = this.bindings.get(identity);
    if (
      binding &&
      lifetime.entityLive &&
      binding.handle.target.incarnation === lifetime.incarnation
    )
      return;
    this.invalidate(identity);
    // An absent component leaves the control unbound until it returns.
    if (!lifetime.entityLive || lifetime.incarnation === null) return;
    this.dirty.add(identity);
    this.refresh();
  }

  private valueChanged(
    identity: number,
    tracked: TrackedControl,
    record: LifecycleValueRecord,
  ): void {
    if (
      this.closed ||
      this.trackingFailure ||
      this.tracked.get(identity) !== tracked
    )
      return;
    const binding = this.bindings.get(identity);
    // A present value without a binding waits for the binding's seed.
    if (!binding && record.values !== null) return;
    this.valueListener?.({ identity, registration: tracked, binding, record });
  }

  /**
   * Deliver the latest value record of a bound, value-observing control
   * again, for callbacks registered since it arrived.
   */
  seedValues(identity: number): void {
    const tracked = this.tracked.get(identity);
    const binding = this.bindings.get(identity);
    const record = tracked?.lease.value();
    if (tracked && binding && record?.values)
      this.valueListener?.({
        identity,
        registration: tracked,
        binding,
        record,
      });
  }

  /** The control a value delivery reaches, while its binding is current. */
  observed(delivery: ControlValueDelivery): ObservedControl | undefined {
    const binding = this.bindings.get(delivery.identity);
    const declaration = this.desired.get(delivery.identity);
    if (
      !binding ||
      binding !== delivery.binding ||
      this.tracked.get(delivery.identity) !== delivery.registration ||
      !declaration?.values ||
      !this.live(binding)
    )
      return undefined;
    return {
      target: binding.handle.target,
      description: declaration.description,
      values: declaration.values,
    };
  }

  setDesired(description: ReactWorldDescription): void {
    if (this.closed) return;
    const entities = new Map(
      description.entities.map((entity) => [entity.identity, entity]),
    );
    const desired = new Map<number, ControlDeclaration>();
    for (const component of description.components) {
      if (!component.control) continue;
      const observes =
        !!description.guiActions ||
        controlValueCallbackNames.some(
          (name) => component.controlListeners?.[name],
        );
      if (!component.controlRef && !observes) continue;
      const entity = entities.get(component.entity);
      if (entity)
        desired.set(component.identity, {
          description: component,
          symbolicId: entity.symbolicId,
          entityKind: entity.kind,
          values: observes ? this.layout(component.component) : undefined,
        });
    }
    const previous = this.desired;
    this.desired = desired;
    for (const [identity, declaration] of previous) {
      if (this.closed || this.desired !== desired) break;
      const next = desired.get(identity);
      if (!next || !sameDeclaration(declaration, next)) {
        this.dirty.delete(identity);
        this.untrack(identity);
        this.invalidate(identity);
      }
    }
    for (const [identity, declaration] of desired) {
      if (this.closed || this.desired !== desired) break;
      const binding = this.bindings.get(identity);
      if (binding)
        this.assign(identity, binding, declaration.description.controlRef);
      else if (!this.tracked.has(identity)) this.dirty.add(identity);
    }
  }

  private invalidate(identity: number): void {
    const binding = this.bindings.get(identity);
    if (!binding) return;
    this.bindings.delete(identity);
    binding.live = false;
    this.release(binding);
  }

  private untrack(identity: number): void {
    const tracked = this.tracked.get(identity);
    if (!tracked) return;
    this.tracked.delete(identity);
    this.cleanup.add(tracked.lease);
    void tracked.lease.release().then(
      () => this.cleanup.delete(tracked.lease),
      (error: unknown) => this.report(error),
    );
  }

  close(): void {
    this.closed = true;
    this.desired.clear();
    this.dirty.clear();
    for (const identity of this.bindings.keys()) this.invalidate(identity);
    for (const identity of this.tracked.keys()) this.untrack(identity);
  }

  reset(reacquire = false): void {
    for (const identity of this.bindings.keys()) this.invalidate(identity);
    if (reacquire) {
      for (const identity of this.tracked.keys()) this.untrack(identity);
      for (const identity of this.desired.keys()) this.dirty.add(identity);
    }
  }

  needsPublication(): boolean {
    return (
      !this.closed &&
      (this.trackingFailure ? this.desired.size > 0 : this.dirty.size > 0)
    );
  }

  private live(binding: ControlBinding): boolean {
    return (
      binding.live &&
      !this.closed &&
      !this.trackingFailure &&
      !this.client.closure &&
      binding.valid()
    );
  }

  /**
   * Bind dirty declarations to their acknowledged components. `reacknowledged`
   * names the declarations whose acknowledgement may have changed since the
   * last publication; only those are rechecked, so publication never scans
   * unrelated controls.
   */
  async publish(
    resolve: (identity: number) => AcknowledgedControl | undefined,
    reacknowledged: Iterable<number> = [],
  ): Promise<void> {
    if (!this.closed && this.desired.size && this.trackingFailure)
      throw this.trackingFailure;
    // A commit may acknowledge a declaration's component anew; tracking of
    // the same entity and component continues, anything else starts over.
    // Tracked controls are desired: setDesired untracks the others.
    for (const identity of reacknowledged) {
      const tracked = this.tracked.get(identity);
      if (!tracked || tracked.acknowledged.valid()) continue;
      const acknowledged = resolve(identity);
      if (
        acknowledged?.valid() &&
        acknowledged.entity === tracked.entity &&
        acknowledged.component === tracked.component
      )
        tracked.acknowledged = acknowledged;
      else {
        this.untrack(identity);
        this.invalidate(identity);
      }
      if (!this.bindings.has(identity)) this.dirty.add(identity);
    }
    const publications: { identity: number; tracked: TrackedControl }[] = [];
    for (const identity of [...this.dirty]) {
      const declaration = this.desired.get(identity);
      if (
        !declaration ||
        this.closed ||
        this.client.closure ||
        this.bindings.has(identity)
      ) {
        this.dirty.delete(identity);
        continue;
      }
      const acknowledged = resolve(identity);
      if (!acknowledged?.valid()) continue;
      if (acknowledged.component !== declaration.description.component)
        throw new Error("Acknowledged control does not match its declaration");
      this.dirty.delete(identity);
      let tracked = this.tracked.get(identity);
      if (!tracked) {
        const created: TrackedControl = {
          entity: acknowledged.entity,
          component: acknowledged.component,
          acknowledged,
          lease: controlTracking(this.client).acquire(
            acknowledged.entity,
            acknowledged.component,
            {
              changed: () => this.changed(identity, created),
              failed: (error) => {
                if (this.tracked.get(identity) === created) this.fail(error);
              },
              value: (record) => this.valueChanged(identity, created, record),
            },
            declaration.values?.fields,
          ),
        };
        this.tracked.set(identity, (tracked = created));
      }
      publications.push({ identity, tracked });
    }
    const readiness = await Promise.allSettled(
      publications.map(({ tracked }) => tracked.lease.ready),
    );
    const failures: unknown[] = [];
    for (let index = 0; index < readiness.length; index++) {
      const result = readiness[index]!;
      if (result.status !== "rejected") continue;
      failures.push(result.reason);
      const { identity, tracked } = publications[index]!;
      if (
        unsubmittedTracking(result.reason) &&
        this.tracked.get(identity) === tracked
      )
        this.untrack(identity);
    }
    if (failures.length) {
      if (!this.closed && !this.trackingFailure)
        for (const { identity } of publications)
          if (this.desired.has(identity) && !this.bindings.has(identity))
            this.dirty.add(identity);
      throw failures[0];
    }
    for (const { identity, tracked } of publications) {
      if (this.trackingFailure) throw this.trackingFailure;
      if (
        !this.closed &&
        !this.client.closure &&
        this.tracked.get(identity) === tracked &&
        !this.bindings.has(identity) &&
        tracked.acknowledged.valid()
      )
        this.bind(identity, tracked);
    }
  }

  /** Publish a binding for the tracked component's current incarnation. */
  private bind(identity: number, tracked: TrackedControl): void {
    const declaration = this.desired.get(identity);
    const lifetime = tracked.lease.lifetime();
    if (!declaration || !lifetime?.entityLive || lifetime.incarnation === null)
      return;
    const world = this.client.worldReference;
    if (!world) throw new Error("GUI controls require an exact World session");
    const target: GuiTarget = Object.freeze({
      world: Object.freeze({ ...world }),
      entity: tracked.entity,
      component: tracked.component,
      incarnation: lifetime.incarnation,
    });
    const session = this.client.session;
    const check = () => {
      if (!this.live(binding)) throw new Error("Control ref is no longer live");
    };
    const binding: ControlBinding = {
      live: true,
      valid: () =>
        this.client.session === session &&
        this.tracked.get(identity) === tracked &&
        tracked.acknowledged.valid(),
      handle: Object.freeze({
        target,
        read: async () => {
          check();
          const page = await this.client.inspectPage!({
            collection: "entities",
            target: target.entity,
            limit: 1,
          });
          check();
          const snapshot = page.entities.find(
            (entity) => entity.id === target.entity,
          );
          const fields =
            snapshot && componentFields(snapshot, target.component);
          if (!fields) throw new Error("Control component is unavailable");
          return fields;
        },
        compareAndSet: async (field, expected, value) => {
          check();
          const descriptor = this.descriptor(target.component)?.fields[field];
          if (!descriptor) throw new Error(`Unknown control field: ${field}`);
          const outcome = await this.client.batch([
            {
              kind: "setFieldIf",
              entity: { kind: "handle", id: target.entity },
              component: target.component,
              field: {
                offset: descriptor.offset,
                value: controlFieldValue(descriptor.kind, value),
              },
              expected: controlFieldValue(descriptor.kind, expected),
            },
          ]);
          if (outcome.ok) return true;
          if (outcome.error.reason === "ValueMismatch") return false;
          throw new Error(
            `Control compare-and-set rejected: ${outcome.error.reason}`,
          );
        },
        action: async (action) => {
          check();
          return this.client.batch([
            {
              kind: "guiAction",
              entity: { kind: "handle", id: target.entity },
              component: target.component,
              incarnation: target.incarnation,
              action,
            },
          ]);
        },
      } satisfies GuiControlHandle),
    };
    this.bindings.set(identity, binding);
    this.assign(identity, binding, declaration.description.controlRef);
    this.seedValues(identity);
  }

  private release(binding: ControlBinding): void {
    const assignment = binding.assignment;
    binding.assignment = undefined;
    try {
      assignment?.release?.();
    } catch (error) {
      this.report(error);
    }
  }

  private assign(
    identity: number,
    binding: ControlBinding,
    ref: GuiControlRef | undefined,
  ): void {
    if (binding.assignment?.ref === ref) return;
    this.release(binding);
    if (
      !ref ||
      this.closed ||
      !binding.live ||
      this.bindings.get(identity) !== binding ||
      this.desired.get(identity)?.description.controlRef !== ref
    )
      return;
    const assignment: RefAssignment = { ref };
    binding.assignment = assignment;
    let release: (() => void) | undefined;
    try {
      if (typeof ref === "function") {
        const cleanup = ref(binding.handle);
        release =
          typeof cleanup === "function"
            ? cleanup
            : () => {
                ref(null);
              };
      } else {
        ref.current = binding.handle;
        release = () => {
          if (ref.current === binding.handle) ref.current = null;
        };
      }
    } catch (error) {
      if (typeof ref === "function")
        release = () => {
          ref(null);
        };
      this.report(error);
    }
    if (release) {
      if (binding.assignment === assignment && binding.live)
        assignment.release = release;
      else {
        try {
          release();
        } catch (error) {
          this.report(error);
        }
      }
    }
  }
}
