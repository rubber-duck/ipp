import type { SpatialWorldClient } from "@ipp/client";
import type { DriverConnectOptions } from "./driver.js";
import {
  lifecycleSubscriptions,
  lifecycleOverflow,
  assetLifecycleSubscriptions,
} from "./scenarios/lifecycle-subscriptions.js";

/** Shared scenario intent; native/socket and browser/worker drivers own launch. */
export const lifecycleHostCases: ReadonlyArray<{
  name: string;
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
    run: (
      client: SpatialWorldClient,
      _contract: unknown,
      record: DriverConnectOptions["record"],
    ) => assetLifecycleSubscriptions(client, record),
  },
  {
    name: "lifecycle subscriptions bound bursts and recover after overflow",
    run: (
      client: SpatialWorldClient,
      _contract: unknown,
      record: DriverConnectOptions["record"],
    ) => lifecycleOverflow(client, record),
  },
];
