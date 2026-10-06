import assert from "node:assert/strict";
import test from "node:test";
import type { GuiNativeTextState } from "@ipp/client";
import {
  clampUtf8Offset,
  createTextBridgeModel,
  selectedCommittedText,
  stampTextFence,
  textFenceOf,
  utf16UnitsToUtf8Bytes,
  utf8BytesToUtf16Units,
  viewportToBridgeOffset,
} from "../src/gui/platform/text-bridge.js";

const FENCE = {
  target: {
    world: { id: 1n, incarnation: 2n },
    entity: 11n,
    component: 28,
    incarnation: 5n,
  },
  generation: 3n,
};

function focus(
  overrides: Partial<GuiNativeTextState> = {},
): GuiNativeTextState {
  return {
    fence: FENCE,
    text: "a😀b",
    masked: false,
    selectionStart: 1,
    selectionEnd: 5,
    ...overrides,
  };
}

test("conversions snap to code-point boundaries", () => {
  assert.equal(utf16UnitsToUtf8Bytes("a😀b", 1), 1);
  assert.equal(utf16UnitsToUtf8Bytes("a😀b", 2), 1);
  assert.equal(utf16UnitsToUtf8Bytes("a😀b", 3), 5);
  assert.equal(utf8BytesToUtf16Units("a😀b", 2), 1);
  assert.equal(utf8BytesToUtf16Units("a😀b", 5), 3);
  assert.equal(clampUtf8Offset("a😀b", 2), 1);
  assert.equal(clampUtf8Offset("a😀b", 99), 6);
});

test("placement maps normalized viewport points to container CSS pixels", () => {
  const canvas = { left: 10, top: 20, width: 200, height: 100 };
  const container = { left: 2, top: 4 };
  assert.deepEqual(viewportToBridgeOffset([0.5, 0.25], canvas, container), {
    x: 108,
    y: 41,
  });
});

test("clipboard selection slices authoritative UTF-8 offsets", () => {
  assert.equal(
    selectedCommittedText({
      text: "a😀b",
      caretUtf8: 5,
      anchorUtf8: 1,
    }),
    "😀",
  );
  assert.equal(
    selectedCommittedText({ text: "hello", caretUtf8: 2, anchorUtf8: 2 }),
    null,
  );
  assert.equal(
    selectedCommittedText({
      text: "provisional",
      caretUtf8: 11,
      anchorUtf8: 0,
      composing: true,
    }),
    null,
  );
});

test("authoritative focus supplies text and UTF-8 selection", () => {
  const model = createTextBridgeModel(7n);
  const before = model.token();
  assert.equal(model.observe(focus()), true);
  assert.deepEqual(model.committed(), {
    masked: false,
    text: "a😀b",
    caretUtf8: 5,
    anchorUtf8: 1,
    fence: FENCE,
  });
  assert.notEqual(model.token(), before);
  assert.equal(model.observe(focus()), false);
});

test("edit generation and component incarnation fence replacements at the same entity", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  const original = model.token();
  model.observe(focus({ fence: { ...FENCE, generation: 4n } }));
  assert.notEqual(model.token(), original);
  const refocused = model.token();
  model.observe(
    focus({
      fence: { ...FENCE, target: { ...FENCE.target, incarnation: 6n } },
    }),
  );
  assert.notEqual(model.token(), refocused);
});

test("selection-only and composition updates reconcile without a text commit", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  const selected = model.token();
  assert.equal(
    model.observe(focus({ selectionStart: 5, selectionEnd: 6 })),
    true,
  );
  assert.deepEqual(model.committed(), {
    masked: false,
    text: "a😀b",
    caretUtf8: 6,
    anchorUtf8: 5,
    fence: FENCE,
  });
  assert.equal(model.token(), selected);
  assert.equal(
    model.observe(
      focus({ composition: { text: "é", caretStart: 2, caretEnd: 2 } }),
    ),
    true,
  );
  assert.deepEqual(model.committed(), {
    masked: false,
    text: "aéb",
    caretUtf8: 3,
    anchorUtf8: 3,
    composing: true,
    fence: FENCE,
  });
});

test("local activations and selections fence delayed clipboard work", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  const focused = model.token();
  model.noteActivation();
  assert.notEqual(model.token(), focused);
  const activated = model.token();
  model.noteLocalSelection(0, 6);
  assert.notEqual(model.token(), activated);
  assert.deepEqual(model.committed(), {
    masked: false,
    text: "a😀b",
    caretUtf8: 6,
    anchorUtf8: 0,
    fence: FENCE,
  });
  assert.equal(
    model.observe(focus({ selectionStart: 0, selectionEnd: 6 })),
    true,
  );
});

test("undefined state cannot infer focus and explicit null clears it", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  assert.equal(model.observe(undefined), false);
  assert.equal(model.committed()?.text, "a😀b");
  assert.equal(model.observe(null), true);
  assert.equal(model.committed(), null);
});

test("text edits stamp the fence of the state they were made against", () => {
  assert.deepEqual(textFenceOf(focus()), FENCE);
  assert.deepEqual(stampTextFence({ kind: "text", text: "x" }, FENCE), {
    kind: "text",
    text: "x",
    fence: FENCE,
  });
  assert.deepEqual(
    stampTextFence({ kind: "selection", start: 0, end: 1 }, FENCE),
    { kind: "selection", start: 0, end: 1, fence: FENCE },
  );
  assert.deepEqual(stampTextFence({ kind: "commitComposition" }, FENCE), {
    kind: "commitComposition",
    fence: FENCE,
  });
  const key = { kind: "key", key: "backspace" } as const;
  assert.deepEqual(stampTextFence(key, FENCE), { ...key, fence: FENCE });
  const text = { kind: "text", text: "x" } as const;
  assert.equal(stampTextFence(text, undefined), text);
});

test("a replaced focused text moves the fence and resynchronizes", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  const before = model.token();
  assert.equal(
    model.observe(
      focus({
        fence: { ...FENCE, generation: 4n },
        text: "reset",
      }),
    ),
    true,
  );
  assert.notEqual(model.token(), before);
  assert.equal(model.committed()?.text, "reset");
  assert.deepEqual(model.committed()?.fence, {
    ...FENCE,
    generation: 4n,
  });
});

test("reveal changes presentation without changing the native editing identity", () => {
  const model = createTextBridgeModel(1n);
  model.observe(focus({ masked: true }));
  const masked = model.committed();
  assert.equal(masked?.masked, true);
  assert.equal(masked?.text, "a😀b");
  const token = model.token();
  assert.equal(model.observe(focus({ masked: false })), true);
  assert.equal(model.token(), token);
  assert.deepEqual(model.committed(), { ...masked, masked: false });
});
