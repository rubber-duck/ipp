/** Small shared operations for maintained artifact and fixture builds. */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { relative, resolve } from "node:path";
import { gzipSync } from "node:zlib";
import { build } from "esbuild";

export const workspace = resolve(import.meta.dirname, "../..");

export function bundleBrowser(
  entryPoint,
  outfile,
  environment = "production",
  options = {},
) {
  const minify = environment === "production";
  return build({
    absWorkingDir: workspace,
    entryPoints: [entryPoint],
    outfile,
    bundle: true,
    format: "esm",
    platform: "browser",
    target: "es2023",
    sourcemap: minify ? false : "linked",
    minify,
    legalComments: "none",
    conditions: ["browser", ...(minify ? [] : ["development"])],
    define: { "process.env.NODE_ENV": JSON.stringify(environment) },
    ...options,
    loader: { ".glsl": "text", ...options.loader },
  });
}

export async function artifact(path) {
  const bytes = await readFile(path);
  return {
    path: relative(workspace, path),
    bytes: bytes.byteLength,
    gzipBytes: gzipSync(bytes, { level: 9 }).byteLength,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

export async function packageVersion(name) {
  const metadata = JSON.parse(
    await readFile(
      resolve(workspace, "node_modules", name, "package.json"),
      "utf8",
    ),
  );
  if (typeof metadata.version !== "string")
    throw new Error(`${name} package metadata has no version`);
  return metadata.version;
}

export async function loadFixtureGenerators(entryPoint) {
  const result = await build({
    absWorkingDir: workspace,
    entryPoints: [entryPoint],
    bundle: true,
    write: false,
    format: "esm",
    platform: "node",
    target: "node22",
    legalComments: "none",
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
  });
  const file = result.outputFiles[0];
  if (!file) throw new Error("esbuild omitted the fixture generator module");
  return import(
    `data:text/javascript;base64,${Buffer.from(file.contents).toString("base64")}`
  );
}

export function exportBuiltin(kind, uri) {
  return new Promise((resolvePromise, reject) => {
    const executable = resolve(
      workspace,
      "target/builtin-exporter",
      process.platform === "win32" ? "export_builtin.exe" : "export_builtin",
    );
    const child = spawn(executable, [kind, uri], {
      cwd: workspace,
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 180_000,
      killSignal: "SIGTERM",
    });
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.once("error", reject);
    child.once("close", (code, signal) => {
      if (code === 0) resolvePromise(Buffer.concat(stdout));
      else
        reject(
          new Error(
            `export_builtin ${kind} '${uri}' failed (${signal ?? code}): ${Buffer.concat(stderr).toString("utf8").trim()}`,
          ),
        );
    });
  });
}
