/** Build the maintained Blender viewer with its matching complete runtime profile. */
import { cp, mkdir, readdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "./build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/blender-viewer");
await mkdir(output, { recursive: true });
await bundleBrowser(
  resolve(workspace, "examples/blender-viewer/main.tsx"),
  resolve(output, "viewer.js"),
);
const profile = resolve(workspace, "target/browser-build/render-expanded");
await mkdir(resolve(output, "runtime"), { recursive: true });
for (const name of await readdir(profile))
  if (name.endsWith(".js") || name === "runtime.wasm")
    await cp(resolve(profile, name), resolve(output, "runtime", name));
for (const name of ["index.html", "styles.css"])
  await cp(
    resolve(workspace, "examples/blender-viewer", name),
    resolve(output, name),
  );
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify(
    {
      profile: "render-expanded",
      artifacts: await Promise.all(
        ["viewer.js", "runtime/generated.js", "runtime/runtime.wasm"].map(
          (name) => artifact(resolve(output, name)),
        ),
      ),
    },
    null,
    2,
  )}\n`,
);
