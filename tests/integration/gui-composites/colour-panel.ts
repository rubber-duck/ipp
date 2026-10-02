import type { GuiTarget } from "@ipp/client";
import { createElement as h } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../packages/ipp-react/src/index.js";
import {
  Color,
  Font,
  Layout,
  type GuiHsva,
} from "../../../packages/ipp-react/src/gui.js";
import { accepted, guiAction } from "../gui-actions.js";
import { controlTarget } from "../scenarios/gui-lifecycle.js";
import { componentFields, symbols, type PanelSpec } from "./page.js";

/**
 * The colour control: a panel World whose React root declares one `120 x 96`
 * colour control with its alpha rail at its top-left corner, at 8 units per
 * em, half the design body size, starting at the sheet's #54F4FF. Its
 * `onColorCommit` records every colour the control reports, as an
 * application receives them.
 */
export const COLOUR_PANEL = [192, 96] as const;

/** The control's font size and box. */
export const COLOUR_FONT = 8;
export const COLOUR_SIZE = [120, 96] as const;

/** The sheet's #54F4FF as HSV on sRGB-encoded values, opaque. */
export const COLOUR_START: GuiHsva = {
  hue: (4 - 160 / 171) / 6,
  saturation: 171 / 255,
  value: 1,
  alpha: 1,
};

/** One colour the application received, with the tick of its frame. */
export interface ColourRecord {
  readonly tick: bigint;
  readonly value: GuiHsva;
}

export function colourPanel(origin: readonly [number, number]) {
  return {
    name: "colour",
    origin,
    extent: COLOUR_PANEL,
    async build({ client, font, report }) {
      const records: ColourRecord[] = [];
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      await root.render(
        h(
          Entity,
          { id: "colour-root" },
          h(Layout, {
            kind: 3,
            width: COLOUR_PANEL[0],
            height: COLOUR_PANEL[1],
          }),
          h(Font, { source: font, font_size: COLOUR_FONT }),
          h(
            Children,
            null,
            h(
              Entity,
              { id: "colour" },
              h(Layout, {
                kind: 0,
                width: COLOUR_SIZE[0],
                height: COLOUR_SIZE[1],
                align_x: -1,
                align_y: -1,
              }),
              h(Color, {
                ...COLOUR_START,
                alpha_rail: true,
                onColorCommit: ({ tick, value }) =>
                  records.push({ tick, value }),
              }),
            ),
          ),
        ),
      );

      const entity = async () => {
        const found = (await symbols(client)).get("colour");
        if (found === undefined)
          throw new Error("The colour control is missing");
        return found;
      };
      const component = client.components.GuiColor!.id;
      const fields = async () => {
        const id = await entity();
        const page = await client.inspectPage({
          collection: "entities",
          target: id,
          limit: 1,
        });
        return {
          color: componentFields(client, page, id, "GuiColor") ?? {},
          bounds: componentFields(client, page, id, "CanvasBounds") ?? {},
        };
      };
      const target = async (): Promise<GuiTarget> => {
        const found = await controlTarget(client, await entity(), component);
        if (!found) throw new Error("The colour control has no target");
        return found;
      };

      return {
        records,
        /** The colour the control's fields hold. */
        async value(): Promise<GuiHsva> {
          const { color } = await fields();
          return {
            hue: Number(color.hue),
            saturation: Number(color.saturation),
            value: Number(color.value),
            alpha: Number(color.alpha),
          };
        },
        /** The control's evaluated box in the panel's canvas. */
        async bounds() {
          const { bounds } = await fields();
          return [bounds.x, bounds.y, bounds.width, bounds.height].map(
            Number,
          ) as [number, number, number, number];
        },
        /** The part the panel World focuses, with its ring, or null. */
        async focus() {
          const page = await client.inspectPage({ collection: "guiFocus" });
          const [record] = page.guiFocus ?? [];
          return record
            ? {
                part: record.part,
                visible: record.visible,
                entity: record.target.entity,
              }
            : null;
        },
        /** A client's `GuiAction` on the control, which must apply. */
        async action(action: Parameters<typeof guiAction>[2]) {
          accepted(
            await guiAction(client, await target(), action),
            "Colour action",
          );
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type ColourPanel = Awaited<
  ReturnType<ReturnType<typeof colourPanel>["build"]>
>;
