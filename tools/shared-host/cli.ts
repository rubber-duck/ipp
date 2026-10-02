/** Shared development Host commands; `shared-host.mjs` runs this module. */
import { readFile, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import {
  closeClient,
  moduleName,
  openClient,
  type OpenClient,
} from "./context.js";
import {
  alive,
  connect,
  lockOwner,
  readHost,
  requireHost,
  startHost,
  stateDirectory,
  stopHost,
} from "./host.js";
import { crop, enlarge, sideBySide, type PixelRect } from "./images.js";
import { decodePng, encodePng } from "./png.js";
import {
  findSession,
  readSessions,
  request,
  serveCapture,
  serveSession,
  sessionDirectory,
  startSession,
  stopSession,
  type CaptureReply,
} from "./session.js";

const USAGE = `Usage: node tools/shared-host/shared-host.mjs <command>

  host start [--egl-dir DIR] [--no-build]  build and start the shared Host
  host status                              Host, Worlds, sessions, lock holder
  host stop                                stop the Host and its sessions
  run MODULE [ARGS...]                     open a client module, capture once
  session start MODULE --name NAME [ARGS...]
                                           keep a client module open
  session capture NAME [ARGS...]           reload the module if edited, capture
  session list | session stop NAME | session stop --all
  compare FIRST.png SECOND.png [--first-box X,Y,W,H] [--second-box X,Y,W,H]
          [--zoom-factor N] [--out FILE]   side-by-side of two PNGs

ARGS go to the client module. Common options, taken before the module sees
the arguments: --host CHECKOUT (or IPP_SHARED_HOST) selects another
checkout's Host; --out DIR replaces target/shared-host-captures/NAME;
--name NAME; --json prints the reply as JSON; --idle-minutes N ends an
unused session (default 120). See docs/development/shared-host.md.`;

/** Options the command consumes; every other argument goes to the module. */
const COMMON = {
  host: "string",
  out: "string",
  name: "string",
  json: "boolean",
  "idle-minutes": "string",
  "egl-dir": "string",
  "no-build": "boolean",
  all: "boolean",
  help: "boolean",
} as const;

type Common = { -readonly [Key in keyof typeof COMMON]?: string | boolean };

function split(argv: readonly string[]): {
  common: Common;
  rest: string[];
} {
  const common: Common = {};
  const rest: string[] = [];
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]!;
    const match = /^--([^=]+)(?:=(.*))?$/.exec(argument);
    const name = match?.[1] as keyof typeof COMMON | undefined;
    const kind = name && COMMON[name];
    if (!kind) {
      rest.push(argument);
      continue;
    }
    if (kind === "boolean") common[name!] = true;
    else {
      const value = match![2] ?? argv[++index];
      if (value === undefined) throw new Error(`--${name} needs a value`);
      common[name!] = value;
    }
  }
  return { common, rest };
}

function text(value: string | boolean | undefined): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function print(json: boolean, value: unknown, lines: string): void {
  console.log(
    json
      ? JSON.stringify(
          value,
          (_, entry) => (typeof entry === "bigint" ? String(entry) : entry),
          2,
        )
      : lines,
  );
}

function describe(reply: CaptureReply): string {
  return [
    ...reply.files,
    ...(reply.summary ?? []),
    `timings ms: ${JSON.stringify(Object.fromEntries(Object.entries(reply.timings).map(([key, value]) => [key, Math.round(value)])))}`,
  ].join("\n");
}

async function hostCommand(
  workspace: string,
  directory: string,
  action: string | undefined,
  common: Common,
): Promise<void> {
  const json = common.json === true;
  if (action === "start") {
    const host = await startHost(workspace, directory, {
      eglDirectory:
        text(common["egl-dir"]) ?? process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
      build: common["no-build"] !== true,
    });
    print(
      json,
      host,
      `shared Host pid ${host.pid} at ${host.url}, contract ${host.contract}\nstate ${join(directory, "host.json")}\nlog ${host.log}`,
    );
  } else if (action === "stop") {
    for (const session of await readSessions(directory)) {
      if (alive(session.pid)) process.kill(session.pid, "SIGTERM");
      await rm(join(sessionDirectory(directory), `${session.name}.json`), {
        force: true,
      });
    }
    const host = await stopHost(directory);
    console.log(
      host ? `stopped shared Host pid ${host.pid}` : "no shared Host",
    );
  } else if (action === "status") {
    const host = await readHost(directory);
    if (!host || !alive(host.pid)) {
      print(json, { running: false }, `no running shared Host at ${directory}`);
      return;
    }
    // The status stays readable while the Host is at its connection limit.
    const worlds = await connect(host).then(
      async (connection) => {
        try {
          return await connection.client.listWorlds();
        } finally {
          await connection.client.close();
        }
      },
      (error: Error) => error,
    );
    const sessions = await readSessions(directory);
    const owner = await lockOwner(directory);
    print(
      json,
      {
        running: true,
        host,
        worlds: worlds instanceof Error ? { error: worlds.message } : worlds,
        sessions,
        lock: owner,
      },
      [
        `shared Host pid ${host.pid} at ${host.url}, built from ${host.worktree} ${host.commit.slice(0, 10)}, contract ${host.contract}, since ${host.startedAt}`,
        worlds instanceof Error
          ? `worlds: unavailable (${worlds.message})`
          : `worlds: ${worlds.map((world) => world.symbolicId).join(", ") || "none"}`,
        `sessions: ${sessions.map((session) => `${session.name} (${session.module}${alive(session.pid) ? "" : ", dead"})`).join(", ") || "none"}`,
        `presentation lock: ${owner ? `${owner.label} since ${owner.since}` : "free"}`,
      ].join("\n"),
    );
  } else throw new Error(USAGE);
}

async function run(
  workspace: string,
  directory: string,
  [module, ...args]: string[],
  common: Common,
): Promise<void> {
  if (!module) throw new Error("run needs a client MODULE");
  const started = performance.now();
  const host = await requireHost(directory);
  const name = text(common.name) ?? moduleName(module);
  const open: OpenClient = await openClient({
    workspace,
    directory,
    host,
    module,
    name: `${name}.${process.pid}`,
    args,
  });
  try {
    const opened = performance.now();
    const reply = await serveCapture(
      open,
      workspace,
      module,
      args,
      resolve(
        text(common.out) ??
          join(workspace, "target/shared-host-captures", name),
      ),
      false,
    );
    const result = {
      ...reply,
      timings: {
        openMs: opened - started,
        ...reply.timings,
        commandMs: performance.now() - started,
      },
    };
    print(common.json === true, result, describe(result));
  } finally {
    await closeClient(open).catch(() => {});
  }
}

async function sessionCommand(
  workspace: string,
  directory: string,
  launcher: string,
  [action, ...rest]: string[],
  common: Common,
): Promise<void> {
  const json = common.json === true;
  if (action === "start") {
    const [module, ...args] = rest;
    const name = text(common.name);
    if (!module || !name)
      throw new Error("session start needs a MODULE and --name NAME");
    const started = performance.now();
    const session = await startSession({
      workspace,
      directory,
      launcher,
      module,
      name,
      args,
      ...(text(common["idle-minutes"])
        ? { idleMinutes: text(common["idle-minutes"])! }
        : {}),
    });
    const startMs = Math.round(performance.now() - started);
    print(
      json,
      { ...session, startMs },
      `session ${name}: ${session.module}, pid ${session.pid}, port ${session.port}, log ${session.log} (${startMs} ms)`,
    );
  } else if (action === "serve") {
    const [module, ...args] = rest;
    await serveSession({
      workspace,
      directory,
      module: module!,
      name: text(common.name)!,
      args,
      idleMinutes: Number(text(common["idle-minutes"]) ?? 120),
    });
  } else if (action === "capture") {
    const [name, ...args] = rest;
    const session = await findSession(directory, name ?? "");
    const started = performance.now();
    const reply = await request(session, {
      command: "capture",
      args,
      ...(text(common.out) ? { out: resolve(text(common.out)!) } : {}),
    });
    if (!reply.ok) throw new Error(String(reply.error));
    const result = reply as unknown as CaptureReply;
    const timed = {
      ...result,
      timings: {
        ...result.timings,
        commandMs: performance.now() - started,
      },
    };
    print(json, timed, describe(timed));
  } else if (action === "stop") {
    const sessions = common.all
      ? await readSessions(directory)
      : [await findSession(directory, rest[0] ?? "")];
    for (const session of sessions) {
      await stopSession(directory, session);
      console.log(`stopped session ${session.name}`);
    }
  } else if (action === "list") {
    const sessions = await readSessions(directory);
    print(
      json,
      sessions,
      sessions.length
        ? sessions
            .map(
              (session) =>
                `${session.name}: ${session.module} ${session.args.join(" ")}, pid ${session.pid}${alive(session.pid) ? "" : " (dead)"}, since ${session.startedAt}`,
            )
            .join("\n")
        : "no sessions",
    );
  } else throw new Error(USAGE);
}

async function compare(argv: readonly string[]): Promise<void> {
  const { values, positionals } = parseArgs({
    args: [...argv],
    allowPositionals: true,
    options: {
      "first-box": { type: "string" },
      "second-box": { type: "string" },
      "zoom-factor": { type: "string" },
      out: { type: "string" },
    },
  });
  const [first, second] = positionals;
  if (!first || !second) throw new Error(USAGE);
  const box = (value: string | undefined): PixelRect | undefined => {
    if (!value) return undefined;
    const numbers = value.split(",").map(Number);
    if (
      numbers.length !== 4 ||
      numbers.some((entry) => !Number.isFinite(entry))
    )
      throw new Error(`Expected X,Y,W,H, got ${value}`);
    return numbers as unknown as PixelRect;
  };
  const load = async (path: string, rect?: PixelRect) => {
    const picture = decodePng(await readFile(resolve(path)));
    return rect ? crop(picture, rect) : picture;
  };
  const factor = Number(values["zoom-factor"] ?? 1);
  const output = resolve(values.out ?? "compare.png");
  await writeFile(
    output,
    encodePng(
      sideBySide(
        [
          await load(first, box(values["first-box"])),
          await load(second, box(values["second-box"])),
        ].map((picture) => enlarge(picture, factor)),
      ),
    ),
  );
  console.log(output);
}

export async function main(
  workspace: string,
  launcher: string,
  argv: readonly string[],
): Promise<void> {
  const [command, ...after] = argv;
  if (!command || command === "--help" || command === "help") {
    console.log(USAGE);
    return;
  }
  if (command === "compare") return compare(after);
  const { common, rest } = split(after);
  if (common.help) {
    console.log(USAGE);
    return;
  }
  const directory = stateDirectory(workspace, text(common.host));
  if (command === "host")
    await hostCommand(workspace, directory, rest[0], common);
  else if (command === "run") await run(workspace, directory, rest, common);
  else if (command === "session")
    await sessionCommand(workspace, directory, launcher, rest, common);
  else throw new Error(USAGE);
}
