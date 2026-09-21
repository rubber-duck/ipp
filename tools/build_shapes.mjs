/** Build the real browser shape fixture and native-compatible built-in corpus. */
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import {
  artifact,
  bundleBrowser,
  exportBuiltin,
  workspace,
} from "./build/helpers.mjs";
import { normalFixtures } from "./build/normal-fixtures.mjs";
const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/shapes-build");
const fixture = resolve(workspace, "tests/render/shapes-fixture.tsx");
const recipes = Object.freeze({
  arrow: "ipp://mesh/arrow?length=1.25&stroke=0.05",
  axis: "ipp://mesh/axis?length=1.25&stroke=0.05",
  cube: "ipp://mesh/cube?width=2&height=2&length=2",
  plane: "ipp://mesh/plane?size=2&normalLength=1.25&stroke=0.05",
  "plane-outline":
    "ipp://mesh/plane-outline?size=2&normalLength=1.25&stroke=0.05",
  sphere: "ipp://mesh/sphere?radius=1",
  pill: "ipp://mesh/pill?radius=0.65&height=2.8",
  cone: "ipp://mesh/cone?radius=1&height=2",
  "cone-outline":
    "ipp://mesh/cone-outline?radius=1&height=2&stroke=0.045&rings=1",
  "cone-outline-rings":
    "ipp://mesh/cone-outline?radius=1&height=2&stroke=0.045&rings=3",
  "cube-outline":
    "ipp://mesh/cube-outline?width=2&height=2&length=2&stroke=0.045",
  "sphere-outline": "ipp://mesh/sphere-outline?radius=1&stroke=0.045",
  "pill-outline": "ipp://mesh/pill-outline?radius=0.65&height=2.8&stroke=0.045",
});
const checkerRecipe =
  "ipp://texture/checkerboard?width=64&height=64&cellsX=8&cellsY=8";

await mkdir(output, { recursive: true });
const meshEntries = await Promise.all(
  Object.entries(recipes).map(async ([name, uri]) => [
    name,
    uri,
    await exportBuiltin("mesh", uri),
  ]),
);
const normalEntries = Object.entries(
  normalFixtures(meshEntries.find(([name]) => name === "sphere")[2]),
);
const checker = await exportBuiltin("texture", checkerRecipe);

await Promise.all([
  bundleBrowser(fixture, resolve(output, "fixture.js"), "development"),
  bundleBrowser(
    fixture,
    resolve(output, "fixture-production.js"),
    "production",
  ),
  ...meshEntries.map(([name, , bytes]) => {
    requireMesh(bytes, name);
    return writeFile(resolve(output, `${name}.mesh`), bytes);
  }),
  ...normalEntries.map(([name, bytes]) =>
    writeFile(resolve(output, `${name}.mesh`), bytes),
  ),
  writeCheckedTexture(checker),
]);

const names = [
  "fixture.js",
  "fixture-production.js",
  ...Object.keys(recipes).map((name) => `${name}.mesh`),
  ...normalEntries.map(([name]) => `${name}.mesh`),
  "checker.texture",
];
const artifacts = await Promise.all(
  names.map((name) => artifact(resolve(output, name))),
);
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify(
    {
      scope:
        "React to generated client to WASM to Rust WebGL built-in shape pipeline",
      meshes: Object.fromEntries(
        meshEntries.map(([name, uri, bytes]) => [
          name,
          { uri, ...meshHeader(bytes) },
        ]),
      ),
      texture: {
        uri: checkerRecipe,
        width: 64,
        height: 64,
        cellsX: 8,
        cellsY: 8,
      },
      artifacts,
    },
    null,
    2,
  )}\n`,
);

console.log(
  "Built the browser shape fixture and native-compatible shape corpus.",
);

function meshHeader(bytes) {
  return {
    format: "IPPM",
    version: bytes.readUInt32LE(4),
    vertices: bytes.readUInt32LE(8),
    indices: bytes.readUInt32LE(12),
  };
}

function requireMesh(bytes, label) {
  if (
    bytes.byteLength < 16 ||
    bytes.subarray(0, 4).toString("ascii") !== "IPPM"
  ) {
    throw new Error(`${label} exporter did not return IPPM`);
  }
  const { version, vertices, indices } = meshHeader(bytes);
  if (
    vertices === 0 ||
    vertices > 65_536 ||
    indices === 0 ||
    indices % 3 !== 0
  ) {
    throw new Error(`${label} exporter returned invalid mesh counts`);
  }
  let expectedBytes;
  if (version === 2) {
    expectedBytes = 16 + vertices * 32 + indices * 2;
  } else if (version === 3 && bytes.byteLength >= 20) {
    const attributes = bytes.readUInt32LE(16);
    if (
      attributes < 1 ||
      attributes > 5 ||
      bytes.byteLength < 20 + attributes * 8
    ) {
      throw new Error(`${label} exporter returned invalid attribute count`);
    }
    expectedBytes = 20 + attributes * 8 + indices * 2;
    let previous = -1;
    for (let i = 0; i < attributes; i += 1) {
      const offset = 20 + i * 8;
      const semantic = bytes[offset];
      const format = bytes[offset + 1];
      const width = [12, 12, 8, 1, 12][semantic];
      if (
        semantic <= previous ||
        (i === 0 && semantic !== 0) ||
        format !== [1, 1, 2, 3, 1][semantic] ||
        bytes.readUInt16LE(offset + 2) !== 0 ||
        bytes.readUInt32LE(offset + 4) !== vertices * width
      ) {
        throw new Error(`${label} exporter returned invalid attribute layout`);
      }
      previous = semantic;
      expectedBytes += vertices * width;
    }
  } else {
    throw new Error(`${label} exporter returned unsupported IPPM version`);
  }
  if (bytes.byteLength !== expectedBytes) {
    throw new Error(`${label} exporter returned incorrect mesh byte count`);
  }
}

async function writeCheckedTexture(bytes) {
  if (
    bytes.byteLength !== 16_400 ||
    bytes.subarray(0, 4).toString("ascii") !== "IPPT" ||
    bytes.readUInt32LE(4) !== 3 ||
    bytes.readUInt32LE(8) !== 64 ||
    bytes.readUInt32LE(12) !== 64
  ) {
    throw new Error(
      "checker exporter returned an invalid 64x64 IPPTv3 RGBA8 payload",
    );
  }
  await writeFile(resolve(output, "checker.texture"), bytes);
}
