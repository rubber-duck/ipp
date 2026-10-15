import {
  exerciseGlassOverlays,
  assertToastFrame,
} from "../scenarios/overlays.js";
import { defineClient } from "../../../../tools/shared-host/client.js";
import { SpecimenSession } from "../../skin-lab/specimen-session.js";
import { overlaysSpecimen } from "./overlays-specimen.js";
export default defineClient<SpecimenSession>({
  async open(context, args) {
    return SpecimenSession.open({
      host: context.host,
      contract: context.contract as unknown as Parameters<
        typeof overlaysSpecimen
      >[0],
      font: await context.font(),
      specimen: overlaysSpecimen(
        context.contract as unknown as Parameters<typeof overlaysSpecimen>[0],
        args.includes("dialog")
          ? "dialog"
          : args.includes("toast")
            ? "toast"
            : "overview",
      ),
      themes: {},
      world: `glass-overlays/${context.name}`,
    });
  },
  async capture(session, context, args) {
    if (args.includes("verify")) {
      const images: Record<
        string,
        { width: number; height: number; pixels: Uint8Array<ArrayBuffer> }
      > = {};
      const records: object[] = [];
      await exerciseGlassOverlays(
        context.host,
        context.contract as unknown as Parameters<
          typeof exerciseGlassOverlays
        >[1],
        await context.font(),
        context.present,
        {
          capture: async (frame) => {
            images[frame.name] = {
              width: frame.width,
              height: frame.height,
              pixels: Uint8Array.from(Buffer.from(frame.rgba, "base64")),
            };
          },
          record: async (value) => {
            records.push(value);
            console.info(JSON.stringify(value));
          },
        },
      );
      return { images, report: { records } };
    }

    await session.update({
      specimen: overlaysSpecimen(
        context.contract as unknown as Parameters<typeof overlaysSpecimen>[0],
        args.includes("dialog")
          ? "dialog"
          : args.includes("toast")
            ? "toast"
            : "overview",
        args.includes("violet")
          ? { primary: "#CF9CFF", secondary: "#785CFF" }
          : {},
        args.includes("right")
          ? { x: 0.9, y: 0.9, intensity: 1 }
          : { x: 0.1, y: 0.1, intensity: 1 },
      ),
    });
    await session.assetSettled("ipp-glass/shader");
    const result = await session.capture(context.present);
    let toastProof: object[] | undefined;
    if (args.includes("toast")) {
      await session.interact(context.present, async (interaction) => {
        toastProof = await assertToastFrame(interaction.client, result.image);
      });
    }
    return {
      images: { overlays: result.image },
      report: { leftover: result.leftover, toastProof },
    };
  },
  async close(session) {
    await session.close();
  },
});
