/** Native development captures of the maintained proportional-font specimen. */
import { setTimeout as delay } from "node:timers/promises";
import { readFile } from "node:fs/promises";
import { canvasOutput, type AssetWorldClient } from "@ipp/client";
import { createRoot, type ReactWorldClient } from "@ipp/react";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { defineClient } from "../../../../tools/shared-host/client.js";
import {
  FONT_CANVAS,
  ProportionalFonts,
} from "../scenarios/proportional-fonts.js";

export default defineClient({
  async open(context) {
    const world = await context.host.createWorld({
      symbolicId: `proportional-fonts/${context.name}`,
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
      ],
    });
    const client = await context.host.openWorld(world.reference);
    const assets = client as unknown as AssetWorldClient;
    const font = (
      await assets.createAsset(
        17,
        Uint8Array.from(
          await readFile(
            `${context.workspace}/target/font-assets/noto-sans.ippf`,
          ),
        ).buffer,
      )
    ).source;
    const iconFont = (
      await assets.createAsset(17, (await context.font()).buffer)
    ).source;
    const root = createRoot(client as unknown as ReactWorldClient);
    const binding = await context.host.setRootOutput(
      canvasOutput(world.reference),
      {
        width: FONT_CANVAS[0],
        height: FONT_CANVAS[1],
        devicePixelRatio: 1,
      },
    );
    return { world, client, root, binding, font, iconFont };
  },
  async capture(state, context) {
    const images: Record<
      string,
      Awaited<ReturnType<typeof context.capture>>
    > = {};
    for (const theme of ["cybertron", "glass"] as const) {
      await state.root.render(
        <ProportionalFonts
          contract={context.contract as unknown as GuiKitContract}
          font={state.font}
          iconFont={state.iconFont}
          theme={theme}
        />,
      );
      const deadline = performance.now() + 10_000;
      for (;;) {
        const fonts = (await state.client.inspect()).resources.filter(
          (resource) =>
            resource.source === state.font ||
            resource.source === state.iconFont,
        );
        const failed = fonts.find((font) => font.status === "failed");
        if (failed)
          throw new Error(`Font failed: ${failed.source}: ${failed.error}`);
        if (
          fonts.length === 2 &&
          fonts.every((font) => font.status === "loaded")
        )
          break;
        if (performance.now() > deadline)
          throw new Error("Specimen fonts did not load");
        await delay(10);
      }
      images[theme] = await context.capture(state.binding);
    }
    return {
      images,
      summary: ["Noto Sans with a separate Nerd Font for icons"],
    };
  },
  async close(state, context) {
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
