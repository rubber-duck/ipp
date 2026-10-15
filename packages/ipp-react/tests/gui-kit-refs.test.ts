import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, StrictMode } from "react";
import { Autocomplete, ColorPicker, LabelledSlider } from "../src/gui-kit.js";
import { Entity } from "../src/components.js";
import { useComposedControlRef } from "../src/gui/control-refs.js";
import type {
  GuiControlHandle,
  GuiControlRef,
} from "../src/gui/control-ref.js";
import { render, settle } from "./gui-kit-support.js";

for (const [name, component] of [
  [
    "scalar",
    (ref: GuiControlRef) =>
      h(LabelledSlider, { id: "control", min: 0, max: 100, ref }),
  ],
  ["color", (ref: GuiControlRef) => h(ColorPicker, { id: "control", ref })],
  [
    "text",
    (ref: GuiControlRef) =>
      h(Autocomplete, { id: "control", suggestions: [], ref }),
  ],
] as const) {
  test(`${name} composed refs replace the observer and retain callback cleanup`, async () => {
    const calls: string[] = [];
    let first: GuiControlHandle | null = null;
    let second: GuiControlHandle | null = null;
    const a: GuiControlRef = (handle) => {
      assert.ok(handle, "cleanup replaces a null callback");
      first = handle;
      calls.push("a attach");
      return () => {
        calls.push("a cleanup");
        first = null;
      };
    };
    const b: GuiControlRef = (handle) => {
      assert.ok(handle);
      second = handle;
      calls.push("b attach");
      return () => {
        calls.push("b cleanup");
        second = null;
      };
    };
    const scene = (ref: GuiControlRef) => h(StrictMode, null, component(ref));
    const { root, draw } = await render(scene(a));
    await settle(root);
    assert.ok(first);
    const target = first;
    await draw(scene(b));
    await settle(root);
    assert.equal(first, null);
    assert.equal(
      second,
      target,
      "changing an observer preserves the native handle",
    );
    assert.deepEqual(calls.slice(-3), ["a attach", "a cleanup", "b attach"]);
    await root.unmount();
    assert.equal(second, null);
    assert.equal(calls.at(-1), "b cleanup");
  });

  test(`${name} composed object refs clear the old ref on replacement`, async () => {
    const a = { current: null as GuiControlHandle | null };
    const b = { current: null as GuiControlHandle | null };
    const { root, draw } = await render(component(a));
    await settle(root);
    const target = a.current;
    assert.ok(target);
    await draw(component(b));
    await settle(root);
    assert.equal(a.current, null);
    assert.equal(b.current, target);
    await root.unmount();
    assert.equal(b.current, null);
  });
}

test("a failed composed attachment releases both observers and preserves the error", async () => {
  const own = { current: null as GuiControlHandle | null };
  const published = { current: null as GuiControlRef };
  const failure = new Error("attachment failed");
  let cleaned = 0;
  const outer: GuiControlRef = (handle) => {
    assert.ok(handle);
    return () => {
      cleaned++;
    };
  };
  function Probe() {
    published.current = useComposedControlRef(own, outer, () => {
      throw failure;
    });
    return h(Entity, { id: "probe" });
  }
  const { root } = await render(h(Probe));
  assert.equal(typeof published.current, "function");
  const attach = published.current as (handle: GuiControlHandle | null) => void;
  assert.throws(
    () => attach({} as GuiControlHandle),
    (error) => error === failure,
  );
  assert.equal(own.current, null);
  assert.equal(cleaned, 1);
  attach(null);
  assert.equal(cleaned, 1, "core failure cleanup is idempotent");
  await root.unmount();
});

test("a reentrant replacement releases the earlier callback after it returns cleanup", async () => {
  const own = { current: null as GuiControlHandle | null };
  const published = { current: null as GuiControlRef };
  const first = {} as GuiControlHandle;
  const second = {} as GuiControlHandle;
  const cleaned: GuiControlHandle[] = [];
  const outer: GuiControlRef = (handle) => {
    assert.ok(handle);
    if (handle === first) {
      const attach = published.current as (handle: GuiControlHandle) => void;
      attach(second);
    }
    return () => {
      cleaned.push(handle);
    };
  };
  function Probe() {
    published.current = useComposedControlRef(own, outer);
    return h(Entity, { id: "probe" });
  }
  const { root } = await render(h(Probe));
  const attach = published.current as (handle: GuiControlHandle | null) => void;
  attach(first);
  assert.equal(own.current, second);
  assert.deepEqual(cleaned, [first]);
  attach(null);
  assert.equal(own.current, null);
  assert.deepEqual(cleaned, [first, second]);
  await root.unmount();
});
