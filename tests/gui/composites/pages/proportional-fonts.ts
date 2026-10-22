import { createElement } from "react";
import { createRoot, type ReactWorldClient } from "@ipp/react";
import {
  FONT_CANVAS,
  ProportionalFonts,
  type FontTheme,
} from "../scenarios/proportional-fonts.js";
import {
  canvasOf,
  componentFields,
  openCompositePage,
  symbols,
  type CompositeSetup,
  type PanelSpec,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANEL = {
  name: "proportional-fonts",
  origin: [0, 0],
  extent: FONT_CANVAS,
  async build({ client, font, iconFont, contract, report }) {
    const root = createRoot(client as unknown as ReactWorldClient, {
      onError: report,
    });
    async function render(theme: FontTheme, mono = false) {
      await root.render(
        createElement(ProportionalFonts, {
          contract,
          font: mono ? iconFont : font,
          iconFont,
          theme,
        }),
      );
    }
    await render("cybertron");
    return {
      entities: {
        "font-input": (await symbols(client)).get("workspace-name/input")!,
      },
      render,
      async read(symbol: string) {
        const entity = (await symbols(client)).get(symbol);
        if (entity === undefined) throw new Error(`Missing ${symbol}`);
        const page = await client.inspectPage({
          collection: "entities",
          target: entity,
          limit: 1,
        });
        const bounds =
          componentFields(client, page, entity, "CanvasBounds") ?? {};
        return {
          entity: String(entity),
          bounds: [bounds.x, bounds.y, bounds.width, bounds.height].map(Number),
          text: componentFields(client, page, entity, "GuiTextInput"),
        };
      },
      async close() {
        await root.unmount();
      },
    };
  },
} satisfies PanelSpec;
export const CANVAS = canvasOf([PANEL]);
export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, [PANEL]);
  const panel = page.panel(PANEL.name).build as Awaited<
    ReturnType<typeof PANEL.build>
  >;
  return {
    ...page.steps,
    read: panel.read,
    render: panel.render,
    async expectWidth(symbol: string, expected: number) {
      let value: Awaited<ReturnType<typeof panel.read>>;
      await page.until(
        async () => {
          value = await panel.read(symbol);
          return Math.abs(value.bounds[2]! - expected) < 0.1;
        },
        () => `${symbol}: ${JSON.stringify(value)}, expected width ${expected}`,
      );
      return (await panel.read(symbol)).bounds;
    },
    async expectText(symbol: string, expected: string) {
      await page.until(
        async () => (await panel.read(symbol)).text?.text === expected,
        () => `${symbol} did not receive ${expected}`,
      );
    },
  };
}
