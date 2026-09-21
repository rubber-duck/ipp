/** Bounded command pages within one Host-issued logical batch. */
import type { BatchOutcome, Command, Request } from "./types.js";

export const COMMAND_PAGE_COMMANDS = 256;
export const COMMAND_PAGE_BYTES = 128 * 1024;
const IN_FLIGHT_PAGES = 8;

export type CommandPageEncoder = (request: Request) => Uint8Array<ArrayBuffer>;

type BatchWriter = {
  beginBatch(): Promise<bigint>;
  batchChunk(id: bigint, operations: Command[]): Promise<BatchOutcome>;
  endBatch(id: bigint): Promise<void>;
};

/** All submitted pages have settled; these identities survived a local failure. */
export class CommandEncodingError extends Error {
  constructor(
    cause: unknown,
    readonly aliases: Map<number, bigint>,
    readonly stateOverlays: BatchOutcome["stateOverlays"] = [],
  ) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
    this.name = "CommandEncodingError";
  }
}

function pageRequest(operations: Command[]): Request {
  // Session, request and batch identities have fixed-width encodings, so these
  // valid placeholders produce the exact page length for every real request.
  return {
    session: 1n,
    requestId: 1n,
    body: { kind: "batchChunk", batch: { id: 1n, operations } },
  };
}

/** Partition without retaining encoded payloads or depending on a target layout. */
export function* commandPages(
  operations: Iterable<Command>,
  encode: CommandPageEncoder,
): Generator<Command[]> {
  const header = encode(pageRequest([])).byteLength;
  let page: Command[] = [];
  let bytes = header;
  for (const operation of operations) {
    const size = encode(pageRequest([operation])).byteLength - header;
    if (size < 0 || header + size > COMMAND_PAGE_BYTES)
      throw new RangeError(
        "An individual command exceeds the command page byte limit",
      );
    if (
      page.length > 0 &&
      (page.length === COMMAND_PAGE_COMMANDS ||
        bytes + size > COMMAND_PAGE_BYTES)
    ) {
      yield page;
      page = [];
      bytes = header;
    }
    page.push(operation);
    bytes += size;
  }
  if (page.length > 0) yield page;
}

/** Plan an ordinary array once so automatic paging does not encode it twice. */
export function planCommandPages(
  operations: readonly Command[],
  encode: CommandPageEncoder,
): Command[][] {
  return [...commandPages(operations, encode)];
}

type SettledPage = { start: number; outcome: BatchOutcome };

function aggregateOutcomes(
  batchId: bigint,
  pages: readonly SettledPage[],
): BatchOutcome {
  const aliases = new Map<number, bigint>();
  const stateOverlays: BatchOutcome["stateOverlays"] = [];
  let latest: BatchOutcome = {
    ok: true,
    batchId,
    tick: 0n,
    aliases: [],
    stateOverlays: [],
  };
  let failure:
    | (SettledPage & { outcome: Extract<BatchOutcome, { ok: false }> })
    | undefined;

  for (const page of [...pages].sort(
    (left, right) => left.start - right.start,
  )) {
    latest = page.outcome;
    for (const alias of page.outcome.aliases) {
      const previous = aliases.get(alias.alias);
      if (previous !== undefined && previous !== alias.id)
        throw new Error("A command alias resolved to conflicting identities");
      aliases.set(alias.alias, alias.id);
    }
    stateOverlays.push(...page.outcome.stateOverlays);
    if (!page.outcome.ok && !failure)
      failure = {
        ...page,
        outcome: page.outcome,
      };
  }

  const identities = {
    aliases: [...aliases].map(([alias, id]) => ({ alias, id })),
    stateOverlays,
  };
  if (!failure) return { ...latest, batchId, ...identities };
  const operation = failure.outcome.error.operation;
  return {
    ...failure.outcome,
    batchId,
    ...identities,
    error: {
      ...failure.outcome.error,
      operation: operation === null ? null : failure.start + operation,
    },
  };
}

async function applyPages(
  client: BatchWriter,
  batchId: bigint,
  pages: Iterator<Command[]>,
): Promise<BatchOutcome> {
  const settled: SettledPage[] = [];
  const pending: Promise<void>[] = [];
  let offset = 0;
  let transportError: unknown;
  let localError: unknown;

  try {
    for (;;) {
      const next = pages.next();
      if (next.done) break;
      const page = next.value;
      const start = offset;
      offset += page.length;
      pending.push(
        client.batchChunk(batchId, page).then(
          (outcome) => {
            settled.push({ start, outcome });
          },
          (error) => {
            transportError ??= error;
          },
        ),
      );
      if (pending.length === IN_FLIGHT_PAGES) await pending.shift();
      if (transportError || settled.some(({ outcome }) => !outcome.ok)) break;
    }
    if (offset === 0)
      pending.push(
        client.batchChunk(batchId, []).then(
          (outcome) => {
            settled.push({ start: 0, outcome });
          },
          (error) => {
            transportError ??= error;
          },
        ),
      );
  } catch (error) {
    localError = error;
  } finally {
    pages.return?.();
    await Promise.all(pending);
  }

  const outcome = aggregateOutcomes(batchId, settled);
  if (!outcome.ok) return outcome;
  if (transportError) throw transportError;
  await client.endBatch(batchId);
  if (localError)
    throw new CommandEncodingError(
      localError,
      new Map(outcome.aliases.map(({ alias, id }) => [alias, id])),
      outcome.stateOverlays,
    );
  return outcome;
}

/** Apply already-planned pages without repeating command-size encoding. */
export async function applyPlannedCommandPages(
  client: BatchWriter,
  pages: readonly Command[][],
): Promise<BatchOutcome> {
  const batchId = await client.beginBatch();
  return applyPages(client, batchId, pages[Symbol.iterator]());
}

export async function applyCommandPages(
  client: BatchWriter,
  operations: Iterable<Command>,
  encode: CommandPageEncoder,
): Promise<BatchOutcome> {
  const batchId = await client.beginBatch();
  return applyPages(client, batchId, commandPages(operations, encode));
}
