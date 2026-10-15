/**
 * Build the shared-host command: cli.ts bundled with React and the IPP
 * client packages from source, plus the input identities the launcher uses
 * to rebuild it when the sources change.
 */
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
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
  // Linked worktrees can share node_modules; resolve the authoring source here.
  alias: {
    "@ipp/react": resolve(workspace, "packages/ipp-react/src/index.ts"),
    "@ipp/react/canvas": resolve(workspace, "packages/ipp-react/src/canvas.ts"),
    "@ipp/react/gui": resolve(workspace, "packages/ipp-react/src/gui.ts"),
    "@ipp/react/gui-kit": resolve(
      workspace,
      "packages/ipp-react/src/gui-kit.ts",
    ),
    "@ipp/react/gui-cybertron": resolve(
      workspace,
      "packages/ipp-react/src/gui-cybertron.ts",
    ),
    "@ipp/react/gui-glass": resolve(
      workspace,
      "packages/ipp-react/src/gui-glass.ts",
    ),
    "@ipp/client": resolve(workspace, "packages/ipp-client/src/index.ts"),
  },
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
for (const path of new Set([
  ...Object.keys(result.metafile.inputs),
  "tools/shared-host/build.mjs",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  ".node-version",
])) {
  const bytes = await readFile(resolve(workspace, path));
  inputs.push([path, createHash("sha256").update(bytes).digest("hex")]);
}
await writeFile(resolve(output, "inputs.json"), JSON.stringify(inputs));
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Shared development Host command", artifacts: [await artifact(bundle)] }, null, 2)}\n`,
);
