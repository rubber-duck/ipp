import assert from "node:assert/strict";
import test from "node:test";
import { HostWireReader, HostWireWriter } from "../src/host-protocol.js";
import { HostProfiling, type ProfileCapture } from "../src/profiling.js";

const tags: Record<string, number> = {
  HOST_REQUEST_PROFILE: 26,
  HOST_RESPONSE_PROFILE: 20,
  PROFILE_REQUEST_START: 1,
  PROFILE_REQUEST_STOP: 2,
  PROFILE_REQUEST_READ: 3,
  PROFILE_REQUEST_RELEASE: 4,
  PROFILE_STATUS_AVAILABLE: 0,
  PROFILE_GPU_OFF: 0,
};

test("lost Stop reply keeps original request bounds and immutable capture ownership", async () => {
  let firstStop = true;
  let earliest = 0;
  const capture: ProfileCapture = {
    format: "ipp-profile/1",
    captureId: "9",
    hostId: "1",
    source: {
      adapter: "headless",
      target: "native",
      schemaHash: "1",
      instrumentation: true,
      scope: "evaluation-thread",
      backgroundAllocations: "excluded",
    },
    window: {
      firstBoundary: "1",
      lastBoundary: "2",
      start: "1",
      end: "2",
      clockDomain: "ipp-core.monotonic",
      unit: "nanoseconds",
    },
    availability: {
      cpu: "not-requested",
      allocations: "not-requested",
      gpu: "not-requested",
      trace: "not-requested",
    },
    units: {
      calls: "calls",
      duration: "nanoseconds",
      allocationCalls: "allocation-or-reallocation calls",
      requestedBytes: "requested bytes, not retained memory",
    },
    stages: [],
    categories: [],
    allocations: { calls: "0", requestedBytes: "0" },
    retention: { artifactLimitBytes: "10000" },
    instrumentationStorage: { counterBytes: "0", metadataBytes: "0" },
  };
  const bytes = new TextEncoder().encode(JSON.stringify(capture));
  const profiler = new HostProfiling(
    async (_tag, encode) => {
      const request = new HostWireWriter();
      encode?.(request);
      const kind = new HostWireReader(request.finish()).u8();
      if (kind === 2 && firstStop) {
        firstStop = false;
        earliest = performance.now();
        throw new Error("reply lost after Host froze capture");
      }
      const response = new HostWireWriter();
      response.u8(20);
      response.u8(0);
      response.u64(9n);
      response.u64(kind === 1 || kind === 4 ? 0n : BigInt(bytes.length));
      response.u64(0n);
      response.bytes(kind === 3 ? bytes : new Uint8Array());
      return new HostWireReader(response.finish());
    },
    (name) => {
      const tag = tags[name];
      if (tag === undefined) throw new Error(`Unknown tag ${name}`);
      return tag;
    },
  );
  const token = await profiler.start({ counters: false });
  await assert.rejects(profiler.stop(), /reply lost/);
  await new Promise((resolve) => setTimeout(resolve, 5));
  const retried = await profiler.stop();
  assert.ok(retried.clockCorrelation!.stop.before <= earliest);
  assert.ok(retried.clockCorrelation!.stop.after >= earliest);
  assert.equal(
    retried.clockCorrelation!.referenceParticipant.timeOrigin,
    performance.timeOrigin,
  );
  assert.ok(
    retried.clockCorrelation!.referenceParticipant.contextId.length > 0,
  );
  assert.ok(Object.isFrozen(retried.clockCorrelation));
  await profiler.release(token);
});
