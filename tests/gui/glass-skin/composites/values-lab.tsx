import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { defineClient } from "../../../../tools/shared-host/client.js";
import { SpecimenSession } from "../../skin-lab/specimen-session.js";
import { exerciseGlassValues } from "../scenarios/composite-values.js";
import { valuesPrototype } from "./values-prototype.js";
import { valuesSpecimen } from "./values-specimen.js";
export default defineClient<{ session: SpecimenSession }>({
  async open(context) {
    const session = await SpecimenSession.open({
      host: context.host,
      contract: context.contract as unknown as Parameters<
        typeof valuesSpecimen
      >[0],
      font: await context.font(),
      specimen: valuesSpecimen(
        context.contract as unknown as Parameters<typeof valuesSpecimen>[0],
      ),
      themes: {},
      world: `glass-values/${context.name}`,
    });
    return { session };
  },
  async capture(state, context, args) {
    const verification: object[] = [];
    const evidence: Record<string, RgbaImage> = {};
    if (args.includes("verify"))
      await exerciseGlassValues(
        context.host,
        context.contract as unknown as Parameters<typeof valuesSpecimen>[0],
        await context.font(),
        context.present,
        {
          capture: async (frame) => {
            evidence[frame.name] = {
              width: frame.width,
              height: frame.height,
              pixels: Uint8Array.from(atob(frame.rgba), (char) =>
                char.charCodeAt(0),
              ),
            };
          },
          record: async (value) => {
            verification.push(value);
          },
        },
      );
    if (args.includes("prototype")) {
      await state.session.update({
        specimen: valuesPrototype(
          context.contract as unknown as Parameters<typeof valuesSpecimen>[0],
        ),
      });
      await state.session.assetSettled("ipp-glass/shader");
      const capture = await state.session.capture(context.present);
      return {
        images: { prototype: capture.image },
        summary: ["Knob and progress visual prototype"],
        report: { leftover: capture.leftover },
      };
    }
    await state.session.update({
      specimen: valuesSpecimen(
        context.contract as unknown as Parameters<typeof valuesSpecimen>[0],
        args.includes("violet")
          ? { primary: "#CF9CFF", secondary: "#785CFF" }
          : {},
        args.includes("light-right") ? { x: 0.9, y: 0.9, intensity: 1 } : {},
        args.includes("disabled"),
      ),
    });
    await state.session.assetSettled("ipp-glass/shader");
    const skins = await state.session.interact(
      context.present,
      async (interaction) =>
        Promise.all(
          ["labelled", "range", "dial"].map(async (name) => {
            const page = await interaction.client.inspectPage({
              collection: "entities",
              target: interaction.control(name).target.entity,
              limit: 1,
            });
            const skin = page.entities[0]?.components.find(
              (item) =>
                item.component === interaction.client.components.GuiSkin?.id,
            );
            const theme = skin?.fields?.theme;
            const themePage = theme
              ? await interaction.client.inspectPage({
                  collection: "entities",
                  target: BigInt(String(theme)),
                  limit: 1,
                })
              : undefined;
            return {
              name,
              fields: skin?.fields,
              theme: themePage?.entities[0],
            };
          }),
        ),
    );
    const stateName =
      args.find((name) =>
        [
          "slider-hover",
          "slider-focus",
          "dial-focus",
          "numeric-focus",
        ].includes(name),
      ) ?? "values-feedback";
    const capture = await state.session.capture(context.present, [stateName]);
    return {
      images: { values: capture.image, ...evidence },
      summary: ["Glass value and feedback composites"],
      report: { leftover: capture.leftover, verification, skins },
    };
  },
  async close(state) {
    await state.session.close();
  },
});
