/** Real shared Host discovery, private assets and independent React sessions. */
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import {
  alive,
  connect,
  listHosts,
  startHost,
  stopHost,
} from "../../tools/shared-host/host.ts";
import { decodePng } from "../../tools/shared-host/png.ts";

const execute = promisify(execFile);
const workspace = process.cwd();
const launcher = resolve("tools/shared-host/shared-host.mjs");

test("shared Host discovery and private font survive owner changes with two React clients", {
  timeout: 120_000,
}, async () => {
  const parent = resolve("target/integration-artifacts/shared-host");
  await mkdir(parent, { recursive: true });
  const evidence = await mkdtemp(join(parent, "run-"));
  const owner = join(evidence, "owner");
  const worker = join(evidence, "worker");
  const state = join(owner, "target/shared-host-state");
  await mkdir(join(owner, "target/font-assets"), { recursive: true });
  const git = (...args) => execute("git", args, { cwd: owner });
  const cli = async (...args) => {
    const result = await execute(
      process.execPath,
      [launcher, ...args, "--host", join(state, "host.json"), "--json"],
      { cwd: workspace, timeout: 45_000, maxBuffer: 8 << 20 },
    );
    return result.stdout;
  };
  let host;
  let observer;
  try {
    await git("init", "-q");
    await writeFile(join(owner, ".gitignore"), "target/\n");
    await git("add", ".gitignore");
    await git(
      "-c",
      "user.name=Test",
      "-c",
      "user.email=test@example.invalid",
      "commit",
      "-qm",
      "Fixture",
    );
    await git("worktree", "add", "--detach", worker, "HEAD");
    await symlink(
      resolve("target/gles-host"),
      join(owner, "target/gles-host"),
      "dir",
    );
    const ownerFont = join(owner, "target/font-assets/shure-tech-mono.ippf");
    await copyFile(
      resolve("target/font-assets/shure-tech-mono.ippf"),
      ownerFont,
    );
    host = await startHost(owner, state, {
      build: false,
      eglDirectory: process.env.IPP_EGL_LIBRARY_DIR ?? "/lib64",
    });
    observer = (await connect(host)).client;
    assert.notEqual(host.font, ownerFont);
    assert.equal(
      createHash("sha256")
        .update(await readFile(host.font))
        .digest("hex"),
      host.snapshot["shure-tech-mono.ippf"],
    );
    const discovered = await listHosts(
      worker,
      join(worker, "target/shared-host-state"),
    );
    assert.equal(discovered.filter((entry) => entry.running).length, 1);
    assert.equal(discovered.find((entry) => entry.running).host.pid, host.pid);
    const listed = JSON.parse(await cli("host", "list"));
    assert.ok(
      listed.some((entry) => entry.host?.pid === host.pid && entry.running),
    );
    await cli(
      "session",
      "start",
      "tools/shared-host/hello.tsx",
      "--name",
      "first",
    );
    const capture = async (name, text) => {
      const reply = JSON.parse(
        await cli(
          "session",
          "capture",
          name,
          text,
          "--out",
          join(evidence, name),
        ),
      );
      const bytes = await readFile(reply.files[0]);
      const frame = decodePng(bytes);
      assert.equal(frame.width, 320);
      assert.equal(frame.height, 96);
      let glyphPixels = 0;
      for (let index = 0; index < frame.pixels.length; index += 4)
        if (frame.pixels[index + 1] > 150 && frame.pixels[index + 2] > 180)
          glyphPixels++;
      assert.ok(
        glyphPixels > 50,
        `expected rendered text, saw ${glyphPixels} bright pixels`,
      );
      return bytes;
    };
    const before = await capture("first", "ALPHA");
    // The second session must load the private font after the owner rebuilds it.
    await writeFile(ownerFont, "invalid rebuilt font");
    await cli(
      "session",
      "start",
      "tools/shared-host/hello.tsx",
      "--name",
      "second",
    );
    const other = await capture("second", "OMEGA");
    assert.notDeepEqual(before, other, "sessions render their own text");
    const after = await capture("first", "ALPHA");
    const firstPixels = decodePng(before).pixels;
    const afterPixels = decodePng(after).pixels;
    let changedPixels = 0;
    for (let index = 0; index < firstPixels.length; index += 4)
      if (
        [0, 1, 2].some(
          (channel) =>
            Math.abs(
              firstPixels[index + channel] - afterPixels[index + channel],
            ) > 2,
        )
      )
        changedPixels++;
    assert.equal(
      changedPixels,
      0,
      "another session did not change the first World",
    );
    assert.equal((await observer.listWorlds()).length, 2);
    await cli("session", "stop", "--all");
    assert.equal(
      (await observer.listWorlds()).length,
      0,
      "sessions release their Worlds",
    );
    await observer.close();
    observer = undefined;
    await stopHost(state);
    assert.equal(alive(host.pid), false);
    assert.equal(
      (await listHosts(worker, state)).filter((entry) => entry.running).length,
      0,
    );
  } finally {
    await observer?.close();
    await cli("session", "stop", "--all").catch(() => {});
    await stopHost(state);
    // Keep images and the Host log, release the temporary source worktrees and binaries.
    await copyFile(join(state, "host.log"), join(evidence, "host.log")).catch(
      () => {},
    );
    await git("worktree", "remove", "--force", worker).catch(() => {});
    await rm(owner, { recursive: true, force: true });
  }
});
