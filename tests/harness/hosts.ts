/**
 * Dual-host selection for presenting scenarios: the same browser page runs
 * against a worker WASM/WebGL build or connects to a native GLES Host.
 */
import { resolve } from "node:path";
import type { BrowserBuildConfiguration } from "./browser.js";
import { runNativeEnvironment } from "./native.js";

/** The native Host a page connects to instead of starting its own worker. */
export interface HostEndpoint {
  readonly url: string;
  readonly presentationUrl: string;
}

/**
 * The presenting build in `directory`, which the caller names: a native GLES
 * Host build or a worker WASM/WebGL render build, plain or instrumented. The
 * name defaults to `gles`, `render` or `render-instrumentation`.
 */
export function presentingBuild(
  native: boolean,
  options: {
    readonly directory: string;
    readonly instrumentation?: boolean;
    readonly name?: BrowserBuildConfiguration["name"];
  },
): BrowserBuildConfiguration {
  const instrumentation = options.instrumentation ?? false;
  const directory = resolve(options.directory);
  return {
    name:
      options.name ??
      (native ? "gles" : instrumentation ? "render-instrumentation" : "render"),
    generatedModule: resolve(directory, "generated.js"),
    runtimeWasm: resolve(directory, native ? "gles_host" : "runtime.wasm"),
    contractArtifact: resolve(directory, "contract.bin"),
  };
}

/**
 * Run `browser` directly for a worker build, or inside a native environment
 * named `name` that launches `build`'s GLES Host and passes its endpoints.
 */
export async function withPresentingHost<T>(
  name: string,
  build: BrowserBuildConfiguration,
  signal: AbortSignal,
  options: {
    readonly native: boolean;
    /** When absent, `IPP_EGL_LIBRARY_DIR`, then `/lib64`. */
    readonly eglDirectory?: string | undefined;
    readonly readinessTimeoutMs?: number;
    readonly operationTimeoutMs: number;
    /** Failure text when the Host's readiness names no presentation channel. */
    readonly missingPresentation?: string;
  },
  browser: (endpoint?: HostEndpoint) => Promise<T>,
): Promise<T> {
  if (!options.native) return browser();
  const { value } = await runNativeEnvironment(
    name,
    {
      executable: build.runtimeWasm,
      schemaArtifact: build.contractArtifact,
      workingDirectory: process.cwd(),
      extraArguments: [
        "--egl-dir",
        options.eglDirectory ?? process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
      ],
      readinessTimeoutMs: options.readinessTimeoutMs ?? 30_000,
      operationTimeoutMs: options.operationTimeoutMs,
    },
    signal,
    async (environment) => {
      if (!environment.presentationUrl)
        throw new Error(
          options.missingPresentation ?? "GLES presentation endpoint absent",
        );
      return browser({
        url: environment.url,
        presentationUrl: environment.presentationUrl,
      });
    },
  );
  return value;
}
