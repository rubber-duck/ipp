import { defineClient } from "../../../../tools/shared-host/client.js";
import {
  openOpticalContext,
  captureOpticalContext,
  closeOpticalContext,
  type OpticalContextSession,
} from "./context-session.js";
import type { ContextOptions } from "./context-scene.js";

export default defineClient<OpticalContextSession>({
  async open(context) {
    return openOpticalContext(context.host, await context.font(), context.name);
  },
  async capture(state, context, args) {
    const options: ContextOptions = {
      shape: args.includes("cylinder")
        ? "cylinder"
        : args.includes("sphere")
          ? "sphere"
          : "flat",
      angle: args.includes("angle") ? 0.65 : 0,
      light: args.includes("right") ? 3 : -3,
      probe: args.includes("view") ? 1 : args.includes("light") ? 2 : 0,
      cache: args.includes("cache"),
    };
    return {
      images: {
        [args.join("-") || "frontal"]: await captureOpticalContext(
          state,
          context.present,
          options,
        ),
      },
      report: { options },
    };
  },
  async close(state) {
    await closeOpticalContext(state);
  },
});
