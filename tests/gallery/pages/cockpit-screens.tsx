/** Native 2D review: same components and renderer as the in-world sheets. */
import { canvasOutput } from "@ipp/client";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import { createRoot } from "@ipp/react";
import type { IppCanvasHandle } from "@ipp/react/canvas";
import { defineClient } from "../../../tools/shared-host/client.js";
import { image, settled } from "../../../tools/shared-host/presentation.js";
import {
  DemoFrameRate,
  type GalleryDiagnosticsMonitor,
} from "../../../examples/demos/frame-rate.js";
import { FlatCockpitScreen } from "../fixtures/cockpit-screens.js";
import type { FrameRateContract } from "../../../examples/demos/frame-rate-panel.js";

export default defineClient({
  async open(context) {
    const world = await context.host.createWorld({
      symbolicId: `${context.name}/screens`,
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.canvas",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
        "ipp.data-bindings",
        "ipp.plot",
      ],
      canvas: { extent: [960, 800], unitsPerMetre: 200 },
    });
    const client = await context.host.openWorld(world.reference);
    const root = createRoot(client, { host: context.host });
    const font = await context.font();
    const binding = await context.host.setRootOutput(
      canvasOutput(world.reference),
      { width: 1920, height: 1600, devicePixelRatio: 2 },
    );
    const state = {
      world,
      client,
      root,
      font,
      binding,
      view: null as IppCanvasHandle["view"],
      history: undefined as DemoFrameRate | undefined,
      started: false,
    };
    const canvas = {
      client,
      host: context.host,
      get viewport() {
        return binding.viewport;
      },
      get view() {
        return state.view;
      },
    } as unknown as IppCanvasHandle;
    state.history = new DemoFrameRate(canvas, (error) => {
      throw error;
    });
    await state.history.open();
    return state;
  },
  async capture(state, context, args) {
    const skin = args.includes("cybertron") ? "cybertron" : "glass";
    const index = args.includes("system")
      ? 1
      : args.includes("terminal")
        ? 2
        : 0;
    const history = state.history!;
    const monitor: GalleryDiagnosticsMonitor = {
      source: history.source,
      get snapshot() {
        return history.getSnapshot();
      },
      getSnapshot: history.getSnapshot,
      subscribe: history.subscribe,
      close: async () => {},
    };
    await state.root.render(
      <FlatCockpitScreen
        font={state.font}
        contract={
          context.contract as unknown as FrameRateContract & GuiKitContract
        }
        skin={skin}
        index={index}
        tab={args.includes("gui") ? "gui" : "scene"}
        monitor={monitor}
      />,
    );
    const deadline = Date.now() + 30000;
    for (;;) {
      const inspection = await state.client.inspect();
      if (inspection.resources.every((r) => r.status === "loaded")) break;
      if (Date.now() > deadline)
        throw new Error("Flat screen assets did not load");
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    const frame = await context.present(state.binding, async (view) => {
      state.view = view;
      if (!state.started) {
        state.started = true;
        history.start();
      }
      if (index === 1)
        await new Promise((resolve) => setTimeout(resolve, 3500));
      return image(
        index !== 0
          ? await context.host.presentation.capture(view, {
              afterOutputs: [canvasOutput(state.world.reference)],
            })
          : await settled(context.host, view, [
              canvasOutput(state.world.reference),
            ]),
      );
    });
    state.view = null;
    return {
      images: { [args.join("-") || "controls"]: frame },
      report: { inspection: await state.client.inspect() },
    };
  },
  async close(state, context) {
    await state.history?.close();
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
