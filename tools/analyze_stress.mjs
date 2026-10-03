/** Summarize retained benchmark profiles; accepts one or more profile.json paths. */
import { readFile } from "node:fs/promises";
const quantile = (values, fraction) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil((sorted.length - 1) * fraction)];
};
for (const path of process.argv.slice(2)) {
  const profile = JSON.parse(await readFile(path, "utf8"));
  const measurements = [
    ...profile.rows.map((row) => ({ ...row, workload: "held, unculled" })),
    ...(profile.culling?.rows ?? []).map((row) => ({
      ...row,
      workload: "held, culled",
    })),
    ...(profile.moving
      ? [
          {
            workload: profile.culling ? "moving, culled" : "moving, unculled",
            timing: profile.moving,
            allocation: profile.movingAllocations,
            frame: profile.movingFrame,
          },
        ]
      : []),
  ];
  const rows = measurements.map(({ workload, timing, allocation, frame }) => ({
    workload,
    frames: timing.frames.length,
    medianMs: quantile(
      timing.frames.map((f) => f[0]),
      0.5,
    ),
    p95Ms: quantile(
      timing.frames.map((f) => f[0]),
      0.95,
    ),
    rustAllocations:
      Number(allocation.allocations.calls) / allocation.frames.length,
    requestedBytes:
      Number(allocation.allocations.requestedBytes) / allocation.frames.length,
    wasmMiB: timing.memoryBytes / (1024 * 1024),
    draws: frame?.draws ?? "not captured",
    shadowDraws: timing.shadowDrawCalls ?? "not captured",
  }));
  console.log(
    path,
    `grid=${profile.fixture.grid}, entities=${profile.setup.entities}, drivers=${profile.setup.drivers}`,
  );
  console.table(rows);
  for (const { workload, allocation } of measurements) {
    console.log(`${workload}: allocation sources`);
    console.table(
      allocation.categories
        .filter((c) => BigInt(c.allocationCalls) > 0n)
        .map((c) => ({
          name: c.name,
          calls: Number(c.allocationCalls) / allocation.frames.length,
          bytes: Number(c.requestedBytes) / allocation.frames.length,
        }))
        .sort((a, b) => b.bytes - a.bytes),
    );
    console.log("Instrumented core System time (separate from timing windows)");
    console.table(
      allocation.stages
        .filter((stage) => stage.kind === "system")
        .map((stage) => ({
          host: stage.identity.hostId,
          world: stage.identity.worldId,
          incarnation: stage.identity.incarnation,
          composition: stage.identity.compositionId,
          name: stage.identity.system,
          phase: stage.identity.phase,
          milliseconds: Number(stage.duration) / 1e6 / allocation.frames.length,
        }))
        .filter((row) => row.milliseconds >= 0.1)
        .sort((a, b) => b.milliseconds - a.milliseconds),
    );
  }
}
