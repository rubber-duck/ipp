/** IME composition bridge mapping and session coverage; headless fakes only. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  attachImeBridge,
  createImeBridge,
  mapCompositionEnd,
  mapCompositionUpdate,
  shouldSkipBeforeInput,
  utf8ByteLength,
  type ImeEventTarget,
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

interface FakeTarget {
  listeners: Map<string, Set<(event: never) => void>>;
  addEventListener(type: string, listener: (event: never) => void): void;
  removeEventListener(type: string, listener: (event: never) => void): void;
  dispatch(type: string, event: Record<string, unknown>): void;
}

function fakeTarget(): FakeTarget {
  const listeners = new Map<string, Set<(event: never) => void>>();
  return {
    listeners,
    addEventListener(type, listener) {
      let group = listeners.get(type);
      if (!group) listeners.set(type, (group = new Set()));
      group.add(listener);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    dispatch(type, event) {
      for (const listener of listeners.get(type) ?? []) {
        listener(event as never);
      }
    },
  };
}

test("attach wires composition events in DOM order and detaches", () => {
  const target = fakeTarget();
  const sent: BrowserGuiInputCommand[] = [];
  const detach = attachImeBridge(target as unknown as ImeEventTarget, {
    send: (command) => sent.push(command),
  });
  target.dispatch("compositionstart", { data: "" });
  target.dispatch("compositionupdate", { data: "世界" });
  target.dispatch("compositionend", { data: "世界" });
  // A cancelled session ends with empty data.
  target.dispatch("compositionstart", { data: "" });
  target.dispatch("compositionend", { data: "" });
  assert.deepEqual(sent, [
    { kind: "composition", text: "世界", caretStart: 6, caretEnd: 6 },
    { kind: "commitComposition" },
    { kind: "cancelComposition" },
  ]);
  detach();
  assert.equal(target.listeners.get("compositionstart")?.size ?? 0, 0);
  assert.equal(target.listeners.get("compositionupdate")?.size ?? 0, 0);
  assert.equal(target.listeners.get("compositionend")?.size ?? 0, 0);
});
