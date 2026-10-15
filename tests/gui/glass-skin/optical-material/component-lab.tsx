import { defineClient } from "../../../../tools/shared-host/client.js";
import {
  openOpticalComponents,
  captureOpticalComponents,
  closeOpticalComponents,
  type OpticalComponentSession,
} from "./component-session.js";
import { ProductionSamples } from "./production-scene.js";
import { LiquidSamples } from "./liquid-scene.js";
import { ComponentSamples } from "./component-scene.js";
import type { GuiKitContract } from "@ipp/react/gui-kit";

export default defineClient<OpticalComponentSession>({
  async open(context, args) {
    return openOpticalComponents(
      context.host,
      await context.font(),
      context.contract as unknown as GuiKitContract,
      context.name,
      args.includes("liquid") ? "liquid" : args.includes("production"),
    );
  },
  async capture(state, context, args) {
    const Samples = state.liquid
      ? LiquidSamples
      : state.production
        ? ProductionSamples
        : ComponentSamples;
    await state.canvasRoot.render(
      <Samples
        font={state.font}
        contract={state.contract}
        controls={state.controls}
        {...(state.production
          ? {
              dropdownY: args.includes("flip") ? 920 : 178,
              skin: args.includes("cybertron")
                ? ("cybertron" as const)
                : ("glass" as const),
            }
          : {})}
      />,
    );
    const options = {
      angle: args.includes("angle") ? 0.45 : 0,
      light: args.includes("right") ? 3 : -3,
    };
    let frame = await captureOpticalComponents(state, context.present, options);
    if (args.includes("open")) {
      frame = await context.present(state.binding, async (view) => {
        const input = await state.host.input.open(view);
        try {
          await input.send({ kind: "key", key: "escape" });
          await captureOpticalComponents(
            state,
            async (_binding, fn) => fn(view),
            options,
          );
          const point = [
            380 / 1536,
            (args.includes("flip") ? 936 : 194) / 1024,
          ] as const;
          await input.send({ kind: "pointerDown", pointer: 42n, point });
          await input.send({ kind: "pointerUp", pointer: 42n, point });
          return await captureOpticalComponents(
            state,
            async (_binding, fn) => fn(view),
            options,
          );
        } finally {
          await input.close();
        }
      });
    }
    if (
      (state.production || state.liquid) &&
      args.some((arg) => ["hover", "focus", "press"].includes(arg))
    ) {
      frame = await context.present(state.binding, async (view) => {
        const input = await state.host.input.open(view);
        const button = state.controls.get("button")!;
        const inspected = await state.canvasClient.inspect();
        const bounds = inspected.entities
          .find((entity) => entity.id === button.target.entity)!
          .components.find(
            (component) =>
              component.component ===
              state.canvasClient.components.CanvasBounds!.id,
          )!.fields;
        const point = [
          (Number(bounds.x) + Number(bounds.width) / 2) / 1536,
          (Number(bounds.y) + Number(bounds.height) / 2) / 1024,
        ] as const;
        try {
          if (args.includes("hover") || args.includes("press"))
            await input.send({ kind: "pointerMove", pointer: 42n, point });
          if (args.includes("focus")) await button.focus();
          if (args.includes("press"))
            await input.send({ kind: "pointerDown", pointer: 42n, point });
          return await captureOpticalComponents(
            state,
            async (_binding, fn) => fn(view),
            options,
          );
        } finally {
          await input.send({ kind: "pointerCancel", pointer: 42n });
          await button.blur();
          await input.close();
        }
      });
    }
    return {
      images: {
        [args.join("-") || "components"]: frame,
      },
      report: { options },
    };
  },
  async close(state) {
    await closeOpticalComponents(state);
  },
});
