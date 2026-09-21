/** Build real canvas consumers through the public React package exports. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  artifact,
  bundleBrowser,
  packageVersion,
  workspace,
} from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/canvas-build");
const fixture = resolve(workspace, "tests/render/canvas-fixture.tsx");
await mkdir(output, { recursive: true });
const artifacts = await Promise.all(
  ["development", "production"].map(async (environment) => {
    const path = resolve(
      output,
      environment === "development" ? "fixture.js" : "fixture-production.js",
    );
    const result = await bundleBrowser(fixture, path, environment, {
      metafile: true,
    });
    if (
      environment === "production" &&
      Object.keys(result.metafile.inputs).some((path) =>
        path.includes("packages/ipp-react/src/"),
      )
    )
      throw new Error("Production canvas bypassed public built React exports");
    return { environment, ...(await artifact(path)) };
  }),
);
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "Public IppCanvas and World through generated client, worker, WASM, and WebGL", dependencies: { react: await packageVersion("react"), reactDom: await packageVersion("react-dom"), reactReconciler: await packageVersion("react-reconciler") }, artifacts }, null, 2)}\n`,
);
console.log("Built development and production declarative canvas fixtures.");
