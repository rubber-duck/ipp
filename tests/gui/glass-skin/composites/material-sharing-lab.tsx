import { defineClient } from "../../../../tools/shared-host/client.js";
import { exerciseGlassMaterialSharing } from "./material-sharing.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";

export default defineClient({
  async open() {
    return {};
  },
  async capture(_state, context) {
    const images: Record<string, RgbaImage> = {};
    const records: object[] = [];
    await exerciseGlassMaterialSharing(
      context.host,
      context.contract as unknown as Parameters<
        typeof exerciseGlassMaterialSharing
      >[1],
      await context.font(),
      context.present,
      {
        async capture(frame) {
          images[frame.name] = {
            width: frame.width,
            height: frame.height,
            pixels: Uint8Array.from(Buffer.from(frame.rgba, "base64")),
          };
        },
        async record(value) {
          records.push(value);
        },
      },
    );
    return { images, report: { records } };
  },
  async close() {},
});
