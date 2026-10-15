import {
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
} from "react";
import { AttachmentContext } from "../../composition/attached-world.js";

export interface ValueObservation {
  readonly tick: bigint;
  readonly initial?: boolean | undefined;
}

export interface ValueWriteContext<Value> {
  /** A changed control lease, unmount or newer committed intent cancels further writes. */
  isCurrent(): boolean;
  /** Register before dispatch. A mismatching report waits for this conditional write's ACK. */
  trackEcho(
    candidate: Value,
    operation: () => Promise<boolean>,
  ): Promise<boolean>;
}

/** A synchronous decision or completion of asynchronous application acceptance. */
export type ValueChangeHandler<Value> =
  | ((value: Value) => void)
  | ((value: Value) => Promise<void>);

export interface ValueSynchronizationOptions<Value> {
  readonly value: Value | undefined;
  readonly defaultValue: Value;
  readonly onChange: ValueChangeHandler<Value> | undefined;
  readonly same: (left: Value, right: Value) => boolean;
  readonly write: (
    from: Value,
    to: Value,
    context: ValueWriteContext<Value>,
  ) => Promise<void>;
  /** The actual native handle (or handle collection), not an application value. */
  readonly identity?: () => unknown;
  readonly onError?: (error: unknown) => void;
}

interface Echo<Value> {
  readonly value: Value;
  readonly lease: number;
  status: "pending" | "accepted" | "failed";
  seen: boolean;
}

interface Report<Value> {
  readonly value: Value;
  readonly echo?: Echo<Value>;
}

/** Writer bookkeeping and observed UI snapshots; native fields remain authoritative. */
export class ValueSynchronization<Value> {
  readonly initial: Value;
  private options: ValueSynchronizationOptions<Value>;
  private snapshot: {
    readonly value: Value;
    readonly ready: boolean;
    readonly pending: boolean;
  };
  private readonly listeners = new Set<() => void>();
  private readonly defaultIdentity = {};
  private identity: unknown;
  private lease = 0;
  private active = true;
  private tick = -1n;
  private echoes: Echo<Value>[] = [];
  private reports: Report<Value>[] = [];
  private writing: object | undefined;
  private attempted: { from: Value; to: Value; identity: unknown } | undefined;
  private change: object | undefined;
  private readonly changes = new Map<object, Value>();
  private settledCandidate: { value: Value } | undefined;

  constructor(options: ValueSynchronizationOptions<Value>) {
    this.options = options;
    this.initial =
      options.value === undefined ? options.defaultValue : options.value;
    this.snapshot = { value: this.initial, ready: false, pending: false };
    this.identity = this.currentIdentity();
  }

  configure(options: ValueSynchronizationOptions<Value>): void {
    // An explicit replacement outside this burst's accepted candidates wins
    // immediately. An older accepted sample may commit while a newer one waits.
    const replaced =
      this.change &&
      options.value !== undefined &&
      (this.options.value === undefined ||
        !options.same(options.value, this.options.value)) &&
      ![
        ...this.changes.values(),
        ...(this.settledCandidate ? [this.settledCandidate.value] : []),
      ].some((value) => options.same(value, options.value!));
    this.options = options;
    if (replaced) {
      this.change = undefined;
      this.changes.clear();
      this.settledCandidate = undefined;
      this.publish();
    }
  }

  private currentIdentity(): unknown {
    return this.options.identity
      ? this.options.identity()
      : this.defaultIdentity;
  }

  private publish(
    value = this.snapshot.value,
    ready = this.snapshot.ready,
  ): void {
    this.snapshot = { value, ready, pending: this.change !== undefined };
    for (const listener of this.listeners) listener();
  }

  readonly getSnapshot = () => this.snapshot;
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  activate(): void {
    this.active = true;
  }

  deactivate(): void {
    this.active = false;
    this.lease++;
    this.echoes = [];
    this.reports = [];
    this.writing = undefined;
    this.attempted = undefined;
    this.change = undefined;
    this.changes.clear();
    this.settledCandidate = undefined;
  }

  /** Ref publication admits a deferred write and resets only changed native identities. */
  readonly notifyReady = (): void => {
    if (!this.active) return;
    const identity = this.currentIdentity();
    if (identity === this.identity) return;
    const firstBaseline = this.identity == null && this.snapshot.ready;
    this.identity = identity;
    this.lease++;
    this.echoes = [];
    this.reports = [];
    this.writing = undefined;
    this.attempted = undefined;
    this.change = undefined;
    this.changes.clear();
    this.settledCandidate = undefined;
    if (!firstBaseline) this.tick = -1n;
    this.publish(this.snapshot.value, firstBaseline);
  };

  /** A grouped adapter admits a genuinely changed member handle without discarding its aggregate observation. */
  readonly retry = (): void => {
    if (!this.active) return;
    this.attempted = undefined;
    this.publish();
  };

  private drain(): boolean {
    let changed = false;
    while (this.reports.length) {
      const next = this.reports[0]!;
      if (next.echo?.status === "pending") break;
      this.reports.shift();
      if (next.echo?.status !== "accepted") {
        changed = true;
        this.changed(next.value);
      }
    }
    return changed;
  }

  private changed(value: Value): void {
    const change = {};
    const lease = this.lease;
    this.change = change;
    this.changes.set(change, value);
    const finish = () => {
      if (!this.active || this.lease !== lease || !this.changes.delete(change))
        return;
      // A completion can precede its parent render's publication. Retain only
      // that last settled candidate alongside genuinely outstanding callbacks.
      this.settledCandidate = { value };
      if (this.change === change) {
        this.change = undefined;
        this.changes.clear();
        this.settledCandidate = undefined;
        this.publish();
      }
    };
    let completion: void | Promise<void>;
    try {
      completion = this.options.onChange?.(value);
    } catch (error) {
      finish();
      throw error;
    }
    if (!completion || typeof completion.then !== "function") {
      finish();
      return;
    }
    // Do not hold the root callback queue: later native samples must continue
    // to report while the application acknowledges this change asynchronously.
    this.publish();
    void Promise.resolve(completion).then(finish, (error: unknown) => {
      if (this.active && this.lease === lease) this.options.onError?.(error);
      finish();
    });
  }

  /** Every report updates the view, including seeds and successful write echoes. */
  readonly observe = (value: Value, event: ValueObservation): boolean => {
    if (!this.active) return false;
    this.notifyReady();
    if (event.tick < this.tick) return false;
    this.tick = event.tick;
    const changed = !this.options.same(value, this.snapshot.value);
    const index = this.echoes.findIndex((echo) =>
      this.options.same(echo.value, value),
    );
    const echo = index < 0 ? undefined : this.echoes[index];
    if (echo) {
      echo.seen = true;
      // Once a later write is reported, its predecessors cannot be future echoes.
      this.echoes.splice(0, index + 1);
    } else if (changed) {
      // An unrelated native edit supersedes earlier accepted, unreported candidates.
      this.echoes = this.echoes.filter(
        (candidate) => candidate.status === "pending",
      );
    }
    if (changed) this.attempted = undefined;
    if (changed || !this.snapshot.ready) this.publish(value, true);
    if (event.initial || !changed) return false;
    this.reports.push({ value, ...(echo ? { echo } : {}) });
    return this.drain();
  };

  /** Called only after React commits the latest callback and application intent. */
  async synchronize(): Promise<void> {
    if (
      !this.active ||
      this.change ||
      this.writing ||
      !this.snapshot.ready ||
      this.options.value === undefined
    )
      return;
    const from = this.snapshot.value;
    const to = this.options.value;
    const identity = this.currentIdentity();
    if (
      identity === undefined ||
      identity === null ||
      this.options.same(from, to)
    )
      return;
    if (
      this.attempted &&
      this.attempted.identity === identity &&
      this.options.same(this.attempted.from, from) &&
      this.options.same(this.attempted.to, to)
    )
      return;
    this.attempted = { from, to, identity };
    const request = {};
    const lease = this.lease;
    this.writing = request;
    const isCurrent = () =>
      this.active &&
      this.lease === lease &&
      this.currentIdentity() === identity &&
      this.options.value !== undefined &&
      this.options.same(this.options.value, to);
    try {
      await this.options.write(from, to, {
        isCurrent,
        trackEcho: async (candidate, operation) => {
          if (!isCurrent()) return false;
          const echo: Echo<Value> = {
            value: candidate,
            lease,
            status: "pending",
            seen: false,
          };
          this.echoes.push(echo);
          let accepted: boolean;
          try {
            accepted = await operation();
          } catch (error) {
            echo.status = "failed";
            this.echoes = this.echoes.filter((value) => value !== echo);
            if (this.active && this.lease === echo.lease) this.drain();
            throw error;
          }
          echo.status = accepted ? "accepted" : "failed";
          if (!accepted || echo.seen)
            this.echoes = this.echoes.filter((value) => value !== echo);
          if (this.active && this.lease === echo.lease) this.drain();
          return accepted;
        },
      });
    } catch (error) {
      if (this.active && this.lease === lease) this.options.onError?.(error);
      // A refused/stale write changes no observed value. New reports or intent
      // can retry; the same failed expected/desired pair cannot spin.
    } finally {
      if (this.writing === request) {
        this.writing = undefined;
        if (this.active) this.publish();
      }
    }
  }
}

export function useValueSynchronization<Value>(
  options: ValueSynchronizationOptions<Value>,
) {
  const container = useContext(AttachmentContext);
  const configured = {
    ...options,
    onError:
      options.onError ?? ((error: unknown) => container?.commits.report(error)),
  };
  const controller = useRef<ValueSynchronization<Value> | null>(null);
  if (!controller.current)
    controller.current = new ValueSynchronization(configured);
  const synchronization = controller.current;
  const observed = useSyncExternalStore(
    synchronization.subscribe,
    synchronization.getSnapshot,
    synchronization.getSnapshot,
  );
  useLayoutEffect(() => {
    synchronization.configure(configured);
  });
  useLayoutEffect(() => {
    synchronization.activate();
    return () => synchronization.deactivate();
  }, [synchronization]);
  useEffect(() => {
    let current = true;
    // A selecting group reports its members separately through the root's
    // ordered callback queue. Let all already queued reports and their React
    // updates finish before comparing against the application's intent.
    const callbacks =
      container?.commits.callbacksSettled() ?? Promise.resolve();
    void callbacks.then(() => {
      if (current) void synchronization.synchronize();
    });
    return () => {
      current = false;
    };
  });
  return {
    initial: synchronization.initial,
    current:
      options.value === undefined || observed.pending
        ? observed.value
        : options.value,
    observe: synchronization.observe,
    notifyReady: synchronization.notifyReady,
    retry: synchronization.retry,
  };
}
