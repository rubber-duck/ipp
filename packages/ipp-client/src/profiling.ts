/** Instrumentation captures owned by one Host connection and evaluation thread. */
import type { HostWireReader, HostWireWriter } from "./host-protocol.js";

export type ProfilePhase =
  | "check"
  | "accept"
  | "prepare"
  | "evaluate"
  | "finish"
  | "observe";
export type ProfileAvailability = "available" | "unavailable" | "not-requested";

/** Unsigned decimal strings preserve all 64-bit identities through JSON/CDP. */
export interface ProfileIdentity {
  readonly scope: "world" | "shared-host" | "unassigned";
  readonly hostId: string;
  readonly worldId: string;
  readonly incarnation: string;
  readonly compositionId: string;
  readonly system: string;
  readonly phase: ProfilePhase | null;
}

/** Inclusive timing and allocation deltas; nested stage durations must not be summed as wall time. */
export interface ProfileStage {
  readonly kind: "system" | "fixed";
  readonly name: string;
  readonly identity: ProfileIdentity;
  readonly calls: string;
  readonly duration: string;
  readonly allocationCalls: string;
  readonly requestedBytes: string;
}

/** Exclusive categories sum to captured-thread allocation totals. */
export interface ProfileCategory {
  readonly name: string;
  readonly identity: ProfileIdentity;
  readonly allocationCalls: string;
  readonly requestedBytes: string;
}

/** CPU envelope also anchors feature-specific asynchronous GPU/trace artifacts. */
export interface ProfileCapture {
  readonly format: "ipp-profile/1";
  readonly captureId: string;
  readonly hostId: string;
  readonly source: {
    readonly adapter: string;
    readonly target: "native" | "wasm";
    readonly schemaHash: string;
    readonly instrumentation: true;
    readonly scope: "evaluation-thread";
    readonly backgroundAllocations: "excluded";
  };
  readonly window: {
    readonly firstBoundary: string;
    readonly lastBoundary: string;
    readonly start: string;
    readonly end: string;
    readonly clockDomain: "ipp-core.monotonic";
    readonly unit: "nanoseconds";
  };
  readonly availability: {
    readonly cpu: ProfileAvailability;
    readonly allocations: ProfileAvailability;
    readonly gpu: ProfileAvailability;
    readonly trace: ProfileAvailability;
  };
  readonly units: {
    readonly calls: "calls";
    readonly duration: "nanoseconds";
    readonly allocationCalls: "allocation-or-reallocation calls";
    readonly requestedBytes: "requested bytes, not retained memory";
  };
  readonly stages: readonly ProfileStage[];
  readonly categories: readonly ProfileCategory[];
  readonly allocations: {
    readonly calls: string;
    readonly requestedBytes: string;
  };
  readonly trace?: ProfileTrace;
  readonly clockCorrelation?: ProfileClockCorrelation;
  readonly gpu?: ProfileGpuCapture;
  readonly retention: { readonly artifactLimitBytes: string };
  readonly instrumentationStorage: {
    readonly counterBytes: string;
    readonly metadataBytes: string;
  };
}

/** Complete CPU scopes; renderer draw IDs and GPU durations use separate clocks. */
export interface ProfileTrace {
  readonly clockDomain: "ipp-core.monotonic";
  readonly unit: "nanoseconds";
  readonly policy: "drop-new";
  readonly capacity: number;
  readonly droppedEvents: string;
  readonly retainedBytes: string;
  readonly events: readonly ProfileSpan[];
}

export interface ProfileSpan {
  readonly sequence: string;
  readonly kind: "world" | "system" | "fixed";
  readonly name: string;
  readonly identity: ProfileIdentity;
  readonly start: string;
  readonly end: string;
  readonly hostFrameId: string | null;
  readonly threadId: string;
}

/** Transport request bounds bracket each actual capture clock stamp. */
export interface ProfileClockCorrelation {
  readonly referenceDomain: "performance.now";
  readonly referenceParticipant: {
    readonly contextId: string;
    readonly timeOrigin: number;
  };
  readonly unit: "milliseconds";
  readonly start: {
    readonly before: number;
    readonly after: number;
    readonly capture: string;
  };
  readonly stop: {
    readonly before: number;
    readonly after: number;
    readonly capture: string;
  };
}

export interface ProfileGpuCapture {
  readonly sampling: "off" | "frame" | "passes" | "all";
  readonly capability: "unsupported" | "elapsed" | "timestamps";
  readonly availability: ProfileGpuObservation;
  readonly droppedRecords: string;
  readonly records: readonly ProfileGpuRecord[];
  readonly glCalls: null | {
    readonly availability: "available" | "unsupported" | "not-requested";
    readonly stopReason: "owner-stopped" | "context-lost";
    readonly window: {
      readonly start: string | null;
      readonly end: string | null;
      readonly clockDomain: "ipp-core.monotonic";
      readonly unit: "nanoseconds";
    };
    readonly source: "webgl" | "gles";
    readonly contextId: string;
    readonly scope: "device-context";
    readonly draws: string;
    readonly state: string;
    readonly uploads: string;
    readonly other: string;
    readonly profiler: string;
    readonly overflowed: boolean;
    readonly unit: "physical entry point calls";
  };
}

export interface ProfileGpuObservation {
  readonly status:
    | "pending"
    | "available"
    | "unsupported"
    | "disjoint"
    | "context-lost"
    | "capacity-dropped"
    | "overlap-skipped"
    | "stopped";
  readonly duration?: string;
  readonly unit?: "nanoseconds";
}

export interface ProfileGpuRecord {
  readonly captureId: string;
  readonly hostId: string;
  readonly contextId: string;
  readonly frameId: string;
  readonly preparation: boolean;
  readonly world: null | {
    readonly id: string;
    readonly incarnation: string;
    readonly compositionId: string | null;
  };
  readonly surface: null | {
    readonly slot: number;
    readonly generation: number;
  };
  readonly scope: "frame" | "surface" | "atlas" | "cache-repaint" | "composite";
  readonly availability: ProfileGpuObservation;
}

/** Feature owners retain this issuance identity through delayed results. */
export interface ProfileAsyncIdentity {
  readonly captureId: string;
  readonly hostId: string;
  readonly contextId: string;
  readonly frameId: string;
  readonly world?: {
    readonly id: string;
    readonly incarnation: string;
    readonly compositionId: string;
  };
  readonly entity?: { readonly id: string; readonly incarnation: string };
}

export interface HostProfileStatus {
  readonly available: boolean;
  readonly captureId: string | null;
}

type Send = (
  tag: number,
  encode?: (writer: HostWireWriter) => void,
) => Promise<HostWireReader>;
const profilingReader = Symbol.for("ipp.host.profiling");

/** Registered by generated Host clients, independently of their presentation. */
export function bindHostProfiling(target: object, reader: HostProfiling): void {
  Object.defineProperty(target, profilingReader, { value: reader });
}

/** Optional profiler of a live generated native/worker Host client. */
export function hostProfiling(target: object): HostProfiling {
  const reader = (target as { [profilingReader]?: HostProfiling })[
    profilingReader
  ];
  if (!reader) throw new TypeError("Expected an IPP Host client");
  return reader;
}

interface Reply {
  status: number;
  capture: bigint;
  total: bigint;
  offset: bigint;
  bytes: Uint8Array;
}

/** Ownership failures are explicit and leave the Host connection usable. */
export class ProfileControlError extends Error {
  constructor(
    readonly reason:
      | "unavailable"
      | "busy"
      | "invalid-capture"
      | "incomplete"
      | "capacity",
  ) {
    super(`Host profiling ${reason}`);
    this.name = "ProfileControlError";
  }
}

/** Native and worker readback use the same generated Host contract and transport. */
export class HostProfiling {
  private startBounds?: { before: number; after: number };
  private stopBounds: { before: number; after: number } | undefined;
  private referenceParticipant?: { contextId: string; timeOrigin: number };
  private active: bigint | undefined;

  constructor(
    private readonly send: Send,
    private readonly tag: (name: string) => number,
  ) {}

  private async request(
    kind: number,
    capture = 0n,
    offset = 0n,
    counters = false,
    maxArtifactBytes = 16_777_216n,
    gpu: "off" | "frame" | "passes" | "all" = "off",
    glCalls = false,
    maxEvents = 0n,
  ): Promise<Reply> {
    const reader = await this.send(
      this.tag("HOST_REQUEST_PROFILE"),
      (writer) => {
        writer.u8(
          this.tag(
            [
              "PROFILE_REQUEST_STATUS",
              "PROFILE_REQUEST_START",
              "PROFILE_REQUEST_STOP",
              "PROFILE_REQUEST_READ",
              "PROFILE_REQUEST_RELEASE",
            ][kind]!,
          ),
        );
        if (kind === 1) {
          writer.u8(Number(counters));
          writer.u64(maxEvents);
          writer.u64(maxArtifactBytes);
          writer.u8(Number(glCalls));
          writer.u8(this.tag(`PROFILE_GPU_${gpu.toUpperCase()}`));
        } else if (kind >= 2) {
          writer.u64(capture);
          if (kind === 3) writer.u64(offset);
        }
      },
    );
    if (reader.u8() !== this.tag("HOST_RESPONSE_PROFILE"))
      throw new Error("Unexpected Host profiling response");
    const reply = {
      status: reader.u8(),
      capture: reader.u64(),
      total: reader.u64(),
      offset: reader.u64(),
      bytes: reader.bytes(),
    };
    reader.end();
    return reply;
  }

  private require(reply: Reply): Reply {
    if (reply.status !== this.tag("PROFILE_STATUS_AVAILABLE")) {
      const reasons = [
        "unavailable",
        "busy",
        "invalid-capture",
        "incomplete",
        "capacity",
      ] as const;
      const labels = [
        "UNAVAILABLE",
        "BUSY",
        "INVALID_CAPTURE",
        "INCOMPLETE",
        "CAPACITY",
      ];
      const reason =
        reasons[
          labels.findIndex(
            (label) => this.tag(`PROFILE_STATUS_${label}`) === reply.status,
          )
        ];
      if (!reason) throw new Error("Unknown Host profiling status");
      throw new ProfileControlError(reason);
    }
    return reply;
  }

  async status(): Promise<HostProfileStatus> {
    const reply = await this.request(0);
    if (reply.status === this.tag("PROFILE_STATUS_UNAVAILABLE"))
      return { available: false, captureId: null };
    this.require(reply);
    return {
      available: true,
      captureId: reply.capture === 0n ? null : reply.capture.toString(),
    };
  }

  /** Starts counters on the Host evaluation thread; never changes its clock. */
  async start(
    options: {
      counters: boolean;
      maxArtifactBytes?: number;
      gpu?: "off" | "frame" | "passes" | "all";
      glCalls?: boolean;
      trace?: { maxEvents: number };
    } = {
      counters: true,
    },
  ): Promise<string> {
    const limit = options.maxArtifactBytes ?? 16_777_216;
    if (!Number.isSafeInteger(limit) || limit <= 0)
      throw new RangeError("maxArtifactBytes must be a positive safe integer");
    const maxEvents = options.trace?.maxEvents ?? 0;
    if (
      !Number.isSafeInteger(maxEvents) ||
      maxEvents < 0 ||
      (options.trace && maxEvents === 0)
    )
      throw new RangeError("trace.maxEvents must be a positive safe integer");
    // Diagnostic identity works on non-secure origins and requires no Web Crypto.
    const participant = this.referenceParticipant ?? {
      contextId: `reader-${performance.timeOrigin}-${Math.random().toString(36).slice(2)}`,
      timeOrigin: performance.timeOrigin,
    };
    const before = performance.now();
    const reply = this.require(
      await this.request(
        1,
        0n,
        0n,
        options.counters,
        BigInt(limit),
        options.gpu,
        options.glCalls,
        BigInt(maxEvents),
      ),
    );
    this.startBounds = { before, after: performance.now() };
    this.stopBounds = undefined;
    this.referenceParticipant = participant;
    this.active = reply.capture;
    return reply.capture.toString();
  }

  /** Stop, drain bounded pages and return a deeply immutable JSON artifact. */
  async stop(): Promise<ProfileCapture> {
    if (this.active === undefined)
      throw new ProfileControlError("invalid-capture");
    const id = this.active;
    const before = performance.now();
    // A failed response can still mean the Host stopped. Retain the first
    // attempted send boundary and widen through the eventually received reply.
    this.stopBounds ??= { before, after: before };
    const stopped = this.require(await this.request(2, id));
    this.stopBounds.after = performance.now();
    if (stopped.total > BigInt(Number.MAX_SAFE_INTEGER))
      throw new Error("Profile artifact length is not representable");
    const bytes = new Uint8Array(Number(stopped.total));
    let offset = 0n;
    while (offset < stopped.total) {
      const page = this.require(await this.request(3, id, offset));
      if (
        page.capture !== id ||
        page.total !== stopped.total ||
        page.offset !== offset ||
        page.bytes.length === 0 ||
        page.bytes.length > 65_536 ||
        offset + BigInt(page.bytes.length) > stopped.total
      )
        throw new Error("Invalid profile artifact page");
      bytes.set(page.bytes, Number(offset));
      offset += BigInt(page.bytes.length);
    }
    const artifact = parseProfileCapture(
      new TextDecoder("utf-8", { fatal: true }).decode(bytes),
    );
    if (artifact.captureId !== id.toString())
      throw new Error("Profile artifact generation mismatch");
    return freeze({
      ...artifact,
      clockCorrelation: {
        referenceDomain: "performance.now",
        referenceParticipant: this.referenceParticipant!,
        unit: "milliseconds",
        start: { ...this.startBounds!, capture: artifact.window.start },
        stop: { ...this.stopBounds, capture: artifact.window.end },
      },
    });
  }

  /** Cancels running/readback state and reclaims history; foreign/stale generations fail. */
  async release(captureId: string): Promise<void> {
    const id = unsigned(captureId, "captureId");
    this.require(await this.request(4, id));
    if (this.active === id) this.active = undefined;
  }
}

function unsigned(value: unknown, label: string): bigint {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error(`Invalid profile ${label}`);
  const number = BigInt(value);
  if (number > 0xffff_ffff_ffff_ffffn)
    throw new Error(`Invalid profile ${label}`);
  return number;
}

function freeze<T>(value: T): T {
  if (value && typeof value === "object") {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}

/** Shared parser for transported native/worker and direct benchmark readback. */
export function parseProfileCapture(text: string): ProfileCapture {
  const capture = JSON.parse(text) as ProfileCapture;
  if (
    capture?.format !== "ipp-profile/1" ||
    capture.source?.instrumentation !== true ||
    capture.source.scope !== "evaluation-thread" ||
    capture.source.backgroundAllocations !== "excluded" ||
    !Array.isArray(capture.stages) ||
    !Array.isArray(capture.categories)
  )
    throw new Error("Invalid profile capture envelope");
  unsigned(capture.captureId, "captureId");
  unsigned(capture.hostId, "hostId");
  unsigned(capture.source.schemaHash, "schemaHash");
  for (const record of [...capture.stages, ...capture.categories]) {
    for (const key of [
      "hostId",
      "worldId",
      "incarnation",
      "compositionId",
    ] as const)
      unsigned(record.identity[key], key);
    unsigned(record.allocationCalls, "allocationCalls");
    unsigned(record.requestedBytes, "requestedBytes");
  }
  for (const record of capture.stages) {
    unsigned(record.calls, "calls");
    unsigned(record.duration, "duration");
  }
  if (capture.trace) {
    const trace = capture.trace;
    if (
      trace.clockDomain !== "ipp-core.monotonic" ||
      trace.unit !== "nanoseconds" ||
      trace.policy !== "drop-new" ||
      !Number.isSafeInteger(trace.capacity) ||
      trace.capacity <= 0 ||
      !Array.isArray(trace.events) ||
      trace.events.length > trace.capacity
    )
      throw new Error("Invalid CPU trace envelope");
    unsigned(trace.droppedEvents, "trace.droppedEvents");
    unsigned(trace.retainedBytes, "trace.retainedBytes");
    let sequence = 0n;
    for (const event of trace.events) {
      const next = unsigned(event.sequence, "trace.sequence");
      const start = unsigned(event.start, "trace.start");
      const end = unsigned(event.end, "trace.end");
      if (
        next <= sequence ||
        end < start ||
        (event.kind !== "world" &&
          event.kind !== "system" &&
          event.kind !== "fixed")
      )
        throw new Error("Invalid complete CPU span");
      sequence = next;
      for (const key of [
        "hostId",
        "worldId",
        "incarnation",
        "compositionId",
      ] as const)
        unsigned(event.identity[key], `trace.${key}`);
      unsigned(event.threadId, "trace.threadId");
      if (event.hostFrameId !== null)
        unsigned(event.hostFrameId, "trace.hostFrameId");
    }
  }
  const calls = unsigned(capture.allocations.calls, "allocations.calls");
  const bytes = unsigned(
    capture.allocations.requestedBytes,
    "allocations.requestedBytes",
  );
  if (
    capture.categories.reduce(
      (sum, category) => sum + BigInt(category.allocationCalls),
      0n,
    ) !== calls ||
    capture.categories.reduce(
      (sum, category) => sum + BigInt(category.requestedBytes),
      0n,
    ) !== bytes
  )
    throw new Error("Profile exclusive category totals disagree");
  return freeze(capture);
}

/** Inclusive measured stage sums by semantic composition/System/phase. */
export interface ProfileCompositionSummary {
  readonly compositionId: string;
  readonly system: string;
  readonly phase: ProfilePhase | null;
  readonly kind: "system" | "fixed";
  readonly name: string;
  readonly calls: string;
  readonly duration: string;
  readonly allocationCalls: string;
  readonly requestedBytes: string;
  readonly worlds: readonly {
    readonly id: string;
    readonly incarnation: string;
  }[];
}

/** Derive deterministic summaries from samples; durations remain inclusive. */
export function summarizeProfileCompositions(
  capture: ProfileCapture,
): readonly ProfileCompositionSummary[] {
  const groups = new Map<
    string,
    {
      sample: ProfileStage;
      values: bigint[];
      worlds: Map<string, { id: string; incarnation: string }>;
    }
  >();
  for (const sample of capture.stages) {
    if (sample.identity.scope !== "world") continue;
    const key = JSON.stringify([
      sample.identity.compositionId,
      sample.identity.system,
      sample.identity.phase,
      sample.kind,
      sample.name,
    ]);
    let group = groups.get(key);
    if (!group) {
      group = { sample, values: [0n, 0n, 0n, 0n], worlds: new Map() };
      groups.set(key, group);
    }
    [
      sample.calls,
      sample.duration,
      sample.allocationCalls,
      sample.requestedBytes,
    ].forEach((value, index) => {
      group.values[index] = group.values[index]! + BigInt(value);
    });
    const world = {
      id: sample.identity.worldId,
      incarnation: sample.identity.incarnation,
    };
    group.worlds.set(`${world.id}/${world.incarnation}`, world);
  }
  const compare = (left: string, right: string) =>
    left < right ? -1 : left > right ? 1 : 0;
  return freeze(
    [...groups.entries()]
      .sort(([left], [right]) => compare(left, right))
      .map(([, group]) => ({
        compositionId: group.sample.identity.compositionId,
        system: group.sample.identity.system,
        phase: group.sample.identity.phase,
        kind: group.sample.kind,
        name: group.sample.name,
        calls: group.values[0]!.toString(),
        duration: group.values[1]!.toString(),
        allocationCalls: group.values[2]!.toString(),
        requestedBytes: group.values[3]!.toString(),
        worlds: [...group.worlds.values()].sort((left, right) =>
          compare(
            `${left.id}/${left.incarnation}`,
            `${right.id}/${right.incarnation}`,
          ),
        ),
      })),
  );
}
