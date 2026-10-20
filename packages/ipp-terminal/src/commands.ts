import {
  FieldKind,
  type Client,
  type Command,
  type EntityRef,
  type FieldWrite,
} from "@ipp/client";

export type TerminalField = number | string | boolean | Uint8Array<ArrayBuffer>;

/** Resolve every write from the connected target's descriptors. */
export function component(
  client: Client,
  name: string,
  entity: EntityRef,
  values: Readonly<Record<string, TerminalField>> = {},
  adopt = false,
): Command {
  const descriptor = client.components[name];
  if (!descriptor) throw new Error(`Terminal requires ${name}`);
  const fields: FieldWrite[] = Object.entries(values).map(([key, value]) => {
    const field = descriptor.fields[key];
    if (!field) throw new Error(`${name} has no field ${key}`);
    return {
      offset: field.offset,
      value:
        typeof value === "boolean"
          ? { kind: "bool", value }
          : typeof value === "string"
            ? { kind: "string", value }
            : typeof value === "number"
              ? { kind: field.kind === FieldKind.U32 ? "u32" : "f32", value }
              : {
                  kind: field.kind === FieldKind.Rows ? "rows" : "bytes",
                  value,
                },
    };
  });
  return {
    kind: "insertComponent",
    entity,
    component: descriptor.id,
    fields,
    adopt,
  };
}

export class TerminalBatchError extends Error {
  constructor(
    readonly reason: string,
    operation: number | null,
  ) {
    super(`Terminal batch: ${reason} at ${operation}`);
  }
}

export async function apply(client: Client, commands: Command[]) {
  const outcome = await client.batch(commands);
  if (!outcome.ok)
    throw new TerminalBatchError(outcome.error.reason, outcome.error.operation);
  return outcome;
}

export const handle = (id: bigint): EntityRef => ({ kind: "handle", id });
export const alias = (id: number): EntityRef => ({ kind: "alias", alias: id });
