import { bulkReadLeases } from "./bulk-reads.js";
import { BulkReadClient } from "../../../packages/ipp-client/src/bulk-reads.js";
/**
 * Clients without a generated SDK on any transport: the Host announces its
 * contract and serves it, and checks no claim at the hello. Results are plain
 * data so a browser page can return them to the Node harness.
 */
import {
  contractIdentity,
  hostContractRequest,
  hostHello,
  isHostContractReply,
  readHostAnnouncement,
  readHostContract,
  readHostContractDescriptor,
} from "../../../packages/ipp-client/src/host-contract.js";
import { HostWireWriter } from "../../../packages/ipp-client/src/host-protocol.js";
import type { MessageTransport } from "../../../packages/ipp-client/src/transport.js";

/** What a client without a generated SDK learned from the Host. */
export interface PulledContract {
  readonly revision: number;
  /** Announced compatibility hash, in decimal. */
  readonly announcedHash: string;
  /** Hash the served contract declares and its descriptors produce. */
  readonly contractHash: string;
  readonly contractBytes: number;
  /** The served contract equals, byte for byte, the contract the build
   * pipeline produced for this Host's target. */
  readonly matchesBuild: boolean;
}

/** Read the announcement and pull the contract with schema-independent code
 * only, then compare it with the contract the build produced. */
export async function pullContractWithoutSdk(
  transport: MessageTransport,
  builtContract: ArrayLike<number>,
  timeoutMs: number,
): Promise<PulledContract> {
  const { announcement, contract } = await readHostContract(transport, {
    timeoutMs,
  });
  return {
    revision: announcement.revision,
    announcedHash: announcement.schemaHash.toString(),
    contractHash: contractIdentity(contract).schemaHash.toString(),
    contractBytes: contract.length,
    matchesBuild:
      contract.length === builtContract.length &&
      contract.every((byte, at) => byte === builtContract[at]),
  };
}

/** The parts of another target's generated contract a careless client uses. */
export interface ForeignContract {
  readonly SCHEMA_HASH: bigint;
  readonly WIRE: {
    readonly HOST_REQUEST_LIST_WORLDS: number;
    readonly HOST_RESPONSE_WORLDS: number;
  };
  encodeRequest(request: {
    readonly session: bigint;
    readonly requestId: bigint;
    readonly body: {
      readonly kind: "inspect";
      readonly collection: "entities";
    };
  }): Uint8Array<ArrayBuffer>;
}

/** How the Host treated a client that saw a different hash and went on. */
export interface IgnoredDifference {
  readonly announcedHash: string;
  readonly clientHash: string;
  /** The contract was served after the difference was seen. */
  readonly contractServed: boolean;
  /** A valid Host request was answered. */
  readonly listedWorlds: boolean;
  /** An operation naming a World session this connection never opened was
   * rejected, with the Host's reason. */
  readonly invalidOperationRejected: boolean;
  readonly detail: string;
}

/**
 * Connect with another target's contract, see that the announced hash
 * differs, and continue anyway: the Host accepts the hello, serves its
 * contract and answers a valid request, and still rejects an operation it
 * cannot validate for this connection.
 */
export function continueDespiteDifference(
  transport: MessageTransport,
  foreign: ForeignContract,
  timeoutMs: number,
): Promise<IgnoredDifference> {
  return new Promise((resolve, reject) => {
    let step: "hello" | "contract" | "list" | "invalid" | "done" = "hello";
    let announcedHash = 0n;
    let connection = 0n;
    const reads = new BulkReadClient(
      () => connection,
      (bytes) => transport.send(bytes),
      timeoutMs,
    );
    let contractServed = false;
    let listedWorlds = false;
    const timer = setTimeout(
      () => finish({ error: new Error(`No Host reply at the ${step} step`) }),
      timeoutMs,
    );
    // Only the Host ending this connection after the invalid operation counts
    // as its rejection; any local failure fails the scenario.
    const finish = (
      outcome: { readonly rejection: string } | { readonly error: Error },
    ) => {
      if (step === "done") return;
      step = "done";
      clearTimeout(timer);
      void transport
        .close()
        .catch(() => {})
        .then(() =>
          "error" in outcome
            ? reject(outcome.error)
            : resolve({
                announcedHash: announcedHash.toString(),
                clientHash: foreign.SCHEMA_HASH.toString(),
                contractServed,
                listedWorlds,
                invalidOperationRejected: true,
                detail: outcome.rejection,
              }),
        );
    };
    const ended = (error: Error) =>
      finish(
        step === "invalid" ? { rejection: error.message } : { error: error },
      );
    const fail = (error: unknown) =>
      finish({
        error: error instanceof Error ? error : new Error(String(error)),
      });
    transport.start({
      ready: () => {
        try {
          transport.send(hostHello());
        } catch (error) {
          fail(error);
        }
      },
      error: ended,
      closed: () => ended(new Error("Host closed the connection")),
      message: (bytes) => {
        try {
          if (reads.receive(bytes)) return;
          if (step === "hello") {
            const { announcement } = readHostAnnouncement(bytes);
            announcedHash = announcement.schemaHash;
            connection = announcement.connection;
            if (announcedHash === foreign.SCHEMA_HASH)
              throw new Error("The foreign contract matches this Host");
            step = "contract";
            transport.send(hostContractRequest());
          } else if (step === "contract" && isHostContractReply(bytes)) {
            const descriptor = readHostContractDescriptor(bytes);
            void reads
              .readAll(descriptor)
              .then((contract) => {
                const served = contractIdentity(contract);
                contractServed = served.schemaHash === announcedHash;
                step = "list";
                const list = new HostWireWriter();
                list.raw(HOST_REQUEST_MAGIC);
                list.u64(connection);
                list.u64(1n);
                list.u8(foreign.WIRE.HOST_REQUEST_LIST_WORLDS);
                list.u64(0n);
                transport.send(list.finish());
              })
              .catch(fail);
          } else if (step === "list") {
            const view = new DataView(
              bytes.buffer,
              bytes.byteOffset,
              bytes.byteLength,
            );
            listedWorlds =
              HOST_RESPONSE_MAGIC.every((byte, at) => bytes[at] === byte) &&
              view.getBigUint64(8, true) === connection &&
              view.getBigUint64(16, true) === 1n &&
              bytes[24] === foreign.WIRE.HOST_RESPONSE_WORLDS;
            const invalid = foreign.encodeRequest({
              session: FOREIGN_SESSION,
              requestId: 1n,
              body: { kind: "inspect", collection: "entities" },
            });
            step = "invalid";
            transport.send(invalid);
          } else if (step === "invalid") {
            throw new Error(
              "The Host answered an operation it cannot validate",
            );
          }
        } catch (error) {
          fail(error);
        }
      },
    });
  });
}

/** A generated client's refusal, as plain data. */
export interface ClientRefusal {
  readonly refused: boolean;
  readonly name: string;
  readonly message: string;
}

/** A generated module able to open a Host connection on a transport. */
export interface GeneratedHostModule {
  readonly SCHEMA_HASH: bigint;
  readonly IppHostClient: {
    connectTransport(
      transport: MessageTransport,
      options: { readonly timeoutMs: number },
    ): Promise<{ close(): Promise<void> }>;
  };
}

/** A generated module able to open a World session on a transport. */
export interface GeneratedWorldModule {
  readonly IppClient: {
    connectTransport(
      transport: MessageTransport,
      options: {
        readonly timeoutMs: number;
        readonly selectedSystems: readonly string[];
      },
    ): Promise<{ readonly session: bigint; close(): Promise<void> }>;
  };
}

/** Everything one Host did for clients with and without its contract. */
export interface ContractClientsObservation {
  readonly pulled: PulledContract;
  readonly refusal: ClientRefusal;
  readonly ignored: IgnoredDifference;
  /** A matching generated client opened a World session afterwards. */
  readonly matchingSession: string;
  readonly bulk: Awaited<ReturnType<typeof bulkReadLeases>>;
}

/**
 * On one Host, in order: a client without a generated SDK pulls the contract;
 * the client generated for the other target refuses the Host; a client that
 * ignores the difference is not rejected at the hello; and the matching
 * generated client still opens a World session. `connect` opens a new
 * physical connection to the same Host each time.
 */
export async function hostServesClientsWithAndWithoutItsContract(
  connect: () => MessageTransport,
  builtContract: ArrayLike<number>,
  matching: GeneratedWorldModule,
  foreign: GeneratedHostModule & ForeignContract,
  selectedSystems: readonly string[],
  timeoutMs: number,
): Promise<ContractClientsObservation> {
  const bulk = await bulkReadLeases(connect, builtContract, timeoutMs);
  const pulled = await pullContractWithoutSdk(
    connect(),
    builtContract,
    timeoutMs,
  );
  let refusal: ClientRefusal;
  try {
    const client = await foreign.IppHostClient.connectTransport(connect(), {
      timeoutMs,
    });
    await client.close();
    refusal = { refused: false, name: "", message: "connected" };
  } catch (error) {
    refusal = {
      refused: true,
      name: error instanceof Error ? error.name : "Error",
      message: error instanceof Error ? error.message : String(error),
    };
  }
  const ignored = await continueDespiteDifference(
    connect(),
    foreign,
    timeoutMs,
  );
  const client = await matching.IppClient.connectTransport(connect(), {
    timeoutMs,
    selectedSystems,
  });
  try {
    return {
      pulled,
      bulk,
      refusal,
      ignored,
      matchingSession: client.session.toString(),
    };
  } finally {
    await client.close();
  }
}

/** Host control framing of the current wire revision. */
const HOST_REQUEST_MAGIC = Uint8Array.of(73, 80, 80, 72, 2, 0, 0, 0);
const HOST_RESPONSE_MAGIC = Uint8Array.of(73, 80, 80, 65, 2, 0, 0, 0);
/** A World session identity no connection of a fresh Host has opened. */
const FOREIGN_SESSION = (1n << 63n) | 0x7fff_fff1n;
