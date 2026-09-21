import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { loadFixtureGenerators, workspace } from "./build/helpers.mjs";

const { poseMesh } = await loadFixtureGenerators(
  resolve(workspace, "tests/render/mesh-pose-assets.ts"),
);

const directory =
  process.env.IPP_BUILD_OUTPUT ?? resolve("target/mesh-pose-build");
await mkdir(directory, { recursive: true });
for (const [name, weight] of [
  ["base", 0],
  ["half", 0.5],
  ["target", 1],
]) {
  await writeFile(resolve(directory, `${name}.mesh`), poseMesh(weight));
}
await writeFile(
  resolve(directory, "affine-half.mesh"),
  poseMesh(0.5, { affine: true }),
);

await writeFile(
  resolve(directory, "aimed-affine-half.mesh"),
  poseMesh(0.5, { affine: true, aim: true }),
);
