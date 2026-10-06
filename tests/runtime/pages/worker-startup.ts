import type {
  Client,
  MessageTransport,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  createWorkerHost,
  workerTransport,
  type WorkerHost,
} from "../../../packages/ipp-client/src/worker.js";
import { PortTransport } from "../../../packages/ipp-client/src/transport.js";
import type { HostLifecycleContract } from "../drivers/browser-host-transport.js";
import { createEntity } from "../../fixtures/commands.js";
import { check } from "../../harness/page/checks.js";

interface Configuration {
  generated: string;
  worker: string;
  wasm: string;
}

type Host = WorldPersistenceHostClient<Client>;
type Contract = HostLifecycleContract & { MAX_MESSAGE_BYTES: number };

function observe(transport: MessageTransport) {
  const counts = { ready: 0, sent: 0, replies: 0 };
  const wrapped: MessageTransport = {
    start(events) {
      transport.start({
        ...events,
        ready() {
          counts.ready++;
          events.ready();
        },
        message(bytes) {
          counts.replies++;
          events.message(bytes);
        },
      });
    },
    send(bytes) {
      counts.sent++;
      transport.send(bytes);
    },
    close: () => transport.close(),
  };
  return { transport: wrapped, counts };
}

type Started = { host: Host } | { error: unknown };
let state:
  | {
      contract: Contract;
      owner?: WorkerHost;
      startup: Promise<Started>;
      abort: AbortController;
      counts: ReturnType<typeof observe>["counts"];
      survivor?: Promise<Started>;
    }
  | undefined;

export async function start(configuration: Configuration, shared: boolean) {
  check(!state, "Startup fixture already exists");
  const contract = (await import(configuration.generated)) as Contract;
  const owner = shared
    ? createWorkerHost(
        configuration.worker,
        configuration.wasm,
        contract.MAX_MESSAGE_BYTES,
      )
    : undefined;
  const observed = observe(
    owner?.connect() ??
      workerTransport(
        configuration.worker,
        configuration.wasm,
        contract.MAX_MESSAGE_BYTES,
      ),
  );
  const abort = new AbortController();
  const startup = contract.IppHostClient.connectTransport(observed.transport, {
    signal: abort.signal,
  }).then<Started, Started>(
    (host) => ({ host }),
    (error: unknown) => ({ error }),
  );
  state = {
    contract,
    startup,
    abort,
    counts: observed.counts,
    ...(owner ? { owner } : {}),
  };
  if (owner)
    state.survivor = contract.IppHostClient.connectTransport(
      owner.connect(),
    ).then<Started, Started>(
      (host) => ({ host }),
      (error: unknown) => ({ error }),
    );
}

export async function cancel() {
  check(state, "Missing startup fixture");
  state.abort.abort();
  const result = await state.startup;
  check(
    "error" in result &&
      result.error instanceof Error &&
      /aborted/.test(result.error.message),
    "Cancelled startup did not reject honestly",
  );
  check(
    state.counts.ready === 0 &&
      state.counts.sent === 0 &&
      state.counts.replies === 0,
    "Cancelled startup sent bootstrap or received late readiness",
  );
  return { ...state.counts, cancelled: true };
}

export async function closeUnstarted() {
  check(state?.owner, "Missing shared Host");
  const endpoint = state.owner.openPort();
  try {
    const rejected = new Promise<void>((resolve, reject) => {
      endpoint.port.onmessage = (event: MessageEvent<unknown>) => {
        const message = event.data;
        if (
          typeof message === "object" &&
          message !== null &&
          "type" in message &&
          message.type === "error" &&
          "connection" in message &&
          message.connection === endpoint.connection &&
          "message" in message &&
          message.message === "Invalid pending worker connection envelope"
        )
          resolve();
        else
          reject(
            new Error(
              "Foreign pending close was not rejected at its own endpoint",
            ),
          );
      };
      endpoint.port.start();
    });
    endpoint.port.postMessage({
      type: "close",
      connection: endpoint.connection + 1n,
    });
    await rejected;
  } finally {
    endpoint.dispose();
  }
  for (let index = 0; index < 70; index++) {
    const transport = state.owner.connect();
    const closing = transport.close();
    check(
      transport.close() === closing,
      "Repeated endpoint close changed promise",
    );
    await closing;
  }
  return { cancelledEndpoints: 70, foreignCloseRejected: true };
}

export async function completeShared() {
  check(state?.owner && state.survivor, "Missing shared startup");
  const { owner, contract } = state;
  const connected = await state.survivor;
  check(
    "host" in connected,
    "Healthy endpoint failed during sibling startup cancellation",
  );
  const survivor = connected.host;
  check(
    (await survivor.listWorlds()).length === 0,
    "Cancelled startup created a late World",
  );
  const world = await survivor.createWorld({ selectedSystems: [] });
  const client = await survivor.openWorld(world.reference);
  try {
    const batch = await client.batch([createEntity(1, "survivor")]);
    check(
      batch.ok && batch.aliases.length === 1,
      "Healthy World stopped progressing",
    );

    const endpoint = owner.openPort();
    const transport = new PortTransport(
      endpoint.port,
      endpoint.connection,
      () => endpoint.dispose(),
    );
    let closing: Promise<void> | undefined;
    const closeAtReady = (event: MessageEvent<unknown>) => {
      const message = event.data;
      if (
        typeof message === "object" &&
        message !== null &&
        "type" in message &&
        message.type === "ready"
      )
        closing = transport.close();
    };
    endpoint.port.addEventListener("message", closeAtReady);
    const race = observe(transport);
    const failed = await contract.IppHostClient.connectTransport(
      race.transport,
    ).then<Started, Started>(
      (host) => ({ host }),
      (error: unknown) => ({ error }),
    );
    endpoint.port.removeEventListener("message", closeAtReady);
    check(closing, "Real ready/close race was not exercised");
    await closing;
    check("error" in failed, "Closing endpoint completed Host bootstrap");
    check(
      race.counts.ready === 0 && race.counts.sent === 0,
      "Late ready revived bootstrap",
    );
    check(
      state.counts.ready === 0 &&
        state.counts.sent === 0 &&
        state.counts.replies === 0,
      "Cancelled endpoint revived after WASM load",
    );
    const inspection = await client.inspect();
    check(
      inspection.entities.length === 1,
      "Sibling cancellation disturbed survivor state",
    );
    check(
      (await survivor.listWorlds()).length === 1,
      "Late ready allocated an unexpected World",
    );
    return {
      ...state.counts,
      raceReady: race.counts.ready,
      raceSent: race.counts.sent,
      healthyEntities: inspection.entities.length,
    };
  } finally {
    await client.close();
    await survivor.destroyWorld(world.reference);
  }
}

export async function close() {
  if (!state) return;
  state.abort.abort();
  const current = state;
  state = undefined;
  await Promise.allSettled(
    [current.startup, ...(current.survivor ? [current.survivor] : [])].map(
      async (pending) => {
        const result = await pending;
        if ("host" in result) await result.host.close();
      },
    ),
  );
  await current.owner?.close();
}
