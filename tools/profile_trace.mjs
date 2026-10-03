/** Export bounded semantic CPU spans without assuming independent clocks share an epoch. */
const decimal = (value, label) => {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error(`${label} must be an unsigned decimal string`);
  return BigInt(value);
};
const finite = (value, label) => {
  if (typeof value !== "number" || !Number.isFinite(value))
    throw new Error(`${label} must be finite`);
  return value;
};

/** Raw clocks and interval calibration remain in metadata; the visual epoch is capture-relative. */
export function exportProfileTrace(
  capture,
  { browserTracks = null, referenceParticipant = null } = {},
) {
  if (capture?.format !== "ipp-profile/1")
    throw new Error("Unknown profile format");
  const host = decimal(capture.hostId, "Host ID");
  const id = decimal(capture.captureId, "capture ID");
  const start = decimal(capture.window?.start, "capture start");
  const end = decimal(capture.window?.end, "capture end");
  if (
    end < start ||
    capture.window.clockDomain !== "ipp-core.monotonic" ||
    capture.window.unit !== "nanoseconds"
  )
    throw new Error("Invalid capture clock window");
  const trace = capture.trace;
  if (!trace || capture.availability?.trace !== "available")
    throw new Error("CPU trace unavailable or not requested");
  if (
    trace.clockDomain !== "ipp-core.monotonic" ||
    trace.unit !== "nanoseconds" ||
    trace.policy !== "drop-new"
  )
    throw new Error("Unsupported trace clock or overflow policy");
  if (
    !Number.isSafeInteger(trace.capacity) ||
    trace.capacity < 1 ||
    !Array.isArray(trace.events) ||
    trace.events.length > trace.capacity
  )
    throw new Error("Invalid trace capacity");
  decimal(trace.droppedEvents, "dropped events");
  decimal(trace.retainedBytes, "retained bytes");
  let previous = -1n;
  const lanes = new Map();
  const events = [];
  for (const span of trace.events) {
    const sequence = decimal(span.sequence, "sequence");
    if (sequence <= previous)
      throw new Error("Trace sequence must increase in issuance order");
    previous = sequence;
    const begin = decimal(span.start, "span start");
    const finish = decimal(span.end, "span end");
    if (finish < begin || begin < start || finish > end)
      throw new Error("Span outside capture window or incomplete");
    const identity = span.identity;
    if (!identity) throw new Error("Missing span identity");
    const spanHost = decimal(identity.hostId, "span Host");
    if (
      spanHost !== host &&
      !(identity.scope === "unassigned" && spanHost === 0n)
    )
      throw new Error("Span Host differs from capture");
    for (const field of ["worldId", "incarnation", "compositionId"])
      decimal(identity[field], field);
    const incarnation = BigInt(identity.incarnation);
    if (
      (identity.scope === "unassigned" &&
        [
          identity.hostId,
          identity.worldId,
          identity.incarnation,
          identity.compositionId,
        ].some((value) => BigInt(value) !== 0n)) ||
      (identity.scope === "shared-host" &&
        (spanHost === 0n ||
          incarnation !== 0n ||
          BigInt(identity.worldId) !== 0n ||
          BigInt(identity.compositionId) !== 0n)) ||
      (identity.scope === "world" && (spanHost === 0n || incarnation === 0n))
    )
      throw new Error("Invalid scope and lifetime identity combination");
    if (
      !["world", "shared-host", "unassigned"].includes(identity.scope) ||
      typeof identity.system !== "string" ||
      ![
        null,
        "check",
        "accept",
        "prepare",
        "evaluate",
        "finish",
        "observe",
      ].includes(identity.phase)
    )
      throw new Error("Invalid semantic identity");
    if (
      !["world", "system", "fixed"].includes(span.kind) ||
      typeof span.name !== "string" ||
      !span.name
    )
      throw new Error("Invalid span name or kind");
    decimal(span.threadId, "thread ID");
    if (span.hostFrameId !== null) decimal(span.hostFrameId, "Host frame ID");
    const laneKey = `${id}/${host}/${span.threadId}`;
    if (!lanes.has(laneKey)) lanes.set(laneKey, lanes.size + 1);
    const delta = begin - start;
    const duration = finish - begin;
    // Capture-relative conversion avoids loss of precision from a large monotonic epoch.
    if (
      delta > BigInt(Number.MAX_SAFE_INTEGER) ||
      duration > BigInt(Number.MAX_SAFE_INTEGER)
    )
      throw new Error("Trace interval exceeds precise numeric export range");
    events.push({
      name: span.name,
      cat: `ipp.${span.kind}`,
      ph: "X",
      pid: 1,
      tid: lanes.get(laneKey),
      ts: Number(delta) / 1000,
      dur: Number(duration) / 1000,
      args: {
        ...identity,
        captureId: capture.captureId,
        hostFrameId: span.hostFrameId,
        threadId: span.threadId,
        sequence: span.sequence,
        rawStart: span.start,
        rawEnd: span.end,
      },
    });
  }
  const calibration = [];
  const correlation = capture.clockCorrelation;
  if (correlation) {
    if (
      correlation.referenceDomain !== "performance.now" ||
      correlation.unit !== "milliseconds"
    )
      throw new Error("Unsupported reference clock");
    if (
      typeof correlation.referenceParticipant?.contextId !== "string" ||
      !correlation.referenceParticipant.contextId
    )
      throw new Error("Missing reference clock participant");
    finite(
      correlation.referenceParticipant.timeOrigin,
      "reference time origin",
    );
    for (const [name, boundary] of [
      ["start", start],
      ["stop", end],
    ]) {
      const bound = correlation[name];
      const before = finite(bound?.before, `${name} before`);
      const after = finite(bound?.after, `${name} after`);
      if (
        after < before ||
        decimal(bound.capture, `${name} boundary`) !== boundary
      )
        throw new Error("Invalid calibration boundary");
      calibration.push({
        boundary: name,
        referenceBefore: before,
        referenceAfter: after,
        coreNanoseconds: bound.capture,
        uncertaintyMilliseconds: after - before,
      });
    }
    if (correlation.stop.before < correlation.start.before)
      throw new Error("Reference clock moved backwards");
  }
  // CDP tracks are retained separately until an adapter measures a compatible clock relation.
  return {
    traceEvents: events,
    displayTimeUnit: "ms",
    metadata: {
      format: "ipp-trace/1",
      captureId: capture.captureId,
      hostId: capture.hostId,
      source: capture.source,
      clock: {
        domain: trace.clockDomain,
        unit: trace.unit,
        visualOrigin: start.toString(),
        alignment: "capture-relative",
        referenceDomain: correlation?.referenceDomain ?? null,
        referenceParticipant: correlation?.referenceParticipant ?? null,
        referenceParticipantLabel: referenceParticipant,
        calibration,
        reason: correlation
          ? "Interval bounds retained; no exact offset assumed"
          : "Reference clock correlation unavailable",
      },
      overflow: {
        policy: trace.policy,
        capacity: trace.capacity,
        retained: trace.events.length,
        droppedEvents: trace.droppedEvents,
        retainedBytes: trace.retainedBytes,
      },
      browserAdapter: browserTracks
        ? {
            availability: browserTracks.availability,
            reason: browserTracks.reason ?? null,
            participant: browserTracks.participant ?? null,
            alignment: browserTracks.alignment ?? null,
          }
        : null,
      tracks: {
        cpu: "available",
        browser:
          browserTracks?.availability === "available"
            ? "available: separate unaligned worker CPU artifact"
            : browserTracks
              ? `unavailable: ${browserTracks.reason ?? "CDP capture failed"}`
              : "unavailable: CDP capture not requested",
        gpu: capture.gpu
          ? "duration metadata only; calibrated GPU clock unavailable"
          : "unavailable: GPU sampling not requested; durations are metadata only",
        gpuProcessCpu: "unavailable: CDP GPU process capture not requested",
      },
      gpu: capture.gpu ?? null,
    },
  };
}
