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

interface DeliveryGateReport {
  readonly bufferedResponses: number;
  readonly bufferedFrames: number;
  readonly renderPendingBeforeRelease: boolean;
  readonly unmountPendingBeforeRelease: boolean;
}

export async function ownershipAndAutomaticLifecycle(
  configuration: ReactRuntimeConfiguration,
): Promise<{
  readonly boundValues: readonly ScalarObservation[];
  readonly sharedFallbackValues: readonly ScalarObservation[];
  readonly boundMissingRejection: string;
  readonly boundMissingAfterRejection: ScalarObservation;
  readonly ownedComponentValues: readonly ScalarObservation[];
  readonly autoEntityValues: readonly ScalarObservation[];
  readonly ownedBeforeClear: ScalarObservation;
  readonly ownedExistsAfterClear: boolean;
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
        symbolicId: "react-shared-fallback",
        classes: ["react-fixture"],
      }),
      contract.Entity.create(3, {
        symbolicId: "react-owned-component",
        classes: ["react-fixture"],
      }),
      contract.Entity.create(4, {
        symbolicId: "react-auto-entity",
        classes: ["react-fixture"],
      }),
      contract.Scalar.insert(contract.Entity.alias(4), { value: 4 }),
      contract.Entity.create(5, {
        symbolicId: "react-bound-missing",
        classes: ["react-fixture"],
      }),
    ]),
  );
  const boundEntity = requireAlias(producer, 1);
  const ownedComponentEntity = requireAlias(producer, 3);
  const autoEntity = requireAlias(producer, 4);

  const boundRoot = createRoot(client);
  const firstFallbackRoot = createRoot(client);
  const secondFallbackRoot = createRoot(client);
  const ownedRoot = createRoot(client);
  const boundMissingRoot = createRoot(client);
  const ownedComponentRoot = createRoot(client);
  const autoEntityRoot = createRoot(client);
  try {
    await boundRoot.render(
      declaration({
        bindTo: "react-bound",
        scalar: { value: 20 },
      }),
    );
    const initial = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await requireSuccess(
      client.batch([
        contract.Scalar.setValue(contract.Entity.handle(boundEntity), 30),
      ]),
    );
    const hiddenBaseUpdate = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await boundRoot.render(
      declaration({
        bindTo: "react-bound",
        scalar: {},
      }),
    );
    const cleared = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await requireSuccess(client.batch([removeScalar(contract, boundEntity)]));
    const fallbackAfterRemoval = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await requireSuccess(
      client.batch([
        contract.Scalar.insert(contract.Entity.handle(boundEntity), {
          value: 40,
        }),
      ]),
    );
    const baseAfterAddition = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await boundRoot.render(
      declaration({
        bindTo: "react-bound",
        scalar: { value: 50 },
      }),
    );
    await requireSuccess(
      client.batch([
        removeScalar(contract, boundEntity),
        contract.Scalar.insert(contract.Entity.handle(boundEntity), {
          value: 45,
        }),
      ]),
    );
    const overlayAfterReplacement = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound",
    );

    await firstFallbackRoot.render(
      declaration({
        bindTo: "react-shared-fallback",
        scalar: { value: 11 },
      }),
    );
    const firstFallback = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );
    await secondFallbackRoot.render(
      declaration({
        bindTo: "react-shared-fallback",
        scalar: { value: 22 },
      }),
    );
    const secondWins = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );
    await firstFallbackRoot.render(
      declaration({
        bindTo: "react-shared-fallback",
        scalar: { value: 33 },
      }),
    );
    const newerStillWins = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );
    await secondFallbackRoot.render(null);
    const firstRevealed = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );
    await firstFallbackRoot.render(
      declaration({
        bindTo: "react-shared-fallback",
        scalar: {},
      }),
    );
    const defaultRevealed = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );
    await firstFallbackRoot.render(null);
    const fallbackReleased = await observeScalar(
      client,
      contract.Scalar.id,
      "react-shared-fallback",
    );

    const boundMissingRejection = await rejectedMessage(
      boundMissingRoot.render(
        declaration({
          bindTo: "react-bound-missing",
          scalar: { bound: true, value: 6 },
        }),
      ),
    );
    const boundMissingAfterRejection = await observeScalar(
      client,
      contract.Scalar.id,
      "react-bound-missing",
    );

    await ownedComponentRoot.render(
      declaration({
        bindTo: "react-owned-component",
        scalar: { bound: false, value: 13 },
      }),
    );
    const ownedComponentMounted = await observeScalar(
      client,
      contract.Scalar.id,
      "react-owned-component",
    );
    await requireSuccess(
      client.batch([removeScalar(contract, ownedComponentEntity)]),
    );
    await requireSuccess(
      client.batch([
        contract.Scalar.insert(contract.Entity.handle(ownedComponentEntity), {
          value: 17,
        }),
      ]),
    );
    const ownedComponentReplacement = await observeScalar(
      client,
      contract.Scalar.id,
      "react-owned-component",
    );
    await ownedComponentRoot.render(null);
    const ownedComponentAfterStaleCleanup = await observeScalar(
      client,
      contract.Scalar.id,
      "react-owned-component",
    );

    await autoEntityRoot.render(
      declaration({
        bindTo: "react-auto-entity",
        scalar: { value: 91 },
      }),
    );
    const autoEntityMounted = await observeScalar(
      client,
      contract.Scalar.id,
      "react-auto-entity",
    );
    await requireSuccess(
      client.batch([
        contract.Entity.delete(contract.Entity.handle(autoEntity)),
      ]),
    );
    await client.waitForFrame();
    const autoEntityDeleted = await observeScalar(
      client,
      contract.Scalar.id,
      "react-auto-entity",
    );
    await requireSuccess(
      client.batch([
        contract.Entity.create(1, {
          symbolicId: "react-auto-entity",
          classes: ["producer-replacement"],
        }),
        contract.Scalar.insert(contract.Entity.alias(1), { value: 27 }),
      ]),
    );
    const autoEntityReplacement = await observeScalar(
      client,
      contract.Scalar.id,
      "react-auto-entity",
    );
    await autoEntityRoot.render(null);
    const autoEntityAfterStaleCleanup = await observeScalar(
      client,
      contract.Scalar.id,
      "react-auto-entity",
    );

    await ownedRoot.render(
      declaration({
        id: "react-owned",
        scalar: { bound: false, value: 7 },
      }),
    );
    const ownedBeforeClear = await observeScalar(
      client,
      contract.Scalar.id,
      "react-owned",
    );
    await ownedRoot.render(null);
    const ownedExistsAfterClear =
      findEntity(await client.inspect(), "react-owned") !== undefined;

    await boundRoot.render(null);
    const boundExistsAfterClear =
      findEntity(await client.inspect(), "react-bound") !== undefined;

    return {
      boundValues: [
        initial,
        hiddenBaseUpdate,
        cleared,
        fallbackAfterRemoval,
        baseAfterAddition,
        overlayAfterReplacement,
      ],
      sharedFallbackValues: [
        firstFallback,
        secondWins,
        newerStillWins,
        firstRevealed,
        defaultRevealed,
        fallbackReleased,
      ],
      boundMissingRejection,
      boundMissingAfterRejection,
      ownedComponentValues: [
        ownedComponentMounted,
        ownedComponentReplacement,
        ownedComponentAfterStaleCleanup,
      ],
      autoEntityValues: [
        autoEntityMounted,
        autoEntityDeleted,
        autoEntityReplacement,
        autoEntityAfterStaleCleanup,
      ],
      ownedBeforeClear,
      ownedExistsAfterClear,
      boundExistsAfterClear,
    };
  } finally {
    await settleRoots([
      boundRoot,
      firstFallbackRoot,
      secondFallbackRoot,
      ownedRoot,
      boundMissingRoot,
      ownedComponentRoot,
      autoEntityRoot,
    ]);
    await client.close();
  }
}

export async function strictBindingAndCorrection(
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
  readonly diagnostics: readonly string[];
  readonly afterStrictLoss: ScalarObservation;
  readonly afterReplacement: ScalarObservation;
  readonly afterStaleCleanup: ScalarObservation;
  readonly afterExplicitRecovery: ScalarObservation;
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
  const diagnostics: string[] = [];
  const root = createRoot(client, {
    onDiagnostic: (diagnostic) => diagnostics.push(diagnostic.reason),
  });
  try {
    const rejection = await rejectedMessage(
      root.render(
        declaration({
          bindTo: "react-strict",
          scalar: { bound: false, value: 6 },
        }),
      ),
    );
    const afterRejection = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    await root.render(
      declaration({
        bindTo: "react-strict",
        scalar: { bound: true, value: 8 },
      }),
    );
    const corrected = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    const unsentRejection = await rejectedMessage(
      root.render(
        declaration({
          bindTo: "react-strict",
          scalar: { bound: true, value: NaN },
        }),
      ),
    );
    const afterUnsentRejection = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );
    await root.render(
      declaration({
        bindTo: "react-strict",
        scalar: { bound: true, value: 10 },
      }),
    );
    const afterUnsentCorrection = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    // The current host has no operation-count quota. Preserve attachment order
    // and cleanup across a batch larger than the retired 256-operation limit.
    await root.render(
      React.createElement(
        Entity,
        { bindTo: "react-strict" },
        Array.from({ length: 257 }, (_, key) =>
          React.createElement(Scalar, { key, value: key }),
        ),
      ),
    );
    const afterLargeBatch = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );
    await root.render(
      declaration({
        bindTo: "react-strict",
        scalar: { bound: true, value: 11 },
      }),
    );
    const afterLargeBatchCleanup = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    await requireSuccess(client.batch([removeScalar(contract, entity)]));
    await client.waitForFrame();
    const afterStrictLoss = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    await requireSuccess(
      client.batch([
        contract.Scalar.insert(contract.Entity.handle(entity), { value: 9 }),
      ]),
    );
    const afterReplacement = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    await root.render(null);
    const afterStaleCleanup = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    await root.render(
      declaration({
        bindTo: "react-strict",
        key: "recovered",
        scalar: { bound: true, value: 12 },
      }),
    );
    const afterExplicitRecovery = await observeScalar(
      client,
      contract.Scalar.id,
      "react-strict",
    );

    return {
      rejection,
      afterRejection,
      corrected,
      unsentRejection,
      afterUnsentRejection,
      afterUnsentCorrection,
      afterLargeBatch,
      afterLargeBatchCleanup,
      diagnostics,
      afterStrictLoss,
      afterReplacement,
      afterStaleCleanup,
      afterExplicitRecovery,
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
  );
  const gate = new BatchDeliveryGate(transport);
  const client = await contract.IppClient.connectTransport(gate, {
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
        React.createElement(Scalar, { bound: false, value: 71 }),
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
  return declaration({
    id: "react-hooks",
    scalar: { bound: false, value },
  });
}

function declaration(
  options: EntityProps & {
    readonly key?: string;
    readonly scalar: {
      readonly bound?: boolean | null;
      readonly value?: number;
    };
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
