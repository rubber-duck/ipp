import { defineClient } from "../../../../tools/shared-host/client.js";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { GuiContract } from "../pages/gui-authoring.js";
import { guiWidgets } from "../scenarios/gui-widgets.js";

/** Reuse the maintained assertions with the shared Host's exclusive presentation. */
export default defineClient<{ readonly font: Uint8Array<ArrayBuffer> }>({
  async open(context) {
    return { font: await context.font() };
  },
  async capture(state, context) {
    const result = await guiWidgets(
      context.host,
      context.contract as unknown as GuiContract & GuiKitContract,
      state.font,
      (binding, section) => context.present(binding, section),
    );
    return {
      images: Object.fromEntries(
        result.images.map((image) => [
          image.label,
          {
            width: image.width,
            height: image.height,
            pixels: Uint8Array.from(image.pixels),
          },
        ]),
      ),
      summary: [...result.report, ...(result.failure ? [result.failure] : [])],
      report: { failure: result.failure, assertions: result.report },
    };
  },
  async close() {},
});
