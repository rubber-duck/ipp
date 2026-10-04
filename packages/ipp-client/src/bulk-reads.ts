/** Schema-independent output reads shared by bootstrap and generated clients. */
import { HostWireReader, HostWireWriter } from "./host-protocol.js";

const requestMagic = Uint8Array.of(0x49, 0x50, 0x44, 0x52);
const responseMagic = Uint8Array.of(0x49, 0x50, 0x44, 0x53);
export const BULK_CHUNK_BYTES = 65_536;
export const BULK_PIPELINE_CHUNKS = 8;

/** Exact authority to one immutable output on one connection incarnation. */
export interface BulkReadReference {
  readonly connection: bigint;
  readonly read: bigint;
}

export interface BulkReadDescriptor {
  readonly reference: BulkReadReference;
  readonly length?: bigint | undefined;
}

export interface BulkReadOptions {
  readonly signal?: AbortSignal | undefined;
  /** Local allocation policy for readAll; streaming does not impose a total limit. */
  readonly maxBytes?: number;
}

interface Chunk {
  readonly offset: bigint;
  readonly eof: boolean;
  readonly bytes: Uint8Array<ArrayBuffer>;
}

interface Waiter {
  readonly reference: BulkReadReference;
  readonly resolve: (reader: HostWireReader) => void;
  readonly reject: (error: Error) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

export function readBulkReference(reader: HostWireReader): BulkReadReference {
  const reference = { connection: reader.u64(), read: reader.u64() };
  if (reference.connection === 0n || reference.read === 0n)
    throw new Error("Invalid bulk read reference");
  return reference;
}

/** One physical connection's shared reader and correlation owner. */
export class BulkReadClient {
  private next = 1n;
  private readonly pending = new Map<bigint, Waiter>();
  private stopped?: Error;

  constructor(
    private readonly connection: () => bigint,
    private readonly send: (bytes: Uint8Array<ArrayBuffer>) => void,
    private readonly timeoutMs = 10_000,
  ) {}

  receive(bytes: Uint8Array): boolean {
    if (!responseMagic.every((value, at) => bytes[at] === value)) return false;
    const reader = new HostWireReader(bytes);
    reader.raw(4);
    if (reader.u64() !== this.connection())
      throw new Error("Bulk reply connection mismatch");
    const id = reader.u64();
    const read = reader.u64();
    if (id === 0n) {
      if (reader.u8() !== 2) throw new Error("Invalid bulk revocation notice");
      const reason = new Error(reader.string());
      reader.end();
      for (const [key, pending] of this.pending)
        if (pending.reference.read === read) {
          this.pending.delete(key);
          clearTimeout(pending.timer);
          pending.reject(reason);
        }
      return true;
    }
    const pending = this.pending.get(id);
    // Release and cancellation can settle a caller before queued delivery arrives.
    if (!pending) return true;
    if (pending.reference.read !== read)
      throw new Error("Bulk reply read identity mismatch");
    this.pending.delete(id);
    clearTimeout(pending.timer);
    if (bytes[28] === 2) {
      reader.u8();
      const error = new Error(reader.string());
      reader.end();
      pending.reject(error);
    } else pending.resolve(reader);
    return true;
  }

  close(error: Error): void {
    this.stopped = error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }

  private request(
    reference: BulkReadReference,
    operation: number,
    encode?: (writer: HostWireWriter) => void,
  ): Promise<HostWireReader> {
    if (this.stopped) return Promise.reject(this.stopped);
    if (reference.connection !== this.connection() || reference.read === 0n)
      return Promise.reject(
        new Error("Bulk reference belongs to another connection"),
      );
    const id = this.next++;
    const writer = new HostWireWriter();
    writer.raw(requestMagic);
    writer.u64(reference.connection);
    writer.u64(id);
    writer.u64(reference.read);
    writer.u8(operation);
    encode?.(writer);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("Bulk request timed out"));
      }, this.timeoutMs);
      this.pending.set(id, { reference, resolve, reject, timer });
      try {
        this.send(writer.finish());
      } catch (error) {
        this.pending.delete(id);
        clearTimeout(timer);
        reject(error);
      }
    });
  }

  async release(reference: BulkReadReference): Promise<void> {
    const reader = await this.request(reference, 2);
    if (reader.u8() !== 1) throw new Error("Invalid bulk release reply");
    reader.end();
  }

  private async acknowledge(
    reference: BulkReadReference,
    consumed: bigint,
    eof: boolean,
  ): Promise<void> {
    const reader = await this.request(reference, 1, (writer) => {
      writer.u64(consumed);
      writer.u8(eof ? 1 : 0);
    });
    if (reader.u8() !== 1)
      throw new Error("Invalid bulk acknowledgement reply");
    reader.end();
  }

  private async chunk(
    reference: BulkReadReference,
    offset: bigint,
  ): Promise<Chunk> {
    const reader = await this.request(reference, 0, (writer) =>
      writer.u64(offset),
    );
    if (reader.u8() !== 0 || reader.u64() !== offset)
      throw new Error("Invalid bulk chunk offset");
    const final = reader.u8();
    if (final > 1) throw new Error("Invalid bulk EOF marker");
    const bytes = reader.bytes().slice();
    reader.end();
    if (
      bytes.length > BULK_CHUNK_BYTES ||
      (!final && bytes.length !== BULK_CHUNK_BYTES)
    )
      throw new Error("Invalid bulk chunk progress");
    return { offset, eof: final === 1, bytes };
  }

  /** Each next() consumes the previous yielded window. Returning releases unread data. */
  async *chunks(
    descriptor: BulkReadDescriptor,
    options: BulkReadOptions = {},
  ): AsyncGenerator<Uint8Array<ArrayBuffer>> {
    let consumed = 0n;
    let complete = false;
    const abort = () => {
      void this.release(descriptor.reference).catch(() => {});
    };
    options.signal?.addEventListener("abort", abort, { once: true });
    try {
      while (!complete) {
        options.signal?.throwIfAborted();
        const length = descriptor.length;
        const count =
          length === undefined
            ? 1
            : Math.max(
                1,
                Math.min(
                  BULK_PIPELINE_CHUNKS,
                  Number(
                    (length - consumed + BigInt(BULK_CHUNK_BYTES - 1)) /
                      BigInt(BULK_CHUNK_BYTES),
                  ),
                ),
              );
        const results = await Promise.allSettled(
          Array.from({ length: count }, (_, index) =>
            this.chunk(
              descriptor.reference,
              consumed + BigInt(index * BULK_CHUNK_BYTES),
            ),
          ),
        );
        for (const result of results) {
          if (result.status === "rejected") throw result.reason;
          const chunk = result.value;
          options.signal?.throwIfAborted();
          if (chunk.offset !== consumed)
            throw new Error("Bulk chunks are out of order");
          const next = consumed + BigInt(chunk.bytes.length);
          if (
            length !== undefined &&
            (next > length || (chunk.eof && next !== length))
          )
            throw new Error("Bulk length changed");
          yield chunk.bytes;
          consumed = next;
          await this.acknowledge(descriptor.reference, consumed, chunk.eof);
          complete = chunk.eof;
        }
      }
    } finally {
      options.signal?.removeEventListener("abort", abort);
      if (!complete) await this.release(descriptor.reference).catch(() => {});
    }
  }

  async readAll(
    descriptor: BulkReadDescriptor,
    options: BulkReadOptions = {},
  ): Promise<Uint8Array<ArrayBuffer>> {
    const max = options.maxBytes ?? 64 * 1024 * 1024;
    if (!Number.isSafeInteger(max) || max < 0)
      throw new Error("Invalid bulk allocation limit");
    if (
      descriptor.length !== undefined &&
      (descriptor.length < 0n || descriptor.length > BigInt(max))
    ) {
      await this.release(descriptor.reference);
      throw new Error("Bulk output exceeds byte budget");
    }
    const output =
      descriptor.length === undefined
        ? undefined
        : new Uint8Array(Number(descriptor.length));
    const chunks: Uint8Array<ArrayBuffer>[] = [];
    let offset = 0;
    for await (const bytes of this.chunks(descriptor, options)) {
      if (bytes.length > max - offset)
        throw new Error("Bulk output exceeds byte budget");
      if (output) output.set(bytes, offset);
      else chunks.push(bytes);
      offset += bytes.length;
    }
    if (output) return output;
    const result = new Uint8Array(offset);
    let at = 0;
    for (const chunk of chunks) {
      result.set(chunk, at);
      at += chunk.length;
    }
    return result;
  }
}
