interface Request<T> {
  value: T;
  promise: Promise<void>;
  resolve(): void;
  reject(error: unknown): void;
}

/** One running action and one replaceable/coalesced request, shared by its callers. */
export class ChartActionLane<T> {
  private active: Request<T> | undefined;
  private queued: Request<T> | undefined;
  private drain = Promise.resolve();
  private submitted = 0;
  private executed = 0;
  private coalesced = 0;
  private dropped = 0;
  private maxInFlight = 0;
  private maxPending = 0;

  constructor(
    private readonly execute: (value: T) => Promise<void>,
    private readonly combine: (previous: T, next: T) => T = (_, next) => next,
  ) {}

  submit(value: T): Promise<void> {
    ++this.submitted;
    if (this.queued) {
      this.queued.value = this.combine(this.queued.value, value);
      ++this.coalesced;
      return this.queued.promise;
    }
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<void>((yes, no) => {
      resolve = yes;
      reject = no;
    });
    const request = { value, promise, resolve, reject };
    if (this.active) {
      this.queued = request;
      this.maxPending = 1;
    } else {
      this.active = request;
      this.maxInFlight = 1;
      this.drain = this.run();
    }
    return promise;
  }

  drop(predicate: (value: T) => boolean = () => true) {
    if (!this.queued || !predicate(this.queued.value)) return;
    this.queued.resolve();
    this.queued = undefined;
    ++this.dropped;
  }

  async settled() {
    await this.drain;
  }

  snapshot() {
    return {
      inFlight: this.active ? 1 : 0,
      pending: this.queued ? 1 : 0,
      submitted: this.submitted,
      executed: this.executed,
      coalesced: this.coalesced,
      dropped: this.dropped,
      maxInFlight: this.maxInFlight,
      maxPending: this.maxPending,
    };
  }

  private async run() {
    while (this.active) {
      const request = this.active;
      ++this.executed;
      try {
        await this.execute(request.value);
        request.resolve();
      } catch (error) {
        request.reject(error);
      }
      this.active = this.queued;
      this.queued = undefined;
    }
  }
}
