import type { Client } from "./client.js";
import { HostClientBase } from "./host-client.js";
import type { WorldCapacityHintsPatch } from "./host-protocol.js";

export interface WorldTransferOptions {
  signal?: AbortSignal;
  /** Bounds the completed owned result. Defaults to 64 MiB. */
  maxBytes?: number;
}

export interface WorldLoadOptions extends WorldTransferOptions {
  symbolicId?: string;
  capacityHints?: WorldCapacityHintsPatch;
}

const CHUNK_BYTES = 65_536;
const TRANSFER_WINDOW = 8;

async function transferChunks(
  start: number,
  total: number,
  signal: AbortSignal | undefined,
  transfer: (offset: number) => Promise<void>,
): Promise<void> {
  for (
    let offset = start;
    offset < total;
    offset += CHUNK_BYTES * TRANSFER_WINDOW
  ) {
    signal?.throwIfAborted();
    const pending: Promise<void>[] = [];
    for (let index = 0; index < TRANSFER_WINDOW; index++) {
      const next = offset + index * CHUNK_BYTES;
      if (next >= total) break;
      pending.push(transfer(next));
    }
    // A failed or cancelled transfer drains every submitted request before the
    // caller cancels Host staging in its finally block.
    const results = await Promise.allSettled(pending);
    const failure = results.find(
      (result): result is PromiseRejectedResult => result.status === "rejected",
    );
    if (failure) throw failure.reason;
  }
}

/** Optional persistence surface; generated Hosts inherit it only with snapshot enabled. */
export abstract class WorldPersistenceHostClient<
  T extends Client,
> extends HostClientBase<T> {
  private transferring = false;

  /** Capture underlying component/controller state with unchanged resource references. */
  async saveWorld(
    options: WorldTransferOptions = {},
  ): Promise<Uint8Array<ArrayBuffer>> {
    const maxBytes = this.beginTransfer(options);
    let job: bigint | undefined;
    try {
      const accepted = await this.request(
        this.hostTag("HOST_REQUEST_SAVE_WORLD"),
      );
      this.expect(accepted, this.hostTag("HOST_RESPONSE_TRANSFER"));
      job = accepted.u64();
      accepted.end();
      let output: Uint8Array<ArrayBuffer> | undefined;
      const readChunk = async (offset: number) => {
        const reader = await this.request(
          this.hostTag("HOST_REQUEST_READ_WORLD_SAVE"),
          (writer) => {
            writer.u64(job!);
            writer.u64(BigInt(offset));
          },
        );
        const tag = reader.u8();
        if (
          tag !== this.hostTag("HOST_RESPONSE_SAVE_CHUNK") ||
          reader.u64() !== job ||
          reader.u64() !== BigInt(offset)
        )
          throw new Error("Invalid save chunk");
        const total = reader.u64();
        if (total < 32n || total > BigInt(maxBytes))
          throw new Error("Saved World exceeds byte budget");
        output ??= new Uint8Array(Number(total));
        if (output.length !== Number(total))
          throw new Error("Save length changed");
        const bytes = reader.bytes();
        reader.end();
        if (bytes.length !== Math.min(CHUNK_BYTES, output.length - offset))
          throw new Error("Invalid save progress");
        output.set(bytes, offset);
      };
      options.signal?.throwIfAborted();
      await readChunk(0);
      await transferChunks(
        CHUNK_BYTES,
        output!.length,
        options.signal,
        readChunk,
      );
      job = undefined;
      options.signal?.throwIfAborted();
      return output!;
    } finally {
      if (job !== undefined)
        await this.complete(
          this.hostTag("HOST_REQUEST_CANCEL_WORLD_TRANSFER"),
          (writer) => writer.u64(job!),
        ).catch(() => {});
      this.transferring = false;
    }
  }

  /** Validate a private candidate and attach after World reconstruction succeeds. */
  async loadWorld(
    input: Uint8Array,
    options: WorldLoadOptions = {},
  ): Promise<T> {
    const maxBytes = this.beginTransfer(options);
    let job: bigint | undefined;
    try {
      if (input.length < 32 || input.length > maxBytes)
        throw new RangeError("World file is outside the byte budget");
      // Caller edits cannot alter a transfer in flight.
      const bytes = input.slice();
      const accepted = await this.request(
        this.hostTag("HOST_REQUEST_BEGIN_WORLD_LOAD"),
        (writer) => {
          writer.u64(BigInt(bytes.length));
          writer.u8(options.symbolicId === undefined ? 0 : 1);
          if (options.symbolicId !== undefined)
            writer.string(options.symbolicId);
          writer.hints(options.capacityHints);
        },
      );
      this.expect(accepted, this.hostTag("HOST_RESPONSE_TRANSFER"));
      job = accepted.u64();
      accepted.end();
      await transferChunks(0, bytes.length, options.signal, (offset) =>
        this.complete(
          this.hostTag("HOST_REQUEST_WRITE_WORLD_LOAD"),
          (writer) => {
            writer.u64(job!);
            writer.u64(BigInt(offset));
            writer.bytes(bytes.subarray(offset, offset + CHUNK_BYTES));
          },
        ),
      );
      options.signal?.throwIfAborted();
      const result = await this.request(
        this.hostTag("HOST_REQUEST_FINISH_WORLD_LOAD"),
        (writer) => writer.u64(job!),
      );
      job = undefined;
      return this.acceptAttachment(result);
    } finally {
      if (job !== undefined)
        await this.complete(
          this.hostTag("HOST_REQUEST_CANCEL_WORLD_TRANSFER"),
          (writer) => writer.u64(job!),
        ).catch(() => {});
      this.transferring = false;
    }
  }

  private beginTransfer(options: WorldTransferOptions): number {
    options.signal?.throwIfAborted();
    const max = options.maxBytes ?? 64 * 1024 * 1024;
    if (!Number.isSafeInteger(max) || max < 32 || max > 64 * 1024 * 1024)
      throw new RangeError("maxBytes must be in 32..=67108864");
    if (this.transferring)
      throw new Error("A World transfer is already active");
    this.transferring = true;
    return max;
  }
}
