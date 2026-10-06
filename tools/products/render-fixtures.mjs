import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { loadFixtureGenerators, workspace } from "../build/helpers.mjs";

const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/render-fixtures");
const { createCubeMesh } = await loadFixtureGenerators(
  resolve(workspace, "examples/world-gallery/assets/cube-mesh.ts"),
);

await mkdir(output, { recursive: true });
await writeFile(resolve(output, "cube.mesh"), Buffer.from(createCubeMesh()));
