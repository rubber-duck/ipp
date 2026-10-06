import type { MessageTransport, TransportEvents } from "@ipp/client";

export class BatchDeliveryGate implements MessageTransport {
  readonly #inner: MessageTransport;
  #events: TransportEvents | undefined;
  #classify: ((bytes: Uint8Array) => string) | undefined;
  #buffer: Uint8Array[] = [];
  #holding = false;
  #bufferedFrames = 0;
  #waiter:
    | { resolve(): void; reject(error: Error): void; timer: number }
    | undefined;

  constructor(inner: MessageTransport) {
    this.#inner = inner;
  }

  get bufferedResponses(): number {
    return this.#buffer.length;
  }

  get bufferedFrames(): number {
    return this.#bufferedFrames;
  }

  start(events: TransportEvents): void {
    if (this.#events !== undefined) throw new Error("gate already started");
    this.#events = events;
    this.#inner.start({
      ready: () => events.ready(),
      message: (bytes) => this.#receive(bytes),
      error: (error) => {
        this.#failWaiter(error);
        events.error(error);
      },
      closed: () => {
        this.#failWaiter(new Error("worker transport closed while gated"));
        events.closed();
      },
    });
  }

  send(bytes: Uint8Array<ArrayBuffer>): void {
    this.#inner.send(bytes);
  }

  close(): Promise<void> {
    this.#failWaiter(new Error("delivery gate closed"));
    return this.#inner.close();
  }

  arm(classify: (bytes: Uint8Array) => string): void {
    if (this.#events === undefined) throw new Error("gate is not ready");
    if (this.#classify !== undefined) throw new Error("gate already armed");
    this.#classify = classify;
  }

  waitForBatchAndFrame(timeoutMs: number): Promise<void> {
    if (this.#waiter !== undefined)
      throw new Error("gate waiter already active");
    if (this.#holding && this.#bufferedFrames > 0) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.#waiter = undefined;
        reject(new Error("timed out waiting for a gated batch and frame"));
      }, timeoutMs);
      this.#waiter = { resolve, reject, timer };
    });
  }

  release(): void {
    const events = this.#events;
    if (events === undefined || this.#classify === undefined) return;
    this.#classify = undefined;
    this.#holding = false;
    const buffered = this.#buffer;
    this.#buffer = [];
    this.#bufferedFrames = 0;
    this.#resolveWaiter();
    for (const bytes of buffered) events.message(bytes);
  }

  #receive(bytes: Uint8Array): void {
    const events = this.#events;
    if (events === undefined) throw new Error("gate received before start");
    const classify = this.#classify;
    if (classify === undefined) {
      events.message(bytes);
      return;
    }

    const kind = classify(bytes);
    if (!this.#holding && kind !== "batch") {
      events.message(bytes);
      return;
    }
    this.#holding = true;
    if (this.#buffer.length >= 64) {
      const error = new Error("delivery gate response limit");
      this.#failWaiter(error);
      events.error(error);
      return;
    }
    this.#buffer.push(bytes.slice());
    if (kind === "frame") this.#bufferedFrames++;
    if (this.#bufferedFrames > 0) this.#resolveWaiter();
  }

  #resolveWaiter(): void {
    const waiter = this.#waiter;
    if (waiter === undefined) return;
    this.#waiter = undefined;
    clearTimeout(waiter.timer);
    waiter.resolve();
  }

  #failWaiter(error: Error): void {
    const waiter = this.#waiter;
    if (waiter === undefined) return;
    this.#waiter = undefined;
    clearTimeout(waiter.timer);
    waiter.reject(error);
  }
}
