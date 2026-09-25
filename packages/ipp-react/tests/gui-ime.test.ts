/** IME composition bridge mapping and session coverage; headless fakes only. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  isComposingKeyEvent,
  createImeBridge,
  mapCompositionEnd,
  mapCompositionUpdate,
  shouldSkipBeforeInput,
  utf8ByteLength,
} from "../src/gui/ime.js";
import type { BrowserGuiInputCommand } from "../src/gui/input.js";

test("utf8 caret collapses at the encoded end", () => {
  assert.equal(utf8ByteLength("k"), 1);
  assert.equal(utf8ByteLength("世界"), 6);
  assert.equal(utf8ByteLength("😀"), 4);
});

test("compositionupdate maps to a provisional with an end-collapsed caret", () => {
  assert.deepEqual(mapCompositionUpdate("世界"), {
    kind: "composition",
    text: "世界",
    caretStart: 6,
    caretEnd: 6,
  });
  assert.deepEqual(mapCompositionUpdate("k"), {
    kind: "composition",
    text: "k",
    caretStart: 1,
    caretEnd: 1,
  });
});

test("empty compositionupdate payloads map to no command", () => {
  assert.equal(mapCompositionUpdate(""), null);
  assert.equal(mapCompositionUpdate(null), null);
  assert.equal(mapCompositionUpdate(undefined), null);
});

test("compositionend commits nonempty sessions and cancels empty ones", () => {
  assert.deepEqual(mapCompositionEnd("世界"), { kind: "commitComposition" });
  assert.deepEqual(mapCompositionEnd(""), { kind: "cancelComposition" });
  assert.deepEqual(mapCompositionEnd(null), { kind: "cancelComposition" });
  assert.deepEqual(mapCompositionEnd(undefined), {
    kind: "cancelComposition",
  });
});

test("beforeinput skips composition payloads exactly once", () => {
  assert.equal(shouldSkipBeforeInput("insertCompositionText"), true);
  assert.equal(shouldSkipBeforeInput("insertFromComposition"), true);
  assert.equal(shouldSkipBeforeInput("insertText"), false);
  assert.equal(shouldSkipBeforeInput("insertFromPaste"), false);
  assert.equal(shouldSkipBeforeInput("deleteContentBackward"), false);
});

test("bridge forwards update then commit and closes the session", () => {
  const sent: BrowserGuiInputCommand[] = [];
  const bridge = createImeBridge({ send: (command) => sent.push(command) });
  bridge.compositionStart();
  assert.equal(bridge.isComposing(), true);
  bridge.compositionUpdate("世");
  bridge.compositionUpdate("世界");
  bridge.compositionEnd("世界");
  assert.equal(bridge.isComposing(), false);
  assert.deepEqual(sent, [
    { kind: "composition", text: "世", caretStart: 3, caretEnd: 3 },
    { kind: "composition", text: "世界", caretStart: 6, caretEnd: 6 },
    { kind: "commitComposition" },
  ]);
});

test("terminal composition data replaces a stale final provisional once", () => {
  const sent: BrowserGuiInputCommand[] = [];
  const bridge = createImeBridge({ send: (command) => sent.push(command) });
  bridge.compositionStart();
  bridge.compositionUpdate("世");
  bridge.compositionEnd("世界");
  assert.deepEqual(sent, [
    { kind: "composition", text: "世", caretStart: 3, caretEnd: 3 },
    { kind: "composition", text: "世界", caretStart: 6, caretEnd: 6 },
    { kind: "commitComposition" },
  ]);
});

test("bridge cancels empty ends and ignores empty updates", () => {
  const sent: BrowserGuiInputCommand[] = [];
  const bridge = createImeBridge({ send: (command) => sent.push(command) });
  bridge.compositionStart();
  bridge.compositionUpdate("");
  bridge.compositionEnd("");
  assert.deepEqual(sent, [{ kind: "cancelComposition" }]);
});

test("stale start cancels the open session instead of merging", () => {
  const sent: BrowserGuiInputCommand[] = [];
  const bridge = createImeBridge({ send: (command) => sent.push(command) });
  bridge.compositionStart();
  bridge.compositionUpdate("あ");
  bridge.compositionStart();
  bridge.compositionUpdate("い");
  bridge.compositionEnd("い");
  assert.deepEqual(sent, [
    { kind: "composition", text: "あ", caretStart: 3, caretEnd: 3 },
    { kind: "cancelComposition" },
    { kind: "composition", text: "い", caretStart: 3, caretEnd: 3 },
    { kind: "commitComposition" },
  ]);
});

test("blur cancels an open session and is a no-op while idle", () => {
  const sent: BrowserGuiInputCommand[] = [];
  const bridge = createImeBridge({ send: (command) => sent.push(command) });
  bridge.blur();
  assert.deepEqual(sent, []);
  bridge.compositionStart();
  bridge.compositionUpdate("あ");
  bridge.blur();
  assert.equal(bridge.isComposing(), false);
  assert.deepEqual(sent, [
    { kind: "composition", text: "あ", caretStart: 3, caretEnd: 3 },
    { kind: "cancelComposition" },
  ]);
});

test("bridge reports sink failures without breaking the session", () => {
  const errors: Error[] = [];
  let calls = 0;
  const bridge = createImeBridge(
    {
      send: () => {
        calls++;
        throw new Error("sink full");
      },
    },
    { onError: (error) => errors.push(error) },
  );
  bridge.compositionStart();
  bridge.compositionUpdate("あ");
  bridge.compositionEnd("あ");
  assert.equal(calls, 2);
  assert.equal(errors.length, 2);
  assert.match(errors[0]!.message, /sink full/);
});

test("keys during an open composition belong to the IME", () => {
  assert.equal(isComposingKeyEvent({ isComposing: true, keyCode: 13 }), true);
  assert.equal(isComposingKeyEvent({ isComposing: false, keyCode: 229 }), true);
  assert.equal(isComposingKeyEvent({ isComposing: false, keyCode: 13 }), false);
  assert.equal(isComposingKeyEvent({}), false);
});
