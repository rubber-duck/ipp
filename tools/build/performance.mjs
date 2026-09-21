/** Bundle opt-in benchmark scenarios; Python owns builds and execution. */
import { build } from "esbuild";
import { resolve } from "node:path";
import { bundleBrowser } from "./helpers.mjs";

const [scenario, output] = process.argv.slice(2);
if (!["native-import", "mixed-import", "stress", "robot"].includes(scenario))
  throw new Error("Unknown performance scenario");
if (scenario === "stress")
  await bundleBrowser(
    resolve("tests/performance/stress-fixture.ts"),
    resolve("target/stress-benchmark/fixture.js"),
  );
await build({
  entryPoints: [`tests/performance/${scenario}.ts`],
  outfile: output,
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  packages: "external",
});
