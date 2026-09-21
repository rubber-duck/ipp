import { PortTransport, type Client } from "@ipp/client";
import {
  HostWireReader,
  HostWireWriter,
} from "../../packages/ipp-client/src/host-protocol.js";
import type { BrowserRuntimeConfiguration } from "./browser-runtime.js";

const HOST_RESPONSE_MAGIC = new Uint8Array([73, 80, 80, 65, 1, 0, 0, 0]);

interface FrameSchedulerObservation {
  callbacks: number;
  cancelled: number;
  pending: number;
}

interface ResourceSchedulerObservation extends FrameSchedulerObservation {
  streamedBytes: number;
  completedResponses: number;
}

/** Hold real worker frames while HTTP bytes and WASM Host loaders continue. */
export async function observeResourceProgressWithoutFrames(
  configuration: BrowserRuntimeConfiguration,
  source: string,
  expectedBytes: number,
) {
  const contract = (await import(
    configuration.contractModuleUrl
  )) as typeof import("../../target/integration-artifacts/client/generated.js");
  const sourceCode = `
    import ${JSON.stringify(configuration.workerScriptUrl)};
    const request = self.requestAnimationFrame.bind(self);
    const cancel = self.cancelAnimationFrame.bind(self);
    const pending = new Map();
    let nextFrame = 1;
    let frozen = false;
    let callbacks = 0;
    let cancelled = 0;
    let streamedBytes = 0;
    let completedResponses = 0;
    let releaseResource;
    const resourceGate = new Promise(resolve => {
      releaseResource = resolve;
    });
    const schedule = (id, entry) => {
      entry.nativeId = request(time => {
        if (!pending.delete(id)) return;
        callbacks++;
        entry.callback(time);
      });
    };
    self.requestAnimationFrame = callback => {
      const id = nextFrame++;
      const entry = { callback, nativeId: undefined };
      pending.set(id, entry);
      if (!frozen) schedule(id, entry);
      return id;
    };
    self.cancelAnimationFrame = id => {
      const entry = pending.get(id);
      if (!entry) return;
      pending.delete(id);
      if (entry.nativeId !== undefined) cancel(entry.nativeId);
      cancelled++;
    };
    const originalFetch = self.fetch.bind(self);
    self.fetch = async (input, init) => {
      const response = await originalFetch(input, init);
      const url = new URL(
        typeof input === "string" || input instanceof URL ? input : input.url,
        self.location.href,
      );
      if (url.pathname.endsWith(".wasm") || !response.body) return response;
      const reader = response.body.getReader();
      const body = new ReadableStream({
        type: "bytes",
        async pull(controller) {
          await resourceGate;
          const result = await reader.read();
          if (result.done) {
            completedResponses++;
            controller.close();
            controller.byobRequest?.respond(0);
            probePort?.postMessage({
              type: "resource-complete",
              callbacks,
              cancelled,
              pending: pending.size,
              streamedBytes,
              completedResponses,
            });
            return;
          }
          streamedBytes += result.value.byteLength;
          controller.enqueue(result.value);
        },
        cancel(reason) {
          return reader.cancel(reason);
        },
      });
      return new Response(body, {
        status: response.status,
        statusText: response.statusText,
        headers: response.headers,
      });
    };
    let probePort;
    self.addEventListener("message", event => {
      if (!event.data.probePort) return;
      probePort = event.data.probePort;
      probePort.onmessage = message => {
        if (message.data.type === "freeze") {
          frozen = true;
          for (const entry of pending.values()) {
            if (entry.nativeId !== undefined) {
              cancel(entry.nativeId);
              entry.nativeId = undefined;
              cancelled++;
            }
          }
        }
        if (message.data.type === "resume") {
          frozen = false;
          for (const [id, entry] of pending) {
            if (entry.nativeId === undefined) schedule(id, entry);
          }
        }
        if (message.data.type === "release-resource") releaseResource();
        probePort.postMessage({
          type: "observation",
          id: message.data.id,
          callbacks,
          cancelled,
          pending: pending.size,
          streamedBytes,
          completedResponses,
        });
      };
    });
  `;
  const workerUrl = URL.createObjectURL(
    new Blob([sourceCode], { type: "text/javascript" }),
  );
  const worker = new Worker(workerUrl, { type: "module" });
  const probe = new MessageChannel();
  const channel = new MessageChannel();
  const transport = new PortTransport(channel.port1, () => worker.terminate());
  let nextControl = 1;
  const controls = new Map<
    number,
    (observation: ResourceSchedulerObservation) => void
  >();
  let resourceCompleted!: (observation: ResourceSchedulerObservation) => void;
  const completed = new Promise<ResourceSchedulerObservation>((resolve) => {
    resourceCompleted = resolve;
  });
  probe.port1.onmessage = (event) => {
    const observation = event.data as ResourceSchedulerObservation & {
      type: string;
      id?: number;
    };
    if (observation.type === "resource-complete") {
      resourceCompleted(observation);
      return;
    }
    if (observation.id !== undefined) {
      controls.get(observation.id)?.(observation);
      controls.delete(observation.id);
    }
  };
  const control = (
    type: "freeze" | "resume" | "release-resource" | "observe",
  ) =>
    new Promise<ResourceSchedulerObservation>((resolve) => {
      const id = nextControl++;
      controls.set(id, resolve);
      probe.port1.postMessage({ type, id });
    });
  worker.addEventListener("error", (event) =>
    transport.fail(new Error(event.message || "Worker failed")),
  );
  let client: Client | undefined;
  try {
    worker.postMessage(
      {
        type: "init",
        wasmUrl: configuration.wasmUrl,
        port: channel.port2,
        probePort: probe.port2,
      },
      [channel.port2, probe.port2],
    );
    const connected = await contract.IppClient.connectTransport(transport, {
      timeoutMs: configuration.timeoutMs,
    });
    client = connected;
    const events: { source: string; status: string }[] = [];
    connected.onResourceChange((event) => events.push({ ...event }));
    const committed = await connected.batch([
      contract.Entity.create(1, { symbolicId: "frame-independent-asset" }),
      contract.Transform.insert(contract.Entity.alias(1)),
      contract.MeshInstance.insert(contract.Entity.alias(1), { source }),
      contract.PickingGeometry.insert(contract.Entity.alias(1)),
    ]);
    await connected.waitForFrame(committed.tick);
    const afterCommitFrame = await control("freeze");
    await control("release-resource");
    const afterTransfer = await Promise.race([
      completed,
      new Promise<never>((_, reject) =>
        setTimeout(
          () =>
            reject(new Error("Resource stream did not finish without frames")),
          configuration.timeoutMs,
        ),
      ),
    ]);
    if (afterTransfer.streamedBytes !== expectedBytes)
      throw new Error(
        `Streamed ${afterTransfer.streamedBytes} of ${expectedBytes} bytes`,
      );
    await control("resume");
    let inspection = await connected.inspect();
    for (
      let frame = 0;
      frame < 60 &&
      !inspection.resources.some(
        (resource) =>
          resource.source === source && resource.status === "loaded",
      );
      frame++
    ) {
      await connected.waitForFrame(inspection.tick);
      inspection = await connected.inspect();
    }
    const afterInspectFrame = await control("freeze");
    return {
      committed,
      afterCommitFrame,
      afterTransfer,
      afterInspectFrame,
      inspection,
      events,
    };
  } finally {
    await client?.close().catch(() => undefined);
    await transport.close().catch(() => undefined);
    worker.terminate();
    probe.port1.close();
    URL.revokeObjectURL(workerUrl);
  }
}

/** Drive the real host lifecycle channel, independently of its command protocol. */
export async function observeVisibility(
  configuration: BrowserRuntimeConfiguration,
  initiallyHidden = false,
) {
  const contract = (await import(
    configuration.contractModuleUrl
  )) as typeof import("../../target/integration-artifacts/client/generated.js");
  // Observe actual browser callbacks without replacing the clock, runtime or
  // transport. The extra port belongs only to this maintained lifecycle probe.
  const source = `
    import ${JSON.stringify(configuration.workerScriptUrl)};
    const request = self.requestAnimationFrame.bind(self);
    const cancel = self.cancelAnimationFrame.bind(self);
    const pending = new Set();
    let callbacks = 0;
    let cancelled = 0;
    self.requestAnimationFrame = callback => {
      const id = request(time => {
        pending.delete(id);
        callbacks++;
        callback(time);
      });
      pending.add(id);
      return id;
    };
    self.cancelAnimationFrame = id => {
      if (pending.delete(id)) cancelled++;
      cancel(id);
    };
    self.addEventListener("message", event => {
      const port = event.data.schedulerPort;
      port.onmessage = () => port.postMessage({
        callbacks, cancelled, pending: pending.size,
      });
    }, { once: true });
  `;
  const workerUrl = URL.createObjectURL(
    new Blob([source], { type: "text/javascript" }),
  );
  const worker = new Worker(workerUrl, { type: "module" });
  const scheduler = new MessageChannel();
  const observeScheduler = () =>
    new Promise<FrameSchedulerObservation>((resolve) => {
      scheduler.port1.onmessage = (event) => resolve(event.data);
      scheduler.port1.postMessage(null);
    });
  const channel = new MessageChannel();
  const transport = new PortTransport(channel.port1, () => worker.terminate());
  worker.addEventListener("error", (event) =>
    transport.fail(new Error(event.message || "Worker failed")),
  );
  let client: Client | undefined;
  try {
    worker.postMessage(
      {
        type: "init",
        wasmUrl: configuration.wasmUrl,
        port: channel.port2,
        schedulerPort: scheduler.port2,
        hidden: initiallyHidden,
      },
      [channel.port2, scheduler.port2],
    );
    client = await contract.IppClient.connectTransport(transport, {
      timeoutMs: configuration.timeoutMs,
    });
    const initial = await client.waitForFrame();
    const initialScheduler = await observeScheduler();
    channel.port1.postMessage({ type: "visibility", hidden: true });
    // Same-port ordering makes this a barrier after the visibility message.
    const paused = await client.inspect();
    const pausedScheduler = await observeScheduler();
    const firstPausedFrame = await client.waitForFrame(paused.tick);
    const secondPausedFrame = await client.waitForFrame(firstPausedFrame.tick);
    const created = await client.batch([
      contract.Entity.create(1, { symbolicId: "created-while-paused" }),
      contract.Scalar.insert(contract.Entity.alias(1), { value: 44 }),
    ]);
    const whilePaused = await client.inspect();
    const whilePausedScheduler = await observeScheduler();
    channel.port1.postMessage({ type: "visibility", hidden: false });
    const resumed = await client.inspect();
    const resumedScheduler = await observeScheduler();
    return {
      initial,
      initialScheduler,
      paused,
      pausedScheduler,
      firstPausedFrame,
      secondPausedFrame,
      created,
      whilePaused,
      whilePausedScheduler,
      resumed,
      resumedScheduler,
    };
  } finally {
    await client?.close().catch(() => undefined);
    await transport.close().catch(() => undefined);
    worker.terminate();
    scheduler.port1.close();
    URL.revokeObjectURL(workerUrl);
  }
}

/** Withhold delivery acknowledgements at the real MessagePort fault boundary. */
export async function observeStalledReceiver(
  configuration: BrowserRuntimeConfiguration,
) {
  const contract = (await import(
    configuration.contractModuleUrl
  )) as typeof import("../../target/integration-artifacts/client/generated.js");
  const worker = new Worker(configuration.workerScriptUrl, { type: "module" });
  const channel = new MessageChannel();
  let delivered = 0;
  let frames = 0;
  let connection: bigint | undefined;
  let session: bigint | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await new Promise<{
      delivered: number;
      frames: number;
      error: string;
    }>((resolve, reject) => {
      timer = setTimeout(
        () => reject(new Error("Stalled receiver was not disconnected")),
        configuration.timeoutMs,
      );
      worker.onerror = (event) => reject(new Error(event.message));
      channel.port1.onmessage = (event) => {
        try {
          const data = event.data;
          if (data.type === "ready") {
            const bytes = contract.bootstrap();
            channel.port1.postMessage({ type: "data", bytes: bytes.buffer }, [
              bytes.buffer,
            ]);
          } else if (data.type === "data") {
            delivered++;
            const bytes = new Uint8Array(data.bytes);
            if (connection === undefined) {
              connection = contract.acceptBootstrap(bytes);
              // Host negotiation does not create a World or emit its frames.
              const request = new HostWireWriter();
              request.raw(new Uint8Array([73, 80, 80, 72, 1, 0, 0, 0]));
              request.u64(connection);
              request.u64(1n);
              request.u8(2);
              request.string("stalled-receiver");
              request.hints();
              request.u8(1);
              const create = request.finish();
              channel.port1.postMessage(
                { type: "data", bytes: create.buffer },
                [create.buffer],
              );
            } else if (session === undefined) {
              const response = new HostWireReader(bytes);
              if (
                !response
                  .raw(8)
                  .every(
                    (byte, index) => byte === HOST_RESPONSE_MAGIC[index],
                  ) ||
                response.u64() !== connection ||
                response.u64() !== 1n ||
                response.u8() !== 2
              )
                throw new Error(
                  "Expected the stalled receiver's World attachment",
                );
              response.world();
              session = response.u64();
              response.end();
            } else {
              const response = contract.decodeResponse(bytes, session);
              if (
                response.session !== session ||
                response.body.kind !== "frame"
              ) {
                throw new Error(
                  "Expected an autonomous frame in the stalled receiver probe",
                );
              }
              frames++;
            }
            // Intentionally no ack: no mocked worker, runtime or response.
          } else if (data.type === "error") {
            resolve({ delivered, frames, error: data.message });
          } else {
            throw new Error(`Unexpected worker envelope: ${data.type}`);
          }
        } catch (error) {
          reject(error);
        }
      };
      channel.port1.start();
      worker.postMessage(
        { type: "init", wasmUrl: configuration.wasmUrl, port: channel.port2 },
        [channel.port2],
      );
    });
  } finally {
    clearTimeout(timer);
    channel.port1.close();
    worker.terminate();
  }
}
