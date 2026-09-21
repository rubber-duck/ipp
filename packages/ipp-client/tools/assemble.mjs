/** Maintained support modules copied beside every target-generated client. */
import { copyFileSync, mkdirSync } from "node:fs";
import { rm } from "node:fs/promises";
import { resolve } from "node:path";
import { build } from "esbuild";

export const CLIENT_SUPPORT_MODULES = [
  "client.ts",
  "command-pages.ts",
  "dynamic-properties.ts",
  "surface-types.ts",
  "gui-types.ts",
  "host-client.ts",
  "host-protocol.ts",
  "asset-sources.ts",
  "logging.ts",
  "presentation.ts",
  "resource-urls.ts",
  "transport.ts",
  "types.ts",
  "worker.ts",
  "world-persistence-client.ts",
];

const source = resolve(import.meta.dirname, "../src");

/** Assemble the target-independent half of a generated client package. */
export function assembleClientSupport(destination) {
  mkdirSync(destination, { recursive: true });
  for (const name of CLIENT_SUPPORT_MODULES) {
    copyFileSync(resolve(source, name), resolve(destination, name));
  }
}

/** Assemble a self-contained host next to a matching generated client and WASM. */
export async function assembleBrowserHost(
  destination,
  {
    rendering = false,
    shadows = false,
    skeletalAnimation = false,
    meshPoses = false,
    particles = false,
    surfaces = false,
  } = {},
) {
  if (shadows && !rendering)
    throw new Error("Shadows require a rendered browser host");
  // Reusing an output directory without rendering must remove its GPU bridge.
  if (!rendering)
    for (const name of ["render-worker.js", "webgl.js"])
      await rm(resolve(destination, name), { force: true });
  const surfaceNotice = resolve(destination, "SLUG-NOTICE");
  if (rendering && surfaces)
    copyFileSync(
      resolve(source, "../../../crates/ipp-render-gl/SLUG-NOTICE"),
      surfaceNotice,
    );
  else await rm(surfaceNotice, { force: true });
  const modules = [
    "wasm-worker.ts",
    "logging.ts",
    "resource-worker.ts",
    "source-availability.ts",
    "resource-urls.ts",
    ...(rendering ? ["render-worker.ts", "presentation.ts"] : []),
  ];
  await build({
    entryPoints: modules.map((name) => resolve(source, name)),
    outdir: destination,
    bundle: false,
    format: "esm",
    platform: "browser",
    target: "es2023",
    legalComments: "none",
  });
  if (rendering)
    await build({
      entryPoints: [
        resolve(
          source,
          "../../../crates/ipp-render-gl/src/services/render/webgl.ts",
        ),
      ],
      outfile: resolve(destination, "webgl.js"),
      bundle: true,
      format: "esm",
      platform: "browser",
      target: "es2023",
      define: {
        IPP_SHADOWS: String(shadows),
        IPP_SKELETAL_ANIMATION: String(skeletalAnimation),
        IPP_MESH_POSES: String(meshPoses),
        IPP_PARTICLES: String(particles),
        IPP_SURFACES: String(surfaces),
      },
      minifySyntax: true,
    });
  return [
    ...modules.map((name) =>
      resolve(destination, name.replace(/\.ts$/, ".js")),
    ),
    ...(rendering ? [resolve(destination, "webgl.js")] : []),
    ...(rendering && surfaces ? [surfaceNotice] : []),
  ];
}
