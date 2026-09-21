/** Build the maintained render fixtures, viewer, or standalone release site. */
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { buildGallerySite } from "./build/gallery-site.mjs";

import {
  artifact,
  bundleBrowser,
  exportBuiltin,
  loadFixtureGenerators,
  workspace,
} from "./build/helpers.mjs";
const selection = process.argv[2];
if (!["application", "fixtures", "site"].includes(selection))
  throw new Error("Expected application, fixtures or site");
const output =
  process.env.IPP_BUILD_OUTPUT ??
  resolve(
    workspace,
    selection === "site" ? "target/gallery-site" : "target/gallery-build",
  );
await mkdir(output, { recursive: true });

const platformerAssets = resolve(workspace, "target/gallery-platformer-assets");
const platformerWorld = resolve(platformerAssets, "platformer.ipp");
const renderContract = resolve(
  workspace,
  process.env.IPP_BROWSER_BUILD_DIR ?? "target/browser-build",
  "render-expanded/contract.bin",
);

async function validateWorldContract(path, label, contract) {
  const world = await readFile(path);
  if (
    world.byteLength < 32 ||
    world.toString("ascii", 0, 4) !== "IPPW" ||
    ![2, 3].includes(world.readUInt32LE(4)) ||
    world.readBigUInt64LE(16) !== BigInt(world.byteLength)
  ) {
    throw new Error(`${label} is not a complete IPPW container`);
  }
  const worldHash = world.readBigUInt64LE(8);
  const targetHash = contract.readBigUInt64LE(8);
  if (worldHash !== targetHash) {
    const hash = (value) => `0x${value.toString(16).padStart(16, "0")}`;
    throw new Error(
      `${label} target contract mismatch (${hash(worldHash)} != ${hash(targetHash)}). Rebuild gallery-platformer-assets; do not patch serialized headers.`,
    );
  }
}

async function validatePlatformerAssets() {
  const [manifest, catalog, route] = await Promise.all(
    ["manifest.json", "catalog.json", "route.json"].map(async (name) =>
      JSON.parse(await readFile(resolve(platformerAssets, name), "utf8")),
    ),
  );
  const expectedClips = [
    "Platformer_Walk",
    "Platformer_Run",
    "Platformer_Crawl",
  ];
  const clipNames = new Set(manifest.clips?.map(({ name }) => name));
  if (!expectedClips.every((name) => clipNames.has(name)))
    throw new Error(
      "Platformer manifest does not expose the required locomotion clips",
    );
  if (
    route.version !== 1 ||
    route.coordinateSystem !== "ipp-runtime-y-up" ||
    route.closed !== true ||
    route.rootEntityId !== "platformer-root" ||
    !Array.isArray(route.waypoints) ||
    route.waypoints.length < 4
  )
    throw new Error("Platformer route must be a versioned traversable loop");
  for (const [mode, clip] of Object.entries({
    walk: "Platformer_Walk",
    run: "Platformer_Run",
    crawl: "Platformer_Crawl",
  })) {
    if (route.modes?.[mode]?.clip !== clip || !(route.modes[mode].speed > 0))
      throw new Error(
        `Platformer route is missing ${mode} locomotion metadata`,
      );
  }
  await Promise.all(
    Object.entries(catalog).map(async ([name, entry]) => {
      const payload = await readFile(resolve(platformerAssets, name));
      if (payload.byteLength !== entry.bytes)
        throw new Error(`Platformer catalog size mismatch for ${name}`);
    }),
  );
}

async function validateSavedWorldContracts() {
  const contract = await readFile(renderContract);
  if (
    contract.byteLength < 16 ||
    contract.toString("ascii", 0, 4) !== "IPPB" ||
    contract.readUInt32LE(4) !== 2
  ) {
    throw new Error(
      "Render-expanded target contract is not a valid IPPB export",
    );
  }
  await Promise.all([
    validateWorldContract(
      platformerWorld,
      "Generated platformer World",
      contract,
    ),
    validatePlatformerAssets(),
  ]);
}

if (selection === "site") {
  await validateSavedWorldContracts();
  await buildGallerySite(output);
} else if (selection === "fixtures") {
  await Promise.all([
    bundleBrowser(
      resolve(workspace, "tests/render/browser-fixture.ts"),
      resolve(output, "fixture.js"),
      "development",
    ),
    bundleBrowser(
      resolve(workspace, "tests/render/browser-fixture.ts"),
      resolve(output, "fixture-production.js"),
    ),
    bundleBrowser(
      resolve(workspace, "tests/render/viewer-browser-helper.ts"),
      resolve(output, "viewer-browser-helper.js"),
    ),
    ...["development", "production"].map((mode) =>
      bundleBrowser(
        resolve(workspace, "tests/render/ready-geometry-fixture.tsx"),
        resolve(output, `ready-geometry-${mode}.js`),
        mode,
      ),
    ),
  ]);
  await writeFile(
    resolve(output, "replacement.mesh"),
    await exportBuiltin("mesh", "ipp://mesh/cube?width=1&height=2&length=2"),
  );
  await writeFile(
    resolve(output, "replacement.texture"),
    await exportBuiltin(
      "texture",
      "ipp://texture/checkerboard?width=64&height=64&cellsX=8&cellsY=8",
    ),
  );
  const artifacts = await Promise.all(
    [
      "fixture.js",
      "fixture-production.js",
      "viewer-browser-helper.js",
      "ready-geometry-development.js",
      "ready-geometry-production.js",
      "replacement.mesh",
      "replacement.texture",
    ].map((name) => artifact(resolve(output, name))),
  );
  await writeFile(
    resolve(output, "fixture-report.json"),
    `${JSON.stringify({ artifacts }, null, 2)}\n`,
  );
} else {
  await validateSavedWorldContracts();
  await bundleBrowser(
    resolve(workspace, "examples/world-gallery/main.tsx"),
    resolve(output, "world-gallery.js"),
  );

  const meshModule = await loadFixtureGenerators(
    resolve(workspace, "examples/world-gallery/assets/cube-mesh.ts"),
  );
  const mesh = Buffer.from(meshModule.createCubeMesh());
  await writeFile(resolve(output, "cube.mesh"), mesh);
  // Markers use the Light transform directly: local -Z is the emission direction.
  // Transform authored streams once, retaining the Rust recipe's topology/UVs.
  for (const [name, recipe] of [
    ["spot", "ipp://mesh/cone?radius=0.3&height=0.7"],
    ["sun", "ipp://mesh/plane?size=2&normalLength=1&stroke=0.06"],
  ]) {
    const bytes = await exportBuiltin("mesh", recipe);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const streams = view.getUint32(16, true);
    let offset = 20 + streams * 8;
    for (let stream = 0; stream < streams; stream++) {
      const semantic = view.getUint8(20 + stream * 8);
      const length = view.getUint32(24 + stream * 8, true);
      if (semantic === 0 || semantic === 4) {
        for (let vertex = offset; vertex < offset + length; vertex += 12) {
          const x = view.getFloat32(vertex, true);
          const y = view.getFloat32(vertex + 4, true);
          const z = view.getFloat32(vertex + 8, true);
          const point =
            name === "spot"
              ? [x, -z, y - (semantic === 0 ? 0.35 : 0)]
              : [-x, y, -z];
          point.forEach((value, axis) =>
            view.setFloat32(vertex + axis * 4, value, true),
          );
        }
      }
      offset += length;
    }
    await writeFile(resolve(output, `${name}-marker.mesh`), bytes);
  }

  const artifacts = await Promise.all([
    ...[
      "world-gallery.js",
      "cube.mesh",
      "spot-marker.mesh",
      "sun-marker.mesh",
    ].map((name) => artifact(resolve(output, name))),
    artifact(platformerWorld),
    artifact(resolve(platformerAssets, "manifest.json")),
    artifact(resolve(platformerAssets, "catalog.json")),
    artifact(resolve(platformerAssets, "route.json")),
  ]);
  await writeFile(
    resolve(output, "application-report.json"),
    `${JSON.stringify(
      {
        scope:
          "React DOM controls and React scenes to generated client to WASM to Rust WebGL",
        mesh: { format: "IPPM", version: 1, vertices: 24, indices: 36 },
        artifacts,
      },
      null,
      2,
    )}\n`,
  );

  console.log(
    "Built the render fixture and the Geometry, Lighting/Picking/Animation Hierarchy/LookAt and Particles scenes.",
  );
}
