/**
 * The shared development Host: one `gles_host` process per state directory,
 * the state file other processes use to find it, the generated client that
 * matches it, and the presentation lock its clients share.
 */
import { spawn, spawnSync } from "node:child_process";
import { closeSync, constants, existsSync, openSync } from "node:fs";
import {
  copyFile,
  mkdir,
  readFile,
  readdir,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Client, HostClientBase, LogLevel } from "@ipp/client";

/** State file contents of a running shared Host. */
export interface HostState {
  readonly pid: number;
  readonly url: string;
  readonly presentationUrl: string;
  /** Checkout the Host was built from, and its commit. */
  readonly worktree: string;
  readonly commit: string;
  /** Compatibility hash of the Host's contract. */
  readonly contract: string;
  /** Private copy of the executable and its generated client. */
  readonly client: string;
  /** The shared GUI font built with the Host. */
  readonly font: string;
  readonly eglDirectory: string;
  readonly log: string;
  readonly startedAt: string;
  readonly ioRead?: readonly {
    readonly prefix: string;
    readonly directory: string;
  }[];
}

/** The generated client module of the running Host. */
export type HostContract = Readonly<Record<string, unknown>> & {
  readonly SCHEMA_HASH: bigint;
  readonly IppHostClient: {
    connectWebSocket(
      url: string,
      options?: { timeoutMs?: number; logLevel?: LogLevel },
    ): Promise<HostClientBase<Client>>;
  };
};

/** Where the state directory lives inside a checkout. */
export const STATE_DIRECTORY = "target/shared-host-state";

export const sleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

export function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/**
 * The state directory: `--host` or `IPP_SHARED_HOST` (a checkout, its state
 * directory or its `host.json`), else this checkout's.
 */
export function stateDirectory(workspace: string, option?: string): string {
  const selected = option ?? process.env.IPP_SHARED_HOST;
  if (!selected) return resolve(workspace, STATE_DIRECTORY);
  const path = resolve(selected);
  if (path.endsWith(".json")) return dirname(path);
  if (existsSync(join(path, "host.json"))) return path;
  return join(path, STATE_DIRECTORY);
}

export async function readHost(directory: string): Promise<HostState | null> {
  try {
    return JSON.parse(
      await readFile(join(directory, "host.json"), "utf8"),
    ) as HostState;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  }
}

/** The running Host's state, or a failure naming how to start one. */
export async function requireHost(directory: string): Promise<HostState> {
  const host = await readHost(directory);
  if (!host || !alive(host.pid))
    throw new Error(
      `No running shared Host at ${directory}; start one with: node tools/shared-host/shared-host.mjs host start`,
    );
  return host;
}

async function loadContract(client: string): Promise<HostContract> {
  return (await import(
    pathToFileURL(join(client, "generated.js")).href
  )) as HostContract;
}

/** Simultaneous connections the native WebSocket Host serves (`MAX_CONNECTIONS`). */
export const HOST_CONNECTIONS = 8;

/** Connect through the Host's own generated client, so the contracts always match. */
export async function connect(host: HostState) {
  const contract = await loadContract(host.client);
  try {
    const client = await contract.IppHostClient.connectWebSocket(host.url, {
      timeoutMs: 60_000,
      logLevel: (process.env.IPP_SHARED_HOST_LOG ?? "warn") as LogLevel,
    });
    return { contract, client };
  } catch (error) {
    throw new Error(
      `Could not connect to the shared Host at ${host.url}: ${error instanceof Error ? error.message : String(error)}. It serves at most ${HOST_CONNECTIONS} connections at once, and every session and running command holds one; stop an unused session or retry.`,
    );
  }
}

function git(workspace: string, ...args: string[]): string {
  const result = spawnSync("git", args, { cwd: workspace, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "";
}

export async function startHost(
  workspace: string,
  directory: string,
  options: {
    eglDirectory: string;
    build: boolean;
    gallery?: boolean;
    ioRead?: readonly { readonly prefix: string; readonly directory: string }[];
  },
): Promise<HostState> {
  const existing = await readHost(directory);
  if (existing && alive(existing.pid))
    throw new Error(
      `A shared Host is already running (pid ${existing.pid}, ${existing.url}); use it, or ask its owner to stop it`,
    );
  await mkdir(directory, { recursive: true });
  if (options.build) {
    const built = spawnSync(
      process.env.PYTHON ?? "python",
      [
        "tools/ipp.py",
        "build",
        "gles-host",
        "font-assets",
        ...(options.gallery
          ? [
              "gallery-assets",
              "gallery-gui-assets",
              "gallery-platformer-native-assets",
            ]
          : []),
      ],
      { cwd: workspace, stdio: ["ignore", process.stderr, process.stderr] },
    );
    if (built.status !== 0)
      throw new Error("Building the GLES host and font assets failed");
  }
  // Run a private copy, so rebuilding target/gles-host never separates the
  // running Host from the generated client that matches its contract.
  const product = join(workspace, "target/gles-host");
  const client = join(directory, "host");
  await rm(client, { recursive: true, force: true });
  await mkdir(client, { recursive: true });
  for (const name of await readdir(product))
    if (name.endsWith(".js") || name === "contract.bin" || name === "gles_host")
      await copyFile(
        join(product, name),
        join(client, name),
        constants.COPYFILE_FICLONE,
      );
  const contract = await loadContract(client);
  const log = join(directory, "host.log");
  const output = openSync(log, "w");
  const child = spawn(
    join(client, "gles_host"),
    [
      "--egl-dir",
      options.eglDirectory,
      ...[
        ...(options.ioRead ?? []),
        ...(options.gallery
          ? [
              {
                prefix: "ipp-gallery://assets/",
                directory: join(workspace, "target"),
              },
              {
                prefix: "https://platformer.ipp.invalid/",
                directory: join(
                  workspace,
                  "target/gallery-platformer-native-assets",
                ),
              },
            ]
          : []),
      ].flatMap(({ prefix, directory }) => [
        "--io-read",
        prefix,
        resolve(workspace, directory),
      ]),
    ],
    { cwd: workspace, detached: true, stdio: ["ignore", output, output] },
  );
  closeSync(output);
  child.unref();
  const pid = child.pid;
  if (!pid) throw new Error("The GLES host did not start");
  const deadline = Date.now() + 60_000;
  for (;;) {
    const text = await readFile(log, "utf8");
    const line = text.split("\n").find((entry) => entry.includes('"ready"'));
    if (line) {
      const ready = JSON.parse(line) as { url: string; presentation: string };
      const state: HostState = {
        pid,
        url: ready.url,
        presentationUrl: ready.presentation,
        worktree: workspace,
        commit: git(workspace, "rev-parse", "HEAD"),
        contract: contract.SCHEMA_HASH.toString(16),
        client,
        font: join(workspace, "target/font-assets/shure-tech-mono.ippf"),
        eglDirectory: options.eglDirectory,
        log,
        startedAt: new Date().toISOString(),
        ioRead: [
          ...(options.ioRead ?? []),
          ...(options.gallery
            ? [
                {
                  prefix: "ipp-gallery://assets/",
                  directory: join(workspace, "target"),
                },
                {
                  prefix: "https://platformer.ipp.invalid/",
                  directory: join(
                    workspace,
                    "target/gallery-platformer-native-assets",
                  ),
                },
              ]
            : []),
        ].map(({ prefix, directory }) => ({
          prefix,
          directory: resolve(workspace, directory),
        })),
      };
      await writeFile(
        join(directory, "host.json"),
        `${JSON.stringify(state, null, 2)}\n`,
      );
      return state;
    }
    if (!alive(pid) || Date.now() > deadline) {
      if (alive(pid)) process.kill(pid, "SIGKILL");
      throw new Error(`The GLES host did not become ready:\n${text}`);
    }
    await sleep(50);
  }
}

export async function stopHost(directory: string): Promise<HostState | null> {
  const host = await readHost(directory);
  if (host && alive(host.pid)) {
    process.kill(host.pid, "SIGTERM");
    const deadline = Date.now() + 5_000;
    while (alive(host.pid) && Date.now() < deadline) await sleep(50);
    if (alive(host.pid)) process.kill(host.pid, "SIGKILL");
  }
  await rm(join(directory, "host.json"), { force: true });
  await rm(join(directory, "presentation.lock"), {
    recursive: true,
    force: true,
  });
  return host;
}

interface LockOwner {
  readonly pid: number;
  readonly label: string;
  readonly since: string;
}

export async function lockOwner(directory: string): Promise<LockOwner | null> {
  try {
    return JSON.parse(
      await readFile(
        join(directory, "presentation.lock", "owner.json"),
        "utf8",
      ),
    ) as LockOwner;
  } catch {
    return null;
  }
}

/**
 * Run `section` holding the Host-wide presentation lock: a directory created
 * atomically and removed on completion; a lock whose owner died is broken.
 */
export async function withPresentationLock<T>(
  directory: string,
  label: string,
  section: () => Promise<T>,
  timeoutMs = 300_000,
): Promise<T> {
  const lock = join(directory, "presentation.lock");
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      await mkdir(lock);
      break;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
    const owner = await lockOwner(directory);
    const created = await stat(lock).catch(() => null);
    const abandoned = owner
      ? !alive(owner.pid)
      : created !== null && Date.now() - created.mtimeMs > 10_000;
    if (abandoned) {
      await rm(lock, { recursive: true, force: true });
      continue;
    }
    if (Date.now() > deadline)
      throw new Error(
        `Presentation lock held for over ${timeoutMs / 1000} s by ${owner?.label ?? "an unknown client"} (pid ${owner?.pid ?? "?"})`,
      );
    await sleep(15);
  }
  try {
    await writeFile(
      join(lock, "owner.json"),
      JSON.stringify({
        pid: process.pid,
        label,
        since: new Date().toISOString(),
      } satisfies LockOwner),
    );
    return await section();
  } finally {
    await rm(lock, { recursive: true, force: true });
  }
}
