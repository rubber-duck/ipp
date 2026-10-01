import type { Response, WorldReference } from "./types.js";
import type {
  GuiEffectSubscription,
  GuiObservationOptions,
  GuiObservationRequest,
  GuiObservedEffect,
  GuiSubscriptionClosure,
  GuiSubscriptionCut,
  GuiSubscriptionId,
} from "./gui-types.js";

interface Adapter {
  nextId(): bigint;
  send(request: bigint, control: GuiObservationRequest): Promise<Response>;
  definitelyUnapplied(error: unknown): boolean;
  fail(error: Error): void;
}

interface Registration {
  world: WorldReference;
  classes: "application" | "feedback" | "all";
  listener: (effect: GuiObservedEffect) => void;
  start?: GuiSubscriptionCut;
  end?: GuiSubscriptionCut;
  ordinal: bigint;
  live: boolean;
  closed: Promise<GuiSubscriptionClosure>;
  finish(closure: GuiSubscriptionClosure): void;
  closing?: Promise<GuiSubscriptionCut>;
}

class GuiObservationRejectedError extends Error {}

/** Pending listeners and active registrations share one bounded live-only identity map. */
export class GuiObservations {
  private controls = new Map<
    bigint,
    { registration: Registration; subscribe: boolean }
  >();
  private active = new Map<string, Registration>();
  private stopped?: Error;

  constructor(private readonly adapter: Adapter) {}

  async subscribe(
    world: WorldReference,
    listener: (effect: GuiObservedEffect) => void,
    options: GuiObservationOptions,
  ): Promise<GuiEffectSubscription> {
    if (this.stopped) throw this.stopped;
    let finish!: (closure: GuiSubscriptionClosure) => void;
    const closed = new Promise<GuiSubscriptionClosure>((resolve) => {
      finish = resolve;
    });
    const registration: Registration = {
      world: immutable({ ...world }),
      classes: options.classes ?? "application",
      listener,
      ordinal: 0n,
      live: false,
      closed,
      finish,
    };
    await this.control(registration, true);
    const start = registration.start;
    if (!start) throw new Error("Missing GUI subscription ACK");
    return Object.freeze({
      id: start.subscription,
      world: registration.world,
      start,
      closed,
      unsubscribe: () => this.unsubscribe(registration),
    });
  }

  private unsubscribe(registration: Registration): Promise<GuiSubscriptionCut> {
    if (registration.end) return Promise.resolve(registration.end);
    if (this.stopped) return Promise.reject(this.stopped);
    if (registration.closing) return registration.closing;
    const operation = this.control(registration, false);
    registration.closing = operation;
    void operation.catch(() => {
      if (registration.closing === operation) delete registration.closing;
    });
    return operation;
  }

  private async control(
    registration: Registration,
    subscribe: boolean,
  ): Promise<GuiSubscriptionCut> {
    const request = this.adapter.nextId();
    this.controls.set(request, { registration, subscribe });
    try {
      const response = await this.adapter.send(
        request,
        subscribe
          ? {
              kind: "subscribe",
              world: registration.world,
              classes: registration.classes,
            }
          : {
              kind: "unsubscribe",
              world: registration.world,
              subscription: registration.start!.subscription,
            },
      );
      if (
        response.body.kind !== "guiObservation" ||
        response.body.record.kind !== "control"
      )
        throw new Error("Invalid GUI subscription correlation");
      const result = response.body.record.result;
      if (result !== (subscribe ? "subscribed" : "unsubscribed"))
        throw new GuiObservationRejectedError(`GUI observation ${result}`);
      return (subscribe ? registration.start : registration.end)!;
    } catch (error) {
      const reason = error instanceof Error ? error : new Error(String(error));
      if (
        !this.adapter.definitelyUnapplied(error) &&
        !(error instanceof GuiObservationRejectedError)
      )
        this.adapter.fail(reason);
      if (subscribe) registration.finish({ kind: "closed", reason });
      throw error;
    } finally {
      this.controls.delete(request);
    }
  }

  receive(response: Response): boolean {
    if (this.stopped) return false;
    const pending = this.controls.get(response.requestId);
    if (response.body.kind !== "guiObservation") {
      if (pending) {
        if (response.body.kind !== "error")
          throw new Error("Invalid GUI subscription correlation");
        if (response.body.code !== 1)
          throw new Error(
            `Host ${response.body.code}: ${response.body.message}`,
          );
        this.controls.delete(response.requestId);
      }
      return false;
    }
    if (response.tick !== 0n)
      throw new Error("GUI observation is not a completed frame");
    const record = response.body.record;
    if (record.kind === "control") {
      if (!pending || response.requestId === 0n)
        throw new Error("Unexpected GUI subscription marker");
      const { registration, subscribe } = pending;
      if (
        !sameWorld(record.world, registration.world) ||
        record.subscription.output <= 0n ||
        record.subscription.generation <= 0n
      )
        throw new Error("Invalid GUI subscription identity");
      if (
        !subscribe &&
        key(record.subscription) !== key(registration.start!.subscription)
      )
        throw new Error("Foreign GUI unsubscribe marker");
      if (record.result === "subscribed" || record.result === "unsubscribed") {
        if ((record.result === "subscribed") !== subscribe)
          throw new Error("Invalid GUI subscription cut");
        const cut: GuiSubscriptionCut = immutable({
          world: { ...record.world },
          subscription: { ...record.subscription },
          session: response.session,
          request: response.requestId,
          kind: record.result,
        });
        if (subscribe) {
          if (this.active.has(key(record.subscription)))
            throw new Error("Duplicate GUI subscription identity");
          registration.start = cut;
          registration.live = true;
          this.active.set(key(record.subscription), registration);
        } else {
          registration.live = false;
          registration.end = cut;
          this.active.delete(key(record.subscription));
          registration.finish({ kind: "unsubscribed", cut });
        }
      }
      this.controls.delete(response.requestId);
      return false;
    }
    if (response.requestId !== 0n)
      throw new Error("Correlated GUI observation effect");
    const registration = this.active.get(key(record.subscription));
    const effect = record.effect;
    if (
      !registration?.live ||
      !effect.id ||
      !sameWorld(effect.id.world, registration.world) ||
      !sameWorld(effect.target.world, registration.world) ||
      effect.id.ordinal <= registration.ordinal
    )
      throw new Error("Invalid GUI observation provenance or ordinal");
    const application =
      effect.effect.kind === "pressed" || effect.effect.kind === "submitted";
    if (
      registration.classes !== "all" &&
      (registration.classes === "application") !== application
    )
      throw new Error("GUI observation class mismatch");
    registration.ordinal = effect.id.ordinal;
    try {
      registration.listener(immutable(effect));
    } catch (error) {
      try {
        globalThis.reportError?.(error);
      } catch {}
    }
    return true;
  }

  close(reason: Error): void {
    if (this.stopped) return;
    this.stopped = reason;
    const registrations = new Set([
      ...this.active.values(),
      ...[...this.controls.values()].map((control) => control.registration),
    ]);
    this.active.clear();
    this.controls.clear();
    for (const registration of registrations) {
      registration.live = false;
      registration.finish({ kind: "closed", reason });
    }
  }
}

function key(id: GuiSubscriptionId): string {
  return `${id.output}:${id.generation}`;
}

function sameWorld(left: WorldReference, right: WorldReference): boolean {
  return left.id === right.id && left.incarnation === right.incarnation;
}

function immutable<Value>(value: Value): Value {
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value)) immutable(child);
    Object.freeze(value);
  }
  return value;
}
