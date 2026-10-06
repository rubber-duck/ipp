import type {
  BatchOutcome,
  Client,
  Command,
  CommandPageLimits,
  EntityRef,
  FieldWrite,
  HostClientBase,
} from "@ipp/client";

/** A generated World client opened through a connect convenience keeps its Host. */
export type HostedWorldClient<T extends Client = Client> = T & {
  readonly host: HostClientBase<Client>;
};

/** Resolve writes from the connected target's generated component descriptors. */
export function componentFields(
  client: Client,
  name: string,
  values: Readonly<
    Record<string, number | string | boolean | Uint8Array<ArrayBuffer> | bigint>
  >,
): FieldWrite[] {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`Target does not expose ${name}`);
  return Object.entries(values).map(([name, value]) => {
    const field = descriptor.fields[name];
    if (!field) throw new Error(`Component has no generated field ${name}`);
    return {
      offset: field.offset,
      value:
        value instanceof Uint8Array
          ? { kind: "bytes", value }
          : typeof value === "bigint"
            ? { kind: "entity", value: { kind: "handle", id: value } }
            : typeof value === "boolean"
              ? { kind: "bool", value }
              : field.kind === 5
                ? { kind: "string", value: String(value) }
                : field.kind === 3
                  ? { kind: "u32", value: Number(value) }
                  : { kind: "f32", value: Number(value) },
    };
  });
}

export function insertComponent(
  client: Client,
  name: string,
  entity: EntityRef,
  values: Readonly<
    Record<string, number | string | boolean | Uint8Array<ArrayBuffer> | bigint>
  > = {},
): Command {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`Target does not expose ${name}`);
  return {
    kind: "insertComponent",
    entity,
    component: descriptor.id,
    fields: componentFields(client, name, values),
  };
}

export function createEntity(alias: number, symbolicId: string): Command {
  return { kind: "create", alias, metadata: { symbolicId, classes: [] } };
}

/** Batch page bounds of the connected target's generated client. */
export function pageLimits(client: Client): CommandPageLimits {
  return (client as unknown as { commandPageLimits: CommandPageLimits })
    .commandPageLimits;
}

/** Commands in one full batch page of the connected target. */
export function pageCommands(client: Client): number {
  return pageLimits(client).commands;
}

export function successfulBatch(
  outcome: BatchOutcome,
): Extract<BatchOutcome, { ok: true }> {
  if (!outcome.ok) {
    throw new Error(
      `World batch rejected at ${outcome.error.operation}: ${outcome.error.reason}`,
    );
  }
  return outcome;
}

export function aliasId(outcome: BatchOutcome, alias: number): bigint {
  const id = successfulBatch(outcome).aliases.find(
    (entry) => entry.alias === alias,
  )?.id;
  if (id === undefined) throw new Error(`World batch omitted alias ${alias}`);
  return id;
}
