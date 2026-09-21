/** Clipboard bridge permission and failure coverage; headless fakes only.
 *
 * Every failure test pins the `.12` invariant: platform failure never
 * transfers text authority — no input command is sent and no DOM value
 * is treated as committed text.
 */
import assert from "node:assert/strict";
import test from "node:test";
import {
  copyFocusedTextToClipboard,
  pasteClipboardToFocusedInput,
  queryClipboardPermission,
  readClipboardText,
  resolveClipboardReader,
  resolveClipboardWriter,
  writeClipboardText,
  type ClipboardTextReader,
  type ClipboardTextWriter,
} from "../src/gui/clipboard.js";
import type { BrowserGuiInputCommand } from "../src/gui/input.js";

function readerFake(text: string, onRead?: () => void): ClipboardTextReader {
  return {
    readText: async () => {
      onRead?.();
      return text;
    },
  };
}

function writerFake(onWrite: (text: string) => void): ClipboardTextWriter {
  return {
    writeText: async (text: string) => {
      onWrite(text);
    },
  };
}

function permissionsFake(state: string): unknown {
  return {
    query: async () => ({ state }),
  };
}

test("resolvers stay undefined without platform clipboard methods", () => {
  assert.equal(resolveClipboardReader(undefined), undefined);
  assert.equal(resolveClipboardWriter(undefined), undefined);
  assert.equal(resolveClipboardReader({}), undefined);
  assert.equal(resolveClipboardWriter({ clipboard: {} }), undefined);
  const reader = resolveClipboardReader({
    clipboard: { readText: async () => "hi" },
  });
  assert.ok(reader !== undefined);
  const writer = resolveClipboardWriter({
    clipboard: { writeText: async () => undefined },
  });
  assert.ok(writer !== undefined);
});

test("permission query maps states and degrades to unknown", async () => {
  assert.equal(
    await queryClipboardPermission("clipboard-read", permissionsFake("denied")),
    "denied",
  );
  assert.equal(
    await queryClipboardPermission(
      "clipboard-write",
      permissionsFake("granted"),
    ),
    "granted",
  );
  assert.equal(
    await queryClipboardPermission("clipboard-read", undefined),
    "unknown",
  );
  assert.equal(
    await queryClipboardPermission("clipboard-read", {
      query: async () => {
        throw new Error("no permissions");
      },
    }),
    "unknown",
  );
  assert.equal(
    await queryClipboardPermission("clipboard-read", permissionsFake("weird")),
    "unknown",
  );
});

test("read returns text and passes empty through as empty", async () => {
  assert.deepEqual(await readClipboardText(readerFake("hello")), {
    ok: true,
    text: "hello",
  });
  assert.deepEqual(await readClipboardText(readerFake("")), {
    ok: true,
    text: "",
  });
});

test("denied permission short-circuits without touching the clipboard", async () => {
  let reads = 0;
  const result = await readClipboardText(
    readerFake("hello", () => {
      reads++;
    }),
    { permissions: permissionsFake("denied") },
  );
  assert.deepEqual(result, {
    ok: false,
    reason: "Clipboard read permission was denied",
  });
  assert.equal(reads, 0);
});

test("read failures carry reasons and no text", async () => {
  assert.deepEqual(
    await readClipboardText({
      readText: async () => {
        throw new Error("NotAllowedError");
      },
    }),
    { ok: false, reason: "NotAllowedError" },
  );
  assert.deepEqual(await readClipboardText(undefined), {
    ok: false,
    reason: "Clipboard is unavailable in this context",
  });
  assert.deepEqual(await readClipboardText(readerFake("x".repeat(70000))), {
    ok: false,
    reason: "Clipboard text exceeds the ingress bound",
  });
});

test("paste sends one text command for clipboard content", async () => {
  const sent: BrowserGuiInputCommand[] = [];
  const ok = await pasteClipboardToFocusedInput(
    { send: (command) => sent.push(command) },
    readerFake("pasted"),
  );
  assert.equal(ok, true);
  assert.deepEqual(sent, [{ kind: "text", text: "pasted" }]);
});

test("paste failures send nothing and report without authority", async () => {
  const denied: BrowserGuiInputCommand[] = [];
  const deniedErrors: Error[] = [];
  const deniedOk = await pasteClipboardToFocusedInput(
    { send: (command) => denied.push(command) },
    readerFake("hello"),
    {
      permissions: permissionsFake("denied"),
      onError: (error) => deniedErrors.push(error),
    },
  );
  assert.equal(deniedOk, false);
  assert.deepEqual(denied, []);
  assert.equal(deniedErrors.length, 1);

  const empty: BrowserGuiInputCommand[] = [];
  assert.equal(
    await pasteClipboardToFocusedInput(
      { send: (command) => empty.push(command) },
      readerFake(""),
    ),
    false,
  );
  assert.deepEqual(empty, []);

  const missing: BrowserGuiInputCommand[] = [];
  const missingErrors: Error[] = [];
  assert.equal(
    await pasteClipboardToFocusedInput(
      { send: (command) => missing.push(command) },
      undefined,
      { onError: (error) => missingErrors.push(error) },
    ),
    false,
  );
  assert.deepEqual(missing, []);
  assert.equal(missingErrors.length, 1);

  const throwing: BrowserGuiInputCommand[] = [];
  const throwingErrors: Error[] = [];
  assert.equal(
    await pasteClipboardToFocusedInput(
      {
        send: () => {
          throw new Error("sink full");
        },
      },
      readerFake("hello"),
      { onError: (error) => throwingErrors.push(error) },
    ),
    false,
  );
  assert.deepEqual(throwing, []);
  assert.equal(throwingErrors.length, 1);
});

test("copy writes committed text and never sends input", async () => {
  const written: string[] = [];
  const ok = await copyFocusedTextToClipboard(
    "committed",
    writerFake((text) => written.push(text)),
  );
  assert.equal(ok, true);
  assert.deepEqual(written, ["committed"]);
});

test("copy failures report without sending input", async () => {
  const errors: Error[] = [];
  assert.equal(
    await copyFocusedTextToClipboard("committed", undefined, {
      onError: (error) => errors.push(error),
    }),
    false,
  );
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /unavailable/);

  const deniedErrors: Error[] = [];
  const written: string[] = [];
  assert.equal(
    await copyFocusedTextToClipboard(
      "committed",
      writerFake((text) => written.push(text)),
      {
        permissions: permissionsFake("denied"),
        onError: (error) => deniedErrors.push(error),
      },
    ),
    false,
  );
  assert.deepEqual(written, []);
  assert.equal(deniedErrors.length, 1);

  const rejectedErrors: Error[] = [];
  assert.equal(
    await copyFocusedTextToClipboard(
      "committed",
      {
        writeText: async () => {
          throw new Error("NotAllowedError");
        },
      },
      { onError: (error) => rejectedErrors.push(error) },
    ),
    false,
  );
  assert.equal(rejectedErrors.length, 1);

  // Empty committed text copies nothing and reports nothing.
  const writtenEmpty: string[] = [];
  assert.equal(
    await copyFocusedTextToClipboard(
      "",
      writerFake((text: string) => {
        writtenEmpty.push(text);
      }),
    ),
    false,
  );
  assert.deepEqual(writtenEmpty, []);
});

test("write guards empty text and missing writers", async () => {
  assert.deepEqual(
    await writeClipboardText(
      "",
      writerFake(() => undefined),
    ),
    { ok: false, reason: "Nothing to copy" },
  );
  assert.deepEqual(await writeClipboardText("hi", undefined), {
    ok: false,
    reason: "Clipboard is unavailable in this context",
  });
});
