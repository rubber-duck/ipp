/** Real AppKit window, generated client, gallery assets, physical input and pixels. */
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  symlink,
  writeFile,
} from "node:fs/promises";
import { join, resolve } from "node:path";
import { setTimeout } from "node:timers/promises";
import test from "node:test";
import { promisify } from "node:util";
import { alive, startHost, stopHost } from "../../tools/shared-host/host.ts";
import { exerciseNativeWindow } from "./scenarios/native-window.mjs";
import { decodePng } from "../../tools/shared-host/png.ts";

const execute = promisify(execFile);
const workspace = process.cwd();

test("macOS window presents, routes clicks, resizes, reloads and closes", {
  skip:
    process.platform !== "darwin"
      ? "macOS window; Linux implementation is a stub"
      : false,
  timeout: 180_000,
}, async () => {
  const parent = resolve("target/integration-artifacts/native-window");
  await mkdir(parent, { recursive: true });
  const evidence = await mkdtemp(join(parent, "run-"));
  const owner = join(evidence, "owner");
  const state = join(evidence, "state");
  await mkdir(join(owner, "target"), { recursive: true });
  for (const [name, source] of [
    ["gles-host", "gles-host-instrumentation"],
    ["font-assets", "font-assets"],
    ["gallery-assets", "gallery-assets"],
  ])
    await symlink(
      resolve("target", source),
      join(owner, "target", name),
      "dir",
    );
  const cli = async (...args) => {
    const { stdout } = await execute(
      process.execPath,
      [
        resolve("tools/shared-host/shared-host.mjs"),
        ...args,
        "--host",
        join(state, "host.json"),
        "--json",
      ],
      { cwd: workspace, timeout: 60_000, maxBuffer: 16 << 20 },
    );
    return JSON.parse(stdout);
  };
  const post = (kind, x = 0, y = 0) =>
    cli(
      "session",
      "action",
      "window",
      "windowTest",
      JSON.stringify([kind, x, y]),
    );
  const inspect = async () =>
    (await cli("session", "inspect", "window")).report;
  const wait = async (condition) => {
    const deadline = Date.now() + 15_000;
    while (!(await condition())) {
      assert.ok(
        Date.now() < deadline,
        "window state reached its expected value",
      );
      await setTimeout(50);
    }
  };
  const capture = async (name) => {
    const reply = await cli(
      "session",
      "capture",
      "window",
      "--out",
      join(evidence, name),
    );
    const frame = decodePng(await readFile(reply.files[0]));
    return { frame, viewport: reply.report.viewport };
  };
  let host;
  try {
    host = await startHost(owner, state, {
      build: false,
      window: true,
      ioRead: [
        { prefix: "ipp-gallery://assets/", directory: join(owner, "target") },
      ],
      eglDirectory: process.env.IPP_EGL_LIBRARY_DIR,
    });
    assert.ok(
      host.snapshot["libipp-window.dylib"],
      "private shim is included in build identity",
    );
    await cli("gallery", "shapes", "--name", "window");
    const viewport = await exerciseNativeWindow({
      inspect,
      post,
      wait,
      capture,
      scale: (value) =>
        cli(
          "session",
          "action",
          "window",
          "renderScale",
          JSON.stringify(value),
        ),
      reload: () => cli("session", "reload", "window"),
      async closeWindow() {
        await post(9);
        await wait(async () => !alive(host.pid));
        await wait(async () => {
          try {
            await readFile(join(state, "sessions/window.json"));
            return false;
          } catch (error) {
            return error.code === "ENOENT";
          }
        });
        const sessionLog = await readFile(
          join(state, "sessions/window.log"),
          "utf8",
        );
        assert.doesNotMatch(sessionLog, /session cleanup failed/);
      },
    });
    await writeFile(
      join(evidence, "result.json"),
      JSON.stringify(
        {
          pid: host.pid,
          snapshot: host.snapshot,
          viewport,
          closed: true,
        },
        null,
        2,
      ),
    );
  } finally {
    await cli("session", "stop", "--all").catch(() => {});
    await stopHost(state);
    await copyFile(join(state, "host.log"), join(evidence, "host.log")).catch(
      () => {},
    );
  }
});
