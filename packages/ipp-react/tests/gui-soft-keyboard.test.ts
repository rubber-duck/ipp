/** Soft-keyboard trusted-gesture policy coverage; headless fakes only. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  isTrustedSoftKeyboardTrigger,
  resolveVirtualKeyboard,
  shouldShowSoftKeyboard,
  type SoftKeyboardRequest,
} from "../src/gui/platform/soft-keyboard.js";

function request(
  trigger: SoftKeyboardRequest["trigger"],
  isTrusted: boolean,
): SoftKeyboardRequest {
  return { trigger, isTrusted };
}

test("only pointer gestures are trusted trigger kinds", () => {
  assert.equal(isTrustedSoftKeyboardTrigger("pointerDown"), true);
  assert.equal(isTrustedSoftKeyboardTrigger("tap"), true);
  assert.equal(isTrustedSoftKeyboardTrigger("key"), false);
  assert.equal(isTrustedSoftKeyboardTrigger("programmatic"), false);
});

test("policy allows trusted gestures and denies the rest", () => {
  assert.equal(shouldShowSoftKeyboard(request("tap", true)), true);
  assert.equal(shouldShowSoftKeyboard(request("pointerDown", true)), true);
  assert.equal(shouldShowSoftKeyboard(request("tap", false)), false);
  assert.equal(shouldShowSoftKeyboard(request("programmatic", true)), false);
  assert.equal(shouldShowSoftKeyboard(request("programmatic", false)), false);
  assert.equal(shouldShowSoftKeyboard(request("key", true)), false);
  assert.equal(
    shouldShowSoftKeyboard(request("key", true), { allowKeyTrigger: true }),
    true,
  );
  assert.equal(
    shouldShowSoftKeyboard(request("key", false), { allowKeyTrigger: true }),
    false,
  );
});

test("resolver stays undefined without a virtual keyboard", () => {
  assert.equal(resolveVirtualKeyboard(undefined), undefined);
  assert.equal(resolveVirtualKeyboard({}), undefined);
  assert.equal(
    resolveVirtualKeyboard({ virtualKeyboard: { show: async () => {} } }),
    undefined,
  );
  const keyboard = resolveVirtualKeyboard({
    virtualKeyboard: {
      show: async () => undefined,
      hide: async () => undefined,
    },
  });
  assert.ok(keyboard !== undefined);
});
