import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type CameraOutputReference,
} from "@ipp/client";
import {
  renderDiagnostics,
  type RenderStatisticsSnapshot,
} from "@ipp/client/diagnostics";
import { createRoot } from "@ipp/react";
import { image, settled } from "../../../../tools/shared-host/presentation.js";
import type { Present } from "../../skin-lab/specimen-session.js";
import {
  contextScene,
  contextSamples,
  CONTEXT_SYSTEMS,
  type ContextOptions,
} from "./context-scene.js";

export async function openOpticalContext(
  host: HostClientBase<Client>,
  font: Uint8Array<ArrayBuffer>,
  name: string,
) {
  const canvas = await host.createWorld({
    symbolicId: `${name}/canvas`,
    temporary: true,
    selectedSystems: ["ipp.animation", "ipp.canvas", "ipp.asset-dependencies"],
    canvas: { extent: [1100, 480], unitsPerMetre: 200 },
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
  await canvasRoot.render(contextSamples(font));
  await sceneRoot.render(contextScene(canvas.reference));
  const camera = (await sceneRoot.bindOutput(
    "camera",
    "camera",
  )) as CameraOutputReference;
  const binding = await host.setRootOutput(camera, {
    width: 1100,
    height: 700,
    devicePixelRatio: 1,
  });
  return {
    host,
    font,
    statistics: null as RenderStatisticsSnapshot | null,
    samples: "[0,0]",
    canvas,
    scene,
    canvasClient,
    sceneClient,
    canvasRoot,
    sceneRoot,
    binding,
  };
}

export type OpticalContextSession = Awaited<
  ReturnType<typeof openOpticalContext>
>;

async function captureOpticalContextFrame(
  session: OpticalContextSession,
  present: Present,
  options: ContextOptions = {},
) {
  const samples = JSON.stringify([options.probe ?? 0, options.layer ?? 0]);
  if (samples !== session.samples) {
    await session.canvasRoot.render(contextSamples(session.font, options));
    session.samples = samples;
  }
  await session.sceneRoot.render(
    contextScene(session.canvas.reference, options),
  );
  for (const id of ["font", "optical", "backdrop"]) {
    const deadline = Date.now() + 30000;
    while (session.canvasRoot.getAsset(id)?.status !== "loaded") {
      const status = session.canvasRoot.getAsset(id);
      if (status?.status === "failed" || Date.now() > deadline)
        throw new Error(`${id}: ${JSON.stringify(status)}`);
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
  }
  return present(session.binding, async (view) => {
    const capture = await settled(session.host, view, [
      session.binding.output,
      canvasOutput(session.canvas.reference),
    ]);
    session.statistics =
      (await renderDiagnostics(session.host)?.statistics()) ?? null;
    return image(capture);
  });
}

export async function closeOpticalContext(s: OpticalContextSession) {
  await s.sceneRoot.unmount();
  await s.canvasRoot.unmount();
  await s.sceneClient.close();
  await s.canvasClient.close();
  await s.host.destroyWorld(s.scene.reference);
  await s.host.destroyWorld(s.canvas.reference);
}

export async function captureOpticalContext(
  session: OpticalContextSession,
  present: Present,
  options: ContextOptions = {},
) {
  try {
    return await captureOpticalContextFrame(session, present, options);
  } catch (error) {
    throw new Error(
      `Glass context ${JSON.stringify(options)}: ${String(error)}`,
      { cause: error },
    );
  }
}
