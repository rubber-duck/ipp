import type {
  GuiTarget,
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

function writeGuiObservation(
  writer: Writer,
  control: GuiObservationRequest,
): void {
  writer.u8(control.kind === "subscribe" ? 0 : 1);
  writeWorldReference(writer, control.world);
  if (control.kind === "subscribe") {
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
