import assert from "node:assert/strict";

export interface TerminalDriver {
  focus(): Promise<void>;
  type(text: string): Promise<void>;
  key(key: string): Promise<void>;
  paste(text: string): Promise<void>;
  compose(text: string): Promise<void>;
  text(): Promise<string>;
  paintIdentities(): Promise<string[]>;
  capture(): Promise<{
    red: number;
    bright: number;
    background: number;
    sequence: bigint;
  }>;
  resize(width: number, height: number): Promise<void>;
  removeTarget(): Promise<void>;
  checkErrors(): Promise<string[]>;
  unfocused(): Promise<void>;
}

export async function terminalScenario(driver: TerminalDriver) {
  await driver.focus();
  await driver.type("printf '\\033[31mIPP_PROTOCOL_READY\\033[0m\\n'");
  await driver.key("Enter");
  await eventually(
    () => driver.text(),
    (value) => value.split("\n").includes("IPP_PROTOCOL_READY"),
  );
  const paint = await driver.capture();
  assert.ok(paint.red > 100);
  assert.ok(paint.bright > 100);
  assert.ok(paint.background > 720 * 384 * 0.8);
  const rows = await driver.paintIdentities();
  await driver.type("echo discarded");
  await eventually(
    () => driver.text(),
    (value) => value.includes("echodiscarded"),
  );
  assert.deepEqual(
    await driver.paintIdentities(),
    rows,
    "typing retains row and glyph entities",
  );
  await driver.key("Control+c");
  await driver.type("echo CONTROL_OK");
  await driver.key("Enter");
  await eventually(
    () => driver.text(),
    (value) => value.split("\n").includes("CONTROL_OK"),
  );
  await driver.paste("printf 'PASTE_OK\\n'");
  await driver.key("Enter");
  await eventually(
    () => driver.text(),
    (value) => value.split("\n").includes("PASTE_OK"),
  );
  await driver.type("echo ");
  await driver.compose("COMPOSE_OK");
  await driver.key("Enter");
  await eventually(
    () => driver.text(),
    (value) => value.split("\n").includes("COMPOSE_OK"),
  );
  await driver.resize(360, 192);
  // Resize is consumed through ordinary layout observations. Ghostty wraps the
  // following output at 37 columns (360 / 9.72), independently of the old 74.
  await driver.type("printf '1234567890123456789012345678901234567890\\n'");
  await driver.key("Enter");
  await eventually(
    () => driver.text(),
    (value) =>
      value.split("\n").includes("1234567890123456789012345678901234567"),
  );
  assert.ok((await driver.capture()).sequence > paint.sequence);
  await driver.key("Control+Shift+Escape");
  await driver.unfocused();
  assert.deepEqual(await driver.checkErrors(), []);
  await driver.removeTarget();
  await eventually(
    () => driver.text(),
    (value) => value === "",
  );
}

async function eventually(
  read: () => Promise<string>,
  matches: (value: string) => boolean,
) {
  let value = "";
  for (const deadline = Date.now() + 15000; Date.now() < deadline; ) {
    value = await read();
    if (matches(value)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.fail(`Terminal content did not converge:\n${value}`);
}
