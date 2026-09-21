import type { Command, EntityRef, SpatialWorldClient } from "@ipp/client";
import type { DriverConnectOptions } from "../driver.js";
import type {
  ResourceObservation,
  ResourcePressureDriver,
} from "../scenarios/resource-pressure.js";

export interface ResourceContract {
  readonly Entity: {
    create(alias: number): Command;
    alias(alias: number): EntityRef;
  };
  readonly UnlitTexture: {
    readonly id: number;
    insert(entity: EntityRef, values: { source: string }): Command;
  };
}

/** Generated SDK operations, shared by the native and browser environments. */
export function resourcePressureDriver(
  client: SpatialWorldClient,
  contract: ResourceContract,
  record: DriverConnectOptions["record"],
): ResourcePressureDriver {
  const events: ResourceObservation[] = [];
  let recordedEvents = 0;
  client.onResourceChange((event) => events.push({ ...event }));
  let tick: bigint | undefined;
  return {
    async declare(sources) {
      const outcome = await client.batch(
        sources.flatMap((source, alias) => [
          contract.Entity.create(alias),
          contract.UnlitTexture.insert(contract.Entity.alias(alias), {
            source,
          }),
        ]),
      );
      await record("resource_batch", { sources, outcome });
      if (!outcome.ok)
        throw new Error(
          `Resource declarations rejected: ${outcome.error.reason}`,
        );
      tick = outcome.tick;
    },
    async observe() {
      const state = await client.inspect();
      const newEvents = events.slice(recordedEvents);
      recordedEvents = events.length;
      await record("resource_inspection", { state, events: newEvents });
      tick = state.tick;
      return {
        session: String(client.session),
        entities: state.entities.length,
        resources: state.resources,
        declarations: state.entities.map((entity) => ({
          entity: String(entity.id),
          baseSource:
            entity.base.find(
              (component) => component.component === contract.UnlitTexture.id,
            )?.fields.source ?? null,
          effectiveSource:
            entity.effective.find(
              (component) => component.component === contract.UnlitTexture.id,
            )?.fields.source ?? null,
        })),
      };
    },
    async nextFrame() {
      tick = (await client.waitForFrame(tick)).tick;
    },
    events: () => events,
  };
}
