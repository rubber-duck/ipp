import { HostWireReader, HostWireWriter } from "./host-protocol.js";
import type { ClientAssetSource } from "./types.js";

const REQUEST_MAGIC = Uint8Array.of(73, 80, 65, 83);
const RESPONSE_MAGIC = Uint8Array.of(73, 80, 65, 82);
const CHUNK_BYTES = 64 * 1024;
const IN_FLIGHT_CHUNKS = 8;

export function isAssetSourceResponse(bytes: Uint8Array): boolean {
  return (
    bytes.byteLength >= RESPONSE_MAGIC.byteLength &&
    RESPONSE_MAGIC.every((value, index) => bytes[index] === value)
  );
}

/** An immutable name in a client's provider namespace; no asset bytes enter commands. */
export function clientAssetSource(
  session: bigint,
  kind: number,
  name: string | bigint,
  variant = 0,
): ClientAssetSource {
  if (session <= 0n || session > 0xffff_ffff_ffff_ffffn)
    throw new RangeError("Invalid asset source session");
  if (!Number.isInteger(kind) || kind <= 0 || kind > 0xffff)
    throw new RangeError("Invalid asset kind");
  if (!Number.isInteger(variant) || variant < 0 || variant > 0xffff_ffff)
    throw new RangeError("Invalid asset variant");
  return {
    kind,
    source: `client://${session}/assets/${kind}/${encodeURIComponent(String(name))}#immutable`,
    variant,
  };
}

type Waiter = {
  resolve(): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
};

/** Owned source delivery with bounded framing, independent of World command outcomes. */
export class ClientAssetSources {
  private nextId = 1n;
  private readonly pending = new Map<bigint, Waiter>();
  private operationTail: Promise<void> = Promise.resolve();
  private stopped?: Error;

  constructor(
    private readonly session: () => bigint,
    private readonly send: (bytes: Uint8Array<ArrayBuffer>) => void,
    private readonly timeoutMs: number,
    private readonly fail: (error: Error) => void,
  ) {}

  receive(bytes: Uint8Array): boolean {
    if (!isAssetSourceResponse(bytes)) return false;
    const reader = new HostWireReader(bytes);
    reader.raw(4);
    if (reader.u64() !== this.session())
      throw new Error("Asset source session mismatch");
    const id = reader.u64();
    const tag = reader.u8();
    if (tag > 1) throw new Error("Invalid asset source result");
    const error = tag === 1 ? new Error(reader.string()) : undefined;
    reader.end();
    const waiter = this.pending.get(id);
    if (!waiter) throw new Error("Unknown asset source result");
    this.pending.delete(id);
    clearTimeout(waiter.timer);
    if (error) waiter.reject(error);
    else waiter.resolve();
    return true;
  }

  register(source: ClientAssetSource, input: ArrayBuffer): Promise<void> {
    // Snapshot both the descriptor and bytes before yielding. Caller writes can
    // never mutate the immutable content being published.
    const descriptor = snapshotSource(source, this.session());
    const bytes = new Uint8Array(input.slice(0));
    return this.enqueue(() => this.deliver(descriptor, bytes));
  }

  release(source: ClientAssetSource): Promise<void> {
    const descriptor = snapshotSource(source, this.session());
    return this.enqueue(async () => {
      await this.request(4, (writer) => writeSource(writer, descriptor)).done;
    });
  }

  private enqueue(operation: () => Promise<void>): Promise<void> {
    const queued = this.operationTail.then(operation);
    this.operationTail = queued.catch(() => {});
    return queued;
  }

  private async deliver(
    source: ClientAssetSource,
    bytes: Uint8Array<ArrayBuffer>,
  ): Promise<void> {
    const begin = this.request(0, (writer) => {
      writeSource(writer, source);
      writer.u64(BigInt(bytes.length));
    });
    await begin.done;
    try {
      for (let offset = 0; offset < bytes.length; ) {
        const pending: Promise<void>[] = [];
        for (
          let chunk = 0;
          chunk < IN_FLIGHT_CHUNKS && offset < bytes.length;
          chunk++
        ) {
          const start = offset;
          const end = Math.min(start + CHUNK_BYTES, bytes.length);
          pending.push(
            this.request(1, (writer) => {
              writer.u64(begin.id);
              writer.u64(BigInt(start));
              writer.bytes(bytes.subarray(start, end));
            }).done,
          );
          offset = end;
        }
        const results = await Promise.allSettled(pending);
        const failure = results.find(
          (result): result is PromiseRejectedResult =>
            result.status === "rejected",
        );
        if (failure) throw failure.reason;
      }
      await this.request(2, (writer) => writer.u64(begin.id)).done;
    } catch (error) {
      // All submitted chunks were drained above before cancelling staging.
      await this.request(3, (writer) => writer.u64(begin.id)).done.catch(
        () => {},
      );
      throw error;
    }
  }

  private request(
    tag: number,
    encode: (writer: HostWireWriter) => void,
  ): { id: bigint; done: Promise<void> } {
    const id = this.nextId++;
    const done = new Promise<void>((resolve, reject) => {
      if (this.stopped) {
        reject(this.stopped);
        return;
      }
      try {
        const writer = new HostWireWriter();
        writer.raw(REQUEST_MAGIC);
        writer.u64(this.session());
        writer.u64(id);
        writer.u8(tag);
        encode(writer);
        const bytes = writer.finish();
        const timer = setTimeout(
          () => this.fail(new Error("Asset source delivery timed out")),
          this.timeoutMs,
        );
        this.pending.set(id, { resolve, reject, timer });
        this.send(bytes);
      } catch (error) {
        const waiter = this.pending.get(id);
        if (waiter) clearTimeout(waiter.timer);
        this.pending.delete(id);
        reject(asError(error));
      }
    });
    return { id, done };
  }

  close(error: Error): void {
    if (this.stopped) return;
    this.stopped = error;
    for (const waiter of this.pending.values()) {
      clearTimeout(waiter.timer);
      waiter.reject(error);
    }
    this.pending.clear();
  }
}

function snapshotSource(
  source: ClientAssetSource,
  session: bigint,
): ClientAssetSource {
  const descriptor = { ...source };
  const prefix = `client://${session}/`;
  if (
    !descriptor.source.startsWith(prefix) ||
    !descriptor.source.slice(prefix.length).includes("#")
  )
    throw new Error("Asset source does not belong to this client session");
  // Reuse the wire validation before queueing behind an earlier delivery.
  const writer = new HostWireWriter();
  writeSource(writer, descriptor);
  return descriptor;
}

function writeSource(writer: HostWireWriter, source: ClientAssetSource): void {
  if (
    !Number.isInteger(source.kind) ||
    source.kind <= 0 ||
    source.kind > 0xffff
  )
    throw new RangeError("Invalid asset kind");
  const variant = source.variant ?? 0;
  if (!Number.isInteger(variant) || variant < 0 || variant > 0xffff_ffff)
    throw new RangeError("Invalid asset variant");
  writer.u32(source.kind);
  writer.string(source.source);
  writer.u32(variant);
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
