import {
  createWorkerHost,
  type WorkerHost,
} from "../../packages/ipp-client/src/worker.js";

let owner: WorkerHost | undefined;

/** Start the maintained production worker and generated-client connection. */
export async function start(urls: {
  generated: string;
  wasm: string;
  workerScript: string;
}) {
  const contract = await import(urls.generated);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 32;
  document.body.append(canvas);
  owner = createWorkerHost(
    urls.workerScript,
    urls.wasm,
    contract.MAX_MESSAGE_BYTES,
    {
      canvas: canvas.transferControlToOffscreen(),
    },
  );
  await contract.IppHostClient.connectTransport(owner.connect());
}

/** Host teardown disposes connections, pending tasks and the rendering context. */
export async function close() {
  await owner?.close();
  owner = undefined;
}
