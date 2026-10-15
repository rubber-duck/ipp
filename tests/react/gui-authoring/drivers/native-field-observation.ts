import { defineClient } from "../../../../tools/shared-host/client.js";
import { guiFieldObservation } from "../scenarios/gui-field-observation.js";
import type { GuiContract } from "../pages/gui-authoring.js";

export default defineClient({
  async open() {},
  async capture(_, context) {
    const result = await guiFieldObservation(
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
      report: { failure: result.failure, report: result.report },
    };
  },
  async close() {},
});
