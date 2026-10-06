/** Bundle opt-in benchmark scenarios; Python owns builds and execution. */
import { build } from "esbuild";
import { resolve } from "node:path";
import { bundleBrowser } from "../build/helpers.mjs";

const [scenario, requestedOutput] = process.argv.slice(2);
const output =
  requestedOutput ??
  resolve(
    process.env.IPP_BUILD_OUTPUT ?? "target/worker-profiling",
    `${scenario}.mjs`,
  );
const source = new Map([
  ["native-import", "tests/performance/native-import.ts"],
  ["mixed-import", "tests/performance/mixed-import.ts"],
  ["stress", "tests/performance/stress.ts"],
  ["robot", "tests/profiling/platformer-profile.test.ts"],
]).get(scenario);
if (source === undefined) throw new Error("Unknown performance scenario");
if (scenario === "stress")
  await bundleBrowser(
    resolve("tests/performance/pages/stress.ts"),
    resolve("target/stress-benchmark/fixture.js"),
  );
await build({
  entryPoints: [source],
  outfile: output,
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  packages: "external",
});
