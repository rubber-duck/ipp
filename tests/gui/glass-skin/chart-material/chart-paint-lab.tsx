/** Standalone paint development specimen; native Plot coverage uses its own scenario. */
import { canvasOutput } from "@ipp/client";
import {
  Entity,
  ShaderAsset,
  PaintShader,
  assetRef,
  createRoot,
} from "@ipp/react";
import { Box, Paint, Style } from "@ipp/react/gui";
import { defineClient } from "../../../../tools/shared-host/client.js";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import {
  GLASS_CHART_PAINT,
  GLASS_CHART_PAINT_PARAMETERS,
  GLASS_CHART_FINISH,
} from "../../../../packages/ipp-react/src/gui-glass/chart-paint.js";

function specimen(light: number, violet = false) {
  const color = glassColor(violet ? "#BF9CFF" : "#00D9FF");
  return (
    <>
      <ShaderAsset
        id="chart-paint"
        recipe={{ presentation: true, lighting: true }}
        parameters={GLASS_CHART_PAINT_PARAMETERS}
      >
        <PaintShader>{GLASS_CHART_PAINT}</PaintShader>
      </ShaderAsset>
      <Entity id="background">
        <Style red={0} green={0.007} blue={0.013} />
        <Box width={400} height={500} />
      </Entity>
      <Entity id="bar">
        <Style
          x={155}
          y={112}
          red={color[0]}
          green={color[1]}
          blue={color[2]}
        />
        <Box width={90} height={280} />
        <Paint
          source={assetRef("chart-paint")}
          accent={[0, 0, 0, 0]}
          light={[-0.3 + light * 2.1, -0.2 + light * 1.4, 0.75, 1]}
          finish={GLASS_CHART_FINISH}
          tint={[0.18, 0.34, 0.39, 1]}
          lighting={[0, 0, 0, 0]}
        />
      </Entity>
    </>
  );
}

export default defineClient({
  async open(context) {
    const world = await context.host.createWorld({
      symbolicId: `chart-paint/${context.name}`,
      temporary: true,
      selectedSystems: ["ipp.canvas", "ipp.asset-dependencies"],
    });
    const client = await context.host.openWorld(world.reference);
    const root = createRoot(client);
    const binding = await context.host.setRootOutput(
      canvasOutput(world.reference),
      { width: 400, height: 500, devicePixelRatio: 1 },
    );
    return { world, client, root, binding };
  },
  async capture(state, context, args) {
    await state.root.render(
      specimen(Number(args[0] ?? 0), args[1] === "violet"),
    );
    const deadline = Date.now() + 30000;
    while (state.root.getAsset("chart-paint")?.status !== "loaded") {
      const status = state.root.getAsset("chart-paint")?.status;
      if (status === "failed" || Date.now() > deadline)
        throw new Error(`paint ${status}`);
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    return {
      images: { bar: await context.capture(state.binding) },
      summary: ["Standalone procedural chart paint"],
    };
  },
  async close(state, context) {
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
