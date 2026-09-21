/** Soft-keyboard trusted-gesture policy coverage; headless fakes only. */
import assert from "node:assert/strict";
import test from "node:test";
import {
  dismissSoftKeyboard,
  isTrustedSoftKeyboardTrigger,
  requestSoftKeyboard,
  resolveVirtualKeyboard,
  shouldHideSoftKeyboardOnBlur,
  shouldShowSoftKeyboard,
  type SoftKeyboardRequest,
  type VirtualKeyboardLike,
} from "../src/gui/soft-keyboard.js";

function request(
  trigger: SoftKeyboardRequest["trigger"],
  isTrusted: boolean,
): SoftKeyboardRequest {
  return { trigger, isTrusted };
}

function keyboardFake(hooks: {
  onShow?: () => void;
  onHide?: () => void;
  failShow?: boolean;
  failHide?: boolean;
}): VirtualKeyboardLike & { shows: number; hides: number } {
  const fake = {
    shows: 0,
    hides: 0,
    show: async () => {
      fake.shows++;
      hooks.onShow?.();
      if (hooks.failShow) throw new Error("show denied");
    },
    hide: async () => {
      fake.hides++;
      hooks.onHide?.();
      if (hooks.failHide) throw new Error("hide denied");
    },
  };
  return fake;
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

test("blur always allows dismissal", () => {
  assert.equal(shouldHideSoftKeyboardOnBlur(), true);
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

test("trusted tap shows the keyboard", async () => {
  const keyboard = keyboardFake({});
  assert.equal(
    await requestSoftKeyboard(keyboard, request("tap", true)),
    "shown",
  );
  assert.equal(keyboard.shows, 1);
});

test("untrusted and programmatic requests skip without touching show", async () => {
  const keyboard = keyboardFake({});
  assert.equal(
    await requestSoftKeyboard(keyboard, request("tap", false)),
    "skipped",
  );
  assert.equal(
    await requestSoftKeyboard(keyboard, request("programmatic", true)),
    "skipped",
  );
  assert.equal(keyboard.shows, 0);
});

test("missing keyboard is unavailable and show failures report", async () => {
  assert.equal(
    await requestSoftKeyboard(undefined, request("tap", true)),
    "unavailable",
  );
  const errors: Error[] = [];
  const keyboard = keyboardFake({ failShow: true });
  assert.equal(
    await requestSoftKeyboard(keyboard, request("tap", true), {
      onError: (error) => errors.push(error),
    }),
    "failed",
  );
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /show denied/);
});

test("dismiss hides, degrades without a keyboard, and reports failures", async () => {
  const keyboard = keyboardFake({});
  assert.equal(await dismissSoftKeyboard(keyboard), "hidden");
  assert.equal(keyboard.hides, 1);
  assert.equal(await dismissSoftKeyboard(undefined), "unavailable");
  const errors: Error[] = [];
  const failing = keyboardFake({ failHide: true });
  assert.equal(
    await dismissSoftKeyboard(failing, {
      onError: (error) => errors.push(error),
    }),
    "failed",
  );
  assert.equal(errors.length, 1);
});
