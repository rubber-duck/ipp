import { defineClient } from "../../../tools/shared-host/client.js";
import { SpecimenSession } from "../skin-lab/specimen-session.js";
import { glassSpecimen } from "./scenarios/glass-skin.js";

export default defineClient<{
  session: SpecimenSession;
  family: "actions" | "values";
}>({
  async open(context, args) {
    const family = args[0] === "values" ? "values" : "actions";
    const session = await SpecimenSession.open({
      host: context.host,
      contract: context.contract as unknown as Parameters<
        typeof glassSpecimen
      >[0],
      font: await context.font(),
      specimen: glassSpecimen(
        context.contract as unknown as Parameters<typeof glassSpecimen>[0],
        family,
      ),
      themes: {},
      world: `glass-lab/${context.name}`,
    });
    return { session, family };
  },
  async capture(state, context, args) {
    const violet = args.includes("violet");
    await state.session.update({
      specimen: glassSpecimen(
        context.contract as unknown as Parameters<typeof glassSpecimen>[0],
        state.family,
        {
          ...(violet ? { primary: "#CF9CFF", secondary: "#785CFF" } : {}),
          ...(args.includes("contrast") ? { background: "#E55A88" } : {}),
        },
        args.includes("light-right")
          ? { intensity: 1, x: 0.9, y: 0.9 }
          : args.includes("light")
            ? { intensity: 1, x: 0.1, y: 0.1 }
            : {},
      ),
    });
    await state.session.assetSettled("ipp-glass/shader");
    const capture = await state.session.capture(context.present);
    return {
      images: { [state.family]: capture.image },
      summary: [`Glass ${state.family}; ${violet ? "violet" : "cyan"}`],
      report: {
        states: capture.states.map(({ name, routing }) => ({ name, routing })),
        leftover: capture.leftover,
      },
    };
  },
  async close(state) {
    await state.session.close();
  },
});
