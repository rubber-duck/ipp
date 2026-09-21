import type {
  FieldWrite,
  RenderWorldClient,
  RenderStatePatch,
  RenderStateUpdatedEvent,
} from "@ipp/client";
import type { DriverConnectOptions } from "../driver.js";
import { aliasId, createEntity, successfulBatch } from "../camera-fixtures.js";

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** Runs unchanged through generated native WebSocket and browser worker clients. */
export async function renderStatePreservesDeclarations(
  client: RenderWorldClient,
  record: DriverConnectOptions["record"],
) {
  const component = client.components.BoundingGeometry;
  check(
    component,
    "World contract must expose BoundingGeometry even without debug assets",
  );
  check(
    component.fields.is_rendered?.kind === 7,
    "Visibility must use the generated boolean kind",
  );
  const visible = (value: boolean): FieldWrite => ({
    offset: component.fields.is_rendered!.offset,
    value: { kind: "bool", value },
  });
  const events: RenderStateUpdatedEvent[] = [];
  const dispose = client.onRenderStateUpdated((event) => events.push(event));
  const entities: bigint[] = [];
  let previousTick = 0n;
  const update = async (
    changes: RenderStatePatch,
    expected: RenderStatePatch | null = changes,
  ) => {
    const seen = events.length;
    const submitted = client.sendCommand({
      type: "RenderStateUpdateCommand",
      changes,
    });
    check(
      submitted === undefined,
      "Render-state commands return no reply waiter",
    );
    const inspection = await client.inspect();
    await client.waitForFrame(inspection.tick);
    const notifications = events.slice(seen);
    await record("render_state_update", { changes, notifications });
    if (expected === null) {
      check(notifications.length === 0, "No-op patches emit no notification");
      return;
    }
    check(
      notifications.length === 1,
      "Changed state must notify observers exactly once",
    );
    const event = notifications[0]!;
    check(
      event.session === client.session && event.requestId === 0n,
      "Notifications identify their session without command correlation",
    );
    check(event.tick >= previousTick, "Render state ticks must remain ordered");
    previousTick = event.tick;
    check(
      JSON.stringify(event.changes) === JSON.stringify(expected),
      "Notification must contain only committed changed fields",
    );
    check(
      !("ok" in event) && !("error" in event),
      "Notifications do not report command outcomes",
    );
  };
  const inspectVisibility = async () => {
    const inspection = await client.inspect();
    await record("debug_geometry_inspection", inspection);
    return entities.map((id) => {
      const entity = inspection.entities.find((entity) => entity.id === id);
      check(entity, "Debug entity must survive global overrides");
      return [entity.base, entity.effective].map((layer) => {
        const value = layer.find((value) => value.component === component.id)
          ?.fields.is_rendered;
        check(
          typeof value === "boolean",
          "Inspected visibility must remain a boolean",
        );
        return value;
      });
    });
  };
  try {
    const created = await client.batch(
      [1, 2].flatMap((alias) => [
        createEntity(alias, `render-state-${alias}`),
        {
          kind: "insertComponent" as const,
          entity: { kind: "alias" as const, alias },
          component: component.id,
          fields: [visible(alias === 1)],
        },
      ]),
    );
    await record("debug_geometry_batch", created);
    entities.push(aliasId(created, 1), aliasId(created, 2));
    const before = await inspectVisibility();
    check(
      JSON.stringify(before) === "[[true,true],[false,false]]",
      "Per-entity visibility must begin independently",
    );
    await update({ showAllDebugGeometries: true });
    await update({ debugGeometryColor: [0.25, 0.5, 1] });
    await update({}, null);
    await update({ showAllDebugGeometries: true }, null);
    await update(
      { showAllDebugGeometries: true, debugGeometryColor: [0.5, 0.5, 1] },
      { debugGeometryColor: [0.5, 0.5, 1] },
    );
    await update({ showAllDebugGeometries: false });
    check(
      JSON.stringify(await inspectVisibility()) === JSON.stringify(before),
      "Global settings must preserve authored and effective visibility",
    );

    const seen = events.length;
    for (const changes of [
      { unknown: true },
      { showAllDebugGeometries: 1 },
      { debugGeometryColor: [1, 2, 0] },
    ]) {
      let rejected = false;
      try {
        client.sendCommand({
          type: "RenderStateUpdateCommand",
          changes: changes as RenderStatePatch,
        });
      } catch (error) {
        rejected =
          typeof error === "object" &&
          error !== null &&
          "code" in error &&
          error.code === "IPP_REQUEST_NOT_SENT";
      }
      check(rejected, "Invalid patch must reject without submitting bytes");
    }
    check(
      events.length === seen,
      "Local rejections must not fabricate runtime notifications",
    );
    await update({ debugGeometryColor: [1, 0.5, 0] });
    successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: entities[1]! },
          component: component.id,
          field: visible(true),
        },
      ]),
    );
    check(
      JSON.stringify(await inspectVisibility()) === "[[true,true],[true,true]]",
      "Boolean mutations must round-trip through both layers",
    );
    return {
      updates: events.length,
      component: component.id,
      debugAssetsEnabled: client.capabilities.debugGeometry,
    };
  } finally {
    dispose();
    if (entities.length)
      successfulBatch(
        await client.batch(
          entities.map((id) => ({
            kind: "delete",
            entity: { kind: "handle", id },
          })),
        ),
      );
  }
}
