/** Optional harness observations; none of these totals is an IPP ownership total. */
import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import type { Browser } from "playwright";

export type MemoryIdentity = {
  capture: string;
  pipelineRun: string | null;
  device: Record<string, unknown> | null;
};

export type MemoryObservation = MemoryIdentity & {
  source: "ipp-statistics" | "linux-drm-fdinfo" | "chrome-memory-infra";
  scope: "ipp-accounted" | "drm-client" | "browser-process";
  sampledAt: string;
  units: "bytes";
  metric: string;
  value: string | null;
  unavailableReason: string | null;
  processId: number | null;
  clientId: string | null;
  deviceId: string | null;
};

type DrmDescriptor = {
  driver: string;
  deviceId: string | null;
  clientId: string | null;
  metrics: { metric: string; value: string }[];
  raw: string;
};

function bytes(value: string): string | null {
  const match = /^(\d+)\s*(bytes|KiB|MiB)?$/.exec(value.trim());
  if (!match) return null;
  const multiplier =
    match[2] === "KiB" ? 1024n : match[2] === "MiB" ? 1048576n : 1n;
  return (BigInt(match[1]!) * multiplier).toString();
}

/** Kernel standard fields only; retain exact integers rather than rounding to JS numbers. */
export function parseDrmFdinfo(text: string): DrmDescriptor | null {
  const fields = new Map<string, string>();
  for (const line of text.split("\n")) {
    const match = /^(drm-[^\s:]+):\s*(.*)$/.exec(line);
    if (match) fields.set(match[1]!, match[2]!);
  }
  const driver = fields.get("drm-driver");
  if (!driver) return null;
  const metrics: DrmDescriptor["metrics"] = [];
  for (const [key, value] of fields) {
    if (!/^drm-(total|shared|resident|purgeable|active|memory)-\S+$/.test(key))
      continue;
    // amdgpu's deprecated memory key aliases resident; never present both as distinct data.
    if (
      key.startsWith("drm-memory-") &&
      fields.has(key.replace("drm-memory-", "drm-resident-"))
    )
      continue;
    const amount = bytes(value);
    if (amount !== null) metrics.push({ metric: key, value: amount });
  }
  return {
    driver,
    deviceId: fields.get("drm-pdev") ?? null,
    clientId: fields.get("drm-client-id") ?? null,
    metrics,
    raw: [...fields].map(([key, value]) => `${key}: ${value}`).join("\n"),
  };
}

function observation(
  identity: MemoryIdentity,
  source: MemoryObservation["source"],
  scope: MemoryObservation["scope"],
  metric: string,
  value: string | null,
  unavailableReason: string | null,
  extra: Partial<
    Pick<MemoryObservation, "processId" | "clientId" | "deviceId">
  > = {},
): MemoryObservation {
  return {
    ...identity,
    source,
    scope,
    sampledAt: new Date().toISOString(),
    units: "bytes",
    metric,
    value,
    unavailableReason,
    processId: null,
    clientId: null,
    deviceId: null,
    ...extra,
  };
}

export function accountedMemory(
  identity: MemoryIdentity,
  statistics: {
    gui?: Record<string, number>;
    surfaces?: Record<string, number>;
  } | null,
): MemoryObservation[] {
  return [
    "guiResidentBytes",
    "glyphResidentBytes",
    "analyticGlyphResidentBytes",
    "surfaceCacheResidentBytes",
  ].map((metric) => {
    const value = statistics?.gui?.[metric] ?? statistics?.surfaces?.[metric];
    const available =
      value !== undefined && Number.isSafeInteger(value) && value >= 0;
    return observation(
      identity,
      "ipp-statistics",
      "ipp-accounted",
      metric,
      available ? String(value) : null,
      available ? null : "Counter unavailable in this runtime statistics set",
    );
  });
}

/** Only explicitly owned process ids are examined. Duplicate DRM clients appear once. */
export async function sampleDrmMemory(
  identity: MemoryIdentity,
  processIds: readonly number[],
  procRoot = "/proc",
) {
  const observations: MemoryObservation[] = [];
  const raw: { processId: number; descriptor: string; fdinfo: string }[] = [];
  const seen = new Set<string>();
  if (process.platform !== "linux" && procRoot === "/proc") {
    observations.push(
      observation(
        identity,
        "linux-drm-fdinfo",
        "drm-client",
        "availability",
        null,
        "Linux DRM fdinfo is unavailable on this platform",
      ),
    );
    return { observations, raw };
  }
  if (!processIds.length)
    observations.push(
      observation(
        identity,
        "linux-drm-fdinfo",
        "drm-client",
        "availability",
        null,
        "No owned GPU process id is available",
      ),
    );
  for (const processId of new Set(processIds)) {
    if (!Number.isSafeInteger(processId) || processId <= 0)
      throw new Error("Invalid owned process id");
    const directory = join(procRoot, String(processId), "fdinfo");
    let descriptors: string[];
    try {
      descriptors = (await readdir(directory))
        .filter((name) => /^\d+$/.test(name))
        .sort();
    } catch (error) {
      observations.push(
        observation(
          identity,
          "linux-drm-fdinfo",
          "drm-client",
          "availability",
          null,
          `Cannot read fdinfo: ${error instanceof Error ? error.message : String(error)}`,
          { processId },
        ),
      );
      continue;
    }
    let found = false;
    for (const descriptor of descriptors) {
      let text: string;
      try {
        text = await readFile(join(directory, descriptor), "utf8");
      } catch (error) {
        observations.push(
          observation(
            identity,
            "linux-drm-fdinfo",
            "drm-client",
            "availability",
            null,
            `Descriptor unavailable during sample: ${error instanceof Error ? error.message : String(error)}`,
            { processId },
          ),
        );
        continue;
      }
      const parsed = parseDrmFdinfo(text);
      if (!parsed) continue;
      found = true;
      raw.push({ processId, descriptor, fdinfo: parsed.raw });
      if (parsed.clientId === null) {
        observations.push(
          observation(
            identity,
            "linux-drm-fdinfo",
            "drm-client",
            "availability",
            null,
            "Driver omitted drm-client-id; descriptor cannot be safely deduplicated",
            { processId, deviceId: parsed.deviceId },
          ),
        );
        continue;
      }
      const key = `${parsed.deviceId ?? "global"}/${parsed.clientId}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const extra = {
        processId,
        clientId: parsed.clientId,
        deviceId: parsed.deviceId,
      };
      if (!parsed.metrics.length)
        observations.push(
          observation(
            identity,
            "linux-drm-fdinfo",
            "drm-client",
            "availability",
            null,
            "Driver exports no supported memory counters",
            extra,
          ),
        );
      for (const metric of parsed.metrics)
        observations.push(
          observation(
            identity,
            "linux-drm-fdinfo",
            "drm-client",
            metric.metric,
            metric.value,
            null,
            extra,
          ),
        );
    }
    if (!found)
      observations.push(
        observation(
          identity,
          "linux-drm-fdinfo",
          "drm-client",
          "availability",
          null,
          "Process has no readable DRM descriptors",
          { processId },
        ),
      );
  }
  return { observations, raw };
}

type TraceEvent = {
  ph?: string;
  pid?: number;
  id?: string;
  args?: {
    dumps?: {
      allocators?: Record<
        string,
        {
          attrs?: Record<
            string,
            { units?: string; value?: string; type?: string }
          >;
        }
      >;
    };
  };
};

/** Preserve allocator paths individually: parents, children and shared backing must not be summed. */
function normalizedGuid(guid: string): string {
  return /^(0x)?[0-9a-f]+$/i.test(guid)
    ? BigInt(guid.startsWith("0x") ? guid : `0x${guid}`).toString(16)
    : guid;
}

export function parseChromeMemory(
  identity: MemoryIdentity,
  events: readonly TraceEvent[],
  dumpGuid: string,
): MemoryObservation[] {
  const observations: MemoryObservation[] = [];
  for (const event of events) {
    if (
      event.ph !== "v" ||
      event.id === undefined ||
      normalizedGuid(event.id) !== normalizedGuid(dumpGuid)
    )
      continue;
    for (const [path, allocator] of Object.entries(
      event.args?.dumps?.allocators ?? {},
    )) {
      if (!/^(gpu|gl|skia|vulkan)(\/|$)/i.test(path)) continue;
      for (const [attribute, value] of Object.entries(allocator.attrs ?? {})) {
        if (
          value.type !== "scalar" ||
          value.units !== "bytes" ||
          !value.value ||
          !/^[0-9a-f]+$/i.test(value.value)
        )
          continue;
        observations.push(
          observation(
            identity,
            "chrome-memory-infra",
            "browser-process",
            `${path}/${attribute}`,
            BigInt(`0x${value.value}`).toString(),
            null,
            { processId: event.pid ?? null },
          ),
        );
      }
    }
  }
  return observations;
}

/** Dedicated trace only. Failed start never grants ownership to end someone else's trace. */
export async function sampleChromeMemory(
  identity: MemoryIdentity,
  browser: Browser,
  timeoutMs = 10_000,
) {
  const session = await browser.newBrowserCDPSession().catch(() => null);
  if (!session)
    return {
      observations: [
        observation(
          identity,
          "chrome-memory-infra",
          "browser-process",
          "availability",
          null,
          "Browser CDP session unavailable",
        ),
      ],
      processIds: [],
      raw: { dumpGuid: null, memoryEvents: [] },
    };
  const events: TraceEvent[] = [];
  let ownsTrace = false;
  let dumpGuid: string | null = null;
  let failure: string | null = null;
  let processIds: number[] = [];
  const bounded = async <T>(promise: Promise<T>): Promise<T> => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([
        promise,
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("MemoryInfra operation timed out")),
            timeoutMs,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  };
  session.on("Tracing.dataCollected", (event) => {
    for (const value of event.value)
      if (value.ph === "v") events.push(value as TraceEvent);
  });
  let completed!: () => void;
  const completion = new Promise<void>((resolve) => {
    completed = resolve;
  });
  session.on("Tracing.tracingComplete", completed);
  try {
    try {
      const info = await bounded(session.send("SystemInfo.getProcessInfo"));
      processIds = info.processInfo
        .filter((item) => item.type.toLowerCase() === "gpu")
        .map((item) => item.id);
    } catch {
      /* GPU process identity is optional; observations preserve dump process ids. */
    }
    await bounded(
      session.send("Tracing.start", {
        transferMode: "ReportEvents",
        traceConfig: {
          excludedCategories: ["*"],
          includedCategories: ["disabled-by-default-memory-infra"],
          memoryDumpConfig: {
            allowed_dump_modes: ["detailed"],
            triggers: [],
          } as unknown as Record<string, string>,
        },
      }),
    );
    ownsTrace = true;
    const result = await bounded(
      session.send("Tracing.requestMemoryDump", { levelOfDetail: "detailed" }),
    );
    dumpGuid = result.dumpGuid;
    if (!result.success)
      failure = "Chrome did not produce the requested memory dump";
  } catch (error) {
    failure = error instanceof Error ? error.message : String(error);
  } finally {
    if (ownsTrace) {
      try {
        await bounded(session.send("Tracing.end"));
        await bounded(completion);
      } catch (error) {
        failure = `MemoryInfra trace cleanup failed: ${error instanceof Error ? error.message : String(error)}`;
      }
    }
    await session.detach().catch(() => {});
  }
  const observations =
    failure === null && dumpGuid !== null
      ? parseChromeMemory(identity, events, dumpGuid)
      : [];
  if (!observations.length)
    observations.push(
      observation(
        identity,
        "chrome-memory-infra",
        "browser-process",
        "availability",
        null,
        failure ??
          `Requested dump ${dumpGuid} has no correlated supported GPU backing allocation fields; physical VRAM residency is not exposed`,
      ),
    );
  return { observations, processIds, raw: { dumpGuid, memoryEvents: events } };
}
