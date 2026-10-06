/** Bundle the independent native benchmark driver against the public client. */
import { build } from "esbuild";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { writeFile, readFile } from "node:fs/promises";

const output = resolve(process.argv[2]);
const configuration = JSON.parse(
  await readFile("tests/performance/tsconfig.chart-data.json", "utf8"),
);
configuration.extends = resolve("tsconfig.json");
// Resolve the shared type-only Host seam against this build's native contract.
configuration.compilerOptions.paths = {
  "@ipp/client": [resolve("packages/ipp-client/src/index.ts")],
  "@ipp/host-contract": [resolve(output, "generated.ts")],
};
configuration.include = [
  resolve("tests/performance/chart-data.ts"),
  resolve("tests/data/scenarios/chart-data-workload.ts"),
];
await writeFile(
  resolve(output, "tsconfig.json"),
  JSON.stringify(configuration),
);
execFileSync(
  process.execPath,
  ["node_modules/typescript/bin/tsc", "-p", resolve(output, "tsconfig.json")],
  {
    stdio: "inherit",
  },
);
await build({
  entryPoints: ["tests/performance/chart-data.ts"],
  outfile: resolve(output, "chart-data.js"),
  bundle: true,
  format: "esm",
  platform: "node",
  target: "node22",
  packages: "external",
});
