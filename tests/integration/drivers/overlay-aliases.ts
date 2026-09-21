import type {
  BatchOutcome,
  Client,
  Command,
  EntityRef,
  FieldDescriptor,
  StateOverlayRef,
} from "@ipp/client";
import type { DriverConnectOptions } from "../driver.js";
import type { OverlayAliasDriver } from "../scenarios/overlay-aliases.js";

export interface OverlayAliasContract {
  readonly Entity: {
    create(alias: number, metadata?: { symbolicId: string }): Command;
    alias(alias: number): EntityRef;
    delete(entity: EntityRef): Command;
  };
  readonly Scalar: {
    readonly id: number;
    insert(entity: EntityRef, values: { value: number }): Command;
  };
  readonly LinearDriver: {
    readonly id: number;
    readonly fields: {
      readonly source: FieldDescriptor;
      readonly scale: FieldDescriptor;
    };
  };
}

export function overlayAliasDriver(
  client: Client,
  contract: OverlayAliasContract,
  record: DriverConnectOptions["record"],
): OverlayAliasDriver {
  const batch = async (operations: Command[]) => {
    const outcome = await client.batch(operations);
    await record("overlay_batch", { operations, outcome });
    return outcome;
  };
  let owner: StateOverlayRef = { kind: "alias", alias: 10 };
  let overlay: StateOverlayRef = { kind: "alias", alias: 12 };
  const sourceField = (alias: number) => ({
    offset: contract.LinearDriver.fields.source.offset,
    value: { kind: "entity" as const, value: contract.Entity.alias(alias) },
  });
  const update = (alias: number): Command => ({
    kind: "updateComponentStateOverlay",
    owner,
    overlay,
    fields: [sourceField(alias)],
    clear: [],
  });
  return {
    async attachToNewSource() {
      const outcome = success(
        await batch([
          contract.Entity.create(1, { symbolicId: "source-a" }),
          contract.Scalar.insert(contract.Entity.alias(1), { value: 2 }),
          contract.Entity.create(2, { symbolicId: "alias-target" }),
          contract.Scalar.insert(contract.Entity.alias(2), { value: 0 }),
          { kind: "createStateOverlayOwner", alias: 10 },
          {
            kind: "attachEntityOverlayBinding",
            owner,
            alias: 11,
            symbolicId: "alias-target",
            mode: "bound",
          },
          {
            kind: "attachComponentStateOverlay",
            owner,
            binding: { kind: "alias", alias: 11 },
            alias: 12,
            component: contract.LinearDriver.id,
            mode: "auto",
            fields: [
              sourceField(1),
              {
                offset: contract.LinearDriver.fields.scale.offset,
                value: { kind: "f32", value: 3 },
              },
            ],
          },
        ]),
      );
      const owned = outcome.stateOverlays.find(
        (resource) => resource.alias === 10,
      );
      const attached = outcome.stateOverlays.find(
        (resource) => resource.alias === 12,
      );
      if (!owned || !attached)
        throw new Error("Attachment omitted acknowledged resource handles");
      owner = { kind: "handle", id: owned.id };
      overlay = { kind: "handle", id: attached.id };
    },
    async updateToNewSource() {
      success(
        await batch([
          contract.Entity.create(3, { symbolicId: "source-b" }),
          contract.Scalar.insert(contract.Entity.alias(3), { value: 5 }),
          update(3),
        ]),
      );
    },
    async rejectInvalidSource(deleted) {
      const operations = deleted
        ? [
            contract.Entity.create(4),
            contract.Entity.delete(contract.Entity.alias(4)),
            update(4),
          ]
        : [update(99)];
      const outcome = await batch(operations);
      if (outcome.ok)
        throw new Error("Invalid source alias unexpectedly committed");
      return outcome.error.reason;
    },
    async releaseStateOverlayOwner() {
      success(await batch([{ kind: "releaseStateOverlayOwner", owner }]));
    },
    async observe() {
      const state = await client.inspect();
      await record("overlay_inspection", state);
      const target = state.entities.find(
        (entity) => entity.metadata.symbolicId === "alias-target",
      );
      const sourceA = state.entities.find(
        (entity) => entity.metadata.symbolicId === "source-a",
      );
      const sourceB = state.entities.find(
        (entity) => entity.metadata.symbolicId === "source-b",
      );
      const scalar = target?.effective.find(
        (component) => component.component === contract.Scalar.id,
      );
      const driver = target?.effective.find(
        (component) => component.component === contract.LinearDriver.id,
      );
      if (!sourceA || typeof scalar?.fields.value !== "number")
        throw new Error("Expected scalar scene state missing");
      return {
        value: scalar.fields.value,
        source: driver ? String(driver.fields.source) : null,
        sourceA: String(sourceA.id),
        sourceB: sourceB ? String(sourceB.id) : null,
      };
    },
  };
}

function success(outcome: BatchOutcome): Extract<BatchOutcome, { ok: true }> {
  if (!outcome.ok)
    throw new Error(
      `Alias batch rejected at ${outcome.error.operation}: ${outcome.error.reason}`,
    );
  return outcome;
}
