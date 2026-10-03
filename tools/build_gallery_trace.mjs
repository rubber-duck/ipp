/** Build fixed gallery semantics against the instrumented product in this checkout. */
import { build } from "esbuild";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve, relative } from "node:path";
import { bundleBrowser, workspace } from "./build/helpers.mjs";
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/performance/tsconfig.gallery-trace.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
const output = resolve(workspace, "target/gallery-trace");
await mkdir(output, { recursive: true });
const aliases = {};
for (const [name, directory] of [
  ["@ipp/react", "packages/ipp-react"],
  ["@ipp/client", "packages/ipp-client"],
]) {
  const metadata = JSON.parse(
    await readFile(resolve(workspace, directory, "package.json"), "utf8"),
  );
  for (const [entry, conditions] of Object.entries(metadata.exports)) {
    const path =
      typeof conditions === "string" ? conditions : conditions.default;
    if (path)
      aliases[`${name}${entry === "." ? "" : entry.slice(1)}`] = resolve(
        workspace,
        directory,
        path,
      );
  }
}
aliases["@ipp/host-contract"] = resolve(
  workspace,
  "target/browser-build/render-instrumentation/generated.js",
);
const productPlugin = {
  name: "instrumented-gallery-product",
  setup(builder) {
    builder.onLoad(
      { filter: /examples[/\\]world-gallery[/\\]shared[/\\]runtime\.ts$/ },
      async ({ path }) => ({
        contents: (await readFile(path, "utf8")).replace(
          "/target/browser-build/render/",
          "/target/browser-build/render-instrumentation/",
        ),
        loader: "ts",
      }),
    );
  },
};
const inputs = {};
for (const [source, file] of [
  ["examples/world-gallery/main.tsx", "application.js"],
  ["tests/render/viewer-browser-helper.ts", "viewer-browser-helper.js"],
]) {
  const result = await bundleBrowser(
    resolve(workspace, source),
    resolve(output, file),
    "production",
    { alias: aliases, plugins: [productPlugin], metafile: true },
  );
  inputs[file] = Object.keys(result.metafile.inputs).map((path) =>
    relative(workspace, resolve(workspace, path)),
  );
  for (const path of inputs[file])
    if (
      (path.includes(".worktrees/") || path.startsWith("../")) &&
      !path.includes("/node_modules/")
    )
      throw new Error(`Foreign worktree fixture input: ${path}`);
}
await writeFile(
  resolve(output, "fixture-inputs.json"),
  JSON.stringify(inputs, null, 2),
);
const html = (
  await readFile(
    resolve(workspace, "examples/world-gallery/index.html"),
    "utf8",
  )
).replace(
  "/target/gallery-build/world-gallery.js",
  "/target/gallery-trace/application.js",
);
await writeFile(resolve(output, "index.html"), html);
await build({
  entryPoints: [resolve(workspace, "tests/performance/gallery-gui-trace.ts")],
  outfile: resolve(output, "gallery-gui-trace.mjs"),
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node22",
  packages: "external",
  alias: aliases,
});
