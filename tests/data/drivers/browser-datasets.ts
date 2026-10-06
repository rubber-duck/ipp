import type { DatasetHost } from "../scenarios/datasets.js";
import { datasets } from "../scenarios/datasets.js";
import { createWorkerHost } from "../../../packages/ipp-client/src/worker.js";

/** A new arrangement supplies a connector; assertions and fixtures remain common. */
export async function workerDatasets(urls: {
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
  const hosts: DatasetHost[] = [];
  const connect = async () => {
    const host: DatasetHost = await contract.IppHostClient.connectTransport(
      owner.connect(),
    );
    hosts.push(host);
    return host;
  };
  try {
    return await datasets(await connect(), connect);
  } finally {
    await Promise.allSettled(hosts.map((host) => host.close()));
    await owner.close();
  }
}

export async function workerDataAuthoring(urls: {
  generated: string;
  worker: string;
  wasm: string;
}) {
  const contract = await import(urls.generated);
  const { dataAuthoring, restoreDataAuthoring } = await import(
    "../scenarios/data-authoring.js"
  );
  const run = async <T>(scenario: (host: DatasetHost) => Promise<T>) => {
    const owner = createWorkerHost(
      urls.worker,
      urls.wasm,
      contract.MAX_MESSAGE_BYTES,
    );
    let host: DatasetHost | undefined;
    try {
      host = await contract.IppHostClient.connectTransport(owner.connect());
      return await scenario(host!);
    } finally {
      await host?.close();
      await owner.close();
    }
  };
  const record = async (label: string, value: unknown) => {
    const bridge = (
      globalThis as unknown as {
        recordData?: (label: string, value: unknown) => Promise<void>;
      }
    ).recordData;
    await bridge?.(
      label,
      JSON.parse(
        JSON.stringify(value, (_, value) =>
          typeof value === "bigint" ? { $bigint: String(value) } : value,
        ),
      ),
    );
  };
  const saved = await run((host) => dataAuthoring(host, contract, record));
  const restored = await run((host) =>
    restoreDataAuthoring(host, contract, saved),
  );
  return { artifacts: saved.artifacts, restored };
}
