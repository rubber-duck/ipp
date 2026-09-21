import { spawn } from "node:child_process";
import { access, readFile, rename, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import type { BrowserEnvironmentContext } from "../browser/environment.js";

export interface BlenderReadiness {
  origin: string;
  token: string;
  session: string;
  revision: number;
  pid: number;
  certificate: string;
  private_key: string;
  blender_version: string;
}

/** Own the real Blender/addon process; scenario operations remain local fixture inputs. */
export async function startBlender(
  environment: BrowserEnvironmentContext,
  options: { blend?: string; fixture?: string; startupTimeoutMs?: number } = {},
) {
  const workspace = resolve(process.env.IPP_BLENDER_WORKSPACE ?? process.cwd());
  const certificate =
    process.env.BLENDER_TEST_CERTIFICATE ??
    resolve(workspace, "target/blender/certificates/localhost.pem");
  const privateKey =
    process.env.BLENDER_TEST_PRIVATE_KEY ??
    resolve(workspace, "target/blender/certificates/localhost-key.pem");
  await Promise.all([access(certificate), access(privateKey)]);
  const readyFile = join(environment.evidence.directory, "blender-ready.json");
  const controlFile = join(
    environment.evidence.directory,
    "blender-control.json",
  );
  const args = [
    "tools/blender.py",
    "serve",
    "--ready-file",
    readyFile,
    "--port",
    "0",
    "--certificate",
    certificate,
    "--private-key",
    privateKey,
    "--allowed-origin",
    environment.urls.origin,
    "--control-file",
    controlFile,
  ];
  if (options.blend) args.push("--blend", resolve(workspace, options.blend));
  if (options.fixture || !options.blend)
    args.push(
      "--fixture",
      resolve(workspace, options.fixture ?? "tests/blender/fixture.py"),
    );
  const child = spawn(process.env.PYTHON_BIN ?? "python3", args, {
    cwd: workspace,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let failed: Error | undefined;
  let exited = false;
  child.once("error", (error) => {
    failed = error;
  });
  const completion = new Promise<void>((done) =>
    child.once("close", (code, signal) => {
      exited = true;
      if (code !== 0) failed = new Error(`Blender exited (${signal ?? code})`);
      void environment.evidence.record("blender_exit", { code, signal });
      done();
    }),
  );
  for (const [name, stream] of [
    ["stdout", child.stdout],
    ["stderr", child.stderr],
  ] as const)
    stream.on("data", (data: Buffer) => {
      void environment.evidence.record(`blender_${name}`, data.toString());
    });
  environment.own({
    async close() {
      if (exited) return;
      child.kill("SIGTERM");
      const force = setTimeout(() => child.kill("SIGKILL"), 5000);
      try {
        await completion;
      } finally {
        clearTimeout(force);
      }
    },
  });
  const check = () => {
    environment.signal.throwIfAborted();
    if (failed) throw failed;
    if (exited)
      throw new Error("Blender exited before the operation completed");
  };
  const ready = await waitForJson<BlenderReadiness>(
    readyFile,
    () => true,
    check,
    options.startupTimeoutMs,
  );
  await environment.evidence.record("blender_ready", {
    ...ready,
    token: "[redacted]",
  });
  if (!/^5\.2\./.test(ready.blender_version))
    throw new Error(`Expected Blender 5.2 LTS, got ${ready.blender_version}`);
  let sequence = 0;
  return {
    ready,
    async command(input: Record<string, unknown>) {
      check();
      const command = { ...input, sequence: ++sequence };
      await writeFile(`${controlFile}.tmp`, JSON.stringify(command));
      await rename(`${controlFile}.tmp`, controlFile);
      const result = await waitForJson<{
        sequence: number;
        revision?: number;
        error?: string;
      }>(
        readyFile.replace(/\.json$/, ".control.json"),
        (value) => value.sequence === sequence,
        check,
      );
      await environment.evidence.record("blender_command", { input, result });
      if (result.error) throw new Error(result.error);
      return result.revision!;
    },
    async stop() {
      child.kill("SIGTERM");
      const force = setTimeout(() => child.kill("SIGKILL"), 5000);
      try {
        await completion;
      } finally {
        clearTimeout(force);
      }
    },
  };
}

async function waitForJson<T>(
  path: string,
  accept: (value: T) => boolean,
  check: () => void,
  timeoutMs = 60_000,
): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    check();
    try {
      const value: T = JSON.parse(await readFile(path, "utf8"));
      if (accept(value)) return value;
    } catch (error) {
      if (
        !(error instanceof SyntaxError) &&
        !(
          error &&
          typeof error === "object" &&
          "code" in error &&
          error.code === "ENOENT"
        )
      )
        throw error;
    }
    if (Date.now() >= deadline)
      throw new Error(`Timed out waiting for ${path}`);
    await delay(25);
  }
}
