/** Build the maintained browser texture fixture and shared encoded asset corpus. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import {
  artifact,
  bundleBrowser,
  exportBuiltin,
  loadFixtureGenerators,
  workspace,
} from "./build/helpers.mjs";
const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/texture-build");
const fixture = resolve(workspace, "tests/render/texture-fixture.tsx");
await mkdir(output, { recursive: true });

const [cube, checker, fixtureModule] = await Promise.all([
  exportBuiltin("mesh", "ipp://mesh/cube?width=2&height=2&length=2"),
  exportBuiltin(
    "texture",
    "ipp://texture/checkerboard?width=3072&height=2048&cellsX=8&cellsY=8",
  ),
  loadFixtureGenerators(fixture),
  bundleBrowser(fixture, resolve(output, "fixture.js"), "development"),
  bundleBrowser(
    fixture,
    resolve(output, "fixture-production.js"),
    "production",
  ),
]);
const quad = Buffer.from(fixtureModule.createSamplerQuadMesh());
const asymmetric = Buffer.from(fixtureModule.createAsymmetricTexture());
const legacyV1 = Buffer.from(fixtureModule.createLegacyV1Texture());
const optionalLayouts = Object.fromEntries(
  ["position", "color", "uv", "weight"].map((layout) => [
    layout,
    Buffer.from(fixtureModule.createOptionalLayoutMesh(layout)),
  ]),
);
requireAsset(cube, "IPPM", 3, 1180, "built-in cube");
requireAsset(checker, "IPPT", 3, 25_165_840, "built-in checker");
requireAsset(quad, "IPPM", 2, 156, "sampler quad");
requireAsset(asymmetric, "IPPT", 3, 40, "asymmetric texture");
requireAsset(legacyV1, "IPPT", 1, 32, "legacy texture rejection fixture");
for (const [layout, bytes] of Object.entries(optionalLayouts)) {
  requireMesh(bytes, `optional ${layout} mesh`);
}

await Promise.all([
  writeFile(resolve(output, "cube.mesh"), cube),
  writeFile(resolve(output, "checker.texture"), checker),
  writeFile(resolve(output, "sampler-quad.mesh"), quad),
  writeFile(resolve(output, "asymmetric.texture"), asymmetric),
  writeFile(resolve(output, "legacy-v1.texture"), legacyV1),
  ...Object.entries(optionalLayouts).map(([layout, bytes]) =>
    writeFile(resolve(output, `optional-${layout}.mesh`), bytes),
  ),
]);

const names = [
  "fixture.js",
  "fixture-production.js",
  "cube.mesh",
  "checker.texture",
  "sampler-quad.mesh",
  "asymmetric.texture",
  "legacy-v1.texture",
  ...Object.keys(optionalLayouts).map((layout) => `optional-${layout}.mesh`),
];
const artifacts = await Promise.all(
  names.map((name) => artifact(resolve(output, name))),
);
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify(
    {
      scope: "React to generated client to WASM to Rust WebGL texture pipeline",
      builtins: {
        mesh: "ipp://mesh/cube?width=2&height=2&length=2",
        texture:
          "ipp://texture/checkerboard?width=3072&height=2048&cellsX=8&cellsY=8",
      },
      sampler: {
        mesh: "IPPMv2 camera-facing quad with UV range 0..2",
        texture: "IPPTv3 3x2 unique nonbinary packed RGBA8 texels",
      },
      optionalLayouts: Object.fromEntries(
        Object.entries(optionalLayouts).map(([layout, bytes]) => [
          layout,
          { format: "IPPM", bytes: bytes.byteLength },
        ]),
      ),
      rejectedLegacy: {
        format: "complete IPPTv1 RGBA8",
        bytes: legacyV1.byteLength,
      },
      artifacts,
    },
    null,
    2,
  )}\n`,
);

console.log("Built the browser texture fixture and shared encoded assets.");

function requireAsset(bytes, magic, version, length, label) {
  if (
    bytes.byteLength !== length ||
    bytes.subarray(0, 4).toString("ascii") !== magic ||
    bytes.readUInt32LE(4) !== version
  ) {
    throw new Error(`${label} exporter returned an unexpected payload`);
  }
}

function requireMesh(bytes, label) {
  if (
    bytes.byteLength < 16 ||
    bytes.subarray(0, 4).toString("ascii") !== "IPPM" ||
    ![1, 2, 3].includes(bytes.readUInt32LE(4)) ||
    bytes.readUInt32LE(8) === 0 ||
    bytes.readUInt32LE(12) === 0
  ) {
    throw new Error(`${label} generator returned an invalid IPPM payload`);
  }
}
