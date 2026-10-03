/** Worker CPU and allocation sampling, separate from ordinary timing windows. */
import type { Browser } from "playwright";

async function sampleWorker<T>(
  browser: Browser,
  workerUrl: string,
  collect: () => Promise<T>,
  kind: "cpu" | "allocations",
) {
  const cdp = await browser.newBrowserCDPSession();
  let sessionId: string | undefined;
  let next = 0;
  const pending = new Map<
    number,
    { resolve: (value: unknown) => void; reject: (error: Error) => void }
  >();
  const rejectPending = (error: Error) => {
    for (const promise of pending.values()) promise.reject(error);
    pending.clear();
  };
  try {
    const { targetInfos } = await cdp.send("Target.getTargets");
    const target = targetInfos.find(
      (candidate) => candidate.type === "worker" && candidate.url === workerUrl,
    );
    if (!target) throw new Error("Profiling worker target unavailable");
    ({ sessionId } = await cdp.send("Target.attachToTarget", {
      targetId: target.targetId,
      flatten: false,
    }));
    cdp.on("Target.receivedMessageFromTarget", (event) => {
      if (event.sessionId !== sessionId) return;
      const message = JSON.parse(event.message);
      const promise = pending.get(message.id);
      if (!promise) return;
      pending.delete(message.id);
      if (message.error) promise.reject(new Error(message.error.message));
      else promise.resolve(message.result);
    });
    cdp.on("Target.detachedFromTarget", (event) => {
      if (event.sessionId === sessionId)
        rejectPending(new Error("Profiling worker detached"));
    });
    const send = (method: string, params = {}) => {
      const id = ++next;
      return new Promise<unknown>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        void cdp
          .send("Target.sendMessageToTarget", {
            sessionId: sessionId!,
            message: JSON.stringify({ id, method, params }),
          })
          .catch((error: Error) => {
            pending.delete(id);
            reject(error);
          });
      });
    };
    if (kind === "cpu") {
      await send("Profiler.enable");
      await send("Profiler.setSamplingInterval", { interval: 1000 });
      await send("Profiler.start");
      const window = await collect();
      const profile = await send("Profiler.stop");
      return { window, profile };
    }
    await send("HeapProfiler.startSampling", {
      samplingInterval: 128,
      stackDepth: 32,
      includeObjectsCollectedByMajorGC: true,
      includeObjectsCollectedByMinorGC: true,
    });
    const window = await collect();
    const heap = await send("HeapProfiler.stopSampling");
    return { window, heap };
  } finally {
    rejectPending(new Error("Worker profiling ended"));
    if (sessionId)
      await cdp.send("Target.detachFromTarget", { sessionId }).catch(() => {});
    await cdp.detach();
  }
}

export function sampleWorkerAllocations<T>(
  browser: Browser,
  workerUrl: string,
  collect: () => Promise<T>,
) {
  return sampleWorker(browser, workerUrl, collect, "allocations");
}

export function sampleWorkerCpu<T>(
  browser: Browser,
  workerUrl: string,
  collect: () => Promise<T>,
) {
  return sampleWorker(browser, workerUrl, collect, "cpu");
}

/** Optional CDP artifact, explicitly separate until its clock relation is measured. */
export async function optionalWorkerCpu<T>(
  browser: Browser,
  workerUrl: string,
  collect: () => Promise<T>,
) {
  let collected = false;
  let value: { result: T } | undefined;
  try {
    const sampled = await sampleWorkerCpu(browser, workerUrl, async () => {
      collected = true;
      value = { result: await collect() };
      return value.result;
    });
    return {
      value: sampled.window,
      track: {
        availability: "available",
        participant: workerUrl,
        clockDomain: "cdp-profiler.unspecified",
        alignment: "unaligned; no measured relation to caller/core clock",
        profile: sampled.profile,
      },
    };
  } catch (error) {
    if (collected && !value) throw error;
    return {
      value: value ? value.result : await collect(),
      track: {
        availability: "unavailable",
        participant: workerUrl,
        reason: error instanceof Error ? error.message : String(error),
      },
    };
  }
}
