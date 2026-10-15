import { defineClient } from "../../../../tools/shared-host/client.js";
import { SpecimenSession } from "../../skin-lab/specimen-session.js";
import { containersSpecimen, containerEvents } from "./containers-specimen.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { exerciseGlassContainers } from "../scenarios/containers.js";
export default defineClient({
  async open(context) {
    const events = containerEvents();
    const session = await SpecimenSession.open({
      host: context.host,
      contract: context.contract as unknown as GlassKitContract & ThemeContract,
      font: await context.font(),
      specimen: containersSpecimen(
        context.contract as unknown as GlassKitContract & ThemeContract,
        events,
      ),
      themes: {},
      world: `glass-containers/${context.name}`,
    });
    return { session, events };
  },
  async capture(state, context, args) {
    if (args.includes("scenario")) {
      const images: Record<
        string,
        { width: number; height: number; pixels: Uint8Array<ArrayBuffer> }
      > = {};
      const reports: object[] = [];
      await exerciseGlassContainers(
        context.host,
        context.contract as unknown as GlassKitContract & ThemeContract,
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
          record: async (report) => {
            reports.push(report);
          },
        },
      );
      return { images, report: { reports } };
    }
    await state.session.update({
      specimen: containersSpecimen(
        context.contract as unknown as GlassKitContract & ThemeContract,
        state.events,
        {
          ...(args.includes("violet")
            ? { palette: { primary: "#CF9CFF", secondary: "#785CFF" } }
            : {}),
          ...(args.includes("light")
            ? {
                light: {
                  intensity: 1,
                  x: args.includes("right") ? 0.9 : 0.1,
                  y: 0.1,
                },
              }
            : {}),
          ...(args.includes("editing") ? { editing: true } : {}),
        },
      ),
    });
    await state.session.assetSettled("ipp-glass/shader");
    const capture = await state.session.capture(context.present);
    return {
      images: { containers: capture.image },
      report: {
        events: state.events,
        states: capture.states.map(({ name, routing }) => ({ name, routing })),
        leftover: capture.leftover,
      },
    };
  },
  async close(state) {
    await state.session.close();
  },
});
