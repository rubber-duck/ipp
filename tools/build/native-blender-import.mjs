/** Save the shared Blender import through the native target's real Host/client. */
import { spawn } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";
import { workspace } from "./helpers.mjs";

export async function importNativeBlenderScene({
  source,
  namespace,
  clipsOnly,
  output,
  origin,
}) {
  const directory = resolve(workspace, "target/native-blender-import");
  await mkdir(directory, { recursive: true });
  const entry = resolve(directory, "import.mjs");
  await build({
    absWorkingDir: workspace,
    entryPoints: ["integrations/blender/client/disk-import.ts"],
    outfile: entry,
    bundle: true,
    format: "esm",
    platform: "node",
    target: "node22",
    alias: {
      "@ipp/client": resolve(workspace, "packages/ipp-client/src/index.ts"),
    },
  });
  const { importBlenderScene } = await import(pathToFileURL(entry).href);
  const contract = await import(
    pathToFileURL(
      resolve(workspace, "target/integration-artifacts/client/generated.js"),
    ).href
  );
  const prefix = `https://${namespace}.ipp.invalid/`;
  const executable = resolve(
    workspace,
    "target/integration-artifacts/native",
    process.platform === "win32" ? "ipp-server.exe" : "ipp-server",
  );
  const child = spawn(
    executable,
    ["--bind", "127.0.0.1:0", "--file-root", output, "--file-prefix", prefix],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let diagnostics = "";
  child.stderr.on("data", (chunk) => {
    diagnostics = (diagnostics + chunk).slice(-8192);
  });
  const exited = new Promise((resolve) => child.once("close", resolve));
  let host;
  try {
    const url = await new Promise((resolve, reject) => {
      const lines = createInterface({ input: child.stdout });
      const timer = setTimeout(
        () => finish(new Error("Native import Host readiness timed out")),
        30000,
      );
      const finish = (error, value) => {
        clearTimeout(timer);
        lines.close();
        child.off("error", fail);
        child.off("exit", failExit);
        if (error) reject(error);
        else resolve(value);
      };
      const fail = (error) => finish(error);
      const failExit = (code) =>
        finish(
          new Error(`Native import Host exited (${code}): ${diagnostics}`),
        );
      child.once("error", fail);
      child.once("exit", failExit);
      lines.on("line", (line) => {
        try {
          const message = JSON.parse(line);
          if (message.event === "ready" && typeof message.url === "string")
            finish(null, message.url);
        } catch {
          /* Non-readiness diagnostics are not protocol messages. */
        }
      });
    });
    host = await contract.IppHostClient.connectWebSocket(url, {
      timeoutMs: 60000,
      logLevel: "error",
    });
    const assetName = (source) => {
      const match = /^\/assets\/([A-Za-z0-9_-]+(?:\/[A-Za-z0-9_-]+)*)$/.exec(
        source,
      );
      if (!match) throw new Error(`Invalid exported source ${source}`);
      return match[1];
    };
    return await importBlenderScene(
      host,
      contract,
      source,
      {
        resolve: (source) => prefix + assetName(source),
        read: (source) => `${origin}/source/${assetName(source)}`,
        publishAnimation: async (bytes) => {
          const response = await fetch(`${origin}/animation`, {
            method: "POST",
            body: bytes,
          });
          if (!response.ok) throw new Error("Animation publication failed");
          return prefix + (await response.text());
        },
      },
      { symbolicId: namespace, clipsOnly },
    );
  } finally {
    try {
      await host?.close();
    } finally {
      if (child.pid && child.exitCode === null && child.signalCode === null)
        child.kill("SIGTERM");
      const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
      try {
        await exited;
      } finally {
        clearTimeout(timer);
      }
    }
  }
}
