import type { IngressStatistics } from "./presentation.js";

export interface WorkerConnectionExports {
  memory: WebAssembly.Memory;
  ipp_connection_limit(): number;
  ipp_delivery_limit(): number;
  ipp_request_window(): number;
  ipp_connection_open(connection: bigint): number;
  ipp_connection_close(connection: bigint): number;
  ipp_connection_dispose(connection: bigint): number;
  ipp_connection_failed(connection: bigint): number;
  ipp_connection_pending(connection: bigint): number;
  ipp_connection_poll(connection: bigint): number;
  ipp_output_delivery_id(): bigint;
  ipp_output_copied(connection: bigint, delivery: bigint): number;
  ipp_delivery_complete(connection: bigint, delivery: bigint): number;
  ipp_accepts_input(connection: bigint): number;
  ipp_input_reserve(length: number): number;
  ipp_receive(connection: bigint, length: number): number;
  ipp_output_ptr(): number;
  ipp_output_len(): number;
}

interface Connection {
  id: bigint;
  port: MessagePort;
  closing: boolean;
  notified: boolean;
  failed: boolean;
  lastDelivery: number;
  deliveries: bigint[];
  inputs: { parts: ArrayBuffer[]; length: number; multipart: boolean }[];
  /** Bytes of `inputs`, charged against the connection's ingress credit. */
  inputBytes: number;
}

/** Ingress credit a connection's sender may use before the worker returns some. */
interface IngressCredit {
  readonly messages: number;
  readonly bytes: number;
}

/**
 * Worker memory one connection's waiting messages may hold, in full-size messages of the
 * target contract's budget (8 MiB at the 1 MiB budget): room for 32 full
 * 256 KiB command pages, so paged batches keep streaming while credit returns.
 */
const INGRESS_CREDIT_FULL_MESSAGES = 8;

/**
 * Physical ports share one Host clock; each retains its own completion credit
 * for output and grants its sender one ingress credit window. Senders wait for
 * returned credit; one that exceeds its window fails only its own connection.
 */
export class WorkerConnections {
  private readonly connections = new Map<bigint, Connection>();
  private readonly credit: IngressCredit;
  /** Connections the runtime serves at once (`ipp_connection_limit`). */
  readonly capacity: number;
  /** Uncompleted deliveries per connection (`ipp_delivery_limit`). */
  private readonly deliveryLimit: number;

  constructor(
    private readonly runtime: WorkerConnectionExports,
    private readonly maxMessageBytes: number,
    private readonly ingress?: IngressStatistics,
  ) {
    this.capacity = positiveLimit(runtime.ipp_connection_limit());
    this.deliveryLimit = positiveLimit(runtime.ipp_delivery_limit());
    // Complete messages one connection may have waiting in the worker for the
    // Host. The worker passes them on only while the Host admits the
    // connection's input, so this bounds how far a sender runs ahead of a
    // throttled Host, never what the Host admits. It covers the Host's request
    // admission window, so a resuming Host is refilled from the worker at once
    // while returned credit travels back over the port.
    this.credit = {
      messages: positiveLimit(runtime.ipp_request_window()),
      bytes: INGRESS_CREDIT_FULL_MESSAGES * maxMessageBytes,
    };
  }

  diagnostic(): string {
    return new TextDecoder("utf-8", { fatal: true }).decode(this.borrowed());
  }

  private borrowed(): Uint8Array {
    const length = this.runtime.ipp_output_len() >>> 0;
    if (length > this.maxMessageBytes)
      throw new Error("WASM response exceeds bounds");
    return new Uint8Array(
      this.runtime.memory.buffer,
      this.runtime.ipp_output_ptr() >>> 0,
      length,
    );
  }

  open(id: bigint, port: MessagePort): void {
    if (this.connections.size >= this.capacity || this.connections.has(id)) {
      this.reject(id, port, "Worker connection capacity exhausted");
      return;
    }
    if (this.runtime.ipp_connection_open(id) !== 1) {
      this.reject(id, port, this.diagnostic());
      return;
    }
    const connection: Connection = {
      id,
      port,
      closing: false,
      notified: false,
      failed: false,
      lastDelivery: performance.now(),
      deliveries: [],
      inputs: [],
      inputBytes: 0,
    };
    this.connections.set(id, connection);
    port.onmessageerror = () =>
      this.fail(connection, new Error("Worker message decode failed"));
    port.onmessage = (event: MessageEvent<unknown>) => {
      try {
        const data = event.data;
        if (
          typeof data !== "object" ||
          data === null ||
          !("type" in data) ||
          !("connection" in data) ||
          data.connection !== id
        )
          throw new Error("Foreign worker connection envelope");
        if (data.type === "ack") {
          if (
            !("delivery" in data) ||
            typeof data.delivery !== "bigint" ||
            connection.deliveries[0] !== data.delivery
          )
            throw new Error("Unexpected output delivery acknowledgement");
          if (this.runtime.ipp_delivery_complete(id, data.delivery) !== 1)
            throw new Error(this.diagnostic());
          connection.deliveries.shift();
          connection.lastDelivery = performance.now();
          if (connection.closing) this.finishClose(connection);
        } else if (data.type === "close") {
          this.close(connection);
        } else if (!connection.closing) {
          const parts: ArrayBuffer[] =
            data.type === "data" &&
            "bytes" in data &&
            data.bytes instanceof ArrayBuffer
              ? [data.bytes]
              : data.type === "data-parts" &&
                  "parts" in data &&
                  Array.isArray(data.parts) &&
                  data.parts.length > 0 &&
                  data.parts.length <= 2 &&
                  data.parts.every(
                    (part: unknown) => part instanceof ArrayBuffer,
                  )
                ? data.parts
                : [];
          const length = parts.reduce(
            (total, part) => total + part.byteLength,
            0,
          );
          if (length === 0 || length > this.maxMessageBytes)
            throw new Error("Message exceeds WASM ingress bounds");
          if (
            connection.inputs.length >= this.credit.messages ||
            connection.inputBytes + length > this.credit.bytes
          )
            throw new Error("Sender exceeded its worker ingress credit");
          connection.inputs.push({
            parts,
            length,
            multipart: data.type === "data-parts",
          });
          connection.inputBytes += length;
        }
        this.pumpInputs();
        this.publish();
      } catch (error) {
        this.fail(connection, asError(error));
      }
    };
    try {
      port.start();
      port.postMessage({ type: "ready", connection: id, credit: this.credit });
    } catch (error) {
      this.fail(connection, asError(error));
    }
  }

  private reject(id: bigint, port: MessagePort, message: string): void {
    try {
      port.postMessage({ type: "error", connection: id, message });
    } catch {}
    port.close();
  }

  /** Pass waiting input to the Host while it admits it, returning the credit used. */
  pumpInputs(): void {
    const returned = new Map<Connection, { messages: number; bytes: number }>();
    let progressed = true;
    while (progressed) {
      progressed = false;
      for (const connection of this.connections.values()) {
        if (
          connection.closing ||
          connection.inputs.length === 0 ||
          this.runtime.ipp_accepts_input(connection.id) !== 1
        )
          continue;
        progressed = true;
        try {
          const { parts, length, multipart } = connection.inputs.shift()!;
          connection.inputBytes -= length;
          const credit = returned.get(connection) ?? { messages: 0, bytes: 0 };
          credit.messages++;
          credit.bytes += length;
          returned.set(connection, credit);
          const pointer = this.runtime.ipp_input_reserve(length) >>> 0;
          if (pointer === 0) throw new Error(this.diagnostic());
          const destination = new Uint8Array(
            this.runtime.memory.buffer,
            pointer,
            length,
          );
          let offset = 0;
          for (const part of parts) {
            destination.set(new Uint8Array(part), offset);
            offset += part.byteLength;
          }
          if (this.ingress) {
            this.ingress.messages++;
            this.ingress.wasmCopyBytes += length;
            if (multipart) {
              this.ingress.partsMessages++;
              this.ingress.transferredAssetBytes += parts[1]?.byteLength ?? 0;
            }
          }
          if (this.runtime.ipp_receive(connection.id, length) !== 1)
            throw new Error(this.diagnostic());
        } catch (error) {
          this.fail(connection, asError(error));
        }
      }
    }
    for (const [connection, credit] of returned) {
      if (connection.closing) continue;
      try {
        connection.port.postMessage({
          type: "credit",
          connection: connection.id,
          ...credit,
        });
      } catch (error) {
        this.fail(connection, asError(error));
      }
    }
  }

  publish(): void {
    for (const connection of this.connections.values()) {
      if (connection.closing) continue;
      try {
        while (connection.deliveries.length < this.deliveryLimit) {
          const result = this.runtime.ipp_connection_poll(connection.id);
          if (result === 0) break;
          if (result !== 1) throw new Error(this.diagnostic());
          const delivery = this.runtime.ipp_output_delivery_id();
          if (delivery === 0n)
            throw new Error("Missing output delivery identity");
          const bytes = this.borrowed().slice();
          if (this.runtime.ipp_output_copied(connection.id, delivery) !== 1)
            throw new Error(this.diagnostic());
          if (connection.deliveries.length === 0)
            connection.lastDelivery = performance.now();
          connection.deliveries.push(delivery);
          connection.port.postMessage(
            {
              type: "data",
              connection: connection.id,
              delivery,
              bytes: bytes.buffer,
            },
            [bytes.buffer],
          );
        }
      } catch (error) {
        this.fail(connection, asError(error));
      }
    }
  }

  maintain(now: number): void {
    for (const connection of this.connections.values()) {
      if (connection.closing) continue;
      if (this.runtime.ipp_connection_failed(connection.id) === 1) {
        this.fail(connection, new Error(this.diagnostic()));
      } else if (
        connection.deliveries.length > 0 &&
        now - connection.lastDelivery >= 30_000
      ) {
        this.fail(
          connection,
          new Error(
            "connection congestion: no delivery progress for 30 seconds",
          ),
        );
      }
    }
  }

  private close(connection: Connection): void {
    if (!connection.closing) {
      connection.closing = true;
      connection.inputs.length = 0;
      connection.inputBytes = 0;
      this.runtime.ipp_connection_close(connection.id);
    }
    this.finishClose(connection);
  }

  private finishClose(connection: Connection): void {
    if (
      connection.notified ||
      this.runtime.ipp_connection_pending(connection.id) !== 0
    )
      return;
    connection.notified = true;
    connection.port.postMessage({ type: "closed", connection: connection.id });
  }

  private fail(connection: Connection, error: Error): void {
    if (connection.failed) return;
    connection.failed = true;
    connection.closing = true;
    connection.inputs.length = 0;
    connection.inputBytes = 0;
    this.runtime.ipp_connection_close(connection.id);
    try {
      connection.port.postMessage({
        type: "error",
        connection: connection.id,
        message: error.message.slice(0, 1024),
      });
    } catch {}
  }

  /** Only the owning adapter may confirm that its receiving endpoint is disposed. */
  dispose(id: bigint): void {
    const connection = this.connections.get(id);
    if (!connection) return;
    connection.port.onmessage = connection.port.onmessageerror = null;
    connection.port.close();
    this.runtime.ipp_connection_dispose(id);
    this.connections.delete(id);
  }

  failAll(error: Error): void {
    for (const connection of this.connections.values())
      this.fail(connection, error);
  }

  get size(): number {
    return this.connections.size;
  }
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/** A runtime-declared bound, which must be a positive integer. */
function positiveLimit(value: number): number {
  const limit = value >>> 0;
  if (limit === 0) throw new Error("WASM runtime declares an invalid limit");
  return limit;
}
