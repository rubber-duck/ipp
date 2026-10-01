import type { Command, SpatialWorldClient } from "@ipp/client";
import { aliasId, createEntity, successfulBatch } from "../camera-fixtures.js";
import type { DriverConnectOptions } from "../driver.js";

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/** Names stay within the 64 KiB protocol string bound; together they exceed 1 MiB. */
const NAME_BYTES = 60_000;
const LARGE_PROPERTIES = 20;
const SMALL_PROPERTIES = ["kept_a", "kept_b"];

/**
 * An inspection record larger than one protocol message is rejected explicitly,
 * never truncated, and the same session keeps serving reads and edits.
 */
export async function oversizedInspectionRecord(
  client: SpatialWorldClient,
  record: DriverConnectOptions["record"],
) {
  const material = client.components.CustomMaterial;
  check(material, "CustomMaterial contract required");
  const large = (index: number) =>
    `large_${index}_${"x".repeat(NAME_BYTES - 16)}`;
  const property = (entity: bigint, name: string): Command => ({
    kind: "setDynamicProperty",
    entity: { kind: "handle", id: entity },
    component: material.id,
    name,
    value: { kind: "f32", value: 0.5 },
  });
  const created = successfulBatch(
    await client.batch([
      createEntity(1, "inspection-limits"),
      {
        kind: "insertComponent",
        entity: { kind: "alias", alias: 1 },
        component: material.id,
        fields: [],
      },
    ]),
  );
  const entity = aliasId(created, 1);
  const properties = async () => {
    const snapshot = (await client.inspect()).entities.find(
      (item) => item.id === entity,
    );
    check(snapshot, "Inspected entity disappeared");
    return Object.keys(
      snapshot.components.find((item) => item.component === material.id)
        ?.properties ?? {},
    ).sort();
  };
  try {
    successfulBatch(
      await client.batch(
        SMALL_PROPERTIES.map((name) => property(entity, name)),
      ),
    );
    check(
      (await properties()).join() === SMALL_PROPERTIES.join(),
      "Named properties were not inspected exactly",
    );
    for (let index = 0; index < LARGE_PROPERTIES; index++)
      successfulBatch(await client.batch([property(entity, large(index))]));
    let oversized: unknown;
    try {
      await client.inspect();
    } catch (error) {
      oversized = error;
    }
    await record("inspection.oversized", {
      entity,
      nameBytes: NAME_BYTES * LARGE_PROPERTIES,
      error: String(oversized),
    });
    check(
      oversized instanceof Error &&
        /Inspection record cannot be encoded/.test(oversized.message),
      `An oversized inspection record was not rejected explicitly: ${String(oversized)}`,
    );
    // The same session still applies edits; nothing was truncated or lost.
    successfulBatch(
      await client.batch(
        Array.from({ length: LARGE_PROPERTIES }, (_, index) => ({
          kind: "removeDynamicProperty" as const,
          entity: { kind: "handle" as const, id: entity },
          component: material.id,
          name: large(index),
        })),
      ),
    );
    check(
      (await properties()).join() === SMALL_PROPERTIES.join(),
      "Inspection after the oversized record lost or truncated named properties",
    );
    return { entity, nameBytes: NAME_BYTES * LARGE_PROPERTIES };
  } finally {
    successfulBatch(
      await client.batch([
        { kind: "delete", entity: { kind: "handle", id: entity } },
      ]),
    );
  }
}
