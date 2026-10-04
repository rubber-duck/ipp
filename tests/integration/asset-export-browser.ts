/** Same generated-client scenario on worker WebGL or a separately owned GLES Host. */
import { createWorkerHost } from "../../packages/ipp-client/src/worker.js";
import { nativePresentationTransport } from "../../packages/ipp-client/src/native-presentation.js";
import { webSocketTransport } from "../../packages/ipp-client/src/transport.js";
import { BulkReadParticipant } from "./scenarios/bulk-reads.js";
import { assetExportRoundTrip } from "./scenarios/asset-exports.js";
import type {
  PresentedCapture,
  HostClientBase,
  AssetWorldClient,
} from "@ipp/client";

export async function probe(
  urls: { generated: string; wasm: string; workerScript: string },
  endpoint?: { url: string; presentationUrl: string },
) {
  const contract = await import(urls.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 96;
  canvas.height = 64;
  document.body.append(canvas);
  const owner = endpoint
    ? undefined
    : createWorkerHost(
        urls.workerScript,
        urls.wasm,
        contract.MAX_MESSAGE_BYTES,
        { canvas: canvas.transferControlToOffscreen() },
      );
  const transport = () =>
    endpoint
      ? nativePresentationTransport(endpoint.url, endpoint.presentationUrl)
      : owner!.connect();
  let fixture: BulkReadParticipant | undefined;
  let host: HostClientBase<AssetWorldClient> | undefined;
  let peer: HostClientBase<AssetWorldClient> | undefined;
  try {
    host = await contract.IppHostClient.connectTransport(transport(), {
      timeoutMs: 15000,
    });
    peer = await contract.IppHostClient.connectTransport(
      endpoint ? webSocketTransport(endpoint.url) : owner!.connect(),
      {
        timeoutMs: 15000,
      },
    );
    fixture = new BulkReadParticipant(
      endpoint ? webSocketTransport(endpoint.url) : owner!.connect(),
      10000,
    );
    await fixture.open();
    return await assetExportRoundTrip(
      host!,
      peer!,
      (op) => fixture!.fixture(op),
      {
        graphics: true,
        evidence: {
          record: (value) => window.assetExportRecord(value),
          capture: (name, image: PresentedCapture) =>
            window.assetExportCapture(name, {
              ...image.view.binding.viewport,
              pixels: [...new Uint8Array(image.pixels)],
            }),
        },
        ...(!endpoint
          ? {
              fenceGate: (
                operation: "hold" | "suspend" | "release" | "resume",
                world: bigint,
              ) => window.assetExportFenceGate(operation, world.toString()),
            }
          : {}),
      },
    );
  } finally {
    await Promise.allSettled([host?.close(), peer?.close(), fixture?.close()]);
    await owner?.close();
    canvas.remove();
  }
}

declare global {
  interface Window {
    assetExportRecord(value: object): Promise<void>;
    assetExportFenceGate(
      operation: "hold" | "suspend" | "release" | "resume",
      world: string,
    ): Promise<string>;
    assetExportCapture(
      name: string,
      image: { width: number; height: number; pixels: number[] },
    ): Promise<void>;
  }
}
