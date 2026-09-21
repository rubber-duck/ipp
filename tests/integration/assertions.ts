import assert from "node:assert/strict";
import type {
  BatchOutcome,
  BatchSuccess,
  EntityId,
  EntityObservation,
  ProtocolRejection,
} from "./driver.js";

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

function outcomeDescription(outcome: BatchOutcome): string {
  return outcome.status === "rejected"
    ? `batch rejected at operation ${outcome.operationIndex}: ${outcome.code}: ${outcome.reason}`
    : `batch committed at tick ${outcome.commitTick}`;
}
