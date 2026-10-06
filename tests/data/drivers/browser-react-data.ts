import type { DatasetHost } from "../scenarios/datasets.js";
import { reactData } from "../scenarios/react-data.js";
import { createWorkerHost } from "../../../packages/ipp-client/src/worker.js";

export async function workerReactData(urls: {
  generated: string;
  worker: string;
  wasm: string;
}) {
  const contract = await import(urls.generated);
  const owner = createWorkerHost(
    urls.worker,
    urls.wasm,
    contract.MAX_MESSAGE_BYTES,
  );
  let host: DatasetHost | undefined;
  try {
    host = await contract.IppHostClient.connectTransport(owner.connect());
    return await reactData(host!, contract);
  } finally {
    await host?.close();
    await owner.close();
  }
}
