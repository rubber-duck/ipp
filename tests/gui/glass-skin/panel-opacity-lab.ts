import { defineClient } from "../../../tools/shared-host/client.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { exercisePanelOpacity } from "./scenarios/panel-opacity.js";
import { exerciseDropdownRowCorners } from "./scenarios/dropdown-row-corners.js";
import type { GlassSkinEnvironment } from "./scenarios/glass-skin.js";

export default defineClient<{ font: Uint8Array<ArrayBuffer> }>({
  open: async (context) => ({ font: await context.font() }),
  async capture(state, context) {
    const images: Record<string, RgbaImage> = {};
    const evidence: object[] = [];
    const environment: GlassSkinEnvironment = {
      async capture(frame) {
        images[frame.name] = {
          ...frame,
          pixels: Uint8Array.from(Buffer.from(frame.rgba, "base64")),
        };
      },
      async record(value) {
        evidence.push(value);
      },
    };
    const contract = context.contract as unknown as Parameters<
      typeof exercisePanelOpacity
    >[1];
    await exercisePanelOpacity(
      context.host,
      contract,
      state.font,
      context.present,
      environment,
    );
    await exerciseDropdownRowCorners(
      context.host,
      contract,
      state.font,
      context.present,
      environment,
    );
    return { images, report: { evidence } };
  },
  async close() {},
});
