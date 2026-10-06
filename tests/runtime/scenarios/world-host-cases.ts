import { surfaceSpacingTransitions } from "./surface-animation.js";
import {
  commandBatches,
  byteLimitedCommandBuffers,
  commandBatchBeforeTrackAsset,
} from "./command-batches.js";
import {
  propertyAnimationScenario,
  type AnimationContract,
} from "../../fixtures/animation.js";
import type {
  AnimationWorldClient,
  Client,
  GeometryEncoder,
  HostClientBase,
} from "@ipp/client";
import {
  cameraCases,
  CameraFixture,
} from "../../rendering/scenarios/cameras-and-picking.js";
import { renderStatePreservesDeclarations } from "../../rendering/scenarios/render-state.js";
import type {
  PickingWorldClient,
  SpatialWorldClient,
  RenderWorldClient,
} from "@ipp/client";
import type { DriverConnectOptions } from "../../harness/driver.js";
import {
  resourcePressureDriver,
  type ResourceContract,
} from "../drivers/resources.js";
import { assetSourceDuringBatch } from "./command-batches.js";
import { resourcePressurePreservesSession } from "./resource-pressure.js";
import { oversizedInspectionRecord } from "./inspection-limits.js";
import {
  CONSTRAINTS,
  LIFECYCLE,
  SCENE,
  selectSystems,
} from "../../fixtures/system-selections.js";

export type WorldHostContract = ResourceContract &
  AnimationContract & {
    encodeBoundingShape: GeometryEncoder;
    encodeRequest(
      request: import("@ipp/client").Request,
    ): Uint8Array<ArrayBuffer>;
  };

/** The same production-client cases run in native and browser host environments. */
/** Systems of the World every world-host case authors: animated Scalars and
 * drivers, cameras with picking geometry, rendered and custom-material meshes,
 * render state and lifecycle observation. */
export const WORLD_HOST_SYSTEMS = selectSystems(SCENE, CONSTRAINTS, LIFECYCLE);

export const worldHostCases: ReadonlyArray<{
  name: string;
  run(
    client: SpatialWorldClient,
    contract: WorldHostContract,
    record: DriverConnectOptions["record"],
    host: HostClientBase<Client>,
  ): Promise<unknown>;
}> = [
  {
    name: "Surface spacing transitions interpolate and retarget at held clip endpoints",
    run: (_client, contract, record, host) =>
      surfaceSpacingTransitions(host, contract, record),
  },
  {
    name: "asset source delivery bypasses open command batches and command byte limits",
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
      host: HostClientBase<Client>,
    ) => {
      return scenario.run(
        new CameraFixture(
          client as PickingWorldClient,
          host,
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
    name: "an oversized inspection record fails explicitly and the session keeps serving",
    run: (client, _contract, record) =>
      oversizedInspectionRecord(client, record),
  },
];
