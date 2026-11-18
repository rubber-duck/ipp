import assert from "node:assert/strict";
import test from "node:test";
import {
  normalizeTokenDocument,
  serializeTokenDocument,
  importTokenDocument,
  replaceTokenSelection,
  type TokenDocument,
} from "../src/gui-kit/token-document.js";
import { validateGuiFile } from "../src/gui-kit/file-input.js";

test("token ranges are grapheme-safe and token identity survives an authorized clipboard round trip", () => {
  const original: TokenDocument = [
    { kind: "text", text: "e\u0301" },
    { kind: "token", id: "x", label: "" },
    { kind: "text", text: "a" },
  ];
  assert.deepEqual(replaceTokenSelection(original, [2, 0], []), {
    value: [{ kind: "text", text: "a" }],
    selection: [0, 0],
  });
  const copied = serializeTokenDocument(original);
  assert.equal(copied.text, "e\u0301a");
  assert.deepEqual(
    importTokenDocument(copied, (token) => token.id === "x"),
    original,
  );
  assert.deepEqual(
    importTokenDocument(copied, () => false),
    [{ kind: "text", text: "e\u0301a" }],
  );
  assert.deepEqual(
    importTokenDocument({ ...copied, text: "original" }, () => true),
    [{ kind: "text", text: "original" }],
  );
  const emptyLabel = [{ kind: "token", id: "unlabelled", label: "" }] as const;
  const emptyCopy = serializeTokenDocument(emptyLabel);
  assert.equal(emptyCopy.text, "");
  assert.deepEqual(
    importTokenDocument(emptyCopy, () => true),
    emptyLabel,
  );
  assert.deepEqual(
    importTokenDocument({ text: "" }, () => true),
    [],
  );
  assert.throws(
    () =>
      replaceTokenSelection(
        original,
        [0, 0],
        [{ kind: "token", id: "x", label: "second" }],
      ),
    /unique/,
  );
  assert.deepEqual(
    normalizeTokenDocument([
      { kind: "text", text: "" },
      { kind: "text", text: "a" },
      { kind: "text", text: "b" },
    ]),
    [{ kind: "text", text: "ab" }],
  );
});

test("file descriptor policy validates extensions, MIME family and size before admission", () => {
  const file = { id: "1", name: "FILE.TXT", size: 8, type: "text/plain" };
  assert.equal(validateGuiFile(file, [".txt"], 8), undefined);
  assert.equal(validateGuiFile(file, ["text/*"], 8), undefined);
  assert.equal(validateGuiFile(file, ["image/*"], 8), "type");
  assert.equal(validateGuiFile(file, [], 7), "size");
  assert.equal(validateGuiFile({ ...file, size: -1 }, [], 8), "metadata");
});
