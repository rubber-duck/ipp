import assert from "node:assert/strict";
import type {
  BatchOutcome,
  BatchSuccess,
  EntityId,
  EntityObservation,
  ProtocolRejection,
} from "./driver.js";

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

/** A generated client's refusal, as plain data. */
export interface ClientRefusal {
  readonly refused: boolean;
  readonly name: string;
  readonly message: string;
}

/** Everything one Host did for clients with and without its contract. */
export interface ContractClientsObservation {
  readonly pulled: PulledContract;
  readonly refusal: ClientRefusal;
  readonly ignored: IgnoredDifference;
  /** A matching generated client opened a World session afterwards. */
  readonly matchingSession: string;
  /** Bulk-read lease isolation, withheld EOF retention and release fencing. */
  readonly bulk: {
    readonly bytes: number;
    readonly peerIsolated: boolean;
    readonly withheldEofRetained: boolean;
    readonly staleFenced: boolean;
  };
}

export function committed(outcome: BatchOutcome): BatchSuccess {
  assert.equal(outcome.status, "committed", outcomeDescription(outcome));
  return outcome;
}

export function rejectedAt(
  outcome: BatchOutcome,
  operationIndex: number,
): void {
  assert.equal(outcome.status, "rejected", outcomeDescription(outcome));
  assert.equal(outcome.operationIndex, operationIndex);
  assert.notEqual(outcome.code, "");
  assert.notEqual(outcome.reason, "");
}

export function observed(
  observation: EntityObservation | null,
): EntityObservation {
  if (observation === null) {
    assert.fail("expected a live entity observation");
  }
  return observation;
}

export function aliasEntity(outcome: BatchSuccess, alias: string): EntityId {
  const result = outcome.aliases[alias];
  if (result === undefined) {
    assert.fail(`missing resolved alias '${alias}'`);
  }
  return result;
}

export function protocolRejected(
  rejection: ProtocolRejection,
  stage: ProtocolRejection["stage"],
): void {
  assert.equal(rejection.rejected, true, rejection.detail);
  assert.equal(rejection.stage, stage);
  assert.notEqual(rejection.code, "");
  assert.notEqual(rejection.detail, "");
}

/** A generated client refused a Host whose contract differs from its own,
 * naming both compatibility hashes; the Host rejected nothing. */
export function contractRefused(
  refusal: ProtocolRejection,
  hostSchemaHash: bigint,
  clientSchemaHash: bigint,
): void {
  protocolRejected(refusal, "handshake");
  assert.notEqual(hostSchemaHash, clientSchemaHash);
  assert.equal(refusal.code, "HostContractMismatchError", refusal.detail);
  for (const hash of [hostSchemaHash, clientSchemaHash])
    assert.ok(
      refusal.detail.includes(`0x${hash.toString(16).padStart(16, "0")}`),
      refusal.detail,
    );
}

/** One Host served its exact build contract to a client without a generated
 * SDK, a client generated for another target refused it, a client that
 * ignored the difference was not rejected at the hello but its invalid
 * operation was, and the matching generated client still connected. */
export function contractClientsServed(
  observation: ContractClientsObservation,
  hostSchemaHash: bigint,
  foreignSchemaHash: bigint,
): void {
  const { pulled, refusal, ignored, bulk } = observation;
  assert.equal(bulk.bytes, pulled.contractBytes);
  assert.equal(bulk.peerIsolated, true);
  assert.equal(bulk.withheldEofRetained, true);
  assert.equal(bulk.staleFenced, true);
  assert.equal(pulled.announcedHash, hostSchemaHash.toString());
  assert.equal(pulled.contractHash, pulled.announcedHash);
  assert.ok(
    pulled.matchesBuild,
    "served contract differs from the contract the build produced",
  );
  contractRefused(
    {
      rejected: refusal.refused,
      stage: "handshake",
      code: refusal.name,
      detail: refusal.message,
    },
    hostSchemaHash,
    foreignSchemaHash,
  );
  assert.equal(ignored.announcedHash, hostSchemaHash.toString());
  assert.equal(ignored.clientHash, foreignSchemaHash.toString());
  assert.equal(ignored.contractServed, true);
  assert.equal(ignored.listedWorlds, true);
  assert.equal(ignored.invalidOperationRejected, true, ignored.detail);
  assert.notEqual(observation.matchingSession, "0");
}

function outcomeDescription(outcome: BatchOutcome): string {
  return outcome.status === "rejected"
    ? `batch rejected at operation ${outcome.operationIndex}: ${outcome.code}: ${outcome.reason}`
    : `batch committed at tick ${outcome.commitTick}`;
}
