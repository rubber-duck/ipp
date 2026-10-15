import { defineClient } from "../../../../tools/shared-host/client.js";
import { guiPresentationRetention } from "../scenarios/gui-presentation-retention.js";
import type { GuiContract } from "../pages/gui-authoring.js";

export default defineClient({
  async open() {},
  async capture(_, context) {
    const result = await guiPresentationRetention(
      context.host,
      context.contract as unknown as GuiContract,
      await context.font(),
      context.present,
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
      report: result,
    };
  },
  async close() {},
});
