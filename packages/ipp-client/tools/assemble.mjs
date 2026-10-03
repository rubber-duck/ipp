/** Maintained support modules copied beside every target-generated client. */
import { copyFileSync, mkdirSync } from "node:fs";
import { rm } from "node:fs/promises";
import { resolve } from "node:path";
import { build } from "esbuild";

export const CLIENT_SUPPORT_MODULES = [
  "profiling.ts",
  "client.ts",
  "command-pages.ts",
  "dynamic-properties.ts",
  "gui-types.ts",
  "gui-observations.ts",
  "lifecycle-types.ts",
  "lifecycle-watches.ts",
  "lifecycle-diagnostics.ts",
  "host-client.ts",
  "host-contract.ts",
  "host-protocol.ts",
  "host-presentation.ts",
  "host-input.ts",
  "asset-sources.ts",
  "datasets.ts",
  "logging.ts",
  "presentation.ts",
  "references.ts",
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

/**
 * Assemble a self-contained host next to a matching generated client and WASM.
 * `rendering` and `instrumentation` follow the runtime's build configuration:
 * only an instrumentation distribution ships the profiler and the worker side
 * of the testing controls, and its worker loads them because the bundler
 * defines `IPP_INSTRUMENTATION`, never by probing the runtime's exports.
 */
export async function assembleBrowserHost(
  destination,
  { rendering = false, instrumentation = false } = {},
) {
  const modules = [
    "wasm-worker.ts",
    "worker-connections.ts",
    "logging.ts",
    "resource-worker.ts",
    "source-availability.ts",
    "resource-urls.ts",
    ...(rendering ? ["render-worker.ts", "presentation.ts"] : []),
    ...(instrumentation ? ["profile-worker.ts"] : []),
    ...(rendering && instrumentation ? ["render-testing.ts"] : []),
  ];
  // Reusing an output directory must remove worker modules this configuration
  // omits; the client support modules beside it stay.
  for (const name of [
    "render-worker.ts",
    "profile-worker.ts",
    "render-testing.ts",
  ])
    if (!modules.includes(name))
      await rm(resolve(destination, name.replace(/\.ts$/, ".js")), {
        force: true,
      });
  if (!rendering) await rm(resolve(destination, "webgl.js"), { force: true });
  const surfaceNotice = resolve(destination, "SLUG-NOTICE");
  if (rendering)
    copyFileSync(
      resolve(source, "../../../crates/ipp-render-gl/SLUG-NOTICE"),
      surfaceNotice,
    );
  else await rm(surfaceNotice, { force: true });
  await build({
    entryPoints: modules.map((name) => resolve(source, name)),
    outdir: destination,
    bundle: false,
    format: "esm",
    platform: "browser",
    target: "es2023",
    legalComments: "none",
    define: { IPP_INSTRUMENTATION: String(instrumentation) },
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
      minifySyntax: true,
      define: { IPP_INSTRUMENTATION: String(instrumentation) },
    });
  return [
    ...modules.map((name) =>
      resolve(destination, name.replace(/\.ts$/, ".js")),
    ),
    ...(rendering ? [resolve(destination, "webgl.js"), surfaceNotice] : []),
  ];
}
