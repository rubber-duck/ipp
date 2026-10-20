/** Inspect the maintained layout scenario through the shared native Host. */
import { defineClient } from "../../../tools/shared-host/client.js";
import { guiWidgetSizing } from "../../react/gui-authoring/scenarios/gui-widget-sizing.js";

export default defineClient({
  async open(context) {
    return { font: await context.font() };
  },
  async close() {},
  async capture(state, context) {
    const result = await guiWidgetSizing(
      context.host,
      context.contract as unknown as Parameters<typeof guiWidgetSizing>[1],
      state.font,
      context.present,
    );
    return {
      images: Object.fromEntries(
        result.images.map((frame) => [
          frame.label,
          {
            width: frame.width,
            height: frame.height,
            pixels: Uint8Array.from(frame.pixels),
          },
        ]),
      ),
      report: { failure: result.failure, checks: result.report },
    };
  },
});
