/**
 * Build the shared-host command: cli.ts bundled with React and the IPP
 * client packages from source, plus the input identities the launcher uses
 * to rebuild it when the sources change.
 */
import { mkdir, stat, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { build } from "esbuild";
import { artifact, workspace } from "../build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/shared-host");
await mkdir(output, { recursive: true });
const bundle = resolve(output, "cli.mjs");
const result = await build({
  absWorkingDir: workspace,
  entryPoints: ["tools/shared-host/cli.ts"],
  outfile: bundle,
  bundle: true,
  metafile: true,
  format: "esm",
  platform: "node",
  target: "node22",
  jsx: "automatic",
  // `development` selects the React package's sources.
  conditions: ["development"],
  sourcemap: "inline",
  // esbuild locates its native binary at run time.
  external: ["esbuild"],
  define: { "process.env.NODE_ENV": JSON.stringify("development") },
  banner: {
    js: "import { createRequire as __sharedHostRequire } from 'node:module'; const require = __sharedHostRequire(import.meta.url);",
  },
  logLevel: "warning",
});
const inputs = [];
for (const path of Object.keys(result.metafile.inputs)) {
  const entry = await stat(resolve(workspace, path));
  inputs.push([path, entry.mtimeMs, entry.size]);
}
await writeFile(resolve(output, "inputs.json"), JSON.stringify(inputs));
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Shared development Host command", artifacts: [await artifact(bundle)] }, null, 2)}\n`,
);
