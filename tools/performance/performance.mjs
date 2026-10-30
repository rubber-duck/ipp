/** Bundle opt-in benchmark scenarios; Python owns builds and execution. */
import { build } from "esbuild";
import { resolve } from "node:path";
import { readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { bundleBrowser } from "../build/helpers.mjs";

const [scenario, requestedOutput, contractDirectory] = process.argv.slice(2);
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
  ["cockpit-surfaces", "tests/performance/cockpit-surfaces.ts"],
]).get(scenario);
if (source === undefined) throw new Error("Unknown performance scenario");
if (scenario === "cockpit-surfaces") {
  const root = JSON.parse(await readFile("tsconfig.json", "utf8"));
  const paths = Object.fromEntries(
    Object.entries(root.compilerOptions.paths).map(([key, values]) => [
      key,
      values.map((value) => resolve(value)),
    ]),
  );
  paths["@ipp/host-contract"] = [resolve(contractDirectory, "generated.ts")];
  const configuration = `${output}.tsconfig.json`;
  await writeFile(
    configuration,
    JSON.stringify({
      extends: resolve("tsconfig.json"),
      compilerOptions: { noEmit: true, paths },
      include: [
        resolve(source),
        resolve("tests/performance/scenarios/cockpit-surfaces.ts"),
        resolve("examples/glsl.d.ts"),
      ],
    }),
  );
  execFileSync(
    process.execPath,
    ["node_modules/typescript/bin/tsc", "-p", configuration],
    { stdio: "inherit" },
  );
}
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
