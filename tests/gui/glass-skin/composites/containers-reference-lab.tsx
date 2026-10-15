import { defineClient } from "../../../../tools/shared-host/client.js";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import { SpecimenSession } from "../../skin-lab/specimen-session.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import { containersReference } from "./containers-reference.js";
export default defineClient({
  async open(context) {
    const contract = context.contract as unknown as GlassKitContract &
      ThemeContract;
    return SpecimenSession.open({
      host: context.host,
      contract,
      font: await context.font(),
      specimen: containersReference(contract),
      themes: {},
      world: `containers-reference/${context.name}`,
    });
  },
  async capture(session, context) {
    await session.assetSettled("ipp-glass/shader");
    const frame = await session.capture(context.present);
    return {
      images: { reference: frame.image },
      report: { routing: frame.states.map(({ routing }) => routing) },
    };
  },
  async close(session) {
    await session.close();
  },
});
