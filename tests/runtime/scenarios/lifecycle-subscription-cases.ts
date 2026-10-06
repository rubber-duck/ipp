import type { SpatialWorldClient } from "@ipp/client";
import type { DriverConnectOptions } from "../../harness/driver.js";
import {
  lifecycleSubscriptions,
  lifecycleBurst,
  assetLifecycleSubscriptions,
} from "./lifecycle-subscriptions.js";
import {
  CONSTRAINTS,
  LIFECYCLE,
  RENDER,
  selectSystems,
} from "../../fixtures/system-selections.js";

/** Systems of the World the lifecycle cases author: Scalars and meshes whose
 * assets and lifecycle the subscriptions observe. */
export const LIFECYCLE_SUBSCRIPTION_SYSTEMS = selectSystems(
  RENDER,
  CONSTRAINTS,
  LIFECYCLE,
);

/** Shared scenario intent; native/socket and browser/worker drivers own launch. */
export const lifecycleHostCases: ReadonlyArray<{
  name: string;
  /** Host unused-asset cache target the case needs; omitted keeps the default. */
  assetCacheBytes?: number;
  run(
    client: SpatialWorldClient,
    contract: unknown,
    record: DriverConnectOptions["record"],
  ): Promise<unknown>;
}> = [
  {
    name: "lifecycle subscriptions preserve applied effects and filtered identities",
    run: (
      client: SpatialWorldClient,
      _contract: unknown,
      record: DriverConnectOptions["record"],
    ) => lifecycleSubscriptions(client, record),
  },
  {
    name: "asset lifecycle subscriptions observe real provider availability",
    // Retirement at the last consumer's release needs a Host that evicts on release.
    assetCacheBytes: 0,
    run: (
      client: SpatialWorldClient,
      _contract: unknown,
      record: DriverConnectOptions["record"],
    ) => assetLifecycleSubscriptions(client, record),
  },
  {
    name: "lifecycle subscriptions deliver every observation of a burst in order",
    run: (
      client: SpatialWorldClient,
      _contract: unknown,
      record: DriverConnectOptions["record"],
    ) => lifecycleBurst(client, record),
  },
];
