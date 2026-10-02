#!/usr/bin/env node
/**
 * Shared development Host entry point:
 * `node tools/shared-host/shared-host.mjs --help`, documented in
 * docs/development/shared-host.md.
 *
 * Runs the `shared-host` pipeline product (target/shared-host/cli.mjs) and
 * rebuilds it through `python tools/ipp.py build shared-host` first when
 * one of its recorded inputs changed.
 */
import { spawnSync } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const workspace = resolve(import.meta.dirname, "../..");
const product = resolve(workspace, "target/shared-host");

async function current() {
  try {
    const inputs = JSON.parse(
      await readFile(resolve(product, "inputs.json"), "utf8"),
    );
    for (const [path, mtimeMs, size] of inputs) {
      const entry = await stat(resolve(workspace, path));
      if (entry.mtimeMs !== mtimeMs || entry.size !== size) return false;
    }
    await stat(resolve(product, "cli.mjs"));
    return true;
  } catch {
    return false;
  }
}

try {
  if (!(await current())) {
    const built = spawnSync(
      process.env.PYTHON ?? "python",
      ["tools/ipp.py", "build", "shared-host"],
      { cwd: workspace, stdio: ["ignore", process.stderr, process.stderr] },
    );
    if (built.status !== 0 || !(await current()))
      throw new Error("python tools/ipp.py build shared-host failed");
  }
  process.setSourceMapsEnabled(true);
  const { main } = await import(
    pathToFileURL(resolve(product, "cli.mjs")).href
  );
  await main(workspace, import.meta.filename, process.argv.slice(2));
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
}
