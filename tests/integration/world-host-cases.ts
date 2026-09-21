import {
  commandBatches,
  byteLimitedCommandBuffers,
  commandBatchBeforeTrackAsset,
} from "./scenarios/command-batches.js";
import {
  propertyAnimationScenario,
  type AnimationContract,
} from "./animation-fixtures.js";
import type { AnimationWorldClient, GeometryEncoder } from "@ipp/client";
import { cameraCases, CameraFixture } from "./scenarios/cameras-and-picking.js";
import { renderStatePreservesDeclarations } from "./scenarios/render-state.js";
import type {
  PickingWorldClient,
  SpatialWorldClient,
  RenderWorldClient,
} from "@ipp/client";
import type { DriverConnectOptions } from "./driver.js";
import {
  overlayAliasDriver,
  type OverlayAliasContract,
} from "./drivers/overlay-aliases.js";
import {
  resourcePressureDriver,
  type ResourceContract,
} from "./drivers/resources.js";
import { overlayAliasesResolve } from "./scenarios/overlay-aliases.js";
import { assetSourceDuringBatch } from "./scenarios/command-batches.js";
import { resourcePressurePreservesSession } from "./scenarios/resource-pressure.js";

export type WorldHostContract = OverlayAliasContract &
  ResourceContract &
  AnimationContract & {
    encodeBoundingShape: GeometryEncoder;
    encodeRequest(
      request: import("@ipp/client").Request,
    ): Uint8Array<ArrayBuffer>;
  };

/** The same production-client cases run in native and browser host environments. */
export const worldHostCases: ReadonlyArray<{
  name: string;
  run(
    client: SpatialWorldClient,
    contract: WorldHostContract,
    record: DriverConnectOptions["record"],
  ): Promise<unknown>;
}> = [
  {
    name: "asset source delivery bypasses held command batches and command byte limits",
    run: (client, contract, record) =>
      assetSourceDuringBatch(client as AnimationWorldClient, contract, record),
  },
  {
    name: "byte-limited command buffers remain one logical batch",
    run: (client, contract, record) =>
      byteLimitedCommandBuffers(client, contract.encodeRequest, record),
  },
  {
    name: "command batches and track references commit before track asset production",
    run: (client, contract, record) =>
      commandBatchBeforeTrackAsset(
        client as AnimationWorldClient,
        contract,
        record,
      ),
  },
  {
    name: "chained command buffers require an explicit terminator and expire without rollback",
    run: (client, _contract, record) => commandBatches(client, record),
  },
  {
    name: "property animation uses controller clocks and Bezier keyframes",
    run: (client, contract, record) =>
      propertyAnimationScenario(
        client as AnimationWorldClient,
        contract,
        record,
      ),
  },
  {
    name: "sparse render state preserves independent boolean debug declarations",
    run: (client, _contract, record) =>
      renderStatePreservesDeclarations(client as RenderWorldClient, record),
  },
  ...cameraCases.map((scenario) => ({
    name: scenario.name,
    run: async (
      client: SpatialWorldClient,
      contract: WorldHostContract,
      record: DriverConnectOptions["record"],
    ) => {
      if (!client.capabilities.picking)
        throw new Error("Camera scenarios require picking");
      return scenario.run(
        new CameraFixture(
          client as PickingWorldClient,
          record,
          contract.encodeBoundingShape,
        ),
      );
    },
  })),
  {
    name: "asset growth beyond former quotas preserves outcomes and the session",
    run: (client, contract, record) =>
      resourcePressurePreservesSession(
        resourcePressureDriver(client, contract, record),
      ),
  },
  {
    name: "overlay aliases resolve and rejected updates preserve bindings",
    run: (client, contract, record) =>
      overlayAliasesResolve(overlayAliasDriver(client, contract, record)),
  },
];
