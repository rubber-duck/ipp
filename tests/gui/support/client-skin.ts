import type { Client, RowPropertyValue } from "@ipp/client";
import {
  cybertronLooks,
  cybertronControlMetrics,
} from "../../../packages/ipp-react/src/gui-cybertron.js";
import {
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import type { GuiContract } from "../scenarios/gui-lifecycle.js";

/** Opt a fixture control into Cybertron; preserve its authored overrides when present. */
export async function applyClientSkin(
  client: Client,
  contract: GuiContract,
  entity: bigint,
  name: keyof ReturnType<typeof cybertronLooks>,
  hasSkin = false,
) {
  const recipe = cybertronLooks(contract)[name];
  const theme = { kind: "alias" as const, alias: 1 };
  const target = { kind: "handle" as const, id: entity };
  const table = (
    rows: readonly ({ readonly part: number } & Readonly<
      Record<string, RowPropertyValue>
    >)[],
  ) => ({
    nextSlot: rows.length,
    rows: new Map(rows.map((row, slot) => [slot, row])),
  });
  const reference = {
    offset: client.components.GuiSkin!.fields.theme!.offset,
    value: { kind: "entity" as const, value: theme },
  };
  successfulBatch(
    await client.batch([
      createEntity(1, `fixture-skin-${entity}-${name}`),
      insertComponent(client, "GuiTheme", theme, {
        em: recipe.em,
        parts: contract.GuiTheme.encodeParts(table(recipe.parts)),
      }),
      insertComponent(
        client,
        "GuiMetrics",
        theme,
        cybertronControlMetrics(name) as Record<string, number>,
      ),
      insertComponent(client, "GuiThemeMotion", theme, {
        parts: contract.GuiThemeMotion.encodeParts(table(recipe.motion ?? [])),
      }),
      hasSkin
        ? {
            kind: "setField",
            entity: target,
            component: client.components.GuiSkin!.id,
            field: reference,
          }
        : {
            kind: "insertComponent",
            entity: target,
            component: client.components.GuiSkin!.id,
            fields: [reference],
          },
    ]),
  );
}
