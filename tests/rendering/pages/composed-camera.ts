import type { Client, HostClientBase, GeometryEncoder } from "@ipp/client";
import { exerciseComposedCamera } from "../scenarios/composed-camera.js";

export async function exercise(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  const canvas = document.createElement("canvas");
  canvas.width = 600;
  canvas.height = 400;
  document.body.replaceChildren(canvas);
  const contract = await import(configuration.generatedModuleUrl);
  const host: HostClientBase<Client> =
    await contract.IppHostClient.connectWorker(
      configuration.workerScriptUrl,
      configuration.wasmUrl,
      { canvas: canvas.transferControlToOffscreen(), timeoutMs: 20_000 },
    );
  try {
    return await exerciseComposedCamera(
      host,
      contract.encodeBoundingShape as GeometryEncoder,
    );
  } finally {
    await host.close();
  }
}
