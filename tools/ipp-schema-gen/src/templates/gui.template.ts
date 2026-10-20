import type {
  GuiTarget,
  GuiClientInput,
  GuiActiveItemRecord,
  GuiFocusRecord,
  GuiPointerRecord,
  GuiCommittedEffect,
  GuiObservationRecord,
  GuiObservationRequest,
  GuiObservedEffect,
  GuiObservationOptions,
  GuiEffectSubscription,
  GuiAction,
} from "./gui-types.js";
export * from "./gui-types.js";

function readGuiTarget(reader: Reader): GuiTarget {
  return {
    world: readWorldReference(reader),
    entity: reader.u64(),
    component: reader.u16(),
    incarnation: reader.u64(),
  };
}

function readGuiAncestry(reader: Reader): bigint[] {
  const count = reader.count(Math.floor(MAX_MESSAGE_BYTES / 8));
  const ancestry: bigint[] = [];
  for (let index = 0; index < count; index++) ancestry.push(reader.u64());
  return ancestry;
}

// Action 5 was the explicit value replacement; retired tags are never reused.
function writeGuiAction(writer: Writer, action: GuiAction): void {
  switch (action.kind) {
    case "press":
      writer.u8(WIRE.GUI_ACTION_PRESS);
      break;
    case "toggle":
      writer.u8(WIRE.GUI_ACTION_TOGGLE);
      break;
    case "scalar":
      writer.u8(WIRE.GUI_ACTION_SET_SCALAR);
      writer.f32(action.value);
      break;
    case "text":
      writer.u8(WIRE.GUI_ACTION_SET_TEXT);
      writer.string(action.value);
      break;
    case "focus":
      writer.u8(WIRE.GUI_ACTION_FOCUS);
      writer.u32(action.part ?? 0);
      break;
    case "blur":
      writer.u8(WIRE.GUI_ACTION_BLUR);
      break;
    case "submit":
      writer.u8(WIRE.GUI_ACTION_SUBMIT);
      break;
    case "scrollTo":
      writer.u8(WIRE.GUI_ACTION_SCROLL_TO);
      writer.f32(action.offset[0]);
      writer.f32(action.offset[1]);
      break;
    case "scrollBy":
      writer.u8(WIRE.GUI_ACTION_SCROLL_BY);
      writer.f32(action.delta[0]);
      writer.f32(action.delta[1]);
      break;
    case "scrollToIndex":
      writer.u8(WIRE.GUI_ACTION_SCROLL_TO_INDEX);
      writer.u32(action.index);
      writer.f32(action.offset ?? 0);
      break;
    case "color":
      writer.u8(WIRE.GUI_ACTION_SET_COLOR);
      for (const channel of action.value) writer.f32(channel);
      break;
    default:
      fail("GUI action");
  }
}

function readGuiEffect(reader: Reader): GuiCommittedEffect {
  const id = reader.boolean()
    ? { world: readWorldReference(reader), ordinal: reader.u64() }
    : null;
  const target = readGuiTarget(reader);
  if (
    id &&
    (id.ordinal === 0n ||
      id.world.id !== target.world.id ||
      id.world.incarnation !== target.world.incarnation)
  )
    fail("GUI effect identity");
  // Sources 1 (replacement) and 3 (layout) are retired.
  const tag = reader.u8();
  const source =
    tag === 0
      ? "semantic"
      : tag === 2
        ? {
            kind: "routed" as const,
            publication: { host: reader.u64(), revision: reader.u64() },
          }
        : fail("GUI effect source");
  if (
    typeof source === "object" &&
    (source.publication.host === 0n || source.publication.revision === 0n)
  )
    fail("GUI routed publication");
  const tick = reader.u64();
  const ancestry = readGuiAncestry(reader);
  // Kinds 2 (value applied) and 5 (scroll changed) are retired; values are
  // observed as component fields.
  switch (reader.u8()) {
    case 0:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: { kind: "pressed" },
      };
    case 4:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: { kind: "submitted", text: reader.string() },
      };
    case 7:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: { kind: "rejected", text: reader.string() },
      };
    case 8:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: { kind: "discarded", text: reader.string() },
      };
    case 6:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: {
          kind: "contextRequested",
          point: [reader.f32(), reader.f32()],
        },
      };
    case 9:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: { kind: "input", input: readGuiClientInput(reader) },
      };
    case 1:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: {
          kind: "focusChanged",
          focused: reader.boolean(),
          changed: reader.boolean(),
          part: reader.u32(),
        },
      };
    case 3:
      return {
        id,
        target,
        source,
        tick,
        ancestry,
        effect: {
          kind: "interactionChanged",
          pointer: reader.u64(),
          state: {
            hovered: reader.boolean(),
            pressed: reader.boolean(),
            captured: reader.boolean(),
          },
          changed: reader.boolean(),
        },
      };
    default:
      return fail("GUI effect");
  }
}

function readGuiClientInput(reader: Reader): GuiClientInput {
  switch (reader.u8()) {
    case WIRE.GUI_CLIENT_INPUT_KEY: {
      const key = reader.string(GUI_CLIENT_KEY_BYTES),
        code = reader.string(GUI_CLIENT_KEY_BYTES),
        modifiers = reader.u8();
      if (!key || !code || modifiers > 15) return fail("client key");
      return {
        kind: "key",
        key,
        code,
        modifiers,
        repeat: reader.boolean(),
        pressed: reader.boolean(),
      };
    }
    case WIRE.GUI_CLIENT_INPUT_TEXT: {
      const text = reader.string(GUI_CLIENT_TEXT_BYTES);
      return { kind: "text", text, paste: reader.boolean() };
    }
    case WIRE.GUI_CLIENT_INPUT_POINTER: {
      const pointer = reader.u64();
      const phase = (["down", "move", "up", "cancel"] as const)[reader.u8()];
      const button = (["primary", "secondary", "auxiliary", "none"] as const)[
        reader.u8()
      ];
      if (!phase || !button) return fail("client pointer");
      return {
        kind: "pointer",
        pointer,
        phase,
        button,
        point: [reader.f32(), reader.f32()],
      };
    }
    case WIRE.GUI_CLIENT_INPUT_WHEEL:
      return {
        kind: "wheel",
        point: [reader.f32(), reader.f32()],
        delta: [reader.f32(), reader.f32()],
      };
    default:
      return fail("client input kind");
  }
}

function writeGuiObservation(
  writer: Writer,
  control: GuiObservationRequest,
): void {
  writer.u8(control.kind === "subscribe" ? 0 : 1);
  writeWorldReference(writer, control.world);
  if (control.kind === "subscribe") {
    if (control.inputTarget) {
      writer.u8(3);
      writeWorldReference(writer, control.inputTarget.world);
      writer.u64(control.inputTarget.entity);
      writer.u16(control.inputTarget.component);
      writer.u64(control.inputTarget.incarnation);
      return;
    }
    const classes = { application: 0, feedback: 1, all: 2 }[control.classes];
    if (classes === undefined) fail("GUI observation class");
    writer.u8(classes);
  } else {
    if (
      control.subscription.output <= 0n ||
      control.subscription.generation <= 0n
    )
      fail("GUI observation identity");
    writer.u64(control.subscription.output);
    writer.u64(control.subscription.generation);
  }
}

function readGuiObservation(reader: Reader): GuiObservationRecord {
  const subscription = { output: reader.u64(), generation: reader.u64() };
  if (subscription.output === 0n || subscription.generation === 0n)
    fail("GUI observation identity");
  const kind = reader.u8();
  if (kind === 0) {
    const world = readWorldReference(reader);
    const result =
      (
        [
          "subscribed",
          "unsubscribed",
          "cancelled",
          "staleWorld",
          "staleSubscription",
          "alreadySubscribed",
        ] as const
      )[reader.u8()] ?? fail("GUI observation result");
    return { kind: "control", world, subscription, result };
  }
  if (kind !== 1) return fail("GUI observation record");
  const effect = readGuiEffect(reader);
  if (effect.id === null)
    return fail("GUI observation has no applied identity");
  return { kind: "effect", subscription, effect: effect as GuiObservedEffect };
}
