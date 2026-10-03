/**
 * Client modules bundled from source on every load, so an edit applies
 * without a pipeline build. React and the IPP packages are not bundled into
 * them: they resolve to this command's own instances, so a reloaded module
 * renders into existing React roots. `@ipp/host-contract` resolves to the
 * connected Host's generated client module, so a module can read the
 * runtime's exported values, such as the design language's tokens, at
 * import.
 */
import { createHash } from "node:crypto";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { build, type Plugin } from "esbuild";
import * as React from "react";
import * as jsxRuntime from "react/jsx-runtime";
import * as IppClient from "@ipp/client";
import * as IppReact from "@ipp/react";
import * as IppReactCanvas from "@ipp/react/canvas";
import * as IppReactGui from "@ipp/react/gui";
import * as IppReactGuiKit from "@ipp/react/gui-kit";
import type { LoadedModule } from "./client.js";

const GLOBAL = "__ippSharedHostModules";
const modules: Record<string, unknown> = {
  react: React,
  "react/jsx-runtime": jsxRuntime,
  "@ipp/client": IppClient,
  "@ipp/react": IppReact,
  "@ipp/react/canvas": IppReactCanvas,
  "@ipp/react/gui": IppReactGui,
  "@ipp/react/gui-kit": IppReactGuiKit,
};
(globalThis as Record<string, unknown>)[GLOBAL] = modules;

/** Resolve `@ipp/host-contract` to the Host's generated client module. */
export function shareHostContract(contract: object): void {
  modules["@ipp/host-contract"] = contract;
}

const shared: Plugin = {
  name: "shared-host-modules",
  setup(build) {
    build.onResolve(
      {
        filter:
          /^(react|react\/jsx-runtime|@ipp\/client|@ipp\/react(\/(canvas|gui(-kit)?))?|@ipp\/host-contract)$/,
      },
      (args) => ({ path: args.path, namespace: "shared-host-modules" }),
    );
    build.onLoad(
      { filter: /.*/, namespace: "shared-host-modules" },
      (args) => ({
        contents: `module.exports = globalThis.${GLOBAL}[${JSON.stringify(args.path)}];`,
        loader: "js",
      }),
    );
  },
};

let sequence = 0;

/** Bundle and import `path`, or return null while its bundle is unchanged. */
export async function loadModule(
  workspace: string,
  path: string,
  previous?: string,
): Promise<LoadedModule | null> {
  const result = await build({
    absWorkingDir: workspace,
    entryPoints: [resolve(workspace, path)],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "node22",
    jsx: "automatic",
    loader: { ".glsl": "text" },
    sourcemap: "inline",
    logLevel: "silent",
    plugins: [shared],
  });
  const code = result.outputFiles[0]!.text;
  const identity = createHash("sha256")
    .update(code.replace(/\/\/# sourceMappingURL=.*$/m, ""))
    .digest("hex");
  if (identity === previous) return null;
  const directory = join(workspace, "target/shared-host-modules");
  await mkdir(directory, { recursive: true });
  const file = join(directory, `${process.pid}-${++sequence}.mjs`);
  await writeFile(file, code);
  try {
    return {
      module: (await import(pathToFileURL(file).href)) as Record<
        string,
        unknown
      >,
      identity,
    };
  } finally {
    await rm(file, { force: true });
  }
}
