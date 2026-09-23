import { resolve } from "node:path";
import { runRetainedGui } from "../render/retained-gui-environment.js";
import { BENCHMARK_SURFACE_CACHE_POLICY } from "../render/retained-gui-scenario.js";

const flags = process.argv.slice(2).filter((value) => value.startsWith("--"));
const [frames, output] = process.argv
  .slice(2)
  .filter((value) => !value.startsWith("--"));
const unknown = flags.filter((flag) => flag !== "--surface-cache");
if (unknown.length) throw new Error(`Unknown options: ${unknown.join(", ")}`);
const iterations = Number(frames ?? 60);
if (!Number.isSafeInteger(iterations) || iterations < 1)
  throw new Error("Iterations must be positive");
await runRetainedGui(
  AbortSignal.timeout(3600000),
  iterations,
  resolve(output ?? "target/performance/retained-gui"),
  flags.includes("--surface-cache")
    ? { surfaceCache: BENCHMARK_SURFACE_CACHE_POLICY }
    : {},
);
