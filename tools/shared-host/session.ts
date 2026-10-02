/**
 * Long-lived client sessions: a detached process keeps one client module
 * open on the shared Host and serves capture requests on a loopback port, so
 * repeated shell commands reuse its connection, Worlds and declarations.
 */
import { spawn } from "node:child_process";
import { closeSync, openSync } from "node:fs";
import { mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { connect as connectSocket, createServer, type Socket } from "node:net";
import { join, resolve } from "node:path";
import {
  closeClient,
  loadClient,
  openClient,
  writeResult,
  type OpenClient,
} from "./context.js";
import { alive, requireHost, sleep } from "./host.js";

export interface SessionState {
  readonly name: string;
  readonly module: string;
  readonly args: readonly string[];
  readonly pid: number;
  readonly port: number;
  readonly workspace: string;
  readonly log: string;
  readonly startedAt: string;
}

/** A capture request and its reply, as JSON lines on the session port. */
export interface SessionRequest {
  readonly command: "capture" | "stop";
  readonly args?: readonly string[];
  readonly out?: string;
}

export interface CaptureReply {
  readonly name: string;
  readonly files: readonly string[];
  readonly report?: unknown;
  readonly summary?: readonly string[];
  /** Whether the client module's bundle changed and was swapped in. */
  readonly reloaded: boolean;
  readonly timings: Readonly<Record<string, number>>;
}

export function sessionDirectory(directory: string): string {
  return join(directory, "sessions");
}

export async function readSessions(directory: string): Promise<SessionState[]> {
  const sessions: SessionState[] = [];
  const names = await readdir(sessionDirectory(directory)).catch(() => []);
  for (const file of names.filter((name) => name.endsWith(".json"))) {
    try {
      sessions.push(
        JSON.parse(
          await readFile(join(sessionDirectory(directory), file), "utf8"),
        ) as SessionState,
      );
    } catch {
      // A session still writing its state is listed on the next call.
    }
  }
  return sessions;
}

export async function findSession(
  directory: string,
  name: string,
): Promise<SessionState> {
  const session = (await readSessions(directory)).find(
    (entry) => entry.name === name,
  );
  if (!session || !alive(session.pid))
    throw new Error(
      `No running session ${name}; start one with: node tools/shared-host/shared-host.mjs session start MODULE --name ${name}`,
    );
  return session;
}

/** Reload the module if its bundle changed, serve one request and write its images. */
export async function serveCapture(
  open: OpenClient,
  workspace: string,
  module: string,
  args: readonly string[],
  out: string,
  reload: boolean,
): Promise<CaptureReply> {
  const started = performance.now();
  const next = reload
    ? await loadClient(workspace, module, open.identity)
    : null;
  if (next) {
    open.client = next.client;
    open.identity = next.identity;
  }
  const loaded = performance.now();
  const result = await open.client.capture(open.state, open.context, args);
  const captured = performance.now();
  const files = await writeResult(result, out);
  return {
    name: open.context.name,
    files,
    ...(result.report === undefined ? {} : { report: result.report }),
    ...(result.summary ? { summary: result.summary } : {}),
    reloaded: !!next,
    timings: {
      reloadMs: loaded - started,
      captureMs: captured - loaded,
      writeMs: performance.now() - captured,
    },
  };
}

/** Start a detached session process and wait until it serves requests. */
export async function startSession(options: {
  readonly workspace: string;
  readonly directory: string;
  readonly launcher: string;
  readonly module: string;
  readonly name: string;
  readonly args: readonly string[];
  readonly idleMinutes?: string;
}): Promise<SessionState> {
  const { directory, name } = options;
  if (!/^[A-Za-z0-9._-]+$/.test(name))
    throw new Error("Session names use letters, digits, '.', '_' and '-'");
  await requireHost(directory);
  const existing = (await readSessions(directory)).find(
    (entry) => entry.name === name,
  );
  if (existing && alive(existing.pid))
    throw new Error(`Session ${name} is already running (pid ${existing.pid})`);
  await mkdir(sessionDirectory(directory), { recursive: true });
  const log = join(sessionDirectory(directory), `${name}.log`);
  const output = openSync(log, "w");
  const child = spawn(
    process.execPath,
    [
      options.launcher,
      "session",
      "serve",
      resolve(options.workspace, options.module),
      "--name",
      name,
      "--host",
      directory,
      ...(options.idleMinutes ? ["--idle-minutes", options.idleMinutes] : []),
      ...options.args,
    ],
    {
      cwd: options.workspace,
      detached: true,
      stdio: ["ignore", output, output],
    },
  );
  closeSync(output);
  child.unref();
  const deadline = Date.now() + 120_000;
  for (;;) {
    const session = (await readSessions(directory)).find(
      (entry) => entry.name === name && entry.pid === child.pid,
    );
    if (session) return session;
    if (!child.pid || !alive(child.pid) || Date.now() > deadline)
      throw new Error(
        `Session ${name} did not start:\n${await readFile(log, "utf8")}`,
      );
    await sleep(50);
  }
}

/** The session process: open the module, then serve requests one at a time. */
export async function serveSession(options: {
  readonly workspace: string;
  readonly directory: string;
  readonly module: string;
  readonly name: string;
  readonly args: readonly string[];
  readonly idleMinutes: number;
}): Promise<void> {
  const { workspace, directory, module, name } = options;
  const host = await requireHost(directory);
  const open = await openClient({
    workspace,
    directory,
    host,
    module,
    name,
    args: options.args,
  });
  const statePath = join(sessionDirectory(directory), `${name}.json`);
  let queue: Promise<unknown> = Promise.resolve();
  let idle: NodeJS.Timeout | undefined;
  let stopping = false;
  const server = createServer();
  const shutdown = async (reason: string) => {
    if (stopping) return;
    stopping = true;
    console.error(`session ${name}: stopping (${reason})`);
    clearTimeout(idle);
    clearInterval(watchdog);
    server.close();
    await rm(statePath, { force: true });
    await closeClient(open).catch(() => {});
    process.exit(0);
  };
  const touch = () => {
    clearTimeout(idle);
    idle = setTimeout(
      () => void shutdown("idle timeout"),
      options.idleMinutes * 60_000,
    );
  };
  const watchdog = setInterval(() => {
    if (!alive(host.pid)) void shutdown("the Host stopped");
  }, 1_000);
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => void shutdown(signal));
  server.on("connection", (socket: Socket) => {
    let buffer = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      const line = buffer.slice(0, newline);
      buffer = "";
      touch();
      const reply = (value: object) =>
        socket.end(
          `${JSON.stringify(value, (_, entry) => (typeof entry === "bigint" ? String(entry) : entry))}\n`,
        );
      queue = queue.then(async () => {
        try {
          const request = JSON.parse(line) as SessionRequest;
          if (request.command === "stop") {
            reply({ ok: true });
            await shutdown("stop requested");
          } else if (request.command === "capture")
            reply({
              ok: true,
              ...(await serveCapture(
                open,
                workspace,
                module,
                request.args ?? [],
                request.out ??
                  join(workspace, "target/shared-host-captures", name),
                true,
              )),
            });
          else throw new Error(`Unknown session command ${request.command}`);
        } catch (error) {
          console.error(error);
          reply({
            ok: false,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      });
    });
  });
  await new Promise<void>((resolve) =>
    server.listen(0, "127.0.0.1", () => resolve()),
  );
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("Session server has no TCP address");
  await mkdir(sessionDirectory(directory), { recursive: true });
  const state: SessionState = {
    name,
    module,
    args: options.args,
    pid: process.pid,
    port: address.port,
    workspace,
    log: join(sessionDirectory(directory), `${name}.log`),
    startedAt: new Date().toISOString(),
  };
  await writeFile(statePath, `${JSON.stringify(state, null, 2)}\n`);
  touch();
  console.error(`session ${name}: ready on 127.0.0.1:${address.port}`);
}

/** Send one request to a session and wait for its reply. */
export function request(
  session: SessionState,
  body: SessionRequest,
): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const socket = connectSocket(session.port, "127.0.0.1");
    let data = "";
    socket.setEncoding("utf8");
    socket.on("connect", () => socket.write(`${JSON.stringify(body)}\n`));
    socket.on("data", (chunk: string) => {
      data += chunk;
    });
    socket.on("error", reject);
    socket.on("end", () => {
      try {
        resolve(JSON.parse(data) as Record<string, unknown>);
      } catch {
        reject(new Error(`Session ${session.name} closed without a reply`));
      }
    });
  });
}

/** Stop a session through its port, or by signal when it does not answer. */
export async function stopSession(
  directory: string,
  session: SessionState,
): Promise<void> {
  if (alive(session.pid))
    await request(session, { command: "stop" }).catch(() =>
      process.kill(session.pid, "SIGTERM"),
    );
  await rm(join(sessionDirectory(directory), `${session.name}.json`), {
    force: true,
  });
}
