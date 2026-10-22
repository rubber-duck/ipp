import { defineClient } from "../../../tools/shared-host/client.js";
import { SpecimenSession } from "../skin-lab/specimen-session.js";
import { edgeQualitySpecimen } from "./specimens/edge-quality.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import {
  measureGlassEdges,
  measureGlassRadiance,
} from "./edge-quality-oracle.js";

export default defineClient<{ sessions: Map<number, SpecimenSession> }>({
  async open(context) {
    const sessions = new Map<number, SpecimenSession>();
    const font = await context.font();
    try {
      for (const scale of [1, 2, 3, 6])
        sessions.set(
          scale,
          await SpecimenSession.open({
            host: context.host,
            contract: context.contract as unknown as Parameters<
              typeof SpecimenSession.open
            >[0]["contract"],
            font,
            specimen: edgeQualitySpecimen(
              context.contract as unknown as Parameters<
                typeof edgeQualitySpecimen
              >[0],
            ),
            themes: {},
            world: `${context.name}/edges/${scale}`,
            captureScale: scale,
          }),
        );
      return { sessions };
    } catch (error) {
      for (const session of sessions.values()) await session.close();
      throw error;
    }
  },
  async capture(state, context, args) {
    const images: Record<string, RgbaImage> = {};
    const edges = [];
    const contract = context.contract as unknown as Parameters<
      typeof edgeQualitySpecimen
    >[0];
    for (const [scale, session] of state.sessions) {
      for (const [name, backdrop] of (args.includes("transmission")
        ? [
            ["black", 0],
            ["white", 1],
          ]
        : [["preview", undefined]]) as [string, number | undefined][]) {
        await session.update({
          specimen: edgeQualitySpecimen(contract, backdrop),
        });
        images[`${name}-${scale}x`] = (
          await session.capture(context.present)
        ).image;
      }
      if (args.includes("transmission"))
        edges.push(
          ...measureGlassEdges(
            images[`black-${scale}x`]!,
            images[`white-${scale}x`]!,
            scale,
          ),
        );
    }
    const radiance = args.includes("transmission")
      ? []
      : [1, 2, 3].flatMap((scale) =>
          measureGlassRadiance(
            images[`preview-${scale}x`]!,
            images["preview-6x"]!,
            scale,
          ),
        );
    return { images, report: { edges, radiance } };
  },
  async close(state) {
    for (const session of state.sessions.values()) await session.close();
  },
});
