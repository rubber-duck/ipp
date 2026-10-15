import {
  canvasOutput,
  type CameraOutputReference,
  type Client,
  type HostClientBase,
} from "@ipp/client";
import { createRoot } from "@ipp/react";
import type { GuiKitContract } from "@ipp/react/gui-kit";
import type { GuiControlHandle } from "@ipp/react/gui";
import { image, settled } from "../../../../tools/shared-host/presentation.js";
import type { Present } from "../../skin-lab/specimen-session.js";
import { ProductionSamples } from "./production-scene.js";
import { LiquidSamples } from "./liquid-scene.js";
import { ComponentSamples, COMPONENT_EXTENT } from "./component-scene.js";
import {
  contextScene,
  CONTEXT_SYSTEMS,
  type ContextOptions,
} from "./context-scene.js";

export async function openOpticalComponents(
  host: HostClientBase<Client>,
  font: Uint8Array<ArrayBuffer>,
  contract: GuiKitContract,
  name: string,
  mode: boolean | "liquid" = false,
) {
  const production = mode === true;
  const liquid = mode === "liquid";
  const canvas = await host.createWorld({
    symbolicId: `${name}/canvas`,
    temporary: true,
    selectedSystems: [
      "ipp.animation",
      "ipp.canvas",
      "ipp.gui",
      "ipp.gui-layout",
      "ipp.asset-dependencies",
      "ipp.lifecycle-publisher",
    ],
    canvas: { extent: COMPONENT_EXTENT, unitsPerMetre: 200 },
  });
  const scene = await host.createWorld({
    symbolicId: `${name}/scene`,
    temporary: true,
    selectedSystems: CONTEXT_SYSTEMS,
  });
  const canvasClient = await host.openWorld(canvas.reference);
  const sceneClient = await host.openWorld(scene.reference);
  const canvasRoot = createRoot(canvasClient, { host });
  const sceneRoot = createRoot(sceneClient, { host });
  const controls = new Map<string, GuiControlHandle>();
  const Samples = liquid
    ? LiquidSamples
    : production
      ? ProductionSamples
      : ComponentSamples;
  await canvasRoot.render(
    <Samples font={font} contract={contract} controls={controls} />,
  );
  await sceneRoot.render(
    contextScene(canvas.reference, { extent: COMPONENT_EXTENT }),
  );
  const camera = (await sceneRoot.bindOutput(
    "camera",
    "camera",
  )) as CameraOutputReference;
  const binding = await host.setRootOutput(camera, {
    width: COMPONENT_EXTENT[0],
    height: COMPONENT_EXTENT[1],
    devicePixelRatio: 1,
  });
  const session = {
    host,
    production,
    liquid,
    font,
    contract,
    controls,
    canvas,
    scene,
    canvasClient,
    sceneClient,
    canvasRoot,
    sceneRoot,
    binding,
  };
  await readyOpticalComponents(session);
  return session;
}

export type OpticalComponentSession = Awaited<
  ReturnType<typeof openOpticalComponents>
>;

export async function readyOpticalComponents(
  session: Pick<
    OpticalComponentSession,
    "canvasRoot" | "production" | "liquid"
  >,
) {
  for (const id of session.liquid
    ? ["font", "component-glass", "liquid-backdrop"]
    : session.production
      ? ["font", "production-glass/shader"]
      : [
          "font",
          "component-glass",
          "component-region",
          "component-backdrop",
          "component-mark",
        ]) {
    const deadline = Date.now() + 30000;
    while (session.canvasRoot.getAsset(id)?.status !== "loaded") {
      const status = session.canvasRoot.getAsset(id);
      if (status?.status === "failed" || Date.now() > deadline)
        throw new Error(`${id}: ${JSON.stringify(status)}`);
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }
}

export async function captureOpticalComponents(
  session: OpticalComponentSession,
  present: Present,
  options: ContextOptions = {},
) {
  await session.sceneRoot.render(
    contextScene(session.canvas.reference, {
      ...options,
      extent: COMPONENT_EXTENT,
    }),
  );
  return present(session.binding, async (view) =>
    image(
      await settled(session.host, view, [
        session.binding.output,
        canvasOutput(session.canvas.reference),
      ]),
    ),
  );
}

export async function closeOpticalComponents(session: OpticalComponentSession) {
  await session.sceneRoot.unmount();
  await session.canvasRoot.unmount();
  await session.sceneClient.close();
  await session.canvasClient.close();
  await session.host.destroyWorld(session.scene.reference);
  await session.host.destroyWorld(session.canvas.reference);
}
