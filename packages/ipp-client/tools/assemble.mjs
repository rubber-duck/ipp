/** Maintained support modules copied beside every target-generated client. */
import { copyFileSync, mkdirSync, readdirSync } from "node:fs";
import { rm } from "node:fs/promises";
import { resolve } from "node:path";
import { build } from "esbuild";

export const CLIENT_SUPPORT_MODULES = [
  "profiling.ts",
  "client.ts",
  "command-pages.ts",
  "dynamic-properties.ts",
  "gui-types.ts",
  "surface-config.ts",
  "gui-observations.ts",
  "lifecycle-types.ts",
  "lifecycle-watches.ts",
  "lifecycle-diagnostics.ts",
  "host-client.ts",
  "host-contract.ts",
  "bulk-reads.ts",
  "asset-exports.ts",
  "host-protocol.ts",
  "host-presentation.ts",
  "host-input.ts",
  "asset-sources.ts",
  "buffer-source.ts",
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

/**
 * Modules built only into the browser host. The host build also compiles the
 * `logging`, `presentation` and `resource-urls` support modules.
 */
const BROWSER_HOST_ONLY_MODULES = [
  "wasm-worker.ts",
  "worker-connections.ts",
  "resource-worker.ts",
  "source-availability.ts",
  "render-worker.ts",
  "profile-worker.ts",
  "render-testing.ts",
];

/**
 * Modules reached only through the package's own entry points; neither copied
 * beside a generated client nor built into the browser host.
 */
const PACKAGE_ONLY_MODULES = [
  "browser.ts",
  "diagnostics.ts",
  "index.ts",
  "native-presentation.ts",
  "surface-mapping.ts",
  "testing.ts",
];

const source = resolve(import.meta.dirname, "../src");

/**
 * Refuse to assemble unless every `src` module is in exactly one set, so adding,
 * renaming or removing a module decides whether generated clients and the
 * browser host ship it.
 */
function checkModuleSets() {
  const sets = {
    CLIENT_SUPPORT_MODULES,
    BROWSER_HOST_ONLY_MODULES,
    PACKAGE_ONLY_MODULES,
  };

  const owners = new Map(
    readdirSync(source)
      .filter((name) => name.endsWith(".ts"))
      .map((name) => [name, []]),
  );

  const problems = [];
  for (const [set, names] of Object.entries(sets))
    for (const name of names)
      if (owners.has(name)) owners.get(name).push(set);
      else problems.push(`${set} lists ${name}, which is not in src/`);

  for (const [name, listedIn] of owners)
    if (listedIn.length === 0) problems.push(`src/${name} is in no module set`);
    else if (listedIn.length > 1)
      problems.push(`src/${name} is in ${listedIn.join(" and ")}`);

  if (problems.length > 0)
    throw new Error(
      `packages/ipp-client/tools/assemble.mjs: every src module must be in exactly one of ${Object.keys(sets).join(", ")}:\n  ${problems.join("\n  ")}`,
    );
}

checkModuleSets();

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
          "../../../crates/ipp-render-gl/src/services/render/device/webgl/webgl.ts",
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
