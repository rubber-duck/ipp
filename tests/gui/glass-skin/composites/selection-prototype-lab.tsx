import { defineClient } from "../../../../tools/shared-host/client.js";
import { SpecimenSession } from "../../skin-lab/specimen-session.js";
import { selectionPrototype } from "./selection-prototype.js";
export default defineClient<SpecimenSession>({
  async open(context) {
    return SpecimenSession.open({
      host: context.host,
      contract: context.contract as unknown as Parameters<
        typeof selectionPrototype
      >[0],
      font: await context.font(),
      specimen: selectionPrototype(
        context.contract as unknown as Parameters<typeof selectionPrototype>[0],
      ),
      themes: {},
      world: `glass-prototype/${context.name}`,
    });
  },
  async capture(session, context, args) {
    await session.update({
      specimen: selectionPrototype(
        context.contract as unknown as Parameters<typeof selectionPrototype>[0],
        args.includes("fixed-light") ? { x: 0.12, y: 0.12, intensity: 1 } : {},
      ),
    });
    await session.assetSettled("ipp-glass/shader");
    const result = await session.capture(context.present);
    return {
      images: { prototype: result.image },
      report: {
        states: result.states.map(({ name, routing }) => ({ name, routing })),
        leftover: result.leftover,
      },
    };
  },
  async close(session) {
    await session.close();
  },
});
