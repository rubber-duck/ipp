/**
 * Bounded pages of one client-identified logical batch.
 *
 * A batch travels as one or more `submitBatch` pages under an identity the
 * client keeps unique among its connection's open batches. Every page but the
 * last is uncorrelated and produces no reply; the Host applies the whole batch
 * when its final page arrives and answers that page with the batch outcome.
 * Pages are handed to the transport back to back without awaiting anything.
 * Each command is encoded once: its bytes both size its page and form part of
 * that page's message.
 */
import type { BatchIdentitySource } from "./transport.js";
import type { BatchOutcome, Command, Request } from "./types.js";

/**
 * Page bounds of the connected target, generated from its contract as
 * `COMMAND_PAGE_LIMITS` so that client and Host agree.
 */
export interface CommandPageLimits {
  /** Most commands in one page. */
  readonly commands: number;
  /** Most encoded bytes of one page message. */
  readonly bytes: number;
}

export type CommandPageEncoder = (request: Request) => Uint8Array<ArrayBuffer>;

/**
 * Connection-scoped batch identities: a wrapping u32 counter that skips
 * identities of batches whose final page has not been handed to the transport.
 */
export class BatchIdentities implements BatchIdentitySource {
  private next = 0;
  private readonly open = new Set<number>();

  allocate(): number {
    if (this.open.size > 0xffff_ffff)
      throw new RangeError("Batch identity space exhausted");
    for (;;) {
      const id = this.next;
      this.next = (this.next + 1) >>> 0;
      if (!this.open.has(id)) {
        this.open.add(id);
        return id;
      }
    }
  }

  /** The final page is with the transport; the Host may see the identity reused. */
  release(id: number): void {
    this.open.delete(id);
  }
}

/** The commands of one page with their encodings. */
export interface EncodedCommandPage {
  readonly operations: Command[];
  /** Each command's encoding within a page, in command order. */
  readonly encoded: readonly Uint8Array[];
  /** Total bytes of `encoded`. */
  readonly bytes: number;
}

/** Transport handoff for the pages of one batch. */
export interface CommandBatchSink {
  /** Hand a non-final page to the transport. It produces no reply. */
  page(batchId: number, page: EncodedCommandPage): void;
  /** Hand the final page to the transport and resolve with the whole batch outcome. */
  finish(batchId: number, page: EncodedCommandPage): Promise<BatchOutcome>;
}

/** A streaming producer's open batch; full pages leave as soon as they fill. */
export interface CommandBatchWriter {
  readonly batchId: number;
  /** Command counts of the pages handed to the transport so far. */
  readonly pages: readonly number[];
  /** Append commands; nothing applies until `finish()`. */
  write(operations: Iterable<Command>): void;
  /** Send the remaining commands as the final page and await the whole outcome. */
  finish(): Promise<BatchOutcome>;
}

function pageRequest(operations: Command[]): Request {
  // Session, request and batch identities have fixed-width encodings, so these
  // valid placeholders produce the exact page length for every real request.
  return {
    session: 1n,
    requestId: 1n,
    body: { kind: "submitBatch", batchId: 0, last: true, operations },
  };
}

/**
 * Encodes each command once and assembles page messages from those bytes,
 * through the target's own request encoder rather than a copy of its layout.
 *
 * A page message is the encoding of the same request without commands, its
 * trailing u32 command count set to the page's, followed by the commands'
 * encodings. The first command encoded checks that layout against the target
 * encoder, so a different layout fails loudly instead of sending a wrong page.
 */
export class CommandPageCodec {
  private readonly header: Uint8Array;
  private verified = false;

  constructor(
    private readonly encode: CommandPageEncoder,
    readonly limits: CommandPageLimits,
  ) {
    this.header = encode(pageRequest([]));
  }

  /** Encoded bytes of a page's header, the same for every page. */
  get headerBytes(): number {
    return this.header.byteLength;
  }

  /** One command's bytes within any page of this target. */
  command(operation: Command): Uint8Array {
    const page = this.encode(pageRequest([operation]));
    const header = this.header.byteLength;
    if (!this.verified) {
      const count = header - 4;
      if (
        count < 0 ||
        page.byteLength < header ||
        page.subarray(0, count).some((byte, at) => byte !== this.header[at]) ||
        u32(page, count) !== 1 ||
        u32(this.header, count) !== 0
      )
        throw new Error(
          "The target's command pages are not a counted header followed by commands",
        );
      this.verified = true;
    }
    if (page.byteLength > this.limits.bytes)
      throw new RangeError(
        "An individual command exceeds the command page byte limit",
      );
    return page.subarray(header);
  }

  /** The complete message of one page. */
  message(
    session: bigint,
    requestId: bigint,
    batchId: number,
    last: boolean,
    page: EncodedCommandPage,
  ): Uint8Array<ArrayBuffer> {
    const header = this.encode({
      session,
      requestId,
      body: { kind: "submitBatch", batchId, last, operations: [] },
    });
    if (header.byteLength !== this.header.byteLength)
      throw new Error("The target's command page header changed length");
    const bytes = new Uint8Array(header.byteLength + page.bytes);
    bytes.set(header);
    new DataView(bytes.buffer).setUint32(
      header.byteLength - 4,
      page.operations.length,
      true,
    );
    let at = header.byteLength;
    for (const command of page.encoded) {
      bytes.set(command, at);
      at += command.byteLength;
    }
    return bytes;
  }
}

function u32(bytes: Uint8Array, at: number): number {
  return new DataView(bytes.buffer, bytes.byteOffset + at, 4).getUint32(
    0,
    true,
  );
}

/** Fills pages in command order within the target's command and byte limits. */
class CommandPageBuilder {
  private operations: Command[] = [];
  private encoded: Uint8Array[] = [];
  private bytes = 0;

  constructor(private readonly codec: CommandPageCodec) {}

  /** Add one command, returning the full page it closed, if any. */
  add(operation: Command): EncodedCommandPage | undefined {
    const encoded = this.codec.command(operation);
    const limits = this.codec.limits;
    const full =
      this.operations.length > 0 &&
      (this.operations.length === limits.commands ||
        this.codec.headerBytes + this.bytes + encoded.byteLength > limits.bytes)
        ? this.take()
        : undefined;
    this.operations.push(operation);
    this.encoded.push(encoded);
    this.bytes += encoded.byteLength;
    return full;
  }

  /** The page built so far, possibly empty, leaving the builder empty. */
  take(): EncodedCommandPage {
    const page = {
      operations: this.operations,
      encoded: this.encoded,
      bytes: this.bytes,
    };
    this.operations = [];
    this.encoded = [];
    this.bytes = 0;
    return page;
  }
}

/**
 * Encode an ordinary array into its pages up front, so a batch never sends a
 * page before every command is known to encode. The last page may be empty.
 */
export function encodeCommandPages(
  operations: Iterable<Command>,
  codec: CommandPageCodec,
): EncodedCommandPage[] {
  const builder = new CommandPageBuilder(codec);
  const pages: EncodedCommandPage[] = [];
  for (const operation of operations) {
    const full = builder.add(operation);
    if (full) pages.push(full);
  }
  pages.push(builder.take());
  return pages;
}

/** The commands of each page a batch of `operations` would send. */
export function planCommandPages(
  operations: readonly Command[],
  encode: CommandPageEncoder,
  limits: CommandPageLimits,
): Command[][] {
  return encodeCommandPages(
    operations,
    new CommandPageCodec(encode, limits),
  ).map((page) => page.operations);
}

/** Send encoded pages back to back; only the final page is awaited. */
export function submitCommandPages(
  identities: BatchIdentitySource,
  sink: CommandBatchSink,
  pages: readonly EncodedCommandPage[],
): Promise<BatchOutcome> {
  const batchId = identities.allocate();
  try {
    for (const page of pages.slice(0, -1)) sink.page(batchId, page);
    return sink.finish(
      batchId,
      pages.at(-1) ?? { operations: [], encoded: [], bytes: 0 },
    );
  } finally {
    identities.release(batchId);
  }
}

/**
 * Open a batch for a producer that yields commands over time. A command that
 * cannot be encoded fails the writer without sending its page; pages already
 * sent stay buffered at the Host until its batch deadline fails them, so a
 * failed writer never applies anything and keeps its identity reserved.
 */
export function openCommandBatch(
  identities: BatchIdentitySource,
  sink: CommandBatchSink,
  codec: CommandPageCodec,
): CommandBatchWriter {
  const batchId = identities.allocate();
  const builder = new CommandPageBuilder(codec);
  let state: "open" | "failed" | "finished" = "open";
  const pages: number[] = [];
  return {
    batchId,
    pages,
    write(operations) {
      if (state !== "open") throw new Error(`Command batch is ${state}`);
      try {
        for (const operation of operations) {
          const full = builder.add(operation);
          if (full) {
            sink.page(batchId, full);
            pages.push(full.operations.length);
          }
        }
      } catch (error) {
        state = "failed";
        throw error;
      }
    },
    finish() {
      if (state !== "open")
        return Promise.reject(new Error(`Command batch is ${state}`));
      state = "finished";
      const page = builder.take();
      pages.push(page.operations.length);
      try {
        return sink.finish(batchId, page);
      } finally {
        identities.release(batchId);
      }
    },
  };
}
