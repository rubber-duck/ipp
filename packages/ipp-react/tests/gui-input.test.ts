import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { canvasOutput } from "@ipp/client";
import { physicalWireTag } from "../../ipp-client/tests/physical-wire-fixture.js";
import {
  GuiPhysicalContext,
  type GuiPhysicalInput,
  type GuiInputRoutingOutcome,
  type GuiNativeEdit,
  type GuiTextFence,
  type HostPhysicalInput,
} from "../../ipp-client/src/host-input.js";
import {
  attachCanvasGuiInput,
  CanvasGuiInput,
  type CanvasGuiInputOptions,
  DEFAULT_GUI_WHEEL_STEP,
  keyboardKeyToGuiKey,
  wheelDeltaToLogical,
} from "../src/gui/input.js";
import {
  createGuiUnhandledInputGate,
  openUnhandledInputGate,
  closeUnhandledInputGate,
  trackUnhandledInputGate,
  settleUnhandledInputGateSubmission,
} from "../src/gui/scene-input.js";

const applied: GuiInputRoutingOutcome = {
  disposition: "routed",
  applied: 1,
  rejected: 0,
  cancelled: 0,
};
const miss: GuiInputRoutingOutcome = {
  ...applied,
  disposition: "miss",
  applied: 0,
};
const fence: GuiTextFence = {
  target: {
    world: { id: 1n, incarnation: 1n },
    entity: 2n,
    component: 9,
    incarnation: 1n,
  },
  generation: 1n,
};

class Element {
  readonly listeners = new Map<string, Set<(event: never) => void>>();
  readonly style: Record<string, string> = { touchAction: "pan-y" };
  readonly dataset: Record<string, string> = {};
  readonly attributes = new Map<string, string>();
  readonly children: Element[] = [];
  readonly captures = new Set<number>();
  parentElement: Element | null = null;
  value = "";
  selectionStart = 0;
  selectionEnd = 0;
  selectionDirection = "none";
  clientHeight = 100;
  tabIndex = -1;
  constructor(readonly owner: Dom) {}
  append(child: Element) {
    this.children.push(child);
    child.parentElement = this;
  }
  remove() {
    if (this.parentElement)
      this.parentElement.children.splice(
        this.parentElement.children.indexOf(this),
        1,
      );
  }
  setAttribute(key: string, value: string) {
    this.attributes.set(key, value);
  }
  getAttribute(key: string) {
    return this.attributes.get(key) ?? null;
  }
  removeAttribute(key: string) {
    this.attributes.delete(key);
  }
  getBoundingClientRect() {
    return { left: 10, top: 20, width: 200, height: 100 };
  }
  addEventListener(type: string, listener: (event: never) => void) {
    const group = this.listeners.get(type) ?? new Set();
    group.add(listener);
    this.listeners.set(type, group);
  }
  removeEventListener(type: string, listener: (event: never) => void) {
    this.listeners.get(type)?.delete(listener);
  }
  dispatch(type: string, values: Record<string, unknown> = {}) {
    let prevented = false;
    for (const listener of this.listeners.get(type) ?? [])
      listener({
        ...values,
        preventDefault() {
          prevented = true;
        },
      } as never);
    return { prevented };
  }
  focus() {
    const previous = this.owner.activeElement;
    if (previous === this) return;
    this.owner.activeElement = this;
    previous?.dispatch("blur", { relatedTarget: this });
  }
  blur() {
    if (this.owner.activeElement !== this) return;
    this.owner.activeElement = null;
    this.dispatch("blur");
  }
  setSelectionRange(start: number, end: number, direction: string) {
    this.selectionStart = start;
    this.selectionEnd = end;
    this.selectionDirection = direction;
  }
  setPointerCapture(pointer: number) {
    this.captures.add(pointer);
  }
  hasPointerCapture(pointer: number) {
    return this.captures.has(pointer);
  }
  releasePointerCapture(pointer: number) {
    this.captures.delete(pointer);
    this.dispatch("lostpointercapture", { pointerId: pointer });
  }
}

class Dom {
  activeElement: Element | null = null;
  readonly body = new Element(this);
  readonly events = new Element(this);
  readonly window = new Element(this);
  createElement() {
    return new Element(this);
  }
  hasFocus() {
    return true;
  }
  addEventListener(type: string, listener: (event: never) => void) {
    this.events.addEventListener(type, listener);
  }
  removeEventListener(type: string, listener: (event: never) => void) {
    this.events.removeEventListener(type, listener);
  }
}

function harness(
  context: TestContext,
  wheelStep?: number,
  cleanup?: () => Promise<void> | undefined,
) {
  const dom = new Dom();
  const previous = new Map(
    ["document", "window", "WheelEvent"].map((key) => [
      key,
      Object.getOwnPropertyDescriptor(globalThis, key),
    ]),
  );
  Object.defineProperties(globalThis, {
    document: { configurable: true, value: dom },
    window: { configurable: true, value: dom.window },
    WheelEvent: {
      configurable: true,
      value: { DOM_DELTA_LINE: 1, DOM_DELTA_PAGE: 2 },
    },
  });
  const canvas = new Element(dom);
  dom.body.append(canvas);
  const input = new GuiPhysicalContext(
    1n,
    {
      surface: { id: 1n, context: 1n, maxWidth: 200, maxHeight: 100 },
      selection: 1n,
      binding: {
        output: canvasOutput(fence.target.world),
        generation: { host: 1n, serial: 1n },
        viewport: { width: 200, height: 100, devicePixelRatio: 1 },
      },
    },
    async () => {
      throw new Error("Unexpected wire request");
    },
    () => {},
    physicalWireTag,
  );
  const sent: GuiPhysicalInput[] = [];
  const edits: { fence: GuiTextFence; edit: GuiNativeEdit }[] = [];
  let route: (event: GuiPhysicalInput) => Promise<GuiInputRoutingOutcome> =
    async () => applied;
  let edit: (value: GuiNativeEdit) => Promise<GuiInputRoutingOutcome> =
    async () => applied;
  input.send = (event) => {
    sent.push(event);
    return route(event);
  };
  input.editText = (value, command) => {
    edits.push({ fence: value, edit: command });
    return edit(command);
  };
  const errors: Error[] = [];
  const unhandled: GuiPhysicalInput[] = [];
  const gate = createGuiUnhandledInputGate();
  const detach = attachCanvasGuiInput(
    canvas as unknown as HTMLCanvasElement,
    input,
    {
      onError: (error) => errors.push(error),
      onUnhandled: (event) => unhandled.push(event),
      unhandledInputGate: gate,
      ...(wheelStep === undefined ? {} : { wheelStep }),
    },
  );
  const area = dom.body.children[1]!.children[0]!;
  context.after(async () => {
    await cleanup?.();
    detach();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  });
  return {
    dom,
    canvas,
    input,
    sent,
    edits,
    errors,
    area,
    gate,
    unhandled,
    detach,
    route(next: typeof route) {
      route = next;
    },
    edit(next: typeof edit) {
      edit = next;
    },
    focus() {
      input.observeText({
        fence,
        text: "ab",
        selectionStart: 2,
        selectionEnd: 2,
      });
    },
  };
}

const flush = async () => {
  for (let turn = 0; turn < 5; turn++) await Promise.resolve();
};

test("Core focus handoff blurs the old buffer without clearing the new target", (context) => {
  const state = harness(context);
  state.focus();
  state.input.observeText(null);
  assert.equal(state.dom.activeElement, state.canvas);
  assert.deepEqual(state.sent, []);
  assert.deepEqual(state.edits, []);
});

test("keyboard mapping preserves traversal, edit and control keys", () => {
  for (const [key, value] of [
    ["Enter", "enter"],
    ["Tab", "tab"],
    [" ", "space"],
    ["ArrowDown", "down"],
    ["Backspace", "backspace"],
    ["Delete", "delete"],
  ])
    assert.equal(keyboardKeyToGuiKey(key!), value);
  assert.equal(keyboardKeyToGuiKey("Tab", true), "backTab");
  assert.equal(keyboardKeyToGuiKey("a"), null);
  // The Menu key and Shift+F10 request a context; F10 alone stays the browser's.
  assert.equal(keyboardKeyToGuiKey("ContextMenu"), "contextMenu");
  assert.equal(keyboardKeyToGuiKey("F10", true), "f10");
  assert.equal(keyboardKeyToGuiKey("F10"), null);
});

test("context keys and Shift reach the runtime and the browser menu never opens", async (context) => {
  const state = harness(context);
  state.canvas.dispatch("keydown", { key: "ContextMenu" });
  state.canvas.dispatch("keydown", { key: "F10" });
  state.canvas.dispatch("keydown", { key: "F10", shiftKey: true });
  state.canvas.dispatch("keydown", { key: "ArrowLeft", shiftKey: true });
  state.canvas.dispatch("wheel", {
    clientX: 110,
    clientY: 45,
    deltaMode: 1,
    deltaX: 0,
    deltaY: 3,
    shiftKey: true,
  });
  assert.equal(state.canvas.dispatch("contextmenu").prevented, true);
  assert.deepEqual(state.sent, [
    { kind: "key", key: "contextMenu" },
    { kind: "key", key: "f10", shift: true },
    { kind: "key", key: "left", shift: true },
    {
      kind: "wheel",
      point: [0.5, 0.25],
      delta: [0, DEFAULT_GUI_WHEEL_STEP],
      shift: true,
    },
  ]);

  // While the native text buffer holds focus, context keys are routed input,
  // not edits, and its own menu is suppressed too.
  state.focus();
  state.area.dispatch("keydown", { key: "ContextMenu" });
  state.area.dispatch("keydown", { key: "F10", shiftKey: true });
  assert.equal(state.area.dispatch("contextmenu").prevented, true);
  await flush();
  assert.deepEqual(state.sent.slice(4), [
    { kind: "key", key: "contextMenu" },
    { kind: "key", key: "f10", shift: true },
  ]);
  assert.deepEqual(state.edits, []);
  assert.deepEqual(state.errors, []);
});

test("pointer, wheel and keys submit immediately in DOM order", async (context) => {
  const state = harness(context);
  state.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 7,
    clientX: 110,
    clientY: 45,
  });
  state.canvas.dispatch("pointermove", {
    pointerId: 7,
    clientX: 210,
    clientY: 120,
  });
  state.canvas.dispatch("wheel", {
    clientX: 110,
    clientY: 45,
    deltaMode: 1,
    deltaX: -3,
    deltaY: 6,
  });
  state.canvas.dispatch("keydown", { key: "Tab", shiftKey: true });
  state.canvas.dispatch("pointerup", {
    button: 0,
    pointerId: 7,
    clientX: 310,
    clientY: 170,
  });
  assert.deepEqual(state.sent, [
    { kind: "pointerDown", pointer: 7n, point: [0.5, 0.25] },
    { kind: "pointerMove", pointer: 7n, point: [1, 1] },
    { kind: "wheel", point: [0.5, 0.25], delta: [-0.25, 0.5] },
    { kind: "key", key: "backTab", shift: true },
    { kind: "pointerUp", pointer: 7n, point: [1.5, 1.5] },
  ]);
  await flush();
  assert.equal(state.canvas.captures.size, 0);
  assert.deepEqual(state.errors, []);
});

test("wheel deltas scroll a fixed number of logical units per notch", (context) => {
  // The harness installs the DOM delta-mode constants.
  harness(context);
  // One notch in each delta mode: 100 CSS px, 3 lines, or an eighth page.
  assert.deepEqual(wheelDeltaToLogical(0, 100, 0), [0, DEFAULT_GUI_WHEEL_STEP]);
  assert.deepEqual(wheelDeltaToLogical(-3, 0, 1), [-DEFAULT_GUI_WHEEL_STEP, 0]);
  assert.deepEqual(wheelDeltaToLogical(0, 0.125, 2), [
    0,
    DEFAULT_GUI_WHEEL_STEP,
  ]);
  // Smooth pixel scrolling moves fractional notches; the step scales them.
  assert.deepEqual(wheelDeltaToLogical(50, -200, 0, 0.1), [0.05, -0.2]);
  assert.deepEqual(wheelDeltaToLogical(0, 6, 1, 2), [0, 4]);
  assert.deepEqual(wheelDeltaToLogical(0, 1, 2, 0.5), [0, 4]);
});

test("the canvas adapter scrolls by its configured wheel step", (context) => {
  const state = harness(context, 15);
  state.canvas.dispatch("wheel", {
    clientX: 110,
    clientY: 45,
    deltaX: 0,
    deltaY: 200,
    deltaMode: 0,
  });
  assert.deepEqual(state.errors, []);
  assert.deepEqual(state.sent, [
    { kind: "wheel", point: [0.5, 0.25], delta: [0, 30] },
  ]);
});

test("an invalid wheel step reports once and scrolls by the default", (context) => {
  const state = harness(context, 0);
  state.canvas.dispatch("wheel", {
    clientX: 110,
    clientY: 45,
    deltaX: 0,
    deltaY: 100,
    deltaMode: 0,
  });
  assert.equal(state.errors.length, 1);
  assert.match(
    state.errors[0]!.message,
    /wheel step must be finite and positive/,
  );
  assert.deepEqual(state.sent, [
    { kind: "wheel", point: [0.5, 0.25], delta: [0, DEFAULT_GUI_WHEEL_STEP] },
  ]);
});

test("secondary buttons do not capture and composition keys do not relay", (context) => {
  const state = harness(context);
  state.canvas.dispatch("pointerdown", {
    button: 2,
    pointerId: 4,
    clientX: 10,
    clientY: 20,
  });
  state.canvas.dispatch("keydown", { key: "Enter", isComposing: true });
  state.canvas.dispatch("keydown", { key: "a", ctrlKey: true });
  assert.deepEqual(state.sent, [
    { kind: "pointerDown", button: "secondary", pointer: 4n, point: [0, 0] },
  ]);
  assert.equal(state.canvas.captures.size, 0);
});

test("wholly unconsumed correlated wheel admits scene fallback exactly once", async (context) => {
  const state = harness(context);
  state.route(async () => ({ ...applied, remaining: [0, 1] }));
  const waiting = state.gate.scroll(new AbortController().signal);
  state.canvas.dispatch("wheel", {
    clientX: 10,
    clientY: 20,
    deltaX: 0,
    deltaY: 400,
    deltaMode: 0,
  });
  assert.equal(await waiting, true);
  assert.equal(state.unhandled.length, 1);
});

test("secondary and auxiliary misses settle the exact button without a primary press", async (context) => {
  const state = harness(context);
  state.route(async () => miss);
  for (const [button, name] of [
    [2, "secondary"],
    [1, "auxiliary"],
  ] as const) {
    let settled: boolean | undefined;
    const abort = new AbortController();
    void state.gate.pointerDown(7, name, abort.signal).then((value) => {
      settled = value;
    });
    state.canvas.dispatch("pointerdown", {
      button,
      pointerId: 7,
      clientX: 10,
      clientY: 20,
    });
    await flush();
    abort.abort();
    assert.equal(settled, true);
    assert.deepEqual(state.sent.at(-1), {
      kind: "pointerDown",
      pointer: 7n,
      point: [0, 0],
      button: name,
    });
  }
});

test("partial consumption and negative terminals never authorize scene wheel", async () => {
  const gate = createGuiUnhandledInputGate();
  const generation = openUnhandledInputGate(gate);
  for (const outcome of [
    { ...applied, remaining: [0, 5] as const },
    { ...applied, remaining: [0, 0] as const },
    { ...applied, remaining: [0, 20] as const, rejected: 1 },
    { ...applied, remaining: [0, 20] as const, cancelled: 1 },
  ]) {
    const waiting = gate.scroll(new AbortController().signal);
    const submission = trackUnhandledInputGate(gate, generation, {
      kind: "wheel",
      point: [0, 0],
      delta: [0, 20],
    });
    settleUnhandledInputGateSubmission(gate, submission, outcome);
    assert.equal(await waiting, false);
  }
  closeUnhandledInputGate(gate, generation);
});

test("secondary gate abort, rebind and a fresh auxiliary hit preserve exact identity", async () => {
  const gate = createGuiUnhandledInputGate();
  const old = openUnhandledInputGate(gate);
  const abort = new AbortController();
  const waiting = gate.pointerDown(1, "secondary", abort.signal);
  const submission = trackUnhandledInputGate(gate, old, {
    kind: "pointerDown",
    button: "secondary",
    pointer: 1n,
    point: [0, 0],
  });
  abort.abort();
  assert.equal(await waiting, false);
  closeUnhandledInputGate(gate, old);
  const current = openUnhandledInputGate(gate);
  const fresh = gate.pointerDown(1, "auxiliary", new AbortController().signal);
  const next = trackUnhandledInputGate(gate, current, {
    kind: "pointerDown",
    button: "auxiliary",
    pointer: 1n,
    point: [0, 0],
  });
  settleUnhandledInputGateSubmission(gate, submission, miss);
  settleUnhandledInputGateSubmission(gate, next, {
    ...miss,
    disposition: "blocked",
  });
  assert.equal(await fresh, false);
  closeUnhandledInputGate(gate, current);
});

test("capture loss cancels once and blur cancels all native capture", (context) => {
  const state = harness(context);
  state.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 7,
    clientX: 10,
    clientY: 20,
  });
  state.canvas.releasePointerCapture(7);
  assert.deepEqual(state.sent.at(-1), { kind: "pointerCancel", pointer: 7n });
  assert.equal(
    state.sent.filter((event) => event.kind === "pointerCancel").length,
    1,
  );
  state.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 8,
    clientX: 10,
    clientY: 20,
  });
  state.dom.window.dispatch("blur");
  assert.equal(state.canvas.captures.size, 0);
  assert.deepEqual(state.sent.at(-1), { kind: "blur" });
});

test("negative routing releases capture and later events are not blocked", async (context) => {
  const state = harness(context);
  state.route(async () => {
    throw new Error("delivery lost");
  });
  state.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 7,
    clientX: 10,
    clientY: 20,
  });
  await flush();
  assert.equal(state.canvas.captures.size, 0);
  assert.equal(state.errors.length, 1);
  state.route(async () => miss);
  state.canvas.dispatch("keydown", { key: "Enter" });
  await flush();
  assert.deepEqual(state.unhandled, [{ kind: "key", key: "enter" }]);
});

test("native editor receives focus without a Core blur and sends each edit once", async (context) => {
  const state = harness(context);
  state.canvas.focus();
  state.focus();
  assert.equal(state.dom.activeElement, state.area);
  assert.deepEqual(state.sent, []);
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "c" });
  state.area.dispatch("keydown", { key: "Backspace" });
  state.area.dispatch("beforeinput", { inputType: "deleteContentBackward" });
  state.area.dispatch("keydown", { key: "Tab" });
  await flush();
  assert.deepEqual(
    state.edits.map((entry) => entry.edit),
    [
      { kind: "text", text: "c" },
      { kind: "key", key: "backspace" },
    ],
  );
  assert.deepEqual(state.sent, [{ kind: "key", key: "tab" }]);
});

test("composition update and terminal payloads do not double commit", async (context) => {
  const state = harness(context);
  state.focus();
  state.area.dispatch("compositionstart");
  state.area.dispatch("compositionupdate", { data: "é" });
  state.area.dispatch("beforeinput", {
    inputType: "insertCompositionText",
    data: "é",
  });
  state.area.dispatch("keydown", { key: "Enter", isComposing: true });
  state.area.dispatch("compositionend", { data: "é" });
  state.area.dispatch("beforeinput", {
    inputType: "insertFromComposition",
    data: "é",
  });
  await flush();
  assert.deepEqual(
    state.edits.map((entry) => entry.edit),
    [
      { kind: "composition", text: "é", caretStart: 2, caretEnd: 2 },
      { kind: "commitComposition" },
    ],
  );
});

test("buffer selections address only the committed text they display", async (context) => {
  const state = harness(context);
  state.focus();
  assert.equal(state.area.value, "ab");
  // Platform text not yet reflected by the runtime (a committed IME
  // candidate before its acknowledgement) must not select inside it.
  state.area.value = "a世b";
  state.area.setSelectionRange(2, 2, "none");
  state.area.dispatch("select");
  state.dom.events.dispatch("selectionchange");
  await flush();
  assert.equal(state.edits.length, 0);
  // Once the buffer shows the committed text again, selections forward in
  // UTF-8 bytes.
  state.area.value = "ab";
  state.area.setSelectionRange(0, 1, "forward");
  state.area.dispatch("select");
  await flush();
  assert.deepEqual(
    state.edits.map((entry) => entry.edit),
    [{ kind: "selection", start: 0, end: 1 }],
  );
  assert.deepEqual(state.errors, []);
});

test("native edit queue uses preceding ACK state and external changes cancel pending work", async (context) => {
  const state = harness(context);
  state.focus();
  let release!: (outcome: GuiInputRoutingOutcome) => void;
  state.edit(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "c" });
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "d" });
  assert.equal(state.edits.length, 1);
  state.input.observeText(
    {
      fence: { ...fence, generation: 2n },
      text: "abc",
      selectionStart: 3,
      selectionEnd: 3,
    },
    "terminal",
  );
  state.edit(async () => applied);
  release(applied);
  await flush();
  assert.equal(state.edits[1]!.fence.generation, 2n);
  state.edit(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "e" });
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "f" });
  state.input.observeText({
    fence: { ...fence, generation: 3n },
    text: "reset",
    selectionStart: 5,
    selectionEnd: 5,
  });
  release(applied);
  await flush();
  assert.equal(state.edits.length, 3);
});

test("detach releases capture, removes native buffer and fences all listeners", (context) => {
  const state = harness(context);
  state.canvas.dispatch("pointerdown", {
    button: 0,
    pointerId: 7,
    clientX: 10,
    clientY: 20,
  });
  state.detach();
  const length = state.sent.length;
  state.canvas.dispatch("keydown", { key: "Enter" });
  state.area.dispatch("beforeinput", { inputType: "insertText", data: "late" });
  assert.equal(state.sent.length, length);
  assert.equal(state.edits.length, 0);
  assert.equal(state.canvas.captures.size, 0);
  assert.equal(state.canvas.style.touchAction, "pan-y");
  assert.equal(state.dom.body.children.length, 1);
});

test("exact correlated replies admit only misses and wholly unconsumed wheels", async () => {
  const gate = createGuiUnhandledInputGate();
  const generation = openUnhandledInputGate(gate);
  for (const outcome of [
    miss,
    applied,
    { ...applied, rejected: 1 },
    { ...applied, disposition: "unhandled" as const },
  ]) {
    const input: GuiPhysicalInput = {
      kind: "pointerDown",
      pointer: 7n,
      point: [0, 0],
    };
    const submission = trackUnhandledInputGate(gate, generation, input);
    const first = gate.pointerDown(7, "primary", new AbortController().signal);
    const secondSubmission = trackUnhandledInputGate(gate, generation, input);
    const second = gate.pointerDown(7, "primary", new AbortController().signal);
    settleUnhandledInputGateSubmission(gate, submission, outcome);
    settleUnhandledInputGateSubmission(gate, secondSubmission, miss);
    assert.equal(await first, outcome === miss);
    assert.equal(await second, true);
  }
  for (const disposition of ["routed", "unhandled"] as const) {
    const submission = trackUnhandledInputGate(gate, generation, {
      kind: "wheel",
      point: [0, 0],
      delta: [0, 5],
    });
    const waiting = gate.scroll(new AbortController().signal);
    settleUnhandledInputGateSubmission(gate, submission, {
      ...applied,
      disposition,
    });
    assert.equal(await waiting, disposition === "unhandled");
  }
});

test("aborted and detached scene gestures cannot admit a replacement generation", async () => {
  const gate = createGuiUnhandledInputGate();
  const generation = openUnhandledInputGate(gate);
  const submission = trackUnhandledInputGate(gate, generation, {
    kind: "pointerDown",
    pointer: 7n,
    point: [0, 0],
  });
  const abort = new AbortController();
  const waiting = gate.pointerDown(7, "primary", abort.signal);
  abort.abort();
  assert.equal(await waiting, false);
  const old = gate.scroll(new AbortController().signal);
  closeUnhandledInputGate(gate, generation);
  assert.equal(await old, false);
  const replacement = openUnhandledInputGate(gate);
  const fresh = gate.pointerDown(7, "primary", new AbortController().signal);
  settleUnhandledInputGateSubmission(gate, submission, miss);
  closeUnhandledInputGate(gate, replacement);
  assert.equal(await fresh, false);
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((accept, refuse) => {
    resolve = accept;
    reject = refuse;
  });
  return { promise, resolve, reject };
}

function ownerHarness(test: TestContext) {
  let owner: CanvasGuiInput | undefined;
  const state = harness(test, undefined, () => owner?.close());
  state.detach();
  const opened: {
    view: typeof state.input.view;
    options: CanvasGuiInputOptions;
  }[] = [];
  const contexts: GuiPhysicalContext[] = [];
  const closed: bigint[] = [];
  let acquire: (() => Promise<GuiPhysicalContext>) | undefined;
  let release: (() => Promise<void>) | undefined;
  function create() {
    const context = new GuiPhysicalContext(
      BigInt(contexts.length + 1),
      state.input.view,
      async () => {
        throw new Error("Unexpected wire request");
      },
      () => {},
      physicalWireTag,
    );
    context.send = async () => miss;
    context.close = async () => {
      if (context.isClosed) return;
      context.stop(new Error("Closed"));
      closed.push(context.identity);
      await release?.();
    };
    contexts.push(context);
    return context;
  }
  const options = {
    onError: (error: Error) => state.errors.push(error),
    unhandledInputGate: state.gate,
  };
  owner = new CanvasGuiInput(
    state.canvas as unknown as HTMLCanvasElement,
    {
      open: async (
        view: typeof state.input.view,
        options: CanvasGuiInputOptions,
      ) => {
        opened.push({ view, options });
        return acquire ? acquire() : create();
      },
    } as unknown as HostPhysicalInput,
    options,
  );
  return {
    ...state,
    owner,
    options,
    opened,
    contexts,
    closed,
    create,
    acquire(next: typeof acquire) {
      acquire = next;
    },
    release(next: typeof release) {
      release = next;
    },
  };
}

const blocker = {
  world: { id: 1n, incarnation: 2n },
  entity: 9n,
  incarnation: 3n,
};

test("input option identity changes arm, lift and replace blockers while equivalent props preserve ownership", async (test) => {
  const h = ownerHarness(test);
  await h.owner.select(h.input.view);
  await h.owner.update({ ...h.options, blockers: [blocker] });
  assert.equal(h.opened.length, 2);
  assert.deepEqual(h.opened[1]!.options.blockers, [blocker]);
  const current = h.contexts[1]!;
  current.send = async () => applied;
  h.canvas.dispatch("pointerdown", {
    pointerId: 4,
    button: 0,
    clientX: 40,
    clientY: 40,
  });
  await h.owner.update({ ...h.options, blockers: structuredClone([blocker]) });
  assert.equal(h.opened.length, 2);
  assert.equal(current.isClosed, false);
  assert.equal(h.canvas.hasPointerCapture(4), true);
  const replacement = { ...blocker, incarnation: 4n };
  await h.owner.update({ ...h.options, blockers: [replacement] });
  assert.equal(current.isClosed, true);
  assert.equal(h.canvas.hasPointerCapture(4), false);
  await h.owner.update(h.options);
  assert.deepEqual(h.opened[3]!.options.blockers, undefined);
  assert.deepEqual(h.closed, [1n, 2n, 3n]);
});

test("callback refresh retains captured pointer and text owner and invokes current handlers", async (test) => {
  const h = ownerHarness(test);
  let first = 0;
  let latest = 0;
  await h.owner.update({ ...h.options, onUnhandled: () => first++ });
  await h.owner.select(h.input.view);
  const current = h.contexts[0]!;
  current.send = async () => applied;
  h.canvas.dispatch("pointerdown", {
    pointerId: 4,
    button: 0,
    clientX: 40,
    clientY: 40,
  });
  current.observeText({
    fence,
    text: "ab",
    selectionStart: 2,
    selectionEnd: 2,
  });
  const focused = h.dom.activeElement;
  await h.owner.update({
    ...h.options,
    blockers: [],
    onUnhandled: () => latest++,
  });
  assert.equal(h.opened.length, 1);
  assert.equal(h.canvas.hasPointerCapture(4), true);
  assert.equal(h.dom.activeElement, focused);
  assert.equal(current.nativeText?.text, "ab");
  current.send = async () => miss;
  h.canvas.dispatch("pointermove", { pointerId: 4, clientX: 40, clientY: 40 });
  await flush();
  assert.equal(first, 0);
  assert.equal(latest, 1);
  await h.owner.update({ ...h.options, wheelStep: 0.5 });
  assert.equal(current.isClosed, true);
  assert.equal(current.nativeText, null);
  assert.equal(h.canvas.hasPointerCapture(4), false);
  assert.equal(h.dom.body.children.length, 2);
});

test("pending acquisition is closed before the latest configuration and view can open", async (test) => {
  const h = ownerHarness(test);
  const pending = deferred<GuiPhysicalContext>();
  h.acquire(() => pending.promise);
  const first = h.owner.select(h.input.view);
  await flush();
  const updated = h.owner.update({ ...h.options, blockers: [blocker] });
  const latestView = { ...h.input.view, selection: 20n };
  const selected = h.owner.select(latestView);
  assert.equal(h.opened.length, 1);
  h.acquire(undefined);
  pending.resolve(h.create());
  await Promise.all([first, updated, selected]);
  assert.deepEqual(h.closed, [1n]);
  assert.equal(h.opened.length, 2);
  assert.equal(h.opened[1]!.view.selection, 20n);
  assert.deepEqual(h.opened[1]!.options.blockers, [blocker]);
  assert.equal(h.dom.body.children.length, 2);
});

test("disposal drains pending opens and releases without late listeners or reacquisition", async (test) => {
  const h = ownerHarness(test);
  const pending = deferred<GuiPhysicalContext>();
  h.acquire(() => pending.promise);
  const first = h.owner.select(h.input.view);
  await flush();
  const updated = h.owner.update({ ...h.options, blockers: [blocker] });
  const closed = h.owner.close();
  pending.resolve(h.create());
  await Promise.all([first, updated, closed]);
  await h.owner.update(h.options);
  await h.owner.select(h.input.view);
  assert.equal(h.opened.length, 1);
  assert.deepEqual(h.closed, [1n]);
  assert.equal(h.dom.body.children.length, 1);
  assert.equal(h.canvas.style.touchAction, "pan-y");
});

test("configuration replacement waits for release and fences a concurrent close", async (test) => {
  const h = ownerHarness(test);
  await h.owner.select(h.input.view);
  const release = deferred<void>();
  h.release(() => release.promise);
  const update = h.owner.update({ ...h.options, blockers: [blocker] });
  await flush();
  assert.equal(h.opened.length, 1);
  const close = h.owner.close();
  release.resolve();
  await Promise.all([update, close]);
  assert.equal(h.opened.length, 1);
  assert.equal(h.dom.body.children.length, 1);
});

test("failed acquisition does not prevent a later update or disposal", async (test) => {
  const h = ownerHarness(test);
  h.acquire(async () => {
    throw new Error("Open failed");
  });
  await assert.rejects(h.owner.select(h.input.view), /Open failed/);
  h.acquire(undefined);
  await h.owner.update({ ...h.options, blockers: [blocker] });
  assert.equal(h.opened.length, 2);
  await h.owner.close();
  assert.deepEqual(h.closed, [1n]);
});

test("retryable release failure retains cleanup ownership before reacquisition or disposal", async (test) => {
  const h = ownerHarness(test);
  await h.owner.select(h.input.view);
  const current = h.contexts[0]!;
  let attempts = 0;
  current.close = async () => {
    current.stop(new Error("Closed locally"));
    if (++attempts === 1) throw new Error("Release not sent");
    h.closed.push(current.identity);
  };
  await assert.rejects(
    h.owner.update({ ...h.options, blockers: [blocker] }),
    /Release not sent/,
  );
  assert.equal(h.opened.length, 1);
  await h.owner.select(h.input.view);
  assert.equal(attempts, 2);
  assert.deepEqual(h.closed, [1n]);
  assert.equal(h.opened.length, 2);
  const final = h.contexts[1]!;
  let cleanupAttempts = 0;
  final.close = async () => {
    final.stop(new Error("Closed locally"));
    if (++cleanupAttempts === 1) throw new Error("Release not sent");
    h.closed.push(final.identity);
  };
  await assert.rejects(h.owner.close(), /Release not sent/);
  await h.owner.close();
  assert.equal(cleanupAttempts, 2);
  assert.deepEqual(h.closed, [1n, 2n]);
});

test("gate replacement cancels only the retired gate and wheel changes reach new listeners", async (test) => {
  const h = ownerHarness(test);
  await h.owner.select(h.input.view);
  const old = deferred<GuiInputRoutingOutcome>();
  h.contexts[0]!.send = () => old.promise;
  const oldAdmission = h.gate.pointerDown(
    4,
    "primary",
    new AbortController().signal,
  );
  h.canvas.dispatch("pointerdown", {
    pointerId: 4,
    button: 0,
    clientX: 40,
    clientY: 40,
  });
  const nextGate = createGuiUnhandledInputGate();
  await h.owner.update({
    ...h.options,
    unhandledInputGate: nextGate,
    wheelStep: 0.5,
  });
  assert.equal(await oldAdmission, false);
  const nextAdmission = nextGate.pointerDown(
    4,
    "primary",
    new AbortController().signal,
  );
  h.canvas.dispatch("pointerdown", {
    pointerId: 4,
    button: 0,
    clientX: 40,
    clientY: 40,
  });
  assert.equal(await nextAdmission, true);
  old.resolve(miss);
  await flush();
  const events: GuiPhysicalInput[] = [];
  h.contexts[1]!.send = async (event) => {
    events.push(event);
    return applied;
  };
  h.canvas.dispatch("wheel", {
    deltaX: 0,
    deltaY: 100,
    deltaMode: 0,
    clientX: 40,
    clientY: 40,
  });
  await flush();
  assert.deepEqual(
    events[0]?.kind === "wheel" ? events[0].delta : null,
    [0, 0.5],
  );
});

test("blocker snapshots include every lifetime field and survive caller mutation", async (test) => {
  const h = ownerHarness(test);
  const authored = structuredClone(blocker);
  await h.owner.update({ ...h.options, blockers: [authored] });
  await h.owner.select(h.input.view);
  for (const mutate of [
    () => authored.world.id++,
    () => authored.world.incarnation++,
    () => authored.entity++,
    () => authored.incarnation++,
  ]) {
    const previous = h.opened.length;
    mutate();
    await h.owner.update({ ...h.options, blockers: [authored] });
    assert.equal(h.opened.length, previous + 1);
  }
  assert.deepEqual(h.opened[0]!.options.blockers, [blocker]);
});

test("disposal completion waits for late acquisition and its release acknowledgement", async (test) => {
  const h = ownerHarness(test);
  const opening = deferred<GuiPhysicalContext>();
  const release = deferred<void>();
  h.acquire(() => opening.promise);
  h.release(() => release.promise);
  const selected = h.owner.select(h.input.view);
  await flush();
  let hostCanDispose = false;
  const closed = h.owner.close().then(() => {
    hostCanDispose = true;
  });
  await flush();
  assert.equal(hostCanDispose, false);
  assert.deepEqual(h.closed, []);
  opening.resolve(h.create());
  await flush();
  assert.deepEqual(h.closed, [1n]);
  assert.equal(hostCanDispose, false);
  assert.equal(h.dom.body.children.length, 1);
  release.resolve();
  await Promise.all([selected, closed]);
  assert.equal(hostCanDispose, true);
  assert.equal(h.opened.length, 1);
});
