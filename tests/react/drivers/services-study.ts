/** Persistent native iteration runs the maintained transport/rendering Plot scenario. */
import { defineClient } from "../../../tools/shared-host/client.js";
import { reactPlots } from "../../data/scenarios/react-plots.js";
import type { ReactWorldContract } from "@ipp/react";
import type * as Generated from "@ipp/host-contract";
import type { RgbaImage } from "../../../tools/shared-host/images.js";

type Contract = ReactWorldContract &
  Pick<typeof Generated, "ExpressionBuilder">;

export default defineClient({
  async open() {
    return {};
  },
  async capture(_state, context) {
    const images: Record<string, RgbaImage> = {};
    await reactPlots(
      context.host,
      context.contract as unknown as Contract,
      await context.font(),
      async (label, binding) => {
        const image = await context.capture(binding);
        images[label] = image;
        return image;
      },
    );
    return { images, report: { scenario: "reactPlots", result: "passed" } };
  },
  async close() {},
});
