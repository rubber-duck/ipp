/** Browser relay mapping, ordered sink and DOM attach coverage; headless fakes only. */
import assert from "node:assert/strict";
import test from "node:test";
import type { GuiInputCommand, GuiInputRoutingOutcome } from "@ipp/client";
import {
  attachCanvasGuiInput,
  canvasRelativePoint,
  createGuiInputSink,
  domMouseButtonToGuiButton,
  keyboardKeyToGuiKey,
  toGuiInputCommand,
  wheelDeltaToLogical,
  type AttachCanvasGuiInputOptions,
  type BrowserGuiInputCommand,
} from "../src/gui/input.js";
import { createGuiUnhandledInputGate } from "../src/gui/scene-input.js";

test("button, key and geometry helpers translate platform values", () => {
  assert.equal(domMouseButtonToGuiButton(0), "primary");
  assert.equal(domMouseButtonToGuiButton(2), "secondary");
  assert.equal(domMouseButtonToGuiButton(1), "auxiliary");
  assert.equal(domMouseButtonToGuiButton(4), null);
  assert.equal(keyboardKeyToGuiKey("Tab"), "tab");
  assert.equal(keyboardKeyToGuiKey(" "), "space");
  assert.equal(keyboardKeyToGuiKey("ArrowDown"), "down");
  assert.equal(keyboardKeyToGuiKey("a"), null);
  assert.equal(keyboardKeyToGuiKey("Enter"), "enter");
  assert.deepEqual(canvasRelativePoint(15, 25, { left: 10, top: 20 }), [5, 5]);
  assert.deepEqual(wheelDeltaToLogical(3, -4, 0), [3, -4]);
  assert.deepEqual(wheelDeltaToLogical(3, -4, 1), [48, -64]);
});

test("browser commands map to wire inputs without panel scope", () => {
  assert.deepEqual(
    toGuiInputCommand({
      kind: "pointerDown",
      pointer: 7,
      position: [2, 1.5],
      button: "primary",
      blockers: [{ entity: 42n, distance: 0.5 }],
      panelDistance: 3.25,
    }),
    {
      kind: "pointerDown",
      pointer: 7,
      position: [2, 1.5],
      button: "primary",
      blockers: [{ entity: 42n, distance: 0.5 }],
      panelDistance: 3.25,
    },
  );
  const move = toGuiInputCommand({
    kind: "pointerMove",
    pointer: 7,
    position: [2, 1.5],
  });
  assert.equal(move.kind, "pointerMove");
  assert.equal(Object.hasOwn(move, "blockers"), false);
  assert.equal(Object.hasOwn(move, "panelDistance"), false);
  assert.equal(Object.hasOwn(move, "panel"), false);
  assert.deepEqual(toGuiInputCommand({ kind: "pointerCancel", pointer: 7 }), {
    kind: "pointerCancel",
    pointer: 7,
  });
  assert.deepEqual(
    toGuiInputCommand({ kind: "scroll", position: [1, 1], delta: [0, -4] }),
    { kind: "scroll", position: [1, 1], delta: [0, -4] },
  );
  assert.deepEqual(
    toGuiInputCommand({ kind: "key", key: "tab", pressed: true }),
    {
      kind: "key",
      key: "tab",
      pressed: true,
    },
  );
  assert.deepEqual(toGuiInputCommand({ kind: "text", text: "hi" }), {
    kind: "text",
    text: "hi",
  });
  assert.deepEqual(toGuiInputCommand({ kind: "blur" }), { kind: "blur" });
  assert.deepEqual(
    toGuiInputCommand({
      kind: "composition",
      text: "世界",
      caretStart: 6,
      caretEnd: 6,
    }),
    { kind: "composition", text: "世界", caretStart: 6, caretEnd: 6 },
  );
  assert.deepEqual(toGuiInputCommand({ kind: "commitComposition" }), {
    kind: "commitComposition",
  });
  assert.deepEqual(toGuiInputCommand({ kind: "cancelComposition" }), {
    kind: "cancelComposition",
  });
});

test("sink submits every input immediately in call order", async () => {
  const submitted: GuiInputCommand[] = [];
  const pending: Array<(outcome: GuiInputRoutingOutcome) => void> = [];
  const failures: unknown[] = [];
  const sink = createGuiInputSink(
    {
      submitGuiInput(input) {
        submitted.push(input);
        return new Promise((resolve) => pending.push(resolve));
      },
    },
    { onError: (error) => failures.push(error) },
  );
  sink.send({
    kind: "pointerDown",
    pointer: 1,
    position: [0, 0],
    button: "primary",
  });
  sink.send({
    kind: "pointerMove",
    pointer: 1,
    position: [0.1, 0.1],
  });
  sink.send({
    kind: "pointerUp",
    pointer: 1,
    position: [0.2, 0.2],
    button: "primary",
  });
  assert.deepEqual(
    submitted.map(({ kind }) => kind),
    ["pointerDown", "pointerMove", "pointerUp"],
  );
  pending[2]!({ tick: 3n });
  pending[0]!({ tick: 1n });
  pending[1]!({ tick: 2n });
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.deepEqual(failures, []);
});

test("sink reports one rejection without delaying later input", async () => {
  const submitted: GuiInputCommand[] = [];
  const errors: Error[] = [];
  let calls = 0;
  const sink = createGuiInputSink(
    {
      submitGuiInput(input) {
        submitted.push(input);
        calls++;
        return calls === 1
          ? Promise.reject(new Error("Pending request limit"))
          : Promise.resolve({ tick: 2n });
      },
    },
    { onError: (error) => errors.push(error) },
  );
  sink.send({ kind: "text", text: "a" });
  sink.send({ kind: "text", text: "b" });
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(submitted.length, 2);
  assert.equal(errors.length, 1);
  assert.match(errors[0]!.message, /Pending request limit/);
  assert.deepEqual(submitted[1], { kind: "text", text: "b" });
});

function gatedSubmitter() {
  const pending: Array<{
    input: GuiInputCommand;
    resolve: (outcome: GuiInputRoutingOutcome) => void;
    reject: (error: Error) => void;
  }> = [];
  return {
    pending,
    submitGuiInput(input: GuiInputCommand): Promise<GuiInputRoutingOutcome> {
      return new Promise((resolve, reject) =>
        pending.push({ input, resolve, reject }),
      );
    },
  };
}

test("correlated replies gate identical consecutive pointer gestures", async () => {
  const submitter = gatedSubmitter();
  const gate = createGuiUnhandledInputGate();
  const sink = createGuiInputSink(submitter, { unhandledInputGate: gate });
  const firstAbort = new AbortController();
  const down = {
    kind: "pointerDown" as const,
    pointer: 3,
    position: [0.1, 0.2] as const,
    button: "primary" as const,
  };
  sink.send(down);
  const firstAdmission = gate.pointerDown(3, "primary", firstAbort.signal);
  sink.send({
    kind: "pointerUp",
    pointer: 3,
    position: [0.1, 0.2],
    button: "primary",
  });
  sink.send(down);
  const secondAbort = new AbortController();
  const secondAdmission = gate.pointerDown(3, "primary", secondAbort.signal);
  assert.equal(submitter.pending.length, 3);
  submitter.pending[2]!.resolve({
    tick: 2n,
    unhandled: { kind: "noPanelHit" },
  });
  assert.equal(await secondAdmission, true);
  submitter.pending[0]!.resolve({ tick: 1n });
  assert.equal(await firstAdmission, false);
  submitter.pending[1]!.resolve({
    tick: 2n,
    unhandled: { kind: "noCapture" },
  });
  sink.close?.();
});

test("only no-panel routing dispositions enter scene controls", async () => {
  const submitter = gatedSubmitter();
  const gate = createGuiUnhandledInputGate();
  const sink = createGuiInputSink(submitter, { unhandledInputGate: gate });
  sink.send({
    kind: "pointerDown",
    pointer: 7,
    position: [0.4, 0.5],
    button: "primary",
  });
  const pointerAbort = new AbortController();
  const pointer = gate.pointerDown(7, "primary", pointerAbort.signal);
  submitter.pending[0]!.resolve({
    tick: 3n,
    unhandled: { kind: "notFocusable" },
  });
  assert.equal(await pointer, false);

  sink.send({ kind: "scroll", position: [0.8, 0.1], delta: [0, 16] });
  const scrollAbort = new AbortController();
  const scroll = gate.scroll(scrollAbort.signal);
  submitter.pending[1]!.resolve({
    tick: 4n,
    unhandled: { kind: "noPanelHit" },
  });
  assert.equal(await scroll, true);

  sink.send({
    kind: "pointerDown",
    pointer: 8,
    position: [0, 0],
    button: "primary",
  });
  const detachedAbort = new AbortController();
  const detached = gate.pointerDown(8, "primary", detachedAbort.signal);
  sink.close?.();
  assert.equal(await detached, false);
});

test("detached routing replies cannot admit a replacement generation", async () => {
  const gate = createGuiUnhandledInputGate();
  const firstSubmitter = gatedSubmitter();
  const firstSink = createGuiInputSink(firstSubmitter, {
    unhandledInputGate: gate,
  });
  const down: BrowserGuiInputCommand = {
    kind: "pointerDown",
    pointer: 8,
    position: [0.2, 0.3],
    button: "primary",
  };
  firstSink.send(down);
  const oldAdmission = gate.pointerDown(
    8,
    "primary",
    new AbortController().signal,
  );
  firstSink.close?.();
  assert.equal(await oldAdmission, false);

  const nextSubmitter = gatedSubmitter();
  const nextSink = createGuiInputSink(nextSubmitter, {
    unhandledInputGate: gate,
  });
  nextSink.send(down);
  const nextAdmission = gate.pointerDown(
    8,
    "primary",
    new AbortController().signal,
  );
  let settled = false;
  void nextAdmission.then(() => {
    settled = true;
  });
  firstSubmitter.pending[0]!.resolve({
    tick: 5n,
    unhandled: { kind: "noPanelHit" },
  });
  await Promise.resolve();
  assert.equal(settled, false);
  nextSubmitter.pending[0]!.resolve({ tick: 6n });
  assert.equal(await nextAdmission, false);
  nextSink.close?.();
});

interface FakeTarget {
  listeners: Map<string, Set<(event: never) => void>>;
  style: Record<string, string>;
  rect: { left: number; top: number; width: number; height: number };
  getBoundingClientRect(): {
    left: number;
    top: number;
    width: number;
    height: number;
  };
  addEventListener(
    type: string,
    listener: (event: never) => void,
    options?: unknown,
  ): void;
  removeEventListener(type: string, listener: (event: never) => void): void;
  dispatch(
    type: string,
    event: Record<string, unknown>,
  ): { prevented: boolean };
}

function fakeTarget(): FakeTarget {
  const listeners = new Map<string, Set<(event: never) => void>>();
  const rect = { left: 10, top: 20, width: 100, height: 100 };
  return {
    listeners,
    style: {},
    rect,
    getBoundingClientRect: () => rect,
    addEventListener(type, listener) {
      let group = listeners.get(type);
      if (!group) listeners.set(type, (group = new Set()));
      group.add(listener);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
    dispatch(type, event) {
      let prevented = false;
      const full = {
        ...event,
        preventDefault: () => {
          prevented = true;
        },
      };
      for (const listener of listeners.get(type) ?? []) listener(full as never);
      return { prevented };
    },
  };
}

function harness(options: AttachCanvasGuiInputOptions = {}) {
  const canvas = fakeTarget();
  const keyboard = fakeTarget();
  const sent: BrowserGuiInputCommand[] = [];
  const errors: Error[] = [];
  const detach = attachCanvasGuiInput(
    canvas as unknown as HTMLCanvasElement,
    { send: (command) => sent.push(command) },
    {
      keyboardTarget: keyboard as unknown as HTMLElement,
      onError: (error) => errors.push(error),
      ...options,
    },
  );
  return { canvas, keyboard, sent, errors, detach };
}

test("pointer, wheel and keyboard events forward in DOM order", () => {
  const { canvas, keyboard, sent, detach } = harness();
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 3,
    clientX: 15,
    clientY: 25,
  });
  canvas.dispatch("pointermove", { pointerId: 3, clientX: 16, clientY: 26 });
  canvas.dispatch("pointerup", {
    button: 0,
    pointerId: 3,
    clientX: 16,
    clientY: 26,
  });
  // Other buttons never complete a press.
  canvas.dispatch("pointerdown", {
    button: 4,
    pointerId: 5,
    clientX: 0,
    clientY: 0,
  });
  const wheel = canvas.dispatch("wheel", {
    clientX: 15,
    clientY: 25,
    deltaX: 0,
    deltaY: -4,
    deltaMode: 0,
  });
  keyboard.dispatch("keydown", { key: "Tab" });
  keyboard.dispatch("keydown", { key: "a" });
  keyboard.dispatch("beforeinput", { inputType: "insertText", data: "x" });
  // Composition text must not double-commit through beforeinput.
  keyboard.dispatch("beforeinput", {
    inputType: "insertCompositionText",
    data: "y",
  });
  keyboard.dispatch("beforeinput", {
    inputType: "deleteContentBackward",
    data: "x",
  });
  assert.equal(wheel.prevented, true);
  assert.deepEqual(sent, [
    {
      kind: "pointerDown",
      pointer: 3,
      position: [0.05, 0.05],
      button: "primary",
      blockers: [],
    },
    {
      kind: "pointerMove",
      pointer: 3,
      position: [0.06, 0.06],
      blockers: [],
    },
    {
      kind: "pointerUp",
      pointer: 3,
      position: [0.06, 0.06],
      button: "primary",
      blockers: [],
    },
    {
      kind: "scroll",
      position: [0.05, 0.05],
      delta: [0, -4],
      blockers: [],
    },
    { kind: "key", key: "tab", pressed: true },
    { kind: "text", text: "x" },
  ]);
  detach();
  assert.equal(canvas.listeners.get("pointerdown")?.size ?? 0, 0);
  assert.equal(keyboard.listeners.get("keydown")?.size ?? 0, 0);
});

test("composition updates commit once and empty ends cancel", () => {
  const { keyboard, sent, detach } = harness();
  keyboard.dispatch("compositionupdate", { data: "世界" });
  keyboard.dispatch("compositionend", { data: "世界" });
  keyboard.dispatch("compositionupdate", { data: "" });
  keyboard.dispatch("compositionend", { data: "" });
  assert.deepEqual(sent, [
    { kind: "composition", text: "世界", caretStart: 6, caretEnd: 6 },
    { kind: "commitComposition" },
    { kind: "cancelComposition" },
  ]);
  detach();
});

test("native editor ownership leaves keyboard and IME off the canvas relay", () => {
  const { keyboard, sent, detach } = harness({ keyboardInput: false });
  keyboard.dispatch("keydown", { key: "Tab" });
  keyboard.dispatch("beforeinput", { inputType: "insertText", data: "x" });
  keyboard.dispatch("compositionstart", { data: "" });
  keyboard.dispatch("compositionupdate", { data: "世" });
  keyboard.dispatch("compositionend", { data: "世界" });
  assert.deepEqual(sent, []);
  assert.equal(keyboard.listeners.get("keydown")?.size ?? 0, 0);
  assert.equal(keyboard.listeners.get("beforeinput")?.size ?? 0, 0);
  assert.equal(keyboard.listeners.get("compositionend")?.size ?? 0, 0);
  detach();
  assert.deepEqual(sent, [{ kind: "blur" }]);
});

test("platform blur cancels live pointers and touch policy restores", () => {
  const { canvas, keyboard, sent, detach } = harness();
  assert.equal(canvas.style["touchAction"], "none");
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: -1,
    clientX: 10,
    clientY: 20,
  });
  keyboard.dispatch("blur", {});
  assert.deepEqual(sent, [
    {
      kind: "pointerDown",
      pointer: 4294967295,
      position: [0, 0],
      button: "primary",
      blockers: [],
    },
    { kind: "pointerCancel", pointer: 4294967295 },
    { kind: "blur" },
  ]);
  detach();
  assert.equal(canvas.style["touchAction"], undefined);
});

test("adapter-owned keyboard focus can transfer without clearing core focus", () => {
  const { keyboard, sent, detach } = harness({
    blurOnKeyboardTarget: false,
  });
  keyboard.dispatch("blur", {});
  assert.deepEqual(sent, []);
  detach();
  assert.deepEqual(sent, [{ kind: "blur" }]);
});

/** Live browser capture (real pointer-capture retargeting, OS focus loss,
 * unmount timing) remains a .15/.17 seam: real-browser drags outside the
 * canvas, window focus loss and unmount-while-held must still be captured
 * there. These headless fakes prove the relay termination and fencing
 * logic that the live captures exercise. */

function capturingCanvas(): FakeTarget & {
  captured: Set<number>;
  released: number[];
} {
  const base = fakeTarget() as FakeTarget & {
    captured: Set<number>;
    released: number[];
    setPointerCapture(id: number): void;
    hasPointerCapture(id: number): boolean;
    releasePointerCapture(id: number): void;
  };
  base.captured = new Set();
  base.released = [];
  base.setPointerCapture = (id: number): void => {
    base.captured.add(id);
  };
  base.hasPointerCapture = (id: number): boolean => base.captured.has(id);
  base.releasePointerCapture = (id: number): void => {
    base.captured.delete(id);
    base.released.push(id);
  };
  return base;
}

test("drag released outside the canvas terminates exactly once on it", () => {
  const canvas = capturingCanvas();
  const sent: BrowserGuiInputCommand[] = [];
  const detach = attachCanvasGuiInput(
    canvas as unknown as HTMLCanvasElement,
    { send: (command) => sent.push(command) },
    {},
  );
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 3,
    clientX: 15,
    clientY: 25,
  });
  assert.deepEqual([...canvas.captured], [3]);
  // Far outside the 100x100 rect: capture retargets the drag to the canvas.
  canvas.dispatch("pointermove", {
    pointerId: 3,
    clientX: 500,
    clientY: 500,
  });
  canvas.dispatch("pointerup", {
    button: 0,
    pointerId: 3,
    clientX: 500,
    clientY: 500,
  });
  assert.deepEqual(canvas.released, [3]);
  assert.deepEqual(sent, [
    {
      kind: "pointerDown",
      pointer: 3,
      position: [0.05, 0.05],
      button: "primary",
      blockers: [],
    },
    {
      kind: "pointerMove",
      pointer: 3,
      position: [4.9, 4.8],
      blockers: [],
    },
    {
      kind: "pointerUp",
      pointer: 3,
      position: [4.9, 4.8],
      button: "primary",
      blockers: [],
    },
  ]);
  // The post-release capture loss sends nothing: one termination total.
  canvas.dispatch("lostpointercapture", { pointerId: 3 });
  assert.equal(sent.length, 3);
  // A duplicate release after termination sends nothing either.
  canvas.dispatch("pointerup", {
    button: 0,
    pointerId: 3,
    clientX: 500,
    clientY: 500,
  });
  assert.equal(sent.length, 3);
  detach();
});

test("capture loss without release cancels the live press", () => {
  const canvas = capturingCanvas();
  const sent: BrowserGuiInputCommand[] = [];
  const detach = attachCanvasGuiInput(
    canvas as unknown as HTMLCanvasElement,
    { send: (command) => sent.push(command) },
    {},
  );
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 9,
    clientX: 15,
    clientY: 25,
  });
  // The browser takes capture (overlay, focus loss, explicit takeover).
  canvas.dispatch("lostpointercapture", { pointerId: 9 });
  assert.deepEqual(sent, [
    {
      kind: "pointerDown",
      pointer: 9,
      position: [0.05, 0.05],
      button: "primary",
      blockers: [],
    },
    { kind: "pointerCancel", pointer: 9 },
  ]);
  assert.deepEqual(canvas.released, [9]);
  // The later release finds no live pointer: no residual runtime capture.
  canvas.dispatch("pointerup", {
    button: 0,
    pointerId: 9,
    clientX: 15,
    clientY: 25,
  });
  assert.equal(sent.length, 2);
  detach();
});

test("detach while held cancels pointers, blurs and revokes the sink", async () => {
  const canvas = capturingCanvas();
  const submitted: GuiInputCommand[] = [];
  const sink = createGuiInputSink({
    submitGuiInput(input) {
      submitted.push(input);
      return Promise.resolve({ tick: 1n });
    },
  });
  const detach = attachCanvasGuiInput(
    canvas as unknown as HTMLCanvasElement,
    sink,
    {},
  );
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 3,
    clientX: 15,
    clientY: 25,
  });
  detach();
  await new Promise((resolve) => setTimeout(resolve, 10));
  // Unmount-while-held terminates the press and clears the context before
  // access is lost: exactly one termination plus blur.
  assert.deepEqual(submitted, [
    {
      kind: "pointerDown",
      pointer: 3,
      position: [0.05, 0.05],
      button: "primary",
      blockers: [],
    },
    { kind: "pointerCancel", pointer: 3 },
    { kind: "blur" },
  ]);
  // Late input after detach never enters a replacement session.
  canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 3,
    clientX: 15,
    clientY: 25,
  });
  sink.send({ kind: "text", text: "late" });
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(submitted.length, 3);
});

test("sink close fences queued sends to the old context", async () => {
  const submitted: GuiInputCommand[] = [];
  const sink = createGuiInputSink({
    submitGuiInput(input) {
      submitted.push(input);
      return Promise.resolve({ tick: 1n });
    },
  });
  sink.send({ kind: "text", text: "first" });
  sink.close?.();
  sink.send({ kind: "text", text: "second" });
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(submitted, [{ kind: "text", text: "first" }]);
});

test("prevent-default and touch policies follow options", () => {
  const first = harness({ enableTouchActionNone: false });
  assert.equal(first.canvas.style["touchAction"], undefined);
  const down = first.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: 10,
    clientY: 20,
  });
  assert.equal(down.prevented, false);
  first.detach();
  const second = harness({ preventDefaultPointer: true });
  const guarded = second.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: 10,
    clientY: 20,
  });
  assert.equal(guarded.prevented, true);
  second.detach();
});
