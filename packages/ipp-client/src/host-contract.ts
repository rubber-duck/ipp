import { BulkReadClient, type BulkReadDescriptor } from "./bulk-reads.js";
/**
 * Schema-independent opening of an IPP connection and retrieval of the Host's
 * contract. Nothing here depends on a generated contract, so any client can
 * connect, read what the Host announces and pull its contract.
 *
 * The Host checks no claim from the client: compatibility is the client's
 * decision. Generated SDKs compare the announcement with the contract they were
 * generated from and refuse a difference with {@link HostContractMismatchError}.
 * A client without a matching SDK reads the contract and applies its own rule;
 * the Host still validates every operation it decodes.
 *
 * Layouts, little-endian, fixed across wire revisions:
 * - hello: `IPPB`; the first message of every connection.
 * - announcement: `IPPB`, wire revision u32, compatibility hash u64 and the
 *   nonzero connection identity u64. A standalone World session appends its
 *   World manifest.
 * - contract request: `IPCQ`, any number of times after the hello.
 * - contract reply: `IPCR`, connection u64, read u64, exact length u64.
 *   The common schema-independent bulk reader retrieves the contract.
 */
import type { MessageTransport } from "./transport.js";

const HELLO = Uint8Array.of(0x49, 0x50, 0x50, 0x42); // IPPB
const CONTRACT_REQUEST = Uint8Array.of(0x49, 0x50, 0x43, 0x51); // IPCQ
const CONTRACT_REPLY = Uint8Array.of(0x49, 0x50, 0x43, 0x52); // IPCR
const ANNOUNCEMENT_BYTES = 24;
const CONTRACT_HEADER_BYTES = 16;

/** What a Host announces in reply to the hello. */
export interface HostAnnouncement {
  /** Wire revision of the Host's protocol. */
  readonly revision: number;
  /** Compatibility hash of the Host's contract. */
  readonly schemaHash: bigint;
  /** Identity of this connection, named by later Host requests. */
  readonly connection: bigint;
}

/** Wire revision and compatibility hash that identify one contract. */
export interface ContractIdentity {
  readonly revision: number;
  readonly schemaHash: bigint;
}

/** A generated client refused a Host whose contract differs from its own.
 * Nothing further was sent on the connection. */
export class HostContractMismatchError extends Error {
  override readonly name = "HostContractMismatchError";

  constructor(
    /** What the Host announced. */
    readonly host: ContractIdentity,
    /** The contract the client was generated from. */
    readonly client: ContractIdentity,
  ) {
    super(
      `Host contract ${contractLabel(host)} differs from this client's contract ${contractLabel(client)}; generate the client from the Host's contract`,
    );
  }
}

/** The hello that opens every connection. */
export function hostHello(): Uint8Array<ArrayBuffer> {
  return HELLO.slice();
}

/** Read the Host's reply to the hello. A standalone World session's manifest
 * is returned undecoded as `trailer`. */
export function readHostAnnouncement(bytes: Uint8Array): {
  readonly announcement: HostAnnouncement;
  readonly trailer: Uint8Array;
} {
  if (bytes.length < ANNOUNCEMENT_BYTES || !startsWith(bytes, HELLO))
    throw new Error("Not an IPP Host announcement");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const connection = view.getBigUint64(16, true);
  if (connection === 0n) throw new Error("Host announced no connection");
  return {
    announcement: {
      revision: view.getUint32(4, true),
      schemaHash: view.getBigUint64(8, true),
      connection,
    },
    trailer: bytes.subarray(ANNOUNCEMENT_BYTES),
  };
}

/** Read the announcement and refuse a Host whose wire revision or hash
 * differs from `client`. Generated SDKs call this with their own contract. */
export function acceptHostAnnouncement(
  bytes: Uint8Array,
  client: ContractIdentity,
): { readonly connection: bigint; readonly trailer: Uint8Array } {
  const { announcement, trailer } = readHostAnnouncement(bytes);
  if (
    announcement.revision !== client.revision ||
    announcement.schemaHash !== client.schemaHash
  )
    throw new HostContractMismatchError(
      {
        revision: announcement.revision,
        schemaHash: announcement.schemaHash,
      },
      { revision: client.revision, schemaHash: client.schemaHash },
    );
  return { connection: announcement.connection, trailer };
}

/** Ask the Host for its full contract. */
export function hostContractRequest(): Uint8Array<ArrayBuffer> {
  return CONTRACT_REQUEST.slice();
}

/** Whether a Host message is a contract reply. */
export function isHostContractReply(bytes: Uint8Array): boolean {
  return startsWith(bytes, CONTRACT_REPLY);
}

/** The schema-independent descriptor carried by the bootstrap reply. */
export function readHostContractDescriptor(
  bytes: Uint8Array,
): BulkReadDescriptor {
  if (!isHostContractReply(bytes) || bytes.length !== 28)
    throw new Error("Invalid contract read descriptor");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const reference = {
    connection: view.getBigUint64(4, true),
    read: view.getBigUint64(12, true),
  };
  if (reference.connection === 0n || reference.read === 0n)
    throw new Error("Invalid contract read identity");
  return { reference, length: view.getBigUint64(20, true) };
}

/** The identity a contract declares, after checking that its descriptors
 * hash to the declared compatibility hash. */
export function contractIdentity(contract: Uint8Array): ContractIdentity {
  if (contract.length < CONTRACT_HEADER_BYTES || !startsWith(contract, HELLO))
    throw new Error("Not an IPP contract");
  const view = new DataView(
    contract.buffer,
    contract.byteOffset,
    contract.byteLength,
  );
  const schemaHash = view.getBigUint64(8, true);
  let actual = 0xcbf2_9ce4_8422_2325n;
  for (let at = CONTRACT_HEADER_BYTES; at < contract.length; at++)
    actual = BigInt.asUintN(
      64,
      (actual ^ BigInt(contract[at] as number)) * 0x100_0000_01b3n,
    );
  if (actual !== schemaHash)
    throw new Error("Contract descriptors do not match their hash");
  return { revision: view.getUint32(4, true), schemaHash };
}

/** Connect without a generated client, read the announcement, pull the
 * contract and close the connection. The contract must hash to the
 * announced compatibility hash. */
export function readHostContract(
  transport: MessageTransport,
  options: { readonly timeoutMs?: number; readonly signal?: AbortSignal } = {},
): Promise<{
  readonly announcement: HostAnnouncement;
  readonly contract: Uint8Array<ArrayBuffer>;
}> {
  return new Promise((resolve, reject) => {
    let announcement: HostAnnouncement | undefined;
    const reads = new BulkReadClient(
      () => announcement?.connection ?? 0n,
      (bytes) => transport.send(bytes),
      options.timeoutMs,
    );
    let settled = false;
    const finish = (
      outcome:
        | {
            readonly announcement: HostAnnouncement;
            readonly contract: Uint8Array<ArrayBuffer>;
          }
        | { readonly error: Error },
    ) => {
      if (settled) return;
      settled = true;
      reads.close(new Error("Contract reader finished"));
      clearTimeout(timer);
      options.signal?.removeEventListener("abort", abort);
      void transport
        .close()
        .catch(() => {})
        .then(() =>
          "error" in outcome ? reject(outcome.error) : resolve(outcome),
        );
    };
    const abort = () =>
      finish({
        error:
          options.signal?.reason instanceof Error
            ? options.signal.reason
            : new Error("Contract read aborted"),
      });
    const timer = setTimeout(
      () => finish({ error: new Error("Contract read timed out") }),
      options.timeoutMs ?? 10_000,
    );
    if (options.signal?.aborted) return abort();
    options.signal?.addEventListener("abort", abort, { once: true });
    try {
      transport.start({
        ready: () => {
          try {
            transport.send(hostHello());
          } catch (error) {
            finish({ error: asError(error) });
          }
        },
        error: (error) => finish({ error }),
        closed: () => finish({ error: new Error("Host transport closed") }),
        message: (bytes) => {
          if (settled) return;
          try {
            if (reads.receive(bytes)) return;
            if (!announcement) {
              announcement = readHostAnnouncement(bytes).announcement;
              transport.send(hostContractRequest());
            } else if (isHostContractReply(bytes)) {
              const descriptor = readHostContractDescriptor(bytes);
              const expected = announcement;
              void reads
                .readAll(descriptor, { signal: options.signal })
                .then((contract) => {
                  if (
                    contractIdentity(contract).schemaHash !==
                    expected.schemaHash
                  )
                    throw new Error(
                      "Served contract differs from the announcement",
                    );
                  finish({ announcement: expected, contract });
                })
                .catch((error) => finish({ error: asError(error) }));
            }
          } catch (error) {
            finish({ error: asError(error) });
          }
        },
      });
    } catch (error) {
      finish({ error: asError(error) });
    }
  });
}

function startsWith(bytes: Uint8Array, prefix: Uint8Array): boolean {
  if (bytes.length < prefix.length) return false;
  for (let at = 0; at < prefix.length; at++)
    if (bytes[at] !== prefix[at]) return false;
  return true;
}

function contractLabel(identity: ContractIdentity): string {
  return `0x${identity.schemaHash.toString(16).padStart(16, "0")} (wire revision ${identity.revision})`;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
