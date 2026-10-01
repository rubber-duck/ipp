import type {
  GuiEffectSubscription,
  GuiObservedEffect,
  GuiTarget,
} from "@ipp/client";
import type { ReactWorldClient } from "./contract.js";
import type { ControlValueDelivery, ReactControlRefs } from "./control_refs.js";
import type {
  ReactComponentDescription,
  ReactEntityDescription,
  ReactWorldDescription,
} from "./tree.js";
import {
  controlValueCallback,
  controlValueCallbackNames,
  controlValues,
  dispatchGuiAction,
  dispatchGuiEffect,
  invokeControlValue,
  type GuiControlValue,
  type GuiControlValues,
  type GuiPropagationStep,
} from "./gui/callbacks.js";

interface AcknowledgedEntity {
  description: ReactEntityDescription;
  entity: bigint;
}

/** Delivery state of one control's value registration. */
interface ValueDeliveries {
  readonly registration: object;
  /** The last delivered value key of each callback name, and of `action`. */
  readonly keys: Map<string, string>;
}

/** The delivered-state name of value propagation to action listeners. */
const ACTION = "action";

function valueKey(value: GuiControlValue): string {
  return value.kind === "scroll"
    ? [
        value.value.offset[0],
        value.value.offset[1],
        value.value.anchorIndex,
        value.value.anchorOffset,
      ].join(" ")
    : String(value.value);
}

function rangeKey(range: NonNullable<GuiControlValues["range"]>): string {
  return [
    ...range.viewport,
    ...range.content,
    ...range.capacity,
    range.itemCount,
    range.first,
    range.last,
  ].join(" ");
}

/**
 * Application callbacks of one root's GUI declarations. Momentary effects
 * (press, submit) come from the GUI effect subscription and propagate along
 * their runtime ancestry; control values come from the value records of
 * `ReactControlRefs` and propagate through React's Entity declarations. All
 * dispatch runs through the root's ordered callback queue.
 */
export class ReactGuiCallbacks {
  private desired: ReactWorldDescription | undefined;
  /** Acknowledged Entity declarations by entity handle. */
  private entities = new Map<bigint, AcknowledgedEntity[]>();
  /** Acknowledged Entity declarations by declaration identity. */
  private declarations = new Map<number, AcknowledgedEntity>();
  private desiredEntities = new Map<number, ReactEntityDescription>();
  private desiredControls = new Map<number, ReactComponentDescription>();
  /** Registered value callback names of each value-observing control. */
  private registered = new Map<number, ReadonlySet<string>>();
  private deliveries = new Map<number, ValueDeliveries>();
  private subscription: Promise<GuiEffectSubscription> | undefined;
  private stopped = false;
  private failure: Error | undefined;
  /** The root's latest commit failed; no registration is published. */
  private suspended = false;
  private readonly session: bigint;

  constructor(
    private readonly client: ReactWorldClient,
    private readonly controls: ReactControlRefs,
    private readonly schedule: (work: () => Promise<void>) => Promise<void>,
    private readonly report: (error: unknown) => unknown,
  ) {
    this.session = client.session;
    controls.observeValues((delivery) => {
      if (!this.usable()) return;
      void this.schedule(async () => this.deliverValues(delivery)).catch(
        () => {},
      );
    });
  }

  setDesired(description: ReactWorldDescription): void {
    this.desired = description;
    this.desiredEntities = new Map(
      description.entities.map((entity) => [entity.identity, entity]),
    );
    this.desiredControls = new Map(
      description.components
        .filter((component) => component.control)
        .map((component) => [component.identity, component]),
    );
    const registered = new Map<number, ReadonlySet<string>>();
    const seeds: number[] = [];
    for (const component of description.components) {
      if (!component.control) continue;
      const names = new Set<string>(
        controlValueCallbackNames.filter(
          (name) => component.controlListeners?.[name],
        ),
      );
      if (description.guiActions) names.add(ACTION);
      if (!names.size) continue;
      registered.set(component.identity, names);
      const previous = this.registered.get(component.identity);
      const keys = this.deliveries.get(component.identity)?.keys;
      for (const name of previous ?? [])
        if (!names.has(name)) keys?.delete(name);
      if ([...names].some((name) => !previous?.has(name)))
        seeds.push(component.identity);
    }
    for (const identity of this.deliveries.keys())
      if (!registered.has(identity)) this.deliveries.delete(identity);
    this.registered = registered;
    // A callback registered on an already observed control receives the
    // latest values once.
    for (const identity of seeds) this.controls.seedValues(identity);
  }

  /** Acknowledged entity declarations with known handles, replacing the previous set. */
  acknowledge(entities: Iterable<AcknowledgedEntity>): void {
    this.entities.clear();
    this.declarations.clear();
    for (const entity of entities) {
      const entries = this.entities.get(entity.entity) ?? [];
      entries.push(entity);
      this.entities.set(entity.entity, entries);
      this.declarations.set(entity.description.identity, entity);
    }
  }

  /**
   * The root's latest commit failed: the registrations of its tree are not
   * published, so dispatch reaches no callback until a commit succeeds.
   */
  suspend(): void {
    this.suspended = true;
  }

  /**
   * A commit succeeded and publishes its tree's registrations. Value
   * callbacks receive the latest values they have not seen yet.
   */
  resume(): void {
    if (!this.suspended) return;
    this.suspended = false;
    for (const identity of this.registered.keys())
      this.controls.seedValues(identity);
  }

  needsPreparation(): boolean {
    return (
      !this.stopped &&
      !!this.desired?.guiEffects &&
      (!this.subscription || !!this.failure)
    );
  }

  async prepare(): Promise<void> {
    if (this.stopped || !this.desired?.guiEffects) return;
    if (this.failure) throw this.failure;
    if (!this.subscription) {
      const pending = this.client.subscribeGuiEffects!((effect) => {
        if (!this.effectsLive()) return;
        const controls = this.targetControls(effect.target);
        const ancestors = this.desiredEntities;
        void this.schedule(async () =>
          this.dispatchEffect(effect, controls, ancestors),
        ).catch(() => {});
      });
      this.subscription = pending;
      void pending.then(
        (subscription) => {
          void subscription.closed.then((closure) => {
            if (this.stopped || this.subscription !== pending) return;
            this.failure =
              closure.kind === "closed"
                ? closure.reason
                : new Error("GUI observation ended");
            this.report(this.failure);
          });
        },
        () => {
          if (this.subscription === pending) this.subscription = undefined;
        },
      );
    }
    await this.subscription;
  }

  private usable(): boolean {
    return (
      !this.stopped &&
      !this.client.closure &&
      this.client.session === this.session
    );
  }

  private effectsLive(): boolean {
    return this.usable() && !this.failure && !!this.desired?.guiEffects;
  }

  /**
   * Fire each registered value callback whose value differs from the one it
   * last received from this registration; an absent component clears them.
   */
  private deliverValues(delivery: ControlValueDelivery): void {
    const names = this.registered.get(delivery.identity);
    if (!this.usable() || this.suspended || !names) return;
    const previous = this.deliveries.get(delivery.identity);
    const values = delivery.record.values;
    if (values === null) {
      if (previous?.registration === delivery.registration)
        previous.keys.clear();
      return;
    }
    const control = this.controls.observed(delivery);
    if (!control) return;
    let state = previous;
    if (state?.registration !== delivery.registration) {
      state = { registration: delivery.registration, keys: new Map() };
      this.deliveries.set(delivery.identity, state);
    }
    const keys = state.keys;
    const changed = (name: string, key: string) => {
      if (!names.has(name) || keys.get(name) === key) return false;
      keys.set(name, key);
      return true;
    };
    const current = () => this.controls.observed(delivery);
    const live = () =>
      this.usable() && !this.suspended && current() !== undefined;
    const { target } = control;
    const { tick } = delivery.record;
    const { value, range } = controlValues(control.values, values);
    if (range && changed("onRangeChange", rangeKey(range)) && live()) {
      try {
        current()?.description.controlListeners?.onRangeChange?.(
          Object.freeze({ target, tick, ...range }),
        );
      } catch (error) {
        this.report(error);
      }
    }
    const key = valueKey(value);
    const own = changed(controlValueCallback[value.kind], key);
    const propagate = changed(ACTION, key);
    if (!own && !propagate) return;
    const event = Object.freeze({ target, tick, value });
    dispatchGuiAction(
      { ...event, kind: "value" },
      own
        ? [
            () =>
              invokeControlValue(
                current()?.description.controlListeners,
                event,
              ),
          ]
        : [],
      propagate ? this.declarationPath(control.description.entity) : [],
      live,
      this.report,
    );
  }

  /**
   * The acknowledged Entity declarations enclosing the declaration `entity`,
   * root-most first; declarations without a known handle are skipped.
   */
  private declarationPath(entity: number): GuiPropagationStep[] {
    const path: GuiPropagationStep[] = [];
    let identity: number | undefined = entity;
    while (identity !== undefined) {
      const declaration: number = identity;
      const acknowledged = this.declarations.get(declaration);
      if (acknowledged)
        path.push({
          currentTarget: acknowledged.entity,
          listeners: () => {
            const current = this.desiredEntities.get(declaration);
            return current?.symbolicId === acknowledged.description.symbolicId
              ? [current]
              : [];
          },
        });
      identity = this.desiredEntities.get(declaration)?.parent;
    }
    return path.reverse();
  }

  /**
   * The control declarations of this root that may declare `target`: those
   * whose acknowledged entity and component `target` names, and those of
   * `target`'s component whose entity has no acknowledged handle yet. A peer
   * can act on a control as soon as the Host applied its mount, before this
   * root processed that batch's outcome; dispatch runs after the outcome and
   * keeps only the declarations that then declare the target.
   */
  private targetControls(target: GuiTarget): number[] {
    const identities: number[] = [];
    for (const [identity, control] of this.desiredControls) {
      if (control.component !== target.component) continue;
      const acknowledged = this.declarations.get(control.entity);
      if (!acknowledged || acknowledged.entity === target.entity)
        identities.push(identity);
    }
    return identities;
  }

  private declares(
    control: ReactComponentDescription | undefined,
    target: GuiTarget,
  ): control is ReactComponentDescription {
    return (
      control?.component === target.component &&
      this.declarations.get(control.entity)?.entity === target.entity
    );
  }

  /**
   * Dispatch a press or submission to the control declarations it reached on
   * receipt that declare its target once earlier commits are acknowledged,
   * then along its runtime ancestry.
   */
  private dispatchEffect(
    effect: GuiObservedEffect,
    controls: readonly number[],
    capturedEntities: ReadonlyMap<number, ReactEntityDescription>,
  ): void {
    const declared = (identity: number) => {
      const control = this.desiredControls.get(identity);
      return this.declares(control, effect.target) ? control : undefined;
    };
    const live = () =>
      this.effectsLive() &&
      !this.suspended &&
      controls.some((identity) => declared(identity) !== undefined);
    dispatchGuiEffect(
      effect,
      () =>
        controls.map((identity) => ({
          get onPress() {
            return declared(identity)?.controlListeners?.onPress;
          },
          get onSubmit() {
            return declared(identity)?.controlListeners?.onSubmit;
          },
        })),
      (entity) =>
        (this.entities.get(entity) ?? []).map((acknowledged) => {
          const currentListeners = () => {
            const current = this.desiredEntities.get(
              acknowledged.description.identity,
            );
            const captured = capturedEntities.get(
              acknowledged.description.identity,
            );
            return captured &&
              current?.symbolicId === captured.symbolicId &&
              current.kind === captured.kind &&
              current.symbolicId === acknowledged.description.symbolicId &&
              current.kind === acknowledged.description.kind
              ? current
              : undefined;
          };
          return {
            get onAction() {
              return currentListeners()?.onAction;
            },
            get onActionCapture() {
              return currentListeners()?.onActionCapture;
            },
          };
        }),
      live,
      this.report,
    );
  }

  fence(): void {
    this.stopped = true;
    this.desired = undefined;
  }

  async dispose(): Promise<void> {
    this.fence();
    const pending = this.subscription;
    if (!pending) return;
    const subscription = await pending;
    if (!this.client.closure) await subscription.unsubscribe();
    if (this.subscription === pending) this.subscription = undefined;
  }
}
