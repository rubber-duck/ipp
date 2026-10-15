import type {
  GuiNativeEdit,
  GuiObservedEffect,
  GuiTarget,
  GuiWorldClient,
} from "@ipp/client";
import type { SpecimenInteraction } from "../../skin-lab/specimen-session.js";
import { check } from "../../../harness/page/checks.js";
import {
  ACTION_SPECIMEN_EXTENT,
  actionSpecimenBounds,
} from "../actions-specimen.js";
import {
  GLASS_BUTTON_CASES,
  GLASS_TEXT_INPUT_CASES,
} from "../fixtures/action-cases.js";

function sameTarget(left: GuiTarget, right: GuiTarget): boolean {
  return (
    left.world.id === right.world.id &&
    left.world.incarnation === right.world.incarnation &&
    left.entity === right.entity &&
    left.component === right.component &&
    left.incarnation === right.incarnation
  );
}

/** Real input and retained-state checks shared by GLES and worker/WebGL. */
export async function glassActions(
  interaction: SpecimenInteraction,
  retheme: () => Promise<void>,
): Promise<Readonly<Record<string, boolean>>> {
  const { input, control, capture, client } = interaction;
  const effects: GuiObservedEffect[] = [];
  check("subscribeGuiEffects" in client, "Glass actions require GUI effects");
  const subscription = await (client as GuiWorldClient).subscribeGuiEffects(
    (effect) => effects.push(effect),
    { classes: "application" },
  );
  const click = async (family: "button" | "textInput", name: string) => {
    const cases =
      family === "button" ? GLASS_BUTTON_CASES : GLASS_TEXT_INPUT_CASES;
    const index = cases.findIndex((item) => item.name === name);
    check(index >= 0, `Unknown glass action case ${family}/${name}`);
    const [x, y, width, height] = actionSpecimenBounds(family, index);
    const point = [
      (x + width / 2) / ACTION_SPECIMEN_EXTENT[0],
      (y + height / 2) / ACTION_SPECIMEN_EXTENT[1],
    ] as const;
    const pointer = 73n;
    await input.send({ kind: "pointerMove", pointer, point });
    await input.send({ kind: "pointerDown", pointer, point });
    await input.send({ kind: "pointerUp", pointer, point });
    await capture();
  };
  const edit = async (value: GuiNativeEdit) => {
    const native = input.nativeText;
    check(native, "Native text focus was not established");
    const outcome = await input.editText(native.fence, value);
    check(
      outcome.disposition === "routed" && !outcome.error,
      `Native text edit refused: ${outcome.disposition}/${outcome.error ?? ""}`,
    );
    await capture();
  };
  const replace = async (text: string) => {
    const native = input.nativeText;
    check(native, "Native text buffer missing before replacement");
    await edit({
      kind: "selection",
      start: 0,
      end: new TextEncoder().encode(native.text).length,
    });
    await edit({ kind: "text", text });
  };
  try {
    const button = control("button-idle");
    check(
      await button.compareAndSet("selected", false, true),
      "Glass button selection write failed",
    );
    check(
      (await button.read()).selected === true,
      "Glass button did not retain native selection",
    );
    const disabled = control("button-disabled");
    const refused = await disabled.action({ kind: "press" });
    check(
      !refused.ok && refused.error.reason === "Unavailable",
      "Disabled glass button accepted a press action",
    );
    await click("button", "disabled");
    check(
      !effects.some(
        (event) =>
          event.target.entity === disabled.target.entity &&
          event.effect.kind === "pressed",
      ),
      "Disabled glass button emitted a press",
    );

    await click("textInput", "numeric");
    await replace("18.25");
    await input.send({ kind: "key", key: "enter" });
    await capture();
    const numeric = control("input-numeric");
    check(
      (await numeric.read()).value === 18.25,
      "Numeric glass input did not commit the actual native edit",
    );
    await replace("oops");
    await input.send({ kind: "key", key: "enter" });
    await capture();
    check(
      (await numeric.read()).value === 18.25,
      "Rejected numeric glass input changed its value",
    );
    check(
      effects.some(
        (event) =>
          event.target.entity === numeric.target.entity &&
          event.effect.kind === "rejected" &&
          event.effect.text === "oops",
      ),
      "Invalid numeric edit emitted no rejection effect",
    );

    await click("textInput", "masked");
    check(input.nativeText?.masked === true, "Masked glass input lost masking");
    await replace("Shadow");
    check(
      (await control("input-masked").read()).text === "Shadow",
      "Masked input did not retain its actual native value",
    );

    await click("textInput", "text");
    await replace("Glåss");
    const text = control("input-text");
    check(
      (await text.read()).text === "Glåss",
      "Glass input did not commit native Unicode typing",
    );
    await edit({ kind: "selection", start: 1, end: 4 });
    const before = input.nativeText;
    check(
      before && sameTarget(before.fence.target, text.target),
      "Native selection targets a different control",
    );
    check(
      before.selectionStart === 1 && before.selectionEnd === 4,
      "Native selection did not retain UTF-8 byte offsets",
    );
    await retheme();
    await capture();
    const after = input.nativeText;
    check(
      sameTarget(control("input-text").target, text.target),
      "Palette replacement changed the text-input incarnation",
    );
    check(
      sameTarget(control("button-idle").target, button.target),
      "Palette replacement changed the button incarnation",
    );
    check(
      after && sameTarget(after.fence.target, before.fence.target),
      "Palette replacement lost native text focus",
    );
    check(
      after.text === before.text &&
        after.selectionStart === before.selectionStart &&
        after.selectionEnd === before.selectionEnd,
      "Palette replacement changed the native text or selection",
    );
    check(
      (await text.read()).text === "Glåss" &&
        (await numeric.read()).value === 18.25 &&
        (await button.read()).selected === true,
      "Palette replacement changed committed control values",
    );
    return {
      nativeEditing: true,
      numericCommit: true,
      numericReject: true,
      maskedEditing: true,
      disabledRejection: true,
      nativeSelection: true,
      paletteRetainsIdentity: true,
      paletteRetainsFocus: true,
      paletteRetainsValues: true,
    };
  } finally {
    await subscription.unsubscribe();
  }
}
