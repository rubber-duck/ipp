import { resolve } from "node:path";
import { runRetainedGui } from "../render/retained-gui-environment.js";

const iterations = Number(process.argv[2] ?? 60);
if (!Number.isSafeInteger(iterations) || iterations < 1)
  throw new Error("Iterations must be positive");
await runRetainedGui(
  AbortSignal.timeout(3600000),
  iterations,
  resolve(process.argv[3] ?? "target/performance/retained-gui"),
);
