/** Assemble public React entries plus the browser-only native text harness. */
import { mkdir, rm, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { build } from "esbuild";
import { artifact, workspace } from "../../../tools/build/helpers.mjs";

const directory = resolve(import.meta.dirname, "..");
const output = process.env.IPP_BUILD_OUTPUT ?? resolve(directory, "dist");
// The package owns this generated directory; stale content-addressed chunks
// must not accumulate in the distribution after source changes.
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
const result = await build({
  absWorkingDir: workspace,
  entryPoints: {
    index: resolve(directory, "src/index.ts"),
    web: resolve(directory, "src/web.tsx"),
    gui: resolve(directory, "src/gui.ts"),
  },
  outdir: output,
  chunkNames: "shared-[hash]",
  bundle: true,
  splitting: true,
  format: "esm",
  platform: "browser",
  target: "es2023",
  minify: true,
  metafile: true,
  legalComments: "none",
  external: ["react", "react/*", "@ipp/client"],
  plugins: [
    {
      name: "react-peer-esm",
      setup(builder) {
        builder.onResolve({ filter: /^react$/ }, (args) =>
          args.kind === "require-call"
            ? { path: "react", namespace: "react-peer" }
            : undefined,
        );
        builder.onLoad({ filter: /^react$/, namespace: "react-peer" }, () => ({
          contents: 'import * as React from "react"; module.exports = React;',
          loader: "js",
        }));
      },
    },
  ],
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
});
await build({
  absWorkingDir: workspace,
  entryPoints: [resolve(directory, "src/gui/text-bridge.ts")],
  outfile: resolve(output, "native-text-bridge.js"),
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2023",
  minify: true,
  legalComments: "none",
});
if (
  Object.keys(result.metafile.inputs).some((path) =>
    path.includes("node_modules/react-dom/"),
  )
)
  throw new Error("React DOM must remain an application dependency");
const artifacts = await Promise.all(
  [
    ...Object.keys(result.metafile.outputs).map((path) =>
      resolve(workspace, path),
    ),
    resolve(output, "native-text-bridge.js"),
  ].map((path) => artifact(path)),
);
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Public React entries with shared reconciler and standalone native text bridge harness; React and IPP client external from public entries", artifacts }, null, 2)}\n`,
);
