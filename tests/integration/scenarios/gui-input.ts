/** Physical GUI pointer, keyboard and native text input through a generated
 * client, independent of process launch and wire layout.
 *
 * Every panel is an ordinary Canvas World presented through a Surface
 * attachment of the scenario's root output; input enters through that
 * presentation's physical context and is observed through control fields,
 * value watch records, effects and native text state.
 */
import type {
  Client,
  Command,
  GuiInputRoutingOutcome,
  GuiNativeEdit,
  GuiObservedEffect,
  GuiPhysicalInput,
  LifecycleFieldValue,
  LifecycleTargetWatch,
  WorldReference,
} from "@ipp/client";
import { guiAction } from "../gui-actions.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  alias,
  applied,
  check,
  cleanup,
  compareAndSet,
  control,
  encoded,
  handle,
  LAYOUT,
  loadedFont,
  nativeText,
  openGui,
  PANEL_CANVAS,
  place,
  presentGui,
  rejectedFor,
  routed,
  setFields,
  VIEWPORT,
  type GuiHost,
  type GuiTestClient,
  type PresentedGui,
} from "./gui-lifecycle.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  GUI,
  selectSystems,
} from "../system-selections.js";

/** One frame-end change of a watched control's value fields. */
interface ValueChange {
  readonly entity: bigint;
  readonly tick: bigint;
  /** Field values in offset order; null while the control is absent. */
  readonly values: readonly LifecycleFieldValue[] | null;
}

/** How many effects and value changes had arrived when the mark was taken. */
interface LogMark {
  readonly effects: number;
  readonly changes: number;
}

/** The mark before anything arrived. */
const LOG_START: LogMark = { effects: 0, changes: 0 };

/**
 * Every effect one subscription observed, in delivery order, and the value
 * changes of the controls it watches (each watch's first record reports the
 * current value and is not a change).
 */
interface EffectLog {
  readonly effects: GuiObservedEffect[];
  readonly changes: ValueChange[];
  /** Watch the value fields of the control on `entity`. */
  watch(entity: bigint): Promise<void>;
  /** Mark what has arrived so far, to select what arrives later. */
  mark(): LogMark;
  stop(): Promise<void>;
}

/** The watched value fields of each control component. */
const VALUE_FIELDS: Readonly<Record<string, readonly string[]>> = {
  GuiCheckbox: ["checked"],
  GuiSlider: ["value"],
  GuiTextInput: ["text"],
  GuiScrollView: ["offset_x", "offset_y"],
  GuiVirtualList: ["offset_x", "offset_y"],
};

async function recordEffects(client: GuiTestClient): Promise<EffectLog> {
  const effects: GuiObservedEffect[] = [];
  const changes: ValueChange[] = [];
  const watches: LifecycleTargetWatch[] = [];
  const subscription = await client.subscribeGuiEffects(
    (effect) => effects.push(effect),
    { classes: "all" },
  );
  return {
    effects,
    changes,
    mark: () => ({ effects: effects.length, changes: changes.length }),
    async watch(entity) {
      const state = await control(client, entity);
      const name = Object.keys(VALUE_FIELDS).find(
        (candidate) =>
          client.components[candidate]?.id === state.target.component,
      );
      check(name, `Control ${entity} has no value fields`);
      const descriptor = client.components[name]!;
      const fields = VALUE_FIELDS[name]!.map(
        (field) => descriptor.fields[field]!.offset,
      ).sort((left, right) => left - right);
      let current = true;
      watches.push(
        await client.watchLifecycle(
          [
            {
              target: {
                kind: "value",
                entity,
                component: descriptor.id,
                fields,
              },
              kinds: 128,
            },
          ],
          (event) => {
            if (event.kind !== "value") return;
            if (current) current = false;
            else
              changes.push({
                entity,
                tick: event.tick,
                values: event.values,
              });
          },
        ),
      );
    },
    async stop() {
      await subscription.unsubscribe();
      for (const watch of watches) await watch.remove();
    },
  };
}

/** Poll, presenting frames, until `read` yields a value. */
async function eventually<T>(
  presentation: PresentedGui,
  read: () => T | undefined | Promise<T | undefined>,
  message: () => string,
): Promise<T> {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const value = await read();
    if (value !== undefined) return value;
    check(Date.now() < deadline, message());
    await presentation.frame();
  }
}

/** Value changes of `entity` (or every watched control) that arrived after `mark`. */
function commitsSince(log: EffectLog, mark: LogMark, entity?: bigint) {
  return log.changes
    .slice(mark.changes)
    .filter((change) => entity === undefined || change.entity === entity);
}

/** The one value of a change of a single-field control. */
function changedValue(change: ValueChange) {
  return change.values?.[0]?.value;
}

/** Text submissions published after `mark`. */
function submissionsSince(log: EffectLog, mark: LogMark) {
  return log.effects
    .slice(mark.effects)
    .filter((effect) => effect.effect.kind === "submitted");
}

async function boolValue(client: GuiTestClient, entity: bigint) {
  const state = await control(client, entity);
  check(state.value.kind === "bool", `Control ${entity} is not a checkbox`);
  return state.value.value;
}

async function textValue(client: GuiTestClient, entity: bigint) {
  const state = await control(client, entity);
  check(state.value.kind === "text", `Control ${entity} is not a TextInput`);
  return state.value.value;
}

/** Layout root commands of a 4 x 3 panel at alias 1, a top-level entity of
 * a World created with {@link PANEL_CANVAS}. */
function panelRoot(
  client: Client,
  symbolicId: string,
  extra: Command[] = [],
): Command[] {
  return [
    createEntity(1, symbolicId),
    insertComponent(client, "GuiLayout", alias(1), {
      kind: LAYOUT.column,
      width: 4,
      height: 3,
    }),
    ...extra,
  ];
}

/**
 * Exercise real physical pointer/keyboard/native-text outcomes against a
 * presented ordinary GUI, observed through committed control values, effects
 * and native text state. Sub-scenarios in their own Worlds cover nested
 * scrolling, removal during interaction and keyboard entry across panels.
 *
 * The panel is a 4 x 3 logical canvas on the full root viewport: the centre
 * tap hits a full-panel checkbox while a far point reaches no panel.
 */
export async function exerciseGuiInput(host: GuiHost, fontBytes: ArrayBuffer) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-input",
      canvas: PANEL_CANVAS,
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const log = await recordEffects(client);
    const font = await client.createAsset(17, fontBytes);
    const outcome = successfulBatch(
      await client.batch(
        panelRoot(client, "gui-input-panel", [
          insertComponent(client, "GuiFont", alias(1), {
            source: font.source,
            font_size: 0.6,
          }),
          createEntity(2, "gui-input-checkbox"),
          insertComponent(client, "GuiCheckbox", alias(2), {
            checked: false,
          }),
          insertComponent(client, "GuiLayout", alias(2), {
            width: 4,
            height: 3,
          }),
          place(alias(2), alias(1)),
        ]),
      ),
    );
    const root = aliasId(outcome, 1);
    const checkbox = aliasId(outcome, 2);
    await log.watch(checkbox);
    presentation = await presentGui(host, [{ child: created.reference }]);
    const p = presentation;
    await loadedFont(client);
    await client.waitForFrame();
    const send = (input: GuiPhysicalInput) => p.send(input);
    const at = p.point(0, [2, 1.5]);
    const miss = p.point(0, [10, 10]);
    const down = (pointer: bigint, point: [number, number]) =>
      send({ kind: "pointerDown", pointer, point });
    const up = (pointer: bigint, point: [number, number]) =>
      send({ kind: "pointerUp", pointer, point });

    // A centre press completes on release over the same full-panel checkbox.
    await down(1n, at);
    await send({ kind: "pointerMove", pointer: 1n, point: at });
    await up(1n, at);
    check(
      (await boolValue(client, checkbox)) === true,
      "Centre press missed the checkbox",
    );

    // A press far outside the 4 x 3 logical extent reaches no panel.
    const missed = [await down(2n, miss), await up(2n, miss)];
    check(
      missed[0]!.disposition === "miss" &&
        (await boolValue(client, checkbox)) === true,
      `An off-panel press toggled the checkbox: ${encoded(missed)}`,
    );

    // Checkboxes commit on tap release, never on press: a cancelled press
    // and a press released outside its control change nothing.
    await down(3n, at);
    await send({ kind: "pointerCancel", pointer: 3n });
    const afterCancel = await up(3n, at);
    check(
      (await boolValue(client, checkbox)) === true &&
        afterCancel.disposition === "unhandled",
      `Cancel-after-down completed the press: ${encoded(afterCancel)}`,
    );
    await down(4n, at);
    await up(4n, miss);
    check(
      (await boolValue(client, checkbox)) === true,
      "Release-outside committed the press",
    );

    // An in-bounds tap commits exactly once.
    let mark = log.mark();
    await down(5n, at);
    await up(5n, at);
    check(
      (await boolValue(client, checkbox)) === false,
      "In-bounds tap missed the checkbox",
    );
    const tapCommits = await eventually(
      p,
      () => {
        const found = commitsSince(log, mark, checkbox);
        return found.length > 0 ? found : undefined;
      },
      () => "The in-bounds tap published no commit",
    );
    check(
      tapCommits.length === 1 && changedValue(tapCommits[0]!) === false,
      `The tap did not change the value exactly once: ${encoded(tapCommits)}`,
    );

    // Escape releases focus. Keys other than traversal then find no target;
    // Tab enters the panel at its first control without a pointer, Space
    // toggles the focused checkbox, and Escape releases focus again.
    await send({ kind: "key", key: "escape" });
    check(
      !(await control(client, checkbox)).focused,
      "Escape did not release the pointer-press focus",
    );
    const enterWithoutFocus = await send({ kind: "key", key: "enter" });
    check(
      enterWithoutFocus.disposition === "unhandled" &&
        (await boolValue(client, checkbox)) === false,
      `Unfocused Enter was handled: ${encoded(enterWithoutFocus)}`,
    );
    await send({ kind: "key", key: "tab" });
    check(
      (await control(client, checkbox)).focused,
      "Tab without focus did not enter the checkbox",
    );
    await send({ kind: "key", key: "space" });
    check(
      (await boolValue(client, checkbox)) === true,
      "Focused Space missed the checkbox",
    );
    await send({ kind: "key", key: "escape" });
    check(
      !(await control(client, checkbox)).focused,
      "Escape did not release keyboard focus",
    );

    // Text input: keep both controls inside the evaluated panel, with the
    // root column as a focus scope that holds every control.
    const field = aliasId(
      successfulBatch(
        await client.batch([
          ...setFields(client, checkbox, "GuiLayout", { height: 1.5 }),
          createEntity(1, "gui-input-text"),
          insertComponent(client, "GuiTextInput", alias(1), {
            text: "",
            placeholder: "",
          }),
          insertComponent(client, "GuiLayout", alias(1), {
            width: 4,
            height: 1.5,
          }),
          place(alias(1), handle(root)),
          insertComponent(client, "GuiBehavior", handle(root), {
            enabled: true,
            visible: true,
            focus_scope: true,
          }),
        ]),
      ),
      1,
    );
    await log.watch(field);
    await client.waitForFrame();
    await p.frame();
    const focusedControl = async () => {
      if ((await control(client, checkbox)).focused) return "checkbox";
      if ((await control(client, field)).focused) return "field";
      return "none";
    };

    // Reverse traversal: BackTab without focus enters the last control and
    // moves backward in tree order, wrapping at the start; Tab wraps forward.
    const traversal: string[] = [];
    for (const key of ["backTab", "backTab", "backTab", "tab"] as const) {
      await send({ kind: "key", key });
      traversal.push(await focusedControl());
    }
    check(
      encoded(traversal) ===
        encoded(["field", "checkbox", "field", "checkbox"]),
      `Keyboard traversal order is wrong: ${encoded(traversal)}`,
    );
    await send({ kind: "key", key: "escape" });
    check(
      (await focusedControl()) === "none",
      "Escape did not release keyboard focus",
    );

    // Native text: every edit carries the fence of the state it observed.
    await send({ kind: "key", key: "tab" });
    await send({ kind: "key", key: "tab" });
    await nativeText(p, (state) => state?.fence.target.entity === field);
    const edit = (value: GuiNativeEdit, fence = p.input.nativeText?.fence) => {
      check(fence, "The field holds no native text focus");
      return p.input.editText(fence, value).catch((error: unknown) => {
        throw new Error(`Native edit ${encoded(value)} failed`, {
          cause: error,
        });
      });
    };
    await edit({ kind: "text", text: "Hi" });
    check(
      (await textValue(client, field)) === "Hi",
      "Typed text missed the field",
    );
    await edit({ kind: "selection", start: 1, end: 2 });
    await edit({ kind: "text", text: "p" });
    check(
      (await textValue(client, field)) === "Hp",
      "Selection replace missed",
    );
    await edit({
      kind: "composition",
      text: "世界",
      caretStart: 6,
      caretEnd: 6,
    });
    await edit({ kind: "commitComposition" });
    check(
      (await textValue(client, field)) === "Hp世界",
      "Composition commit missed",
    );
    await edit({ kind: "composition", text: "??", caretStart: 2, caretEnd: 2 });
    await edit({ kind: "cancelComposition" });
    check(
      (await textValue(client, field)) === "Hp世界",
      "Cancelled composition committed",
    );

    // Transient caret, selection and provisional work never touches the
    // committed value.
    const transientMark = log.mark();
    await edit({ kind: "key", key: "left" });
    await edit({ kind: "selection", start: 1, end: 2 });
    await edit({ kind: "composition", text: "zz", caretStart: 2, caretEnd: 2 });
    const transient = await control(client, field);
    await p.frame();
    check(
      transient.value.kind === "text" &&
        transient.value.value === "Hp世界" &&
        commitsSince(log, transientMark).length === 0,
      `Transient text state leaked into the commit: ${encoded({ transient, changes: commitsSince(log, transientMark) })}`,
    );
    await edit({ kind: "cancelComposition" });

    // A provisional fenced to its focus never writes a new target: start
    // composition on the field, move focus to the checkbox, then commit
    // with the old fence. Both values hold still.
    await edit({
      kind: "composition",
      text: "stale",
      caretStart: 5,
      caretEnd: 5,
    });
    const composingFence = p.input.nativeText!.fence;
    await send({ kind: "key", key: "backTab" });
    await nativeText(p, (state) => state === null);
    const staleCommit = await edit(
      { kind: "commitComposition" },
      composingFence,
    );
    check(
      staleCommit.applied === 0 &&
        staleCommit.rejected === 1 &&
        (await textValue(client, field)) === "Hp世界" &&
        (await boolValue(client, checkbox)) === true,
      `Stale composition wrote across focus: ${encoded(staleCommit)}`,
    );
    // Refocus with a tap: a rejected stale edit currently also drops the
    // context's traversal focus (reported separately), so Tab would re-enter
    // at the first control instead of continuing from the checkbox.
    const fieldPoint = p.point(0, [2, 2.25]);
    const tapField = async (pointer: bigint) => {
      await down(pointer, fieldPoint);
      await up(pointer, fieldPoint);
      return await nativeText(
        p,
        (state) => state?.fence.target.entity === field,
        async () => ({
          focus: await focusedControl(),
          field: await control(client, field),
        }),
      );
    };
    const refocused = await tapField(6n);
    check(
      refocused!.composition === undefined,
      "A composition survived its focus loss",
    );

    // A paste stamped before focus moved to the checkbox conflicts and
    // writes neither control.
    const stalePaste = refocused!.fence;
    await send({ kind: "key", key: "backTab" });
    await nativeText(p, (state) => state === null);
    const pasted = await edit({ kind: "text", text: "pasted" }, stalePaste);
    check(
      pasted.rejected === 1 &&
        pasted.applied === 0 &&
        (await textValue(client, field)) === "Hp世界" &&
        (await boolValue(client, checkbox)) === true,
      `A stale paste wrote a control: ${encoded(pasted)}`,
    );

    // Fences are strict: of two keystrokes stamped with the same observed
    // state, the first commits and the second conflicts instead of rebasing
    // onto the first; the sender resends it against the refreshed state.
    await tapField(7n);
    await edit({ kind: "key", key: "end" });
    const typing = p.input.nativeText!.fence;
    const chained = await Promise.all([
      edit({ kind: "text", text: "!" }, typing),
      edit({ kind: "text", text: "?" }, typing),
    ]);
    check(
      chained[0]!.applied === 1 &&
        chained[1]!.applied === 0 &&
        chained[1]!.rejected === 1 &&
        (await textValue(client, field)) === "Hp世界!",
      `A keystroke stamped with a consumed fence was rebased: ${encoded(chained)} -> ${await textValue(client, field)}`,
    );
    await edit({ kind: "text", text: "?" });
    check(
      (await textValue(client, field)) === "Hp世界!?",
      "A keystroke resent against the refreshed fence was lost",
    );

    // A stamped range after an equal-length external replacement conflicts,
    // and the replacement refreshes the native text without new input.
    const beforeReplace = (await nativeText(
      p,
      (state) => state?.text === "Hp世界!?",
    ))!.fence;
    successfulBatch(
      await client.batch(
        setFields(client, field, "GuiTextInput", { text: "Hp世" }),
      ),
    );
    const replaced = (await nativeText(p, (state) => state?.text === "Hp世"))!;
    check(
      replaced.fence.generation !== beforeReplace.generation,
      "An external replacement kept the native text generation",
    );
    const lateEdits = [
      await edit({ kind: "selection", start: 1, end: 2 }, beforeReplace),
      await edit({ kind: "text", text: "late" }, beforeReplace),
    ];
    check(
      lateEdits.every((outcome) => outcome.rejected === 1) &&
        (await textValue(client, field)) === "Hp世",
      `Stamped edits rebased onto replaced text: ${encoded(lateEdits)}`,
    );
    await edit({ kind: "text", text: "界" }, replaced.fence);
    check(
      (await textValue(client, field)) === "Hp世界",
      "An edit stamped against the refreshed text missed",
    );

    // Enter on the focused field submits the committed text exactly once,
    // with its logical ancestry; Enter during an open composition belongs to
    // the IME and submits nothing.
    mark = log.mark();
    await edit({ kind: "key", key: "enter" });
    const submitted = await eventually(
      p,
      () => {
        const found = submissionsSince(log, mark);
        return found.length > 0 ? found : undefined;
      },
      () => "Enter on the focused field published no submission",
    );
    const submission = submitted[0]!.effect;
    check(
      submitted.length === 1 &&
        submission.kind === "submitted" &&
        submission.text === "Hp世界" &&
        encoded(submitted[0]!.ancestry) === encoded([root, field]),
      `Unexpected submission: ${encoded(submitted)}`,
    );
    await edit({ kind: "composition", text: "zz", caretStart: 2, caretEnd: 2 });
    mark = log.mark();
    await edit({ kind: "key", key: "enter" });
    await edit({ kind: "cancelComposition" });
    // One more ordered submission lets any stray submission publish first.
    await edit({ kind: "key", key: "enter" });
    const afterComposition = await eventually(
      p,
      () => {
        const found = submissionsSince(log, mark);
        return found.length > 0 ? found : undefined;
      },
      () => "Enter after the cancelled composition published no submission",
    );
    check(
      afterComposition.length === 1,
      `Enter during composition submitted: ${afterComposition.length} submissions`,
    );

    // A delayed native range is fenced to its generation: an equal-length
    // client write ("Hp世界" and "Hpqrstuv" are both 8 bytes) moves the
    // generation, so the stale range conflicts instead of rebasing and the
    // next insert lands at the reset end.
    const rangeFence = p.input.nativeText!.fence;
    successfulBatch(
      await client.batch(
        setFields(client, field, "GuiTextInput", { text: "Hpqrstuv" }),
      ),
    );
    await nativeText(p, (state) => state?.text === "Hpqrstuv");
    const staleRange = await edit(
      { kind: "selection", start: 1, end: 2 },
      rangeFence,
    );
    await edit({ kind: "text", text: "!" });
    check(
      staleRange.rejected === 1 &&
        (await textValue(client, field)) === "Hpqrstuv!",
      `Stale selection rebased onto replaced text: ${encoded(staleRange)}`,
    );

    // A provisional fenced to its generation never commits over an
    // equal-length client write ("Hpqrstuv!" and "123456789" are both
    // 9 bytes): the commit conflicts, then a fresh provisional commits
    // exactly once.
    await edit({ kind: "composition", text: "zz", caretStart: 2, caretEnd: 2 });
    const provisional = p.input.nativeText!.fence;
    successfulBatch(
      await client.batch(
        setFields(client, field, "GuiTextInput", { text: "123456789" }),
      ),
    );
    await nativeText(p, (state) => state?.text === "123456789");
    const staleProvisional = await edit(
      { kind: "commitComposition" },
      provisional,
    );
    check(
      staleProvisional.rejected === 1 &&
        (await textValue(client, field)) === "123456789",
      `Stale composition committed over replaced text: ${encoded(staleProvisional)}`,
    );
    await edit({ kind: "cancelComposition" });
    await edit({ kind: "composition", text: "zz", caretStart: 2, caretEnd: 2 });
    await edit({ kind: "commitComposition" });
    check(
      (await textValue(client, field)) === "123456789zz",
      "Composition commit missed or duplicated",
    );

    // Every value change is observed whoever wrote it: typed text, client
    // writes of a checkbox, slider and TextInput, and a semantic action; a
    // stale compare-and-set conflicts and changes nothing.
    check(
      commitsSince(log, LOG_START, field).length > 0,
      "Typed text published no value change",
    );
    const slider = aliasId(
      successfulBatch(
        await client.batch([
          createEntity(1, "gui-input-slider"),
          insertComponent(client, "GuiSlider", alias(1), {
            value: 0.25,
            min: 0,
            max: 1,
            step: 0,
          }),
          insertComponent(client, "GuiLayout", alias(1), {
            width: 4,
            height: 0.5,
          }),
          place(alias(1), handle(root)),
        ]),
      ),
      1,
    );
    await log.watch(slider);
    for (const { entity, name, values, expected } of [
      {
        entity: checkbox,
        name: "GuiCheckbox",
        values: { checked: false },
        expected: false,
      },
      {
        entity: slider,
        name: "GuiSlider",
        values: { value: 0.75 },
        expected: 0.75,
      },
      {
        entity: field,
        name: "GuiTextInput",
        values: { text: "external" },
        expected: "external",
      },
    ] as const) {
      mark = log.mark();
      successfulBatch(
        await client.batch(setFields(client, entity, name, values)),
      );
      const published = await eventually(
        p,
        () => {
          const found = commitsSince(log, mark, entity);
          return found.length > 0 ? found : undefined;
        },
        () => `A client write of ${entity} published no value change`,
      );
      check(
        published.length === 1 && changedValue(published[0]!) === expected,
        `Unexpected value change for ${entity}: ${encoded(published)}`,
      );
    }
    await nativeText(p, (state) => state?.text === "external");
    const current = await control(client, checkbox);
    mark = log.mark();
    const staleReplacement = await compareAndSet(
      client,
      checkbox,
      "GuiCheckbox",
      "checked",
      true,
      true,
    );
    check(
      !staleReplacement.ok && staleReplacement.error.reason === "ValueMismatch",
      `A stale compare-and-set was accepted: ${encoded(staleReplacement)}`,
    );
    applied(await guiAction(client, current.target, { kind: "toggle" }));
    const semanticCommits = await eventually(
      p,
      () => {
        const found = commitsSince(log, mark, checkbox);
        return found.length > 0 ? found : undefined;
      },
      () => "A semantic toggle published no value change",
    );
    check(
      semanticCommits.length === 1 &&
        changedValue(semanticCommits[0]!) === true,
      `A stale compare-and-set published or the toggle was lost: ${encoded(semanticCommits)}`,
    );

    // No ScrollView sits under the panel's controls: the wheel is reported
    // unhandled for scene controls instead of scrolling anything.
    const unscrolled = await send({ kind: "wheel", point: at, delta: [0, -4] });
    check(
      unscrolled.disposition === "unhandled",
      `A wheel over non-scrollable content was handled: ${encoded(unscrolled)}`,
    );
    const valueChanges = commitsSince(log, LOG_START).length;
    await log.stop();
    await presentation.close();
    presentation = undefined;

    const scrolling = await exerciseGuiScrolling(host);
    const removal = await exerciseGuiRemoval(host, fontBytes);
    const keyboardEntry = await exerciseGuiKeyboardEntry(host);
    completed = true;
    return {
      traversal,
      submissions: submitted.length,
      valueChanges,
      unhandledScroll: unscrolled.disposition,
      scrolling,
      removal,
      keyboardEntry,
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

/**
 * Nested ScrollViews in their own World, observed through committed control
 * values: a tap at a fixed logical point toggles whichever checkbox the
 * committed scroll offsets moved under it.
 *
 * The 4 x 3 panel holds an outer ScrollView (4 x 3 viewport over 5 units of
 * content) whose content starts with an inner ScrollView (4 x 2 viewport over
 * 3 units), then a narrow 1 x 2 spacer. The inner checkbox rides inner
 * content at y 2..3 and the outer checkbox rides outer content at y 4..5, so
 * the inner view can scroll by 1 and the outer view by 2.
 */
async function exerciseGuiScrolling(host: GuiHost) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-scrolling",
      canvas: PANEL_CANVAS,
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const layout = (
      at: number,
      values: Readonly<Record<string, number>>,
    ): Command => insertComponent(client, "GuiLayout", alias(at), values);
    const outcome = successfulBatch(
      await client.batch(
        panelRoot(client, "gui-scrolling-panel", [
          createEntity(2, "gui-scrolling-outer"),
          insertComponent(client, "GuiScrollView", alias(2), { axis: 1 }),
          layout(2, { width: 4, height: 3 }),
          place(alias(2), alias(1)),
          createEntity(3, "gui-scrolling-outer-content"),
          layout(3, { kind: LAYOUT.column }),
          place(alias(3), alias(2)),
          createEntity(4, "gui-scrolling-inner"),
          insertComponent(client, "GuiScrollView", alias(4), { axis: 1 }),
          layout(4, { width: 4, height: 2 }),
          place(alias(4), alias(3)),
          createEntity(5, "gui-scrolling-inner-content"),
          layout(5, { kind: LAYOUT.column }),
          place(alias(5), alias(4)),
          createEntity(6, "gui-scrolling-inner-spacer"),
          layout(6, { kind: LAYOUT.sizedBox, width: 4, height: 2 }),
          place(alias(6), alias(5)),
          createEntity(7, "gui-scrolling-inner-checkbox"),
          insertComponent(client, "GuiCheckbox", alias(7)),
          layout(7, { width: 4, height: 1 }),
          place(alias(7), alias(5)),
          createEntity(8, "gui-scrolling-outer-spacer"),
          layout(8, { kind: LAYOUT.sizedBox, width: 1, height: 2 }),
          place(alias(8), alias(3)),
          createEntity(9, "gui-scrolling-outer-checkbox"),
          insertComponent(client, "GuiCheckbox", alias(9)),
          layout(9, { width: 4, height: 1 }),
          place(alias(9), alias(3)),
        ]),
      ),
    );
    const outerView = aliasId(outcome, 2);
    const innerView = aliasId(outcome, 4);
    const inner = aliasId(outcome, 7);
    const outer = aliasId(outcome, 9);
    presentation = await presentGui(host, [{ child: created.reference }]);
    const p = presentation;
    await client.waitForFrame();
    await p.frame();
    const point = (logical: [number, number]) => p.point(0, logical);
    const tap = async (pointer: bigint, logical: [number, number]) => {
      await p.send({ kind: "pointerDown", pointer, point: point(logical) });
      await p.send({ kind: "pointerUp", pointer, point: point(logical) });
    };
    const wheel = (delta: [number, number], at: [number, number] = [2, 1]) =>
      p.send({ kind: "wheel", point: point(at), delta });
    const offset = async (entity: bigint) => {
      const state = await control(client, entity);
      check(
        state.value.kind === "scroll",
        `ScrollView ${entity} has no scroll value`,
      );
      return state.value.offset[1];
    };

    // Two wheel samples over the inner view, pipelined so they may route in
    // one Host tick: the inner view takes its 1 unit and the rest passes
    // outward in both orders. The outer view then holds 2, lifting its
    // checkbox to y 2..3.
    const replies = await Promise.all([wheel([0, 1.5]), wheel([0, 1.5])]);
    check(
      replies.every(routed),
      `Nested wheel scrolling was unhandled: ${encoded(replies)}`,
    );
    check(
      Math.abs((await offset(innerView)) - 1) < 1e-4 &&
        Math.abs((await offset(outerView)) - 2) < 1e-4,
      `Same-tick wheel chaining lost movement: ${await offset(innerView)}/${await offset(outerView)}`,
    );
    await p.frame();
    await tap(1n, [2, 2.5]);
    check(
      (await boolValue(client, outer)) && !(await boolValue(client, inner)),
      "Same-tick wheel chaining lost movement before the outer ScrollView",
    );

    // A drag over plain outer content scrolls it by the dragged distance:
    // two units down return the outer view to 0, bringing the inner view
    // (still scrolled by 1) back with its checkbox at y 1..2.
    const drag = async (
      pointer: bigint,
      from: [number, number],
      to: [number, number],
    ) => {
      const replies = [
        await p.send({ kind: "pointerDown", pointer, point: point(from) }),
        await p.send({
          kind: "pointerMove",
          pointer,
          point: point([from[0], (from[1] + to[1]) / 2]),
        }),
        await p.send({ kind: "pointerMove", pointer, point: point(to) }),
        await p.send({ kind: "pointerUp", pointer, point: point(to) }),
      ];
      check(
        replies.every(routed),
        `A ScrollView drag was not routed: ${encoded(replies)}`,
      );
      await p.frame();
    };
    await drag(2n, [2, 0.5], [2, 2.5]);
    await tap(3n, [2, 1.5]);
    check(
      (await boolValue(client, inner)) && (await boolValue(client, outer)),
      "A drag over ScrollView content did not scroll it",
    );

    // A drag starting on the inner checkbox wins over its tap: the inner
    // view is already at its end, so the unit of upward travel passes to the
    // outer view and the checkbox commits nothing. The outer shift then
    // lifts the checkbox to y 0..1.
    await drag(4n, [2, 1.5], [2, 0.5]);
    check(
      await boolValue(client, inner),
      "A drag starting on a checkbox committed its toggle",
    );
    await tap(5n, [2, 0.5]);
    check(
      !(await boolValue(client, inner)),
      "Drag travel beyond the inner ScrollView did not pass outward",
    );

    // Clips follow the outer scroll: with the outer view at 1 the inner
    // viewport spans y -1..1. Wheeling the inner view back to 0 leaves its
    // checkbox at y 1..2, below the moved viewport, so a tap there reaches
    // only plain outer content and toggles nothing.
    await wheel([0, -1], [2, 0.5]);
    await p.frame();
    await tap(6n, [2, 1.5]);
    check(
      !(await boolValue(client, inner)),
      "A checkbox below the moved inner viewport was still hittable",
    );

    // Scrolling toward the start passes the inner view (already at 0) and
    // returns the outer view to 0, reporting the unconsumed unit; once every
    // view sits at its edge the whole movement is reported unhandled.
    const toStart = await wheel([0, -2]);
    check(
      routed(toStart) &&
        toStart.remaining !== undefined &&
        Math.abs(toStart.remaining[1] + 1) < 1e-4,
      `A partially consumed wheel lost its remainder: ${encoded(toStart)}`,
    );
    const atEdge = await wheel([0, -2]);
    check(
      atEdge.remaining !== undefined &&
        Math.abs(atEdge.remaining[1] + 2) < 1e-4,
      `A wheel at every ScrollView edge was consumed: ${encoded(atEdge)}`,
    );

    // Scroll bars: the outer view's vertical bar spans x 3.85..4 with a
    // 1.8-unit thumb travelling 1.2 units over its capacity of 2.
    const outerState = await control(client, outerView);
    const innerState = await control(client, innerView);
    check(
      (await offset(outerView)) === 0 &&
        outerState.scroll?.capacity[1] === 2 &&
        innerState.scroll?.capacity[1] === 1,
      `Unexpected scroll positions: ${encoded({ outerState, innerState })}`,
    );
    const press = async (
      pointer: bigint,
      points: readonly [number, number][],
    ) => {
      const [first, ...rest] = points;
      const replies = [
        await p.send({ kind: "pointerDown", pointer, point: point(first!) }),
      ];
      for (const logical of rest)
        replies.push(
          await p.send({ kind: "pointerMove", pointer, point: point(logical) }),
        );
      replies.push(
        await p.send({
          kind: "pointerUp",
          pointer,
          point: point(points.at(-1)!),
        }),
      );
      check(
        replies.every(routed),
        `A scroll bar press was not routed: ${encoded(replies)}`,
      );
      await p.frame();
    };
    // A track press below the thumb pages by the 3-unit viewport, clamped
    // to the end.
    await press(7n, [[3.92, 2.5]]);
    const paged = await offset(outerView);
    check(Math.abs(paged - 2) < 1e-4, `Track paging missed: ${paged}`);
    // Dragging the thumb (now at y 1.2..3) up by its whole travel returns
    // the view to the start, with the pointer leaving the bar on the way.
    await press(8n, [
      [3.92, 2],
      [2, 1.4],
      [2, 0.8],
    ]);
    const dragged = await offset(outerView);
    check(Math.abs(dragged) < 1e-4, `Thumb drag missed: ${dragged}`);
    check(
      !(await boolValue(client, inner)) && (await boolValue(client, outer)),
      "A scroll bar press reached content under the bar",
    );
    completed = true;
    return {
      outerCapacity: outerState.scroll?.capacity[1],
      innerCapacity: innerState.scroll?.capacity[1],
      partialRemainder: toStart.remaining,
      edgeRemainder: atEdge.remaining,
      paged,
      dragged,
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

/**
 * Remove GUI targets while they hold interaction state, in their own World:
 * a focused TextInput with an open composition, a slider captured mid-drag,
 * and then the whole panel subtree mid-interaction. Each removal clears the
 * native text focus and capture, publishes no later effect for the removed
 * target, routes later input over the vacated area to its new occupant and
 * rejects the removed targets.
 *
 * The 4 x 3 panel stacks two 4 x 1.5 controls: the upper half at y 0..1.5
 * and the lower half at y 1.5..3.
 */
async function exerciseGuiRemoval(host: GuiHost, fontBytes: ArrayBuffer) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let presentation: PresentedGui | undefined;
  let completed = false;
  try {
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-removal",
      canvas: PANEL_CANVAS,
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const log = await recordEffects(client);
    const font = await client.createAsset(17, fontBytes);
    const half = { width: 4, height: 1.5 } as const;
    const populate = async (name: string) => {
      const outcome = successfulBatch(
        await client.batch(
          panelRoot(client, name, [
            insertComponent(client, "GuiFont", alias(1), {
              source: font.source,
              font_size: 0.6,
            }),
            createEntity(2, `${name}-text`),
            insertComponent(client, "GuiTextInput", alias(2), {
              text: "",
              placeholder: "",
            }),
            insertComponent(client, "GuiLayout", alias(2), half),
            place(alias(2), alias(1)),
            createEntity(3, `${name}-slider`),
            insertComponent(client, "GuiSlider", alias(3), {
              value: 0,
              min: 0,
              max: 1,
              step: 0,
            }),
            insertComponent(client, "GuiLayout", alias(3), half),
            place(alias(3), alias(1)),
          ]),
        ),
      );
      return [1, 2, 3].map((index) => aliasId(outcome, index)) as [
        bigint,
        bigint,
        bigint,
      ];
    };
    let [root, text, slider] = await populate("gui-removal-panel");
    await log.watch(slider);
    presentation = await presentGui(host, [{ child: created.reference }]);
    const p = presentation;
    await loadedFont(client);
    await client.waitForFrame();
    await p.frame();
    const upper = p.point(0, [2, 0.75]);
    const lower = p.point(0, [2, 2.25]);
    const effectsOf = (mark: LogMark, entity: bigint) =>
      log.effects
        .slice(mark.effects)
        .filter(
          (effect) =>
            effect.target.entity === entity &&
            effect.effect.kind !== "interactionChanged" &&
            effect.effect.kind !== "focusChanged",
        );
    const tapCommits = async (
      pointer: bigint,
      point: [number, number],
      entity: bigint,
    ) => {
      const mark = log.mark();
      await p.send({ kind: "pointerDown", pointer, point });
      await p.send({ kind: "pointerUp", pointer, point });
      return await eventually(
        p,
        () => {
          const found = commitsSince(log, mark, entity);
          return found.length > 0 ? found : undefined;
        },
        () => `A tap over the vacated area did not reach ${entity}`,
      );
    };

    // A focused TextInput with an open composition is removed: native text
    // focus clears, a late commit with its fence conflicts, and the typed
    // prefix never publishes an effect for the removed control.
    await p.send({ kind: "pointerDown", pointer: 9n, point: upper });
    await p.send({ kind: "pointerUp", pointer: 9n, point: upper });
    await nativeText(p, (state) => state?.fence.target.entity === text);
    await p.input.editText(p.input.nativeText!.fence, {
      kind: "text",
      text: "ab",
    });
    await p.input.editText(p.input.nativeText!.fence, {
      kind: "composition",
      text: "zz",
      caretStart: 2,
      caretEnd: 2,
    });
    const focus = (await nativeText(
      p,
      (state) => state?.composition?.text === "zz",
    ))!;
    const removedText = await control(client, text);
    const textMark = log.mark();
    successfulBatch(
      await client.batch([{ kind: "delete", entity: handle(text) }]),
    );
    await nativeText(p, (state) => state === null);
    const lateCommit = await p.input.editText(focus.fence, {
      kind: "commitComposition",
    });
    const lateText = await p.input.editText(focus.fence, {
      kind: "text",
      text: "late",
    });
    check(
      lateCommit.applied === 0 &&
        lateCommit.rejected === 1 &&
        lateText.applied === 0 &&
        lateText.rejected === 1,
      `A native edit after removal found a target: ${encoded([lateCommit, lateText])}`,
    );
    const staleText = await guiAction(client, removedText.target, {
      kind: "text",
      value: "stale",
    });
    check(
      rejectedFor(staleText, "StaleTarget"),
      `The removed TextInput target was accepted: ${encoded(staleText)}`,
    );

    // A slider captured mid-drag is removed: the Host cancels the capture,
    // the rest of the drag commits nothing, and a new checkbox in the
    // vacated area takes later input. The column reflowed the slider into
    // the upper half.
    await client.waitForFrame();
    await p.frame();
    let sliderMark = log.mark();
    const sliderState = await control(client, slider);
    await p.send({
      kind: "pointerDown",
      pointer: 1n,
      point: p.point(0, [1, 0.75]),
    });
    await p.send({
      kind: "pointerMove",
      pointer: 1n,
      point: p.point(0, [3, 0.75]),
    });
    const draggedCommits = await eventually(
      p,
      () => {
        const found = commitsSince(log, sliderMark, slider);
        return found.length > 0 ? found : undefined;
      },
      () => "The captured slider drag committed no value",
    );
    sliderMark = log.mark();
    const cancelMark = p.cancellations.length;
    successfulBatch(
      await client.batch([{ kind: "delete", entity: handle(slider) }]),
    );
    const afterSlider = [
      await p.send({
        kind: "pointerMove",
        pointer: 1n,
        point: p.point(0, [2, 0.75]),
      }),
      await p.send({ kind: "pointerUp", pointer: 1n, point: upper }),
    ];
    const replacementCheckbox = aliasId(
      successfulBatch(
        await client.batch([
          createEntity(1, "gui-removal-checkbox"),
          insertComponent(client, "GuiCheckbox", alias(1)),
          insertComponent(client, "GuiLayout", alias(1), {
            width: 4,
            height: 3,
          }),
          place(alias(1), handle(root), null),
        ]),
      ),
      1,
    );
    await log.watch(replacementCheckbox);
    await client.waitForFrame();
    await p.frame();
    const vacated = await tapCommits(2n, upper, replacementCheckbox);
    check(
      effectsOf(textMark, text).length === 0 &&
        effectsOf(sliderMark, slider).length === 0 &&
        p.cancellations
          .slice(cancelMark)
          .some((cancellation) => cancellation.pointers.includes(1n)) &&
        afterSlider.every((reply) => reply.applied === 0) &&
        vacated.length === 1 &&
        changedValue(vacated[0]!) === true,
      `Removed controls left stale effects or misrouted later input: ${encoded({ text: effectsOf(textMark, text), slider: effectsOf(sliderMark, slider), afterSlider, vacated, cancellations: p.cancellations.slice(cancelMark) })}`,
    );
    const staleSlider = await guiAction(client, sliderState.target, {
      kind: "scalar",
      value: 0.5,
    });
    check(
      rejectedFor(staleSlider, "StaleTarget"),
      `The removed slider target was accepted: ${encoded(staleSlider)}`,
    );

    // The whole panel subtree is removed while a TextInput composes and a
    // press holds the checkbox: native focus and capture clear, the press
    // completes nothing, and every old target is rejected. A fresh panel
    // presented on the same Surface takes the same input.
    const second = aliasId(
      successfulBatch(
        await client.batch([
          ...setFields(client, replacementCheckbox, "GuiLayout", {
            height: 1.5,
          }),
          createEntity(1, "gui-removal-second-text"),
          insertComponent(client, "GuiTextInput", alias(1), {
            text: "",
            placeholder: "",
          }),
          insertComponent(client, "GuiLayout", alias(1), half),
          place(alias(1), handle(root)),
        ]),
      ),
      1,
    );
    await client.waitForFrame();
    await p.frame();
    await p.send({ kind: "pointerDown", pointer: 8n, point: lower });
    await p.send({ kind: "pointerUp", pointer: 8n, point: lower });
    await nativeText(p, (state) => state?.fence.target.entity === second);
    await p.input.editText(p.input.nativeText!.fence, {
      kind: "composition",
      text: "qq",
      caretStart: 2,
      caretEnd: 2,
    });
    const secondFocus = (await nativeText(
      p,
      (state) => state?.composition?.text === "qq",
    ))!;
    await p.send({ kind: "pointerDown", pointer: 3n, point: upper });
    const oldTargets = await Promise.all(
      [replacementCheckbox, second].map((entity) => control(client, entity)),
    );
    const rootMark = log.mark();
    const oldEntities = [root, replacementCheckbox, second];
    successfulBatch(
      await client.batch(
        oldEntities.map((entity) => ({
          kind: "delete",
          entity: handle(entity),
        })),
      ),
    );
    await nativeText(p, (state) => state === null);
    // The held press completes nothing: its release either finds no capture
    // or is refused because the captured path no longer exists.
    const settledApplied = (pending: Promise<GuiInputRoutingOutcome>) =>
      pending.then(
        (outcome) => ({ outcome: encoded(outcome), applied: outcome.applied }),
        (error: unknown) => ({
          outcome: String(
            error instanceof Error && error.cause ? error.cause : error,
          ),
          applied: 0,
        }),
      );
    const afterRoot = [
      await settledApplied(
        p.send({ kind: "pointerUp", pointer: 3n, point: upper }),
      ),
      await settledApplied(
        p.input.editText(secondFocus.fence, { kind: "commitComposition" }),
      ),
    ];
    const staleRoot = await Promise.all(
      oldTargets.map((state) =>
        guiAction(client, state.target, { kind: "focus" }),
      ),
    );
    [root, text, slider] = await populate("gui-removal-fresh");
    await log.watch(slider);
    check(
      !oldEntities.includes(root) &&
        !oldEntities.includes(text) &&
        !oldEntities.includes(slider),
      "A fresh panel reused a removed entity identity",
    );
    // The World canvas presents the fresh top-level panel in its place.
    await client.waitForFrame();
    await p.frame();
    // The upper half now holds a fresh TextInput: a tap focuses it with no
    // composition, and the lower slider commits.
    await p.send({ kind: "pointerDown", pointer: 4n, point: upper });
    await p.send({ kind: "pointerUp", pointer: 4n, point: upper });
    const freshFocus = (await nativeText(
      p,
      (state) => state?.fence.target.entity === text,
    ))!;
    const replacement = await tapCommits(5n, p.point(0, [3, 2.25]), slider);
    const staleRootEffects = log.effects
      .slice(rootMark.effects)
      .filter((effect) => oldEntities.includes(effect.target.entity))
      .filter(
        (effect) =>
          effect.effect.kind !== "interactionChanged" &&
          effect.effect.kind !== "focusChanged",
      );
    check(
      staleRootEffects.length === 0 &&
        afterRoot[0]!.applied === 0 &&
        afterRoot[1]!.applied === 0 &&
        staleRoot.every((terminal) => rejectedFor(terminal, "StaleTarget")) &&
        freshFocus.composition === undefined &&
        replacement.length > 0,
      `Removing the panel left stale effects, accepted stale input or targets: ${encoded({ staleRootEffects, afterRoot, staleRoot, freshFocus })}`,
    );
    await log.stop();
    completed = true;
    return {
      textRemoval: {
        lateCommit: [lateCommit.applied, lateCommit.rejected],
        lateStampedEdit: [lateText.applied, lateText.rejected],
      },
      sliderRemoval: {
        draggedCommits: draggedCommits.length,
        afterRemoval: afterSlider.map((reply) => reply.disposition),
        cancellations: p.cancellations.slice(cancelMark),
      },
      rootRemoval: {
        afterRemoval: afterRoot.map((reply) => reply.outcome),
        staleTargetsRejected: staleRoot.length,
      },
    };
  } finally {
    await presentation?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

/**
 * Keyboard entry and cross-panel traversal follow the camera of a presented
 * spatial root. Three 2 x 1 Surface panels each attach their own GUI World
 * holding checkboxes A and B: a back-facing panel 2 m in front of the
 * camera, created first, then front-facing panels 10 m and 6 m away. Tab
 * from no focus enters the nearest front-facing panel, traversal crosses the
 * front-facing panels by distance before the back-facing one, and turning
 * the camera around makes the formerly back-facing panel the only
 * front-facing entry.
 */
async function exerciseGuiKeyboardEntry(host: GuiHost) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let completed = false;
  let input: PresentedInput | undefined;
  try {
    const panels: { name: string; boxes: [bigint, bigint] }[] = [];
    const anchors: Command[] = [];
    const scene = await host.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
      symbolicId: "gui-keyboard-scene",
    });
    worlds.push(scene.reference);
    const parent = await openGui(host, scene.reference);
    sessions.push(parent);
    const clients: GuiTestClient[] = [];
    for (const [index, { name, placement }] of [
      { name: "back", placement: { z: 8, qy: 1, qw: 0 } },
      { name: "far", placement: { z: 0 } },
      { name: "near", placement: { z: 4 } },
    ].entries()) {
      const created = await host.createWorld({
        selectedSystems: selectSystems(GUI, LIFECYCLE),
        symbolicId: `gui-keyboard-${name}`,
        canvas: { extent: [2, 1], unitsPerMetre: 1 },
      });
      worlds.push(created.reference);
      const client = await openGui(host, created.reference);
      sessions.push(client);
      clients.push(client);
      const checkbox = (at: number): Command[] => [
        createEntity(at, `gui-keyboard-${name}-${at}`),
        insertComponent(client, "GuiCheckbox", alias(at)),
        insertComponent(client, "GuiLayout", alias(at), {
          width: 1,
          height: 1,
        }),
        place(alias(at), alias(1)),
      ];
      const outcome = successfulBatch(
        await client.batch([
          createEntity(1, `gui-keyboard-${name}-panel`),
          insertComponent(client, "GuiLayout", alias(1), {
            kind: LAYOUT.row,
            width: 2,
            height: 1,
          }),
          ...checkbox(2),
          ...checkbox(3),
        ]),
      );
      panels.push({
        name,
        boxes: [aliasId(outcome, 2), aliasId(outcome, 3)],
      });
      const anchor = alias(index + 2);
      anchors.push(
        createEntity(index + 2, `gui-keyboard-${name}-anchor`),
        insertComponent(parent, "Transform", anchor, placement),
        insertComponent(parent, "Surface", anchor, { width: 2, height: 1 }),
        {
          kind: "insertComponent",
          entity: anchor,
          component: parent.components.WorldAttachment!.id,
          fields: [
            {
              offset: parent.components.WorldAttachment!.fields.child!.offset,
              value: { kind: "world", value: created.reference },
            },
            {
              offset: parent.components.WorldAttachment!.fields.mode!.offset,
              value: { kind: "u32", value: 1 },
            },
          ],
        },
      );
    }
    // A perspective camera 10 m along +Z looks down -Z at every panel.
    const sceneOutcome = successfulBatch(
      await parent.batch([
        createEntity(1, "gui-keyboard-camera"),
        insertComponent(parent, "Transform", alias(1), { z: 10 }),
        insertComponent(parent, "Camera", alias(1), {
          projection: 0,
          fov_y: Math.PI / 4,
          near: 0.1,
          far: 100,
        }),
        ...anchors,
      ]),
    );
    const camera = aliasId(sceneOutcome, 1);
    input = await presentCamera(host, scene.reference, camera);
    const focused = async () => {
      for (const [index, panel] of panels.entries())
        for (const [box, entity] of panel.boxes.entries())
          if ((await control(clients[index]!, entity)).focused)
            return `${panel.name}:${"AB"[box]}`;
      return "none";
    };
    const press = async (key: "tab" | "backTab" | "escape") => {
      await input!.send({ kind: "key", key });
      return focused();
    };
    const enter = async (key: "tab" | "backTab") => {
      await press("escape");
      return press(key);
    };

    const entry = [await enter("tab"), await enter("backTab")];
    await enter("tab");
    const traversal: string[] = [];
    for (let step = 0; step < 6; step += 1) traversal.push(await press("tab"));
    check(
      encoded(entry) === encoded(["near:A", "near:B"]) &&
        encoded(traversal) ===
          encoded(["near:B", "far:A", "far:B", "back:A", "back:B", "near:A"]),
      `Keyboard entry ignored the view order: ${encoded({ entry, traversal })}`,
    );

    // Turning the camera around behind the panels leaves the formerly
    // back-facing panel as the only front-facing one.
    successfulBatch(
      await parent.batch(
        componentFields(parent, "Transform", { z: -10, qy: 1, qw: 0 }).map(
          (field) => ({
            kind: "setField",
            entity: handle(camera),
            component: parent.components.Transform!.id,
            field,
          }),
        ),
      ),
    );
    await parent.waitForFrame();
    await input.frame();
    const turned = await enter("tab");
    check(
      turned === "back:A",
      `Camera motion did not move keyboard entry: ${turned}`,
    );
    await press("escape");
    completed = true;
    return { entry, traversal, turned };
  } finally {
    await input?.close().catch(() => {});
    await cleanup(host, sessions, worlds, completed);
  }
}

interface PresentedInput {
  send(input: GuiPhysicalInput): Promise<GuiInputRoutingOutcome>;
  frame(): Promise<void>;
  close(): Promise<void>;
}

/** Select a camera output as the root presentation and open its input. */
async function presentCamera(
  host: GuiHost,
  world: WorldReference,
  camera: bigint,
): Promise<PresentedInput> {
  const output = await host.bindOutput(world, camera, "camera");
  const binding = await host.setRootOutput(output, VIEWPORT);
  const view = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
  await host.presentation.frame(view);
  const input = await host.input.open(view);
  return {
    send: (event) => input.send(event),
    async frame() {
      await host.presentation.frame(view);
    },
    async close() {
      await input.close().catch(() => {});
      await host.presentation.clear(view).catch(() => {});
    },
  };
}
