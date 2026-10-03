import assert from "node:assert/strict";
import test from "node:test";
import { exportProfileTrace } from "../profile_trace.mjs";
const capture = () => ({
  format: "ipp-profile/1",
  captureId: "18446744073709551615",
  hostId: "2",
  source: { target: "native" },
  window: {
    start: "10000000000000000000",
    end: "10000000000000002000",
    clockDomain: "ipp-core.monotonic",
    unit: "nanoseconds",
  },
  availability: { trace: "available" },
  clockCorrelation: {
    referenceDomain: "performance.now",
    referenceParticipant: { contextId: "viewer-1", timeOrigin: 1234 },
    unit: "milliseconds",
    start: { before: 10, after: 11, capture: "10000000000000000000" },
    stop: { before: 12, after: 13, capture: "10000000000000002000" },
  },
  trace: {
    clockDomain: "ipp-core.monotonic",
    unit: "nanoseconds",
    policy: "drop-new",
    capacity: 1,
    droppedEvents: "7",
    retainedBytes: "64",
    events: [
      {
        sequence: "0",
        kind: "system",
        name: "Gui.evaluate",
        identity: {
          scope: "world",
          hostId: "2",
          worldId: "3",
          incarnation: "4",
          compositionId: "5",
          system: "Gui",
          phase: "evaluate",
        },
        start: "10000000000000000100",
        end: "10000000000000001100",
        threadId: "1",
        hostFrameId: "9",
      },
    ],
  },
});
test("exports exact identities, relative microseconds and measured uncertainty", () => {
  const result = exportProfileTrace(capture());
  assert.equal(result.traceEvents[0].ts, 0.1);
  assert.equal(result.traceEvents[0].dur, 1);
  assert.equal(result.traceEvents[0].args.captureId, "18446744073709551615");
  assert.equal(result.metadata.clock.calibration[0].uncertaintyMilliseconds, 1);
  assert.equal(result.metadata.overflow.droppedEvents, "7");
  assert.match(result.metadata.tracks.gpu, /metadata only/);
  assert.equal(JSON.parse(JSON.stringify(result)).traceEvents.length, 1);
});
test("missing correlation and CDP remain explicit", () => {
  const value = capture();
  delete value.clockCorrelation;
  const result = exportProfileTrace(value);
  assert.equal(result.metadata.clock.referenceDomain, null);
  assert.match(result.metadata.tracks.browser, /unavailable/);
});
test("rejects malformed windows, identities, ordering, capacity and clocks", () => {
  for (const mutate of [
    (v) => {
      v.trace.events[0].identity.hostId = "8";
    },
    (v) => {
      v.trace.events[0].end = "0";
    },
    (v) => {
      v.trace.events[0].identity.worldId = 3;
    },
    (v) => {
      v.trace.capacity = 0;
    },
    (v) => {
      v.trace.clockDomain = "gpu";
    },
    (v) => {
      v.clockCorrelation.stop.after = 1;
    },
    (v) => {
      v.trace.events.push(v.trace.events[0]);
      v.trace.capacity = 2;
    },
    (v) => {
      v.trace.droppedEvents = "-1";
    },
    (v) => {
      delete v.trace;
    },
  ]) {
    const value = capture();
    mutate(value);
    assert.throws(() => exportProfileTrace(value));
  }
});

test("retains explicitly unassigned spans and rejects misleading scope assignments", () => {
  const value = capture();
  value.trace.events[0].identity = {
    scope: "unassigned",
    hostId: "0",
    worldId: "0",
    incarnation: "0",
    compositionId: "0",
    system: "",
    phase: null,
  };
  assert.equal(exportProfileTrace(value).traceEvents[0].args.hostId, "0");
  for (const mutate of [
    (v) => {
      v.trace.events[0].identity.worldId = "4";
    },
    (v) => {
      v.trace.events[0].identity.scope = "world";
    },
    (v) => {
      v.trace.events[0].identity.scope = "shared-host";
    },
  ]) {
    const invalid = structuredClone(value);
    mutate(invalid);
    assert.throws(() => exportProfileTrace(invalid));
  }
});

test("optional unavailable adapter remains unavailable with its reason", () => {
  const result = exportProfileTrace(capture(), {
    browserTracks: {
      availability: "unavailable",
      reason: "Target unavailable",
      participant: "worker-1",
    },
  });
  assert.match(
    result.metadata.tracks.browser,
    /unavailable: Target unavailable/,
  );
  assert.equal(result.metadata.browserAdapter.participant, "worker-1");
});
