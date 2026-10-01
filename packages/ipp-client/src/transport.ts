import type { RenderDiagnostics } from "./presentation.js";

/** Allocates batch identities unique among a connection's open batches. */
export interface BatchIdentitySource {
  allocate(): number;
  /** The identity's final page has been handed to the transport. */
  release(id: number): void;
}

/** Complete binary messages; transports own their connection and shutdown. */
export interface TransportEvents {
  ready(): void;
  message(bytes: Uint8Array): void;
  error(error: Error): void;
  closed(): void;
}

/**
 * Settles when a message that had to wait for flow control goes on the wire;
 * `undefined` means it left immediately. Callers start a message's reply
 * deadline only then, so waiting for credit never times a request out.
 */
export type TransportSend = Promise<void> | undefined;

/**
 * One physical connection. Sends never fail for congestion: messages wait,
 * in call order, until the connection's flow control lets them leave, so one
 * connection-wide window backs every session that shares it. The transport
 * owns the bytes it is given. Wrappers return the wrapped transport's result.
 */
export interface MessageTransport {
  start(events: TransportEvents): void;
  send(bytes: Uint8Array<ArrayBuffer>): TransportSend | void;
  sendParts?(parts: Uint8Array<ArrayBuffer>[]): TransportSend | void;
  /** Batch identities shared by every World session on this connection. */
  readonly batchIdentities?: BatchIdentitySource;
  readonly renderDiagnostics?: RenderDiagnostics;
  close(): Promise<void>;
}

export function webSocketTransport(url: string): MessageTransport {
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";
  let closing: Promise<void> | undefined;

  return {
    start(events) {
      socket.onopen = () => events.ready();
      socket.onmessage = (event: MessageEvent<unknown>) => {
        if (event.data instanceof ArrayBuffer) {
          events.message(new Uint8Array(event.data));
        } else {
          events.error(new Error("Expected a binary WebSocket message"));
        }
      };
      socket.onerror = () => events.error(new Error("WebSocket error"));
      socket.onclose = () => events.closed();
    },
    send(bytes) {
      if (socket.readyState !== WebSocket.OPEN) {
        throw new Error("WebSocket is closed");
      }
      socket.send(bytes);
    },
    close() {
      if (closing) return closing;
      if (socket.readyState === WebSocket.CLOSED) return Promise.resolve();
      closing = new Promise<void>((resolve, reject) => {
        const cleanup = () => {
          clearTimeout(timer);
          socket.removeEventListener("close", done);
          socket.onopen =
            socket.onmessage =
            socket.onerror =
            socket.onclose =
              null;
        };
        const done = () => {
          cleanup();
          resolve();
        };
        const timer = setTimeout(() => {
          cleanup();
          reject(new Error("WebSocket close timed out"));
        }, 1_000);
        socket.addEventListener("close", done, { once: true });
        socket.close();
      });
      return closing;
    },
  };
}

/**
 * The envelope is host lifecycle and flow control only; data contains the
 * unchanged IPP wire. The worker grants an ingress credit window with its ready
 * envelope and returns credit as its Host admits messages, so while the Host
 * throttles this connection, messages wait here instead of in the worker.
 */
export class PortTransport implements MessageTransport {
  readonly renderDiagnostics?: RenderDiagnostics;
  private events: TransportEvents | undefined;
  private closing: Promise<void> | undefined;
  private finishClose: ((error?: Error) => void) | undefined;
  private stopped = false;
  private ready = false;
  private lastDelivery = 0n;
  /** The worker's whole window, and the part of it not held by sent messages. */
  private window = { messages: 0, bytes: 0 };
  private readonly credit = { messages: 0, bytes: 0 };
  private readonly waiting: {
    envelope: Record<string, unknown>;
    transfer: ArrayBuffer[];
    bytes: number;
    sent: () => void;
  }[] = [];
  /** Fails the connection when waiting messages see no credit for too long. */
  private stall: ReturnType<typeof setTimeout> | undefined;

  constructor(
    private readonly port: MessagePort,
    private readonly connection: bigint,
    private readonly dispose: () => void = () => {},
    renderDiagnostics?: RenderDiagnostics,
    private readonly closeTimeoutMs = 10_000,
    /**
     * How long messages may wait while the worker returns no credit before the
     * connection fails. It matches the worker's own 30-second delivery-progress
     * deadline: a Host that admits nothing from a connection with input waiting
     * for that long is stalled, and waiting requests have no reply deadline yet.
     */
    private readonly ingressProgressMs = 30_000,
  ) {
    for (const [name, value] of [
      ["closeTimeoutMs", closeTimeoutMs],
      ["ingressProgressMs", ingressProgressMs],
    ] as const)
      if (!Number.isFinite(value) || value <= 0 || value > 60_000)
        throw new RangeError(`${name} must be in (0, 60000]`);
    if (connection <= 0n)
      throw new RangeError("Worker connection must be nonzero");
    if (renderDiagnostics) this.renderDiagnostics = renderDiagnostics;
  }

  start(events: TransportEvents): void {
    if (this.events) throw new Error("Transport already started");
    if (this.stopped || this.closing) throw new Error("Worker port is closed");
    this.events = events;
    this.listen();
  }

  private listen(): void {
    this.port.onmessageerror = () =>
      this.fail(new Error("Worker message error"));
    this.port.onmessage = (event: MessageEvent<unknown>) => {
      if (this.stopped) return;
      const data = event.data;
      if (
        typeof data !== "object" ||
        data === null ||
        !("type" in data) ||
        !("connection" in data) ||
        data.connection !== this.connection
      ) {
        this.fail(new Error("Invalid worker envelope"));
        return;
      }
      if (data.type === "ready" && !this.ready) {
        const credit = "credit" in data ? creditOf(data.credit) : undefined;
        if (!credit || credit.messages === 0 || credit.bytes === 0) {
          this.fail(new Error("Worker ready envelope has no ingress credit"));
          return;
        }
        this.window = credit;
        Object.assign(this.credit, credit);
        this.ready = true;
        if (!this.closing) this.events?.ready();
      } else if (data.type === "credit" && this.ready) {
        // Credit racing with closure belongs to input that closure discards.
        if (this.closing) return;
        const credit = creditOf(data);
        if (
          !credit ||
          this.credit.messages + credit.messages > this.window.messages ||
          this.credit.bytes + credit.bytes > this.window.bytes
        ) {
          this.fail(new Error("Worker returned credit it had not granted"));
          return;
        }
        this.credit.messages += credit.messages;
        this.credit.bytes += credit.bytes;
        clearTimeout(this.stall);
        this.stall = undefined;
        this.flush();
      } else if (
        data.type === "data" &&
        "bytes" in data &&
        data.bytes instanceof ArrayBuffer &&
        "delivery" in data &&
        typeof data.delivery === "bigint" &&
        data.delivery > this.lastDelivery &&
        this.ready
      ) {
        this.lastDelivery = data.delivery;
        try {
          if (!this.closing) this.events?.message(new Uint8Array(data.bytes));
        } catch (error) {
          this.fail(error instanceof Error ? error : new Error(String(error)));
        } finally {
          if (!this.stopped) {
            try {
              this.port.postMessage({
                type: "ack",
                connection: this.connection,
                delivery: data.delivery,
              });
            } catch (error) {
              this.fail(
                error instanceof Error ? error : new Error(String(error)),
              );
            }
          }
        }
      } else if (data.type === "closed") {
        this.finish();
        this.events?.closed();
      } else if (
        data.type === "error" &&
        "message" in data &&
        typeof data.message === "string"
      ) {
        this.fail(new Error(data.message));
      } else {
        this.fail(new Error("Unexpected worker envelope"));
      }
    };
    this.port.start();
  }

  send(bytes: Uint8Array<ArrayBuffer>): TransportSend {
    this.requireReady();
    // Encoding creates an exclusive buffer. Transfer it instead of cloning it.
    const owned =
      bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
        ? bytes
        : bytes.slice();
    return this.enqueue(
      { type: "data", connection: this.connection, bytes: owned.buffer },
      [owned.buffer],
      owned.byteLength,
    );
  }

  sendParts(parts: Uint8Array<ArrayBuffer>[]): TransportSend {
    this.requireReady();
    if (parts.length === 0 || parts.length > 2)
      throw new RangeError("Expected one or two message parts");
    const owned = parts.map((part) =>
      part.byteOffset === 0 && part.byteLength === part.buffer.byteLength
        ? part.buffer
        : part.slice().buffer,
    );
    if (new Set(owned).size !== owned.length)
      throw new Error("Message parts must have separate owners");
    return this.enqueue(
      { type: "data-parts", connection: this.connection, parts: owned },
      owned,
      owned.reduce((total, part) => total + part.byteLength, 0),
    );
  }

  /** Messages leave in call order, each once the worker's credit covers it. */
  private enqueue(
    envelope: Record<string, unknown>,
    transfer: ArrayBuffer[],
    bytes: number,
  ): TransportSend {
    if (bytes > this.window.bytes)
      throw new RangeError(
        "Message exceeds the worker's ingress credit window",
      );
    if (
      this.waiting.length === 0 &&
      this.credit.messages > 0 &&
      bytes <= this.credit.bytes
    ) {
      this.credit.messages--;
      this.credit.bytes -= bytes;
      this.port.postMessage(envelope, transfer);
      return undefined;
    }
    // A message discarded by closure never settles; its connection has ended.
    const leaving = new Promise<void>((sent) => {
      this.waiting.push({ envelope, transfer, bytes, sent });
    });
    this.armStall();
    return leaving;
  }

  private flush(): void {
    while (this.waiting.length > 0 && !this.stopped && !this.closing) {
      const next = this.waiting[0]!;
      if (this.credit.messages === 0 || next.bytes > this.credit.bytes) break;
      this.waiting.shift();
      this.credit.messages--;
      this.credit.bytes -= next.bytes;
      try {
        this.port.postMessage(next.envelope, next.transfer);
        next.sent();
      } catch (error) {
        this.fail(error instanceof Error ? error : new Error(String(error)));
      }
    }
    // Messages still waiting after credit returned get a fresh deadline.
    if (this.waiting.length > 0 && !this.stopped && !this.closing)
      this.armStall();
  }

  private armStall(): void {
    this.stall ??= setTimeout(
      () =>
        this.fail(
          new Error(
            `connection congestion: no ingress credit returned for ${this.ingressProgressMs / 1000} seconds`,
          ),
        ),
      this.ingressProgressMs,
    );
  }

  /**
   * The worker discards input it has not passed to the Host when a connection
   * closes, so waiting messages end the same way.
   */
  private discardWaiting(): void {
    this.waiting.length = 0;
    clearTimeout(this.stall);
    this.stall = undefined;
  }

  private requireReady(): void {
    if (!this.ready || this.stopped || this.closing)
      throw new Error("Worker port is closed");
  }

  fail(error: Error): void {
    if (this.stopped) return;
    this.finish(error);
    this.events?.error(error);
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    if (this.stopped) return Promise.resolve();
    if (!this.events) this.listen();
    this.discardWaiting();
    this.closing = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.finish(new Error("Worker close timed out"));
      }, this.closeTimeoutMs);
      this.finishClose = (error) => {
        clearTimeout(timer);
        if (error) reject(error);
        else resolve();
      };
    });
    try {
      this.port.postMessage({ type: "close", connection: this.connection });
    } catch (error) {
      this.finish(error instanceof Error ? error : new Error(String(error)));
    }
    return this.closing;
  }

  private finish(error?: Error): void {
    if (this.stopped) return;
    this.stopped = true;
    this.discardWaiting();
    this.port.onmessage = this.port.onmessageerror = null;
    this.port.close();
    this.dispose();
    this.finishClose?.(error);
  }
}

function creditOf(
  value: unknown,
): { messages: number; bytes: number } | undefined {
  if (
    typeof value !== "object" ||
    value === null ||
    !("messages" in value) ||
    !("bytes" in value) ||
    !Number.isSafeInteger(value.messages) ||
    !Number.isSafeInteger(value.bytes) ||
    (value.messages as number) < 0 ||
    (value.bytes as number) < 0
  )
    return undefined;
  return { messages: value.messages as number, bytes: value.bytes as number };
}
