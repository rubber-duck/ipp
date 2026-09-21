import assert from "node:assert/strict";
import test from "node:test";
import type { GuiObservationBatch, GuiTextFocusState } from "@ipp/client";
import {
  clampUtf8Offset,
  createTextBridgeModel,
  observeTextBridgeBatch,
  selectedCommittedText,
  utf16UnitsToUtf8Bytes,
  utf8BytesToUtf16Units,
  viewportToBridgeOffset,
} from "../src/gui/text-bridge.js";

function focus(overrides: Partial<GuiTextFocusState> = {}): GuiTextFocusState {
  return {
    session: 7n,
    contextGeneration: 2n,
    focusGeneration: 3n,
    entity: 11n,
    rootIncarnation: 5n,
    node: 13,
    lifetime: 17,
    revision: 19,
    text: "a😀b",
    selectionStart: 1,
    selectionEnd: 5,
    ...overrides,
  };
}

function batch(textFocus?: GuiTextFocusState | null): GuiObservationBatch {
  return {
    effects: [],
    ...(textFocus === undefined ? {} : { textFocus }),
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
  assert.equal(observeTextBridgeBatch(model, batch(focus())), true);
  assert.deepEqual(model.committed(), {
    text: "a😀b",
    caretUtf8: 5,
    anchorUtf8: 1,
  });
  assert.notEqual(model.token(), before);
  assert.equal(observeTextBridgeBatch(model, batch(focus())), false);
});

test("focus and context generations fence replacements at the same node", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  const original = model.token();
  model.observe(focus({ focusGeneration: 4n }));
  assert.notEqual(model.token(), original);
  const refocused = model.token();
  model.observe(focus({ contextGeneration: 3n, focusGeneration: 1n }));
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
    text: "a😀b",
    caretUtf8: 6,
    anchorUtf8: 5,
  });
  assert.equal(model.token(), selected);
  assert.equal(
    model.observe(
      focus({ composition: { text: "é", caretStart: 2, caretEnd: 2 } }),
    ),
    true,
  );
  assert.deepEqual(model.committed(), {
    text: "aéb",
    caretUtf8: 3,
    anchorUtf8: 3,
    composing: true,
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
    text: "a😀b",
    caretUtf8: 6,
    anchorUtf8: 0,
  });
  assert.equal(
    model.observe(focus({ selectionStart: 0, selectionEnd: 6 })),
    true,
  );
});

test("foreign sessions are ignored and explicit null clears focus", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  assert.equal(model.observe(focus({ session: 8n, text: "wrong" })), false);
  assert.equal(model.committed()?.text, "a😀b");
  assert.equal(observeTextBridgeBatch(model, batch(null)), true);
  assert.equal(model.committed(), null);
});

test("ordinary effects never infer or clear native text focus", () => {
  const model = createTextBridgeModel(7n);
  model.observe(focus());
  assert.equal(
    observeTextBridgeBatch(model, {
      effects: [
        {
          kind: "controlCommitted",
          entity: 99n,
          rootIncarnation: 1n,
          node: 1,
          lifetime: 1,
          value: { kind: "bool", value: true },
          revision: 1,
        },
      ],
    }),
    false,
  );
  assert.equal(model.committed()?.text, "a😀b");
});
