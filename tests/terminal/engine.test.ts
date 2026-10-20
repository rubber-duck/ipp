import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { TerminalEngine } from "../../packages/ipp-terminal/src/engine.js";

const binary = async (specifier: string) =>
  new Uint8Array(await readFile(new URL(import.meta.resolve(specifier))));

test("real BashKit and Ghostty preserve shell bytes, ANSI state, replies, resize and exit", async () => {
  const engine = await TerminalEngine.create({
    wasm: await binary("@gespenst/core/ghostty-vt.wasm"),
    callbacksWasm: await binary("@gespenst/core/ghostty-callbacks.wasm"),
    bashkit: await binary("@everruns/bashkit-wasm/bashkit_wasm_bg.wasm"),
    rows: 12,
    cols: 80,
    cellWidth: 9,
    cellHeight: 18,
  });
  const command = async (text: string) => {
    engine.input({ kind: "text", text });
    engine.input({
      kind: "key",
      key: "Enter",
      code: "Enter",
      modifiers: 0,
      pressed: true,
      repeat: false,
    });
    await engine.run();
    return engine.terminal.viewport();
  };
  try {
    await engine.run();
    let screen = await command("printf '\\033[31mIPP_READY\\033[0m\\n'");
    const row = screen.viewportRows.find(
      (row) => row.text.trim() === "IPP_READY",
    );
    assert.ok(row, screen.viewportRows.map((row) => row.text).join("\n"));
    assert.ok(
      row.cells.some(
        (cell) => cell.foreground && cell.foreground.r > cell.foreground.g + 60,
      ),
    );
    screen = await command("printf '\\033[?1049hALT_SCREEN'");
    assert.ok(
      screen.viewportRows.some((row) => row.text.includes("ALT_SCREEN")),
    );
    await command("printf '\\033[?1049l'");
    engine.resize(48, 8);
    await engine.run();
    assert.equal(engine.terminal.viewport().cols, 48);
    assert.equal(engine.terminal.viewport().rows, 8);
    // Device status query produces a reply, which must drain without spinning.
    await command("printf '\\033[6n'");
    engine.input({
      kind: "key",
      key: "c",
      code: "KeyC",
      modifiers: 2,
      pressed: true,
      repeat: false,
    });
    await engine.run();
    await command("exit 7");
    assert.equal(engine.exitCode, 7);
  } finally {
    engine.dispose();
  }
});
