import { canvasOutput, type RootBinding } from "@ipp/client";
import { createRoot, type ReactWorldRoot } from "@ipp/react";
import {
  defineClient,
  type ClientContext,
} from "../../../../tools/shared-host/client.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import {
  OPTICAL_EXTENT,
  opticalScene,
  type OpticalStudyOptions,
} from "./scene.js";

interface State {
  world: Awaited<ReturnType<ClientContext["host"]["createWorld"]>>;
  client: Awaited<ReturnType<ClientContext["host"]["openWorld"]>>;
  root: ReactWorldRoot;
  binding: RootBinding;
  font: Uint8Array<ArrayBuffer>;
  paintComponent: number;
}

async function ready(root: ReactWorldRoot, animated: boolean) {
  for (const id of [
    "optical/font",
    "optical/paint",
    ...(animated ? ["optical/motion"] : []),
  ]) {
    const deadline = Date.now() + 30000;
    while (root.getAsset(id)?.status !== "loaded") {
      if (root.getAsset(id)?.status === "failed" || Date.now() > deadline)
        throw new Error(`${id}: ${JSON.stringify(root.getAsset(id))}`);
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }
}

export default defineClient<State>({
  async open(context) {
    const world = await context.host.createWorld({
      symbolicId: `glass-optics/${context.name}`,
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.canvas",
        "ipp.asset-dependencies",
      ],
    });
    const client = await context.host.openWorld(world.reference);
    const root = createRoot(client, { host: context.host });
    const font = await context.font();
    const paintComponent = client.components.CanvasPaint!.id;
    await root.render(opticalScene(font, paintComponent));
    const binding = await context.host.setRootOutput(
      canvasOutput(world.reference),
      OPTICAL_EXTENT,
    );
    return { world, client, root, binding, font, paintComponent };
  },
  async capture(state, context, args) {
    const animated = args.includes("motion");
    const options: OpticalStudyOptions = {
      animated,
      ...(args.includes("right") ? { source: [3, 0.5, 5, 0.8] as const } : {}),
      ...(args.includes("view") ? { viewShift: 2.8 } : {}),
    };
    await state.root.render(
      opticalScene(state.font, state.paintComponent, options),
    );
    await ready(state.root, animated);
    const images: Record<string, RgbaImage> = {};
    if (animated) {
      await context.present(state.binding, async (view) => {
        const start = Date.now();
        for (let i = 0; i < 48; i++) {
          const frame = await context.host.presentation.capture(view);
          images[`motion-${String(i).padStart(3, "0")}`] = {
            width: frame.view.binding.viewport.width,
            height: frame.view.binding.viewport.height,
            pixels: new Uint8Array(frame.pixels),
          };
          const until = start + (i + 1) * 125 - Date.now();
          if (until > 0)
            await new Promise((resolve) => setTimeout(resolve, until));
        }
      });
    } else images[args[0] ?? "study"] = await context.capture(state.binding);
    return {
      images,
      report: {
        options,
        viewInput: "explicit local eye",
        sceneReflectionPasses: 0,
      },
    };
  },
  async close(state, context) {
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
