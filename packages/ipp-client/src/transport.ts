import { PortPresentation, type Presentation } from "./presentation.js";

/** Complete binary messages; transports own their connection and shutdown. */
export interface TransportEvents {
  ready(): void;
  message(bytes: Uint8Array): void;
  error(error: Error): void;
  closed(): void;
}

export interface MessageTransport {
  start(events: TransportEvents): void;
  send(bytes: Uint8Array<ArrayBuffer>): void;
  sendParts?(parts: Uint8Array<ArrayBuffer>[]): void;
  readonly presentation?: Presentation;
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

/** The envelope is host lifecycle only; data contains the unchanged IPP wire. */
export class PortTransport implements MessageTransport {
  readonly presentation?: PortPresentation;
  private events: TransportEvents | undefined;
  private closing: Promise<void> | undefined;
  private finishClose: ((error?: Error) => void) | undefined;
  private stopped = false;
  private ready = false;

  constructor(
    private readonly port: MessagePort,
    private readonly dispose: () => void = () => {},
    hasPresentation = false,
    private readonly closeTimeoutMs = 10_000,
  ) {
    if (
      !Number.isFinite(closeTimeoutMs) ||
      closeTimeoutMs <= 0 ||
      closeTimeoutMs > 60_000
    )
      throw new RangeError("closeTimeoutMs must be in (0, 60000]");
    if (hasPresentation) {
      this.presentation = new PortPresentation((message) => {
        this.requireReady();
        this.port.postMessage(message);
      });
    }
  }

  start(events: TransportEvents): void {
    if (this.events) throw new Error("Transport already started");
    this.events = events;
    this.port.onmessageerror = () =>
      this.fail(new Error("Worker message error"));
    this.port.onmessage = (event: MessageEvent<unknown>) => {
      const data = event.data;
      if (typeof data !== "object" || data === null || !("type" in data)) {
        this.fail(new Error("Invalid worker envelope"));
        return;
      }
      try {
        if (this.presentation?.receive(data as Record<string, unknown>)) return;
      } catch (error) {
        this.fail(error instanceof Error ? error : new Error(String(error)));
        return;
      }
      if (data.type === "ready" && !this.ready && !this.closing) {
        this.ready = true;
        events.ready();
      } else if (
        data.type === "data" &&
        "bytes" in data &&
        data.bytes instanceof ArrayBuffer &&
        this.ready &&
        !this.closing
      ) {
        events.message(new Uint8Array(data.bytes));
        // Return one delivery credit after synchronous decoding/dispatch. This
        // bounds worker output even when the receiving thread stops running.
        if (!this.stopped && !this.closing) {
          this.port.postMessage({ type: "ack" });
        }
      } else if (data.type === "closed") {
        this.finish();
        events.closed();
      } else if (
        data.type === "error" &&
        "message" in data &&
        typeof data.message === "string"
      ) {
        this.fail(new Error(data.message));
      } else if (!this.closing) {
        this.fail(new Error("Unexpected worker envelope"));
      }
    };
    this.port.start();
  }

  send(bytes: Uint8Array<ArrayBuffer>): void {
    this.requireReady();
    // Encoding creates an exclusive buffer. Transfer it instead of cloning it.
    const owned =
      bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength
        ? bytes
        : bytes.slice();
    this.port.postMessage({ type: "data", bytes: owned.buffer }, [
      owned.buffer,
    ]);
  }

  sendParts(parts: Uint8Array<ArrayBuffer>[]): void {
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
    this.port.postMessage({ type: "data-parts", parts: owned }, owned);
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
    this.closing = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.finish(new Error("Worker close timed out"));
      }, this.closeTimeoutMs);
      this.finishClose = (error) => {
        clearTimeout(timer);
        if (error) reject(error);
        else resolve();
      };
      this.port.postMessage({ type: "close" });
    });
    return this.closing;
  }

  private finish(error?: Error): void {
    if (this.stopped) return;
    this.stopped = true;
    this.presentation?.close(error ?? new Error("Presentation closed"));
    this.port.onmessage = this.port.onmessageerror = null;
    this.port.close();
    this.dispose();
    this.finishClose?.(error);
  }
}
