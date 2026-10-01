import { BatchDeliveryGate } from "./batch-delivery-gate.js";
import {
  type ReactRuntimeConfiguration,
  type ScalarObservation,
  connect,
  loadContract,
  removeScalar,
  requireSuccess,
  requireAlias,
  observeScalar,
  observeScalarInInspection,
  findEntity,
  rejectedMessage,
  settleRoots,
  isPending,
} from "./fixture-helpers.js";
export type { ReactRuntimeConfiguration } from "./fixture-helpers.js";
export { childrenHierarchy } from "./hierarchy-case.js";
export {
  customMaterialProperties,
  createShaderPreview,
} from "./materials-case.js";
export { namedAssets, createPendingAsset } from "./assets-case.js";
export { animationComponents } from "./animation-case.js";

import * as React from "react";
import {
  createRoot,
  Children,
  Entity,
  Scalar,
  type EntityProps,
} from "@ipp/react";
import { workerTransport } from "@ipp/client";
import { reactRootSystems } from "../integration/system-selections.js";

interface DeliveryGateReport {
  readonly bufferedResponses: number;
  readonly bufferedFrames: number;
  readonly renderPendingBeforeRelease: boolean;
  readonly unmountPendingBeforeRelease: boolean;
}

/**
 * React writes plain component fields: bound entities are adopted and written
 * in place, last write wins between roots and clients, a removed prop leaves
 * its value, and cleanup deletes declared entities and removes only the
 * components the root inserted.
 */
export async function plainFieldLifecycle(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly boundValues: readonly ScalarObservation[];
  readonly sharedValues: readonly ScalarObservation[];
  readonly boundMissingRejection: string;
  readonly boundMissingAfterRejection: ScalarObservation;
  readonly insertedComponentValues: readonly ScalarObservation[];
  readonly adoptedEntityValues: readonly ScalarObservation[];
  readonly declaredBeforeClear: ScalarObservation;
  readonly declaredExistsAfterClear: boolean;
  readonly boundExistsAfterClear: boolean;
}> {
  const { contract, client } = await connect(configuration);
  const producer = await requireSuccess(
    client.batch([
      contract.Entity.create(1, {
        symbolicId: "react-bound",
        classes: ["react-fixture"],
      }),
      contract.Scalar.insert(contract.Entity.alias(1), { value: 10 }),
      contract.Entity.create(2, {
        symbolicId: "react-shared",
        classes: ["react-fixture"],
      }),
      contract.Entity.create(3, {
        symbolicId: "react-inserted-component",
        classes: ["react-fixture"],
      }),
      contract.Entity.create(4, {
        symbolicId: "react-adopted-entity",
        classes: ["react-fixture"],
      }),
      contract.Scalar.insert(contract.Entity.alias(4), { value: 4 }),
    ]),
  );
  const boundEntity = requireAlias(producer, 1);
  const insertedComponentEntity = requireAlias(producer, 3);
  const adoptedEntity = requireAlias(producer, 4);
  const observe = (symbolicId: string) =>
    observeScalar(client, contract.Scalar.id, symbolicId);

  const boundRoot = createRoot(client);
  const firstSharedRoot = createRoot(client);
  const secondSharedRoot = createRoot(client);
  const declaredRoot = createRoot(client);
  const boundMissingRoot = createRoot(client);
  const insertedComponentRoot = createRoot(client);
  const adoptedEntityRoot = createRoot(client);
  try {
    // The bound entity's existing Scalar is adopted and written in place.
    await boundRoot.render(
      declaration({ bindTo: "react-bound", scalar: { value: 20 } }),
    );
    const adopted = await observe("react-bound");
    // A client write is the newest value.
    await requireSuccess(
      client.batch([
        contract.Scalar.setValue(contract.Entity.handle(boundEntity), 30),
      ]),
    );
    const clientWrite = await observe("react-bound");
    // Removing the prop writes nothing.
    await boundRoot.render(declaration({ bindTo: "react-bound", scalar: {} }));
    const propRemoved = await observe("react-bound");
    await requireSuccess(client.batch([removeScalar(contract, boundEntity)]));
    const componentRemoved = await observe("react-bound");
    await requireSuccess(
      client.batch([
        contract.Scalar.insert(contract.Entity.handle(boundEntity), {
          value: 40,
        }),
      ]),
    );
    const componentInserted = await observe("react-bound");
    // Declaring the prop again writes it again.
    await boundRoot.render(
      declaration({ bindTo: "react-bound", scalar: { value: 50 } }),
    );
    const declaredAgain = await observe("react-bound");
    await requireSuccess(
      client.batch([
        removeScalar(contract, boundEntity),
        contract.Scalar.insert(contract.Entity.handle(boundEntity), {
          value: 45,
        }),
      ]),
    );
    const replaced = await observe("react-bound");

    // Two roots write one field: the last write wins, and removing either
    // root's declaration removes the component, adopted or inserted; the
    // other root's later removal finds it already gone.
    await firstSharedRoot.render(
      declaration({ bindTo: "react-shared", scalar: { value: 11 } }),
    );
    const firstInserted = await observe("react-shared");
    await secondSharedRoot.render(
      declaration({ bindTo: "react-shared", scalar: { value: 22 } }),
    );
    const secondWrote = await observe("react-shared");
    await firstSharedRoot.render(
      declaration({ bindTo: "react-shared", scalar: { value: 33 } }),
    );
    const firstWroteLast = await observe("react-shared");
    await secondSharedRoot.render(null);
    const adopterUnmounted = await observe("react-shared");
    await firstSharedRoot.render(
      declaration({ bindTo: "react-shared", scalar: {} }),
    );
    const firstPropRemoved = await observe("react-shared");
    await firstSharedRoot.render(null);
    const inserterUnmounted = await observe("react-shared");

    // A bound entity that does not exist rejects the commit.
    const boundMissingRejection = await rejectedMessage(
      boundMissingRoot.render(
        declaration({ bindTo: "react-bound-missing", scalar: { value: 6 } }),
      ),
    );
    const boundMissingAfterRejection = await observe("react-bound-missing");

    // A removed component declaration removes the component even after a
    // client replaced it: removal is not fenced by incarnation.
    await insertedComponentRoot.render(
      declaration({
        bindTo: "react-inserted-component",
        scalar: { value: 13 },
      }),
    );
    const insertedMounted = await observe("react-inserted-component");
    await requireSuccess(
      client.batch([
        removeScalar(contract, insertedComponentEntity),
        contract.Scalar.insert(
          contract.Entity.handle(insertedComponentEntity),
          { value: 17 },
        ),
      ]),
    );
    const insertedReplaced = await observe("react-inserted-component");
    await insertedComponentRoot.render(null);
    const insertedAfterUnmount = await observe("react-inserted-component");

    // A bound entity replaced by a client under the same symbolic id is
    // written through its symbol, and removing the declaration removes the
    // replacement's component through the same symbol.
    await adoptedEntityRoot.render(
      declaration({ bindTo: "react-adopted-entity", scalar: { value: 91 } }),
    );
    const adoptedMounted = await observe("react-adopted-entity");
    await requireSuccess(
      client.batch([
        contract.Entity.delete(contract.Entity.handle(adoptedEntity)),
      ]),
    );
    await client.waitForFrame();
    const adoptedDeleted = await observe("react-adopted-entity");
    await requireSuccess(
      client.batch([
        contract.Entity.create(1, {
          symbolicId: "react-adopted-entity",
          classes: ["producer-replacement"],
        }),
        contract.Scalar.insert(contract.Entity.alias(1), { value: 27 }),
      ]),
    );
    const adoptedReplacement = await observe("react-adopted-entity");
    await adoptedEntityRoot.render(null);
    const adoptedAfterUnmount = await observe("react-adopted-entity");

    // A declared entity is created with its values and deleted when its
    // declaration is removed.
    await declaredRoot.render(
      declaration({ id: "react-declared", scalar: { value: 7 } }),
    );
    const declaredBeforeClear = await observe("react-declared");
    await declaredRoot.render(null);
    const declaredExistsAfterClear =
      findEntity(await client.inspect(), "react-declared") !== undefined;

    await boundRoot.render(null);
    const boundExistsAfterClear =
      findEntity(await client.inspect(), "react-bound") !== undefined;

    return {
      boundValues: [
        adopted,
        clientWrite,
        propRemoved,
        componentRemoved,
        componentInserted,
        declaredAgain,
        replaced,
      ],
      sharedValues: [
        firstInserted,
        secondWrote,
        firstWroteLast,
        adopterUnmounted,
        firstPropRemoved,
        inserterUnmounted,
      ],
      boundMissingRejection,
      boundMissingAfterRejection,
      insertedComponentValues: [
        insertedMounted,
        insertedReplaced,
        insertedAfterUnmount,
      ],
      adoptedEntityValues: [
        adoptedMounted,
        adoptedDeleted,
        adoptedReplacement,
        adoptedAfterUnmount,
      ],
      declaredBeforeClear,
      declaredExistsAfterClear,
      boundExistsAfterClear,
    };
  } finally {
    await settleRoots([
      boundRoot,
      firstSharedRoot,
      secondSharedRoot,
      declaredRoot,
      boundMissingRoot,
      insertedComponentRoot,
      adoptedEntityRoot,
    ]);
    await client.close();
  }
}

/**
 * Rejected and unsent commits leave the World and the root's records
 * consistent, a corrected render continues from them, and large declaration
 * sets write in order.
 */
export async function rejectionAndCorrection(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly rejection: string;
  readonly afterRejection: ScalarObservation;
  readonly corrected: ScalarObservation;
  readonly unsentRejection: string;
  readonly afterUnsentRejection: ScalarObservation;
  readonly afterUnsentCorrection: ScalarObservation;
  readonly afterLargeBatch: ScalarObservation;
  readonly afterLargeBatchCleanup: ScalarObservation;
  readonly afterLoss: ScalarObservation;
  readonly afterReplacement: ScalarObservation;
  readonly afterRemoval: ScalarObservation;
  readonly afterRecovery: ScalarObservation;
}> {
  const { contract, client } = await connect(configuration);
  const producer = await requireSuccess(
    client.batch([
      contract.Entity.create(1, {
        symbolicId: "react-strict",
        classes: ["react-fixture"],
      }),
      contract.Scalar.insert(contract.Entity.alias(1), { value: 5 }),
    ]),
  );
  const entity = requireAlias(producer, 1);
  const observe = () =>
    observeScalar(client, contract.Scalar.id, "react-strict");
  const root = createRoot(client);
  try {
    // A symbolic reference to an absent entity rejects in the World.
    const rejection = await rejectedMessage(
      root.render(
        declaration({ bindTo: "react-strict-absent", scalar: { value: 6 } }),
      ),
    );
    const afterRejection = await observe();

    await root.render(
      declaration({ bindTo: "react-strict", scalar: { value: 8 } }),
    );
    const corrected = await observe();

    const unsentRejection = await rejectedMessage(
      root.render(
        declaration({ bindTo: "react-strict", scalar: { value: NaN } }),
      ),
    );
    const afterUnsentRejection = await observe();
    await root.render(
      declaration({ bindTo: "react-strict", scalar: { value: 10 } }),
    );
    const afterUnsentCorrection = await observe();

    // The current host has no operation-count quota. Declarations of one
    // component write in order across a batch larger than the retired
    // 256-operation limit; the last write wins.
    await root.render(
      React.createElement(
        Entity,
        { bindTo: "react-strict" },
        Array.from({ length: 257 }, (_, key) =>
          React.createElement(Scalar, { key, value: key }),
        ),
      ),
    );
    const afterLargeBatch = await observe();
    await root.render(
      declaration({ bindTo: "react-strict", scalar: { value: 11 } }),
    );
    const afterLargeBatchCleanup = await observe();

    await requireSuccess(client.batch([removeScalar(contract, entity)]));
    await client.waitForFrame();
    const afterLoss = await observe();

    await requireSuccess(
      client.batch([
        contract.Scalar.insert(contract.Entity.handle(entity), { value: 9 }),
      ]),
    );
    const afterReplacement = await observe();

    // Removing the declaration removes the component, adopted or inserted.
    await root.render(null);
    const afterRemoval = await observe();

    await root.render(
      declaration({
        bindTo: "react-strict",
        key: "recovered",
        scalar: { value: 12 },
      }),
    );
    const afterRecovery = await observe();

    return {
      rejection,
      afterRejection,
      corrected,
      unsentRejection,
      afterUnsentRejection,
      afterUnsentCorrection,
      afterLargeBatch,
      afterLargeBatchCleanup,
      afterLoss,
      afterReplacement,
      afterRemoval,
      afterRecovery,
    };
  } finally {
    await settleRoots([root]);
    await client.close();
  }
}

export async function hooksAndStrictMode(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly observation: ScalarObservation;
  readonly matchingEntities: number;
  readonly existsAfterUnmount: boolean;
}> {
  const { contract, client } = await connect(configuration);
  const root = createRoot(client);
  try {
    await root.render(
      React.createElement(
        React.StrictMode,
        null,
        React.createElement(StatefulDeclaration),
      ),
    );
    await root.flush();
    const inspection = await client.inspect();
    const observation = observeScalarInInspection(
      inspection,
      contract.Scalar.id,
      "react-hooks",
    );
    const matchingEntities = inspection.entities.filter(
      (candidate) => candidate.metadata.symbolicId === "react-hooks",
    ).length;

    // Unmount deletes nothing.
    await root.unmount();
    const existsAfterUnmount =
      findEntity(await client.inspect(), "react-hooks") !== undefined;
    return { observation, matchingEntities, existsAfterUnmount };
  } finally {
    await settleRoots([root]);
    await client.close();
  }
}

export async function pendingUnmountUsesRealAcknowledgement(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly gate: DeliveryGateReport;
  readonly entityExistsAfterUnmount: boolean;
}> {
  const contract = await loadContract(configuration.generatedModuleUrl);
  const transport = workerTransport(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    contract.MAX_MESSAGE_BYTES,
  );
  const gate = new BatchDeliveryGate(transport);
  const client = await contract.IppClient.connectTransport(gate, {
    selectedSystems: reactRootSystems(contract.CAPABILITIES),
    timeoutMs: configuration.timeoutMs,
  });
  const root = createRoot(client);
  try {
    await client.waitForFrame();
    gate.arm((bytes) => {
      const response = contract.decodeResponse(bytes, client.session);
      return response.body.kind;
    });

    const render = root.render(
      React.createElement(
        Entity,
        { id: "react-pending-unmount" },
        React.createElement(Scalar, { value: 71 }),
        React.createElement(
          Children,
          null,
          declaration({ id: "react-pending-child", scalar: { value: 12 } }),
        ),
      ),
    );
    await gate.waitForBatchAndFrame(configuration.timeoutMs);
    const renderPendingBeforeRelease = await isPending(render);
    const unmount = root.unmount();
    const unmountPendingBeforeRelease = await isPending(unmount);
    const bufferedResponses = gate.bufferedResponses;
    const bufferedFrames = gate.bufferedFrames;

    // The pending render commits; unmount then deletes nothing.
    gate.release();
    await Promise.all([render, unmount]);
    const afterUnmount = await client.inspect();
    const entityExistsAfterUnmount = [
      "react-pending-unmount",
      "react-pending-child",
    ].some((id) => findEntity(afterUnmount, id) !== undefined);

    return {
      gate: {
        bufferedResponses,
        bufferedFrames,
        renderPendingBeforeRelease,
        unmountPendingBeforeRelease,
      },
      entityExistsAfterUnmount,
    };
  } finally {
    gate.release();
    await settleRoots([root]);
    await client.close();
  }
}

function StatefulDeclaration(): React.ReactNode {
  const [value, setValue] = React.useState(1);
  React.useLayoutEffect(() => {
    setValue(42);
  }, []);
  return declaration({ id: "react-hooks", scalar: { value } });
}

function declaration(
  options: EntityProps & {
    readonly key?: string;
    readonly scalar: { readonly value?: number };
  },
): React.ReactElement {
  const entityProps =
    options.id === undefined
      ? { bindTo: options.bindTo, key: options.key }
      : { id: options.id, key: options.key };
  return React.createElement(
    Entity,
    entityProps,
    React.createElement(Scalar, options.scalar),
  );
}

/** Named resources use real worker decoding and independently owned playback. */
/** The WebGL scenario owns the host; this driver only authors an editor through React. */
/** Standalone declarations survive no consumer and cancel pending observer delivery. */
/** Real worker controllers with only their resource notification boundary gated. */
