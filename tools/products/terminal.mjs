/** Hostable browser terminal and maintained real shell/protocol/rendering exercises. */
import { execFileSync } from "node:child_process";
import { cp, copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { artifact, bundleBrowser, workspace } from "../build/helpers.mjs";
const output =
  process.env.IPP_BUILD_OUTPUT ?? resolve(workspace, "target/terminal-build");
execFileSync(
  process.execPath,
  [
    "node_modules/typescript/bin/tsc",
    "--project",
    "tests/terminal/tsconfig.json",
  ],
  { cwd: workspace, stdio: "inherit" },
);
await mkdir(output, { recursive: true });
for (const [source, name] of [
  ["main.ts", "main.js"],
  ["application.ts", "application.js"],
  ["terminal-worker.ts", "terminal-worker.js"],
])
  await bundleBrowser(
    `examples/browser-terminal/${source}`,
    resolve(output, name),
    "production",
    { external: ["node:*"] },
  );
await copyFile(
  resolve(workspace, "examples/browser-terminal/index.html"),
  resolve(output, "index.html"),
);
await cp(
  resolve(workspace, "target/browser-build/render"),
  resolve(output, "runtime"),
  { recursive: true },
);
for (const [source, name] of [
  ["@gespenst/core/ghostty-vt.wasm", "ghostty-vt.wasm"],
  ["@gespenst/core/ghostty-callbacks.wasm", "ghostty-callbacks.wasm"],
  ["@everruns/bashkit-wasm/bashkit_wasm_bg.wasm", "bashkit.wasm"],
])
  await copyFile(new URL(import.meta.resolve(source)), resolve(output, name));
await copyFile(
  resolve(workspace, "target/font-assets/shure-tech-mono.ippf"),
  resolve(output, "font.ippf"),
);
await copyFile(
  resolve(workspace, "target/font-assets/shure-tech-mono.json"),
  resolve(output, "terminal-font.json"),
);
for (const source of [
  "engine.test.ts",
  "terminal.browser.test.ts",
  "cockpit.browser.test.ts",
])
  await bundleBrowser(
    `tests/terminal/${source}`,
    resolve(output, source.replace(/\.ts$/, ".js")),
    "production",
    {
      platform: "node",
      conditions: ["node"],
      packages: "external",
      define: {},
      minify: false,
    },
  );
const notices = await Promise.all(
  [
    "node_modules/@gespenst/core/LICENSE",
    "node_modules/@gespenst/core/THIRD_PARTY_NOTICES.md",
    "licences/terminal/BashKit-MIT.txt",
    "licences/fonts/ShureTechMono-OFL.txt",
    "licences/fonts/ShureTechMono-Nerd-Font-README.md",
  ].map(
    async (path) =>
      `${path}\n\n${await readFile(resolve(workspace, path), "utf8")}`,
  ),
);
await writeFile(
  resolve(output, "THIRD_PARTY_NOTICES.txt"),
  notices.join("\n\n"),
);
const artifacts = await Promise.all(
  [
    "terminal-worker.js",
    "ghostty-vt.wasm",
    "ghostty-callbacks.wasm",
    "bashkit.wasm",
  ].map((name) => artifact(resolve(output, name))),
);
if (
  artifacts.find((a) => a.path.endsWith("ghostty-vt.wasm")).sha256 !==
  "b3a643550f2390fb90b541ec9229c65963fb0bdb9af40648059477bfa2cb79f8"
)
  throw new Error("Pinned Ghostty Wasm checksum changed");
await writeFile(
  resolve(output, "build-report.json"),
  `${JSON.stringify({ scope: "BashKit/Ghostty over standard IPP MessagePort protocol", artifacts }, null, 2)}\n`,
);
