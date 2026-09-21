import type {
  GuiCancelObservation,
  GuiCommittedEffect,
  GuiConflictObservation,
  GuiContainerKind,
  GuiControlState,
  GuiControlValue,
  GuiControls,
  GuiEdit,
  GuiEditBatchOutcome,
  GuiInputBlocker,
  GuiInputCommand,
  GuiInputRoutingOutcome,
  GuiInspectQuery,
  GuiInspectResponse,
  GuiInspectedNode,
  GuiKey,
  GuiNode,
  GuiNodeContent,
  GuiNodeHandle,
  GuiNodePatchStyle,
  GuiNodeStyle,
  GuiObservationTarget,
  GuiPointerButton,
  GuiSemanticActionKind,
  GuiSemanticActionRequest,
  GuiSemanticFocus,
  GuiSemanticNode,
  GuiSemanticSnapshotQuery,
  GuiSemanticTree,
  GuiTree,
  GuiTextFocusState,
  GuiUnhandledObservation,
} from "./gui-types.js";
export * from "./gui-types.js";

function guiVector(
  w: Writer,
  values: readonly number[],
  length: number,
  unit = false,
): void {
  if (values.length !== length) fail("GUI vector length");
  for (const value of values) {
    if (unit && (value < 0 || value > 1)) fail("GUI color range");
    w.f32(value);
  }
}

function readGuiVector(r: Reader, length: number): number[] {
  const result: number[] = [];
  for (let i = 0; i < length; i++) {
    result.push(r.f32());
  }
  return result;
}

const CONTAINER_KINDS: readonly GuiContainerKind[] = [
  "row",
  "column",
  "stack",
  "padding",
  "align",
  "sizedBox",
  "scrollView",
];

function writeGuiNodeContent(w: Writer, content: GuiNodeContent): void {
  switch (content.kind) {
    case "container": {
      w.u8(1);
      const index = CONTAINER_KINDS.indexOf(content.containerKind);
      if (index === -1) fail("Unknown GUI container kind");
      w.u8(index);
      break;
    }
    case "text":
      w.u8(2);
      w.string(content.text);
      break;
    case "drawing":
      w.u8(3);
      break;
    case "image":
      w.u8(4);
      guiVector(w, content.size, 2);
      break;
    case "button":
      w.u8(5);
      w.string(content.label);
      break;
    case "checkbox":
      w.u8(6);
      w.boolean(content.checked);
      break;
    case "slider":
      w.u8(7);
      w.f32(content.value);
      w.f32(content.min);
      w.f32(content.max);
      w.f32(content.step);
      break;
    case "textInput":
      w.u8(8);
      w.string(content.text);
      w.string(content.placeholder);
      break;
    default:
      fail("Unknown GUI node content kind");
  }
}

function readGuiNodeContent(r: Reader): GuiNodeContent {
  const kind = r.u8();
  switch (kind) {
    case 1: {
      const idx = r.u8();
      const containerKind = CONTAINER_KINDS[idx];
      if (!containerKind) fail("Invalid GUI container kind byte");
      return { kind: "container", containerKind };
    }
    case 2:
      return { kind: "text", text: r.string() };
    case 3:
      return { kind: "drawing" };
    case 4:
      return { kind: "image", size: [r.f32(), r.f32()] };
    case 5:
      return { kind: "button", label: r.string() };
    case 6:
      return { kind: "checkbox", checked: r.boolean() };
    case 7:
      return {
        kind: "slider",
        value: r.f32(),
        min: r.f32(),
        max: r.f32(),
        step: r.f32(),
      };
    case 8:
      return {
        kind: "textInput",
        text: r.string(),
        placeholder: r.string(),
      };
    default:
      return fail("Invalid GUI node content tag");
  }
}

function writeGuiControlValue(w: Writer, value: GuiControlValue): void {
  switch (value.kind) {
    case "none":
      w.u8(0);
      break;
    case "bool":
      w.u8(1);
      w.boolean(value.value);
      break;
    case "scalar":
      w.u8(2);
      w.f32(value.value);
      break;
    case "text":
      w.u8(3);
      w.string(value.value);
      break;
    default:
      fail("Unknown GUI control value kind");
  }
}

function readGuiControlValue(r: Reader): GuiControlValue {
  const tag = r.u8();
  switch (tag) {
    case 0:
      return { kind: "none" };
    case 1:
      return { kind: "bool", value: r.boolean() };
    case 2:
      return { kind: "scalar", value: r.f32() };
    case 3:
      return { kind: "text", value: r.string() };
    default:
      return fail("Invalid GUI control value tag");
  }
}

function writeGuiNodeStyle(w: Writer, style: GuiNodeStyle): void {
  let mask = 0;
  if (style.width !== undefined) mask |= 1 << 0;
  if (style.height !== undefined) mask |= 1 << 1;
  if (style.minWidth !== undefined) mask |= 1 << 2;
  if (style.minHeight !== undefined) mask |= 1 << 3;
  if (style.maxWidth !== undefined) mask |= 1 << 4;
  if (style.maxHeight !== undefined) mask |= 1 << 5;
  if (style.padding !== undefined) mask |= 1 << 6;
  if (style.margin !== undefined) mask |= 1 << 7;
  if (style.flex !== undefined) mask |= 1 << 8;
  if (style.alignX !== undefined) mask |= 1 << 9;
  if (style.alignY !== undefined) mask |= 1 << 10;
  if (style.backgroundColor !== undefined) mask |= 1 << 11;
  if (style.asset != null) mask |= 1 << 12;
  if (style.enabled !== undefined) mask |= 1 << 13;

  w.u16(mask);
  if (style.width !== undefined) w.f32(style.width);
  if (style.height !== undefined) w.f32(style.height);
  if (style.minWidth !== undefined) w.f32(style.minWidth);
  if (style.minHeight !== undefined) w.f32(style.minHeight);
  if (style.maxWidth !== undefined) w.f32(style.maxWidth);
  if (style.maxHeight !== undefined) w.f32(style.maxHeight);
  if (style.padding !== undefined) guiVector(w, style.padding, 4);
  if (style.margin !== undefined) guiVector(w, style.margin, 4);
  if (style.flex !== undefined) w.f32(style.flex);
  if (style.alignX !== undefined) w.f32(style.alignX);
  if (style.alignY !== undefined) w.f32(style.alignY);
  guiVector(w, style.color ?? [1, 1, 1, 1], 4, true);
  if (style.backgroundColor !== undefined)
    guiVector(w, style.backgroundColor, 4, true);
  w.f32(style.opacity ?? 1);
  w.f32(style.fontSize ?? 0.1);
  if (style.asset != null) {
    w.u16(style.asset.kind);
    w.u32(style.asset.variant ?? 0);
    w.string(style.asset.source);
  }
  if (style.enabled !== undefined) w.boolean(style.enabled);
}

function readGuiNodeStyle(r: Reader): GuiNodeStyle {
  const mask = r.u16();
  const width = (mask & (1 << 0)) !== 0 ? r.f32() : undefined;
  const height = (mask & (1 << 1)) !== 0 ? r.f32() : undefined;
  const minWidth = (mask & (1 << 2)) !== 0 ? r.f32() : undefined;
  const minHeight = (mask & (1 << 3)) !== 0 ? r.f32() : undefined;
  const maxWidth = (mask & (1 << 4)) !== 0 ? r.f32() : undefined;
  const maxHeight = (mask & (1 << 5)) !== 0 ? r.f32() : undefined;
  const padding =
    (mask & (1 << 6)) !== 0
      ? (readGuiVector(r, 4) as [number, number, number, number])
      : undefined;
  const margin =
    (mask & (1 << 7)) !== 0
      ? (readGuiVector(r, 4) as [number, number, number, number])
      : undefined;
  const flex = (mask & (1 << 8)) !== 0 ? r.f32() : undefined;
  const alignX = (mask & (1 << 9)) !== 0 ? r.f32() : undefined;
  const alignY = (mask & (1 << 10)) !== 0 ? r.f32() : undefined;
  const color = readGuiVector(r, 4) as [number, number, number, number];
  const backgroundColor =
    (mask & (1 << 11)) !== 0
      ? (readGuiVector(r, 4) as [number, number, number, number])
      : undefined;
  const opacity = r.f32();
  const fontSize = r.f32();
  let asset: import("./gui-types.js").GuiAssetSource | undefined;
  if ((mask & (1 << 12)) !== 0) {
    const kind = r.u16();
    const variant = r.u32();
    const source = r.string();
    asset = { kind, variant, source };
  }
  const enabled = (mask & (1 << 13)) !== 0 ? r.boolean() : true;

  return {
    width,
    height,
    minWidth,
    minHeight,
    maxWidth,
    maxHeight,
    padding,
    margin,
    flex,
    alignX,
    alignY,
    color,
    backgroundColor,
    opacity,
    fontSize,
    asset,
    enabled,
  };
}

/** Encode a complete GUI tree and its committed control values for a new GuiRoot incarnation. */
export function encodeGuiTree(tree: GuiTree): Uint8Array<ArrayBuffer> {
  const w = new Writer(65536);
  w.u8(1);
  w.u32(tree.nextId);
  w.u32(tree.rootNode ?? 0);
  w.count(tree.nodes.length, 65536);
  for (const node of tree.nodes) {
    w.u32(node.id);
    w.u32(node.parent ?? 0);
    w.u32(node.lifetime);
    w.count(node.children.length, 65536);
    for (const child of node.children) {
      w.u32(child);
    }
    writeGuiNodeContent(w, node.content);
  }
  writeGuiControls(w, tree.controls ?? []);
  return w.finish();
}

/** Decode full GUI node tree from binary representation. */
export function decodeGuiTree(bytes: Uint8Array): GuiTree {
  const r = new Reader(bytes);
  if (r.u8() !== 1) fail("GUI tree version");
  const nextId = r.u32();
  const rootIdVal = r.u32();
  const rootNode = rootIdVal === 0 ? undefined : rootIdVal;
  const count = r.count(65536);
  const nodes: GuiNode[] = [];
  for (let i = 0; i < count; i++) {
    const id = r.u32();
    const parentVal = r.u32();
    const parent = parentVal === 0 ? undefined : parentVal;
    const lifetime = r.u32();
    const childrenCount = r.count(65536);
    const children: number[] = [];
    for (let c = 0; c < childrenCount; c++) {
      children.push(r.u32());
    }
    nodes.push({
      id,
      parent,
      lifetime,
      children,
      content: readGuiNodeContent(r),
    });
  }
  const controls = readGuiControls(r);
  r.done();
  return { nextId, rootNode, nodes, controls };
}

function writeGuiControls(w: Writer, controls: GuiControls): void {
  w.count(controls.length, 65536);
  let previous = 0;
  for (const state of controls) {
    if (state.id <= previous) fail("GUI controls must be in identity order");
    if (state.revision <= 0) fail("GUI control revision");
    previous = state.id;
    w.u32(state.id);
    w.u32(state.revision);
    writeGuiControlValue(w, state.value);
  }
}

function readGuiControls(r: Reader): GuiControls {
  const count = r.count(65536);
  const controls: GuiControlState[] = [];
  for (let i = 0; i < count; i++) {
    const id = r.u32();
    const revision = r.u32();
    controls.push({ id, revision, value: readGuiControlValue(r) });
  }
  return controls;
}

function writeGuiNodeHandle(w: Writer, handle: GuiNodeHandle): void {
  if (handle.session < 0n || handle.entity === 0n)
    fail("GUI node handle session/entity");
  if (handle.rootIncarnation < 0n) fail("GUI node handle root incarnation");
  if (handle.nodeId <= 0) fail("GUI node handle id");
  w.u64(handle.session);
  w.u64(handle.entity);
  w.u64(handle.rootIncarnation);
  w.u32(handle.nodeId);
  w.u32(handle.nodeLifetime);
}

function writeNullablePatchField<T>(
  w: Writer,
  val: T | null | undefined,
  encodeVal: (v: T) => void,
): void {
  if (val === undefined) {
    w.u8(0);
  } else if (val === null) {
    w.u8(1);
  } else {
    w.u8(2);
    encodeVal(val);
  }
}

function writeRequiredPatchField<T>(
  w: Writer,
  val: T | undefined,
  encodeVal: (v: T) => void,
): void {
  if (val === undefined) {
    w.u8(0);
  } else {
    w.u8(1);
    encodeVal(val);
  }
}

function writeGuiNodePatchStyle(w: Writer, style: GuiNodePatchStyle): void {
  writeNullablePatchField(w, style.width, (v) => w.f32(v));
  writeNullablePatchField(w, style.height, (v) => w.f32(v));
  writeNullablePatchField(w, style.minWidth, (v) => w.f32(v));
  writeNullablePatchField(w, style.minHeight, (v) => w.f32(v));
  writeNullablePatchField(w, style.maxWidth, (v) => w.f32(v));
  writeNullablePatchField(w, style.maxHeight, (v) => w.f32(v));
  writeNullablePatchField(w, style.padding, (v) => guiVector(w, v, 4));
  writeNullablePatchField(w, style.margin, (v) => guiVector(w, v, 4));
  writeNullablePatchField(w, style.flex, (v) => w.f32(v));
  writeNullablePatchField(w, style.alignX, (v) => w.f32(v));
  writeNullablePatchField(w, style.alignY, (v) => w.f32(v));
  writeRequiredPatchField(w, style.color, (v) => guiVector(w, v, 4, true));
  writeNullablePatchField(w, style.backgroundColor, (v) =>
    guiVector(w, v, 4, true),
  );
  writeRequiredPatchField(w, style.opacity, (v) => w.f32(v));
  writeRequiredPatchField(w, style.fontSize, (v) => w.f32(v));
  writeNullablePatchField(w, style.asset, (v) => {
    if (v != null) {
      w.u16(v.kind);
      w.u32(v.variant ?? 0);
      w.string(v.source);
    }
  });
  writeNullablePatchField(w, style.enabled, (v) => w.boolean(v));
}

/** Encode incremental GUI edit command. */
function encodeGuiEditBody(edit: GuiEdit): Uint8Array<ArrayBuffer> {
  const w = new Writer();
  const actions = {
    insert: 1,
    update: 2,
    move: 3,
    remove: 4,
    setControlValue: 5,
  };
  if (!Object.hasOwn(actions, edit.action)) fail("GUI edit action");
  w.u8(actions[edit.action]);
  switch (edit.action) {
    case "insert":
      exactFields(edit, [
        "action",
        "entity",
        "rootIncarnation",
        "id",
        "parent",
        "index",
        "content",
        "style",
      ]);
      if (edit.entity === 0n || edit.id <= 0) fail("GUI edit identity");
      if (edit.rootIncarnation < 0n) fail("GUI edit root incarnation");
      w.u64(edit.entity);
      w.u64(edit.rootIncarnation);
      w.u32(edit.id);
      w.boolean(edit.parent !== undefined);
      if (edit.parent !== undefined) w.u32(edit.parent);
      w.u32(edit.index);
      writeGuiNodeContent(w, edit.content);
      writeGuiNodeStyle(w, edit.style ?? {});
      break;
    case "update": {
      exactFields(edit, ["action", "handle", "patch"]);
      writeGuiNodeHandle(w, edit.handle);
      const p = edit.patch;
      w.boolean(p.content !== undefined);
      if (p.content !== undefined) writeGuiNodeContent(w, p.content);
      w.boolean(p.style !== undefined);
      if (p.style !== undefined) writeGuiNodePatchStyle(w, p.style);
      break;
    }
    case "move":
      exactFields(edit, ["action", "handle", "parent", "index"]);
      writeGuiNodeHandle(w, edit.handle);
      w.boolean(edit.parent !== undefined);
      if (edit.parent !== undefined) w.u32(edit.parent);
      w.u32(edit.index);
      break;
    case "remove":
      exactFields(edit, ["action", "handle"]);
      writeGuiNodeHandle(w, edit.handle);
      break;
    case "setControlValue":
      exactFields(edit, ["action", "handle", "expectedRevision", "value"]);
      writeGuiNodeHandle(w, edit.handle);
      w.u32(edit.expectedRevision);
      writeGuiControlValue(w, edit.value);
      break;
  }
  return w.finish();
}

const GUI_DIRECT_REQUEST_ENVELOPE_BYTES = 8 + 8 + 1 + 1 + 4;
const GUI_STREAM_REQUEST_ENVELOPE_BYTES = GUI_DIRECT_REQUEST_ENVELOPE_BYTES + 8;
const GUI_BATCH_HEADER_BYTES = 1 + 4;
const MAX_DIRECT_GUI_BATCH_BYTES =
  MAX_MESSAGE_BYTES - GUI_DIRECT_REQUEST_ENVELOPE_BYTES;
const MAX_STREAM_GUI_BATCH_BYTES =
  MAX_MESSAGE_BYTES - GUI_STREAM_REQUEST_ENVELOPE_BYTES;

/** Split ordered edits only when the exact encoded request would exceed framing. */
export function planGuiEditBatches(
  edits: readonly GuiEdit[],
): readonly (readonly GuiEdit[])[] {
  if (edits.length === 0) fail("empty GUI edit batch");
  const encoded = edits.map((edit) => ({
    edit,
    body: encodeGuiEditBody(edit),
  }));
  const total = encoded.reduce(
    (bytes, item) => bytes + item.body.byteLength,
    GUI_BATCH_HEADER_BYTES,
  );
  if (total <= MAX_DIRECT_GUI_BATCH_BYTES) return [[...edits]];
  const batches: GuiEdit[][] = [];
  let batch: GuiEdit[] = [];
  let bytes = GUI_BATCH_HEADER_BYTES;
  for (const { edit, body } of encoded) {
    if (GUI_BATCH_HEADER_BYTES + body.byteLength > MAX_STREAM_GUI_BATCH_BYTES)
      fail("GUI edit exceeds message limit");
    if (bytes + body.byteLength > MAX_STREAM_GUI_BATCH_BYTES) {
      batches.push(batch);
      batch = [];
      bytes = GUI_BATCH_HEADER_BYTES;
    }
    batch.push(edit);
    bytes += body.byteLength;
  }
  batches.push(batch);
  return batches;
}

/** Encode one byte-bounded ordered GUI edit group. */
export function encodeGuiEdits(
  edits: readonly GuiEdit[],
): Uint8Array<ArrayBuffer> {
  const bodies = edits.map(encodeGuiEditBody);
  const w = new Writer(MAX_DIRECT_GUI_BATCH_BYTES);
  w.u8(2);
  w.count(bodies.length, 0xffffffff);
  for (const body of bodies) w.raw(body);
  return w.finish();
}

/** Encode GUI inspection query. */
export function encodeGuiInspectQuery(
  query: GuiInspectQuery,
): Uint8Array<ArrayBuffer> {
  const w = new Writer(65536);
  w.u8(1);
  if (query.entity === 0n) fail("GUI inspect query entity");
  w.u64(query.entity);
  w.boolean(query.nodeId !== undefined);
  if (query.nodeId !== undefined) w.u32(query.nodeId);
  w.u32(query.maxDepth ?? 32);
  w.u32(query.limit ?? 256);
  return w.finish();
}

/** Decode GUI inspection response payload. */
export function decodeGuiInspectResponse(
  bytes: Uint8Array,
): GuiInspectResponse {
  const r = new Reader(bytes);
  if (r.u8() !== 1) fail("GUI inspect response version");
  const rootEntity = r.u64();
  const rootIncarnation = r.u64();
  const count = r.count(65536);
  const nodes: GuiInspectedNode[] = [];
  for (let i = 0; i < count; i++) {
    const id = r.u32();
    const parentVal = r.u32();
    const parent = parentVal === 0 ? undefined : parentVal;
    const lifetime = r.u32();
    const controlRevision = r.u32();
    const childrenCount = r.count(65536);
    const children: number[] = [];
    for (let c = 0; c < childrenCount; c++) {
      children.push(r.u32());
    }
    const content = readGuiNodeContent(r);
    const controlValue = readGuiControlValue(r);
    const style = readGuiNodeStyle(r);
    nodes.push({
      id,
      parent,
      lifetime,
      controlRevision,
      children,
      content,
      controlValue,
      style,
    });
  }
  r.done();
  return { rootEntity, rootIncarnation, nodes };
}

const GUI_POINTER_BUTTONS: Record<GuiPointerButton, number> = {
  primary: 0,
  secondary: 1,
  auxiliary: 2,
};

const GUI_KEYS: readonly GuiKey[] = [
  "tab",
  "enter",
  "space",
  "escape",
  "backspace",
  "delete",
  "left",
  "right",
  "up",
  "down",
  "home",
  "end",
];

function writeGuiInputBlockers(
  w: Writer,
  blockers: readonly GuiInputBlocker[] | undefined,
): void {
  const list = blockers ?? [];
  w.count(list.length, 1024);
  for (const blocker of list) {
    exactFields(blocker, ["entity", "distance"]);
    if (blocker.entity === 0n) fail("GUI input blocker entity");
    w.u64(blocker.entity);
    w.f32(blocker.distance);
  }
}

function writeGuiInputPanel(w: Writer, panel: bigint | undefined): void {
  w.boolean(panel !== undefined);
  if (panel !== undefined) {
    if (panel === 0n) fail("GUI input panel entity");
    w.u64(panel);
  }
}

function writeGuiInputPanelDistance(
  w: Writer,
  panelDistance: number | undefined,
): void {
  w.boolean(panelDistance !== undefined);
  if (panelDistance !== undefined) w.f32(panelDistance);
}

/** Encode one ordered GUI input command. */
export function encodeGuiInput(
  input: GuiInputCommand,
): Uint8Array<ArrayBuffer> {
  const w = new Writer();
  w.u8(1);
  switch (input.kind) {
    case "pointerDown":
    case "pointerUp": {
      exactFields(input, [
        "kind",
        "pointer",
        "panel",
        "position",
        "button",
        "blockers",
        "panelDistance",
      ]);
      w.u8(input.kind === "pointerDown" ? 1 : 2);
      w.u32(uint(input.pointer, 0xffffffff));
      writeGuiInputPanel(w, input.panel);
      guiVector(w, [...input.position], 2);
      const button = GUI_POINTER_BUTTONS[input.button];
      if (button === undefined) fail("GUI input button");
      w.u8(button);
      writeGuiInputBlockers(w, input.blockers);
      writeGuiInputPanelDistance(w, input.panelDistance);
      break;
    }
    case "pointerMove": {
      exactFields(input, [
        "kind",
        "pointer",
        "panel",
        "position",
        "blockers",
        "panelDistance",
      ]);
      w.u8(3);
      w.u32(uint(input.pointer, 0xffffffff));
      writeGuiInputPanel(w, input.panel);
      guiVector(w, [...input.position], 2);
      writeGuiInputBlockers(w, input.blockers);
      writeGuiInputPanelDistance(w, input.panelDistance);
      break;
    }
    case "pointerCancel":
      exactFields(input, ["kind", "pointer"]);
      w.u8(4);
      w.u32(uint(input.pointer, 0xffffffff));
      break;
    case "scroll": {
      exactFields(input, [
        "kind",
        "panel",
        "position",
        "delta",
        "blockers",
        "panelDistance",
      ]);
      w.u8(5);
      writeGuiInputPanel(w, input.panel);
      guiVector(w, [...input.position], 2);
      guiVector(w, [...input.delta], 2);
      writeGuiInputBlockers(w, input.blockers);
      writeGuiInputPanelDistance(w, input.panelDistance);
      break;
    }
    case "key": {
      exactFields(input, ["kind", "key", "pressed"]);
      w.u8(6);
      const key = GUI_KEYS.indexOf(input.key);
      if (key === -1) fail("GUI input key");
      w.u8(key);
      w.boolean(input.pressed);
      break;
    }
    case "text":
      exactFields(input, ["kind", "text"]);
      w.u8(7);
      w.string(input.text);
      break;
    case "focus":
      exactFields(input, ["kind", "handle"]);
      w.u8(8);
      writeGuiNodeHandle(w, input.handle);
      break;
    case "blur":
      exactFields(input, ["kind"]);
      w.u8(9);
      break;
    case "setTextSelection":
      exactFields(input, ["kind", "start", "end"]);
      w.u8(10);
      w.u32(uint(input.start, 0xffffffff));
      w.u32(uint(input.end, 0xffffffff));
      break;
    case "composition":
      exactFields(input, ["kind", "text", "caretStart", "caretEnd"]);
      w.u8(11);
      w.string(input.text);
      w.u32(uint(input.caretStart, 0xffffffff));
      w.u32(uint(input.caretEnd, 0xffffffff));
      break;
    case "commitComposition":
      exactFields(input, ["kind"]);
      w.u8(12);
      break;
    case "cancelComposition":
      exactFields(input, ["kind"]);
      w.u8(13);
      break;
    default:
      fail("GUI input action");
  }
  return w.finish();
}

/** Decode one fenced node handle, mirroring the admission checks. */
function readGuiNodeHandle(r: Reader): GuiNodeHandle {
  const session = r.u64();
  const entity = r.u64();
  const rootIncarnation = r.u64();
  const nodeId = r.u32();
  const nodeLifetime = r.u32();
  if (entity === 0n) fail("GUI node handle entity");
  if (nodeId === 0) fail("GUI node handle id");
  return { session, entity, rootIncarnation, nodeId, nodeLifetime };
}

function readGuiPointerButton(r: Reader): GuiPointerButton {
  const button = r.u8();
  if (button === 0) return "primary";
  if (button === 1) return "secondary";
  if (button === 2) return "auxiliary";
  return fail("GUI input button");
}

function readGuiInputBlockers(r: Reader): GuiInputBlocker[] {
  const count = r.count(1024);
  const blockers: GuiInputBlocker[] = [];
  for (let i = 0; i < count; i++) {
    const entity = r.u64();
    if (entity === 0n) fail("GUI input blocker entity");
    blockers.push({ entity, distance: r.f32() });
  }
  return blockers;
}

function readGuiInputPanel(r: Reader): bigint | undefined {
  if (!r.boolean()) return undefined;
  const panel = r.u64();
  if (panel === 0n) fail("GUI input panel entity");
  return panel;
}

function readGuiInputPanelDistance(r: Reader): number | undefined {
  if (!r.boolean()) return undefined;
  return r.f32();
}

/** Decode one ordered GUI input, mirroring the admission field order. */
function readGuiInputCommand(r: Reader): GuiInputCommand {
  if (r.u8() !== 1) fail("GUI input version");
  const action = r.u8();
  switch (action) {
    case 1:
    case 2: {
      const pointer = r.u32();
      const panel = readGuiInputPanel(r);
      const position: [number, number] = [r.f32(), r.f32()];
      const button = readGuiPointerButton(r);
      const blockers = readGuiInputBlockers(r);
      const panelDistance = readGuiInputPanelDistance(r);
      return {
        kind: action === 1 ? "pointerDown" : "pointerUp",
        pointer,
        ...(panel === undefined ? {} : { panel }),
        position,
        button,
        ...(blockers.length === 0 ? {} : { blockers }),
        ...(panelDistance === undefined ? {} : { panelDistance }),
      };
    }
    case 3: {
      const pointer = r.u32();
      const panel = readGuiInputPanel(r);
      const position: [number, number] = [r.f32(), r.f32()];
      const blockers = readGuiInputBlockers(r);
      const panelDistance = readGuiInputPanelDistance(r);
      return {
        kind: "pointerMove",
        pointer,
        ...(panel === undefined ? {} : { panel }),
        position,
        ...(blockers.length === 0 ? {} : { blockers }),
        ...(panelDistance === undefined ? {} : { panelDistance }),
      };
    }
    case 4:
      return { kind: "pointerCancel", pointer: r.u32() };
    case 5: {
      const panel = readGuiInputPanel(r);
      const position: [number, number] = [r.f32(), r.f32()];
      const delta: [number, number] = [r.f32(), r.f32()];
      const blockers = readGuiInputBlockers(r);
      const panelDistance = readGuiInputPanelDistance(r);
      return {
        kind: "scroll",
        ...(panel === undefined ? {} : { panel }),
        position,
        delta,
        ...(blockers.length === 0 ? {} : { blockers }),
        ...(panelDistance === undefined ? {} : { panelDistance }),
      };
    }
    case 6: {
      const key = GUI_KEYS[r.u8()];
      if (key === undefined) fail("GUI input key");
      return { kind: "key", key, pressed: r.boolean() };
    }
    case 7:
      return { kind: "text", text: r.string() };
    case 8:
      return { kind: "focus", handle: readGuiNodeHandle(r) };
    case 9:
      return { kind: "blur" };
    case 10:
      return { kind: "setTextSelection", start: r.u32(), end: r.u32() };
    case 11:
      return {
        kind: "composition",
        text: r.string(),
        caretStart: r.u32(),
        caretEnd: r.u32(),
      };
    case 12:
      return { kind: "commitComposition" };
    case 13:
      return { kind: "cancelComposition" };
    default:
      return fail("GUI input action");
  }
}

function readGuiObservationPath(r: Reader): number[] {
  const count = r.count(65536);
  const path: number[] = [];
  for (let i = 0; i < count; i++) {
    const id = r.u32();
    if (id === 0) fail("GUI observation path identity");
    path.push(id);
  }
  return path;
}

/** Decode one committed control outcome; transient cursors never arrive here. */
function readGuiObservationEffect(r: Reader): GuiCommittedEffect {
  const kind = r.u8();
  const session = r.u64();
  if (session === 0n) fail("GUI observation session");
  const sourceTick = r.u64();
  const effectTick = r.u64();
  const entity = r.u64();
  if (entity === 0n) fail("GUI observation entity");
  const rootIncarnation = r.u64();
  const node = r.u32();
  if (node === 0) fail("GUI observation node");
  const lifetime = r.u32();
  const path = readGuiObservationPath(r);
  if (kind === 0)
    return {
      kind: "buttonPressed",
      entity,
      rootIncarnation,
      node,
      lifetime,
      path,
      sourceTick,
      effectTick,
    };
  if (kind !== 1) fail("GUI observation effect kind");
  const revision = r.u32();
  const value = readGuiControlValue(r);
  if (value.kind !== "bool" && value.kind !== "scalar" && value.kind !== "text")
    fail("GUI observation effect value");
  return {
    kind: "controlCommitted",
    entity,
    rootIncarnation,
    node,
    lifetime,
    value,
    revision,
    path,
    sourceTick,
    effectTick,
  };
}

function readGuiObservationTarget(r: Reader): GuiObservationTarget | undefined {
  const tag = r.u8();
  if (tag === 0) return undefined;
  if (tag !== 1) fail("GUI observation target option");
  const entity = r.u64();
  if (entity === 0n) fail("GUI observation target entity");
  const rootIncarnation = r.u64();
  const node = r.u32();
  if (node === 0) fail("GUI observation target node");
  return { entity, rootIncarnation, node, lifetime: r.u32() };
}

function readGuiConflictObservation(r: Reader): GuiConflictObservation {
  const session = r.u64();
  if (session === 0n) fail("GUI conflict session");
  const sourceTick = r.u64();
  const effectTick = r.u64();
  const target = readGuiObservationTarget(r);
  const reason = r.u8();
  if (reason === 0)
    return {
      session,
      sourceTick,
      effectTick,
      ...(target === undefined ? {} : { target }),
      reason: { kind: "revisionMismatch", expected: r.u32(), found: r.u32() },
    };
  if (reason === 1)
    return {
      session,
      sourceTick,
      effectTick,
      ...(target === undefined ? {} : { target }),
      reason: { kind: "admissionFailed", reason: r.string() },
    };
  if (reason === 2)
    return {
      session,
      sourceTick,
      effectTick,
      ...(target === undefined ? {} : { target }),
      reason: { kind: "touchArbitration", ownerPointer: r.u32() },
    };
  return fail("GUI conflict reason");
}

const GUI_CANCEL_REASONS = [
  "targetRemoved",
  "targetHidden",
  "sessionReplaced",
  "gestureCancelled",
] as const;

function readGuiCancelObservation(r: Reader): GuiCancelObservation {
  const session = r.u64();
  if (session === 0n) fail("GUI cancellation session");
  const sourceTick = r.u64();
  const effectTick = r.u64();
  const target = readGuiObservationTarget(r);
  const reason = GUI_CANCEL_REASONS[r.u8()];
  if (reason === undefined) fail("GUI cancellation reason");
  return {
    session,
    sourceTick,
    effectTick,
    ...(target === undefined ? {} : { target }),
    reason,
  };
}

function readGuiUnhandledObservation(r: Reader): GuiUnhandledObservation {
  const session = r.u64();
  if (session === 0n) fail("GUI unhandled session");
  const tick = r.u64();
  const input = readGuiInputCommand(r);
  const reason = r.u8();
  if (reason === 0)
    return { session, tick, input, reason: { kind: "noPanelHit" } };
  if (reason === 1) {
    const entity = r.u64();
    if (entity === 0n) fail("GUI unhandled blocker");
    return { session, tick, input, reason: { kind: "blocked", entity } };
  }
  if (reason === 2)
    return { session, tick, input, reason: { kind: "staleTarget" } };
  if (reason === 3)
    return { session, tick, input, reason: { kind: "noFocus" } };
  if (reason === 4)
    return { session, tick, input, reason: { kind: "noCapture" } };
  if (reason === 5)
    return { session, tick, input, reason: { kind: "notFocusable" } };
  if (reason === 6)
    return { session, tick, input, reason: { kind: "notOwner" } };
  return fail("GUI unhandled reason");
}

/** Decode one broadcast observation payload into its three record lists. */
function readGuiObservations(bytes: Uint8Array): {
  effects: GuiCommittedEffect[];
  conflicts: GuiConflictObservation[];
  cancellations: GuiCancelObservation[];
  textFocus?: GuiTextFocusState | null;
} {
  const r = new Reader(bytes);
  if (r.u8() !== 3) fail("GUI observations version");
  const effectCount = r.count(128);
  const effects: GuiCommittedEffect[] = [];
  for (let i = 0; i < effectCount; i++)
    effects.push(readGuiObservationEffect(r));
  const conflictCount = r.count(128);
  const conflicts: GuiConflictObservation[] = [];
  for (let i = 0; i < conflictCount; i++)
    conflicts.push(readGuiConflictObservation(r));
  const cancellationCount = r.count(128);
  const cancellations: GuiCancelObservation[] = [];
  for (let i = 0; i < cancellationCount; i++)
    cancellations.push(readGuiCancelObservation(r));
  const textFocusCount = r.count(1);
  let textFocus: GuiTextFocusState | null | undefined;
  if (textFocusCount === 1) {
    const tag = r.u8();
    const session = r.u64();
    const contextGeneration = r.u64();
    const focusGeneration = r.u64();
    if (tag === 0) textFocus = null;
    else if (tag === 1) {
      const entity = r.u64();
      const rootIncarnation = r.u64();
      const node = r.u32();
      const lifetime = r.u32();
      const revision = r.u32();
      const text = r.string();
      const selectionStart = r.u32();
      const selectionEnd = r.u32();
      const compositionTag = r.u8();
      const composition =
        compositionTag === 0
          ? undefined
          : compositionTag === 1
            ? { text: r.string(), caretStart: r.u32(), caretEnd: r.u32() }
            : fail("GUI text composition option");
      textFocus = {
        session,
        contextGeneration,
        focusGeneration,
        entity,
        rootIncarnation,
        node,
        lifetime,
        revision,
        text,
        selectionStart,
        selectionEnd,
        ...(composition === undefined ? {} : { composition }),
      };
    } else fail("GUI text focus update");
  }
  if (
    effects.length +
      conflicts.length +
      cancellations.length +
      textFocusCount ===
    0
  )
    fail("empty GUI observations");
  if (effects.length + conflicts.length + cancellations.length > 128)
    fail("GUI observation count");
  r.done();
  return {
    effects,
    conflicts,
    cancellations,
    ...(textFocus === undefined ? {} : { textFocus }),
  };
}

/** Decode one supplier-private unhandled payload into its input list. */
function readGuiUnhandledInputs(bytes: Uint8Array): GuiUnhandledObservation[] {
  const r = new Reader(bytes);
  if (r.u8() !== 1) fail("GUI unhandled version");
  const count = r.count(128);
  if (count === 0) fail("empty GUI unhandled inputs");
  const inputs: GuiUnhandledObservation[] = [];
  for (let i = 0; i < count; i++) inputs.push(readGuiUnhandledObservation(r));
  r.done();
  return inputs;
}

/** Encode one bounded semantic snapshot query for a panel. */
export function encodeGuiSemanticSnapshotQuery(
  query: GuiSemanticSnapshotQuery,
): Uint8Array<ArrayBuffer> {
  exactFields(query, ["entity", "maxDepth", "limit"]);
  if (query.entity === 0n) fail("zero GUI semantic entity");
  const maxDepth = query.maxDepth ?? 32;
  const limit = query.limit ?? 256;
  if (maxDepth < 1 || maxDepth > 32 || limit < 1 || limit > 256)
    fail("GUI semantic snapshot bounds");
  const w = new Writer();
  w.u8(1);
  w.u64(query.entity);
  w.u32(maxDepth);
  w.u32(limit);
  return w.finish();
}

/** Encode one lifetime/revision-fenced semantic action. */
export function encodeGuiSemanticAction(
  action: GuiSemanticActionRequest,
): Uint8Array<ArrayBuffer> {
  exactFields(action, [
    "entity",
    "rootIncarnation",
    "node",
    "lifetime",
    "expectedRevision",
    "action",
  ]);
  if (action.entity === 0n) fail("zero GUI semantic entity");
  if (action.node === 0) fail("zero GUI semantic node");
  const w = new Writer();
  w.u8(2);
  w.u64(action.entity);
  w.u64(action.rootIncarnation);
  w.u32(action.node);
  w.u32(action.lifetime);
  w.u32(action.expectedRevision);
  const command = action.action;
  exactFields(
    command,
    command.kind === "setScalar"
      ? ["kind", "value"]
      : command.kind === "setText"
        ? ["kind", "value"]
        : ["kind"],
  );
  switch (command.kind) {
    case "press":
      w.u8(0);
      break;
    case "toggle":
      w.u8(1);
      break;
    case "setScalar":
      w.u8(2);
      w.f32(command.value);
      break;
    case "setText":
      w.u8(3);
      w.string(command.value);
      break;
    case "focus":
      w.u8(4);
      break;
    default:
      fail("GUI semantic action kind");
  }
  return w.finish();
}

const GUI_SEMANTIC_ROLES = [
  "container",
  "text",
  "drawing",
  "image",
  "button",
  "checkbox",
  "slider",
  "textInput",
] as const;

const GUI_SEMANTIC_ACTION_KINDS = [
  "press",
  "toggle",
  "setScalar",
  "setText",
  "focus",
] as const;

/** Decode one semantic snapshot node with its value, bounds and actions. */
function readGuiSemanticNode(r: Reader): GuiSemanticNode {
  const id = r.u32();
  if (id === 0) fail("GUI semantic node identity");
  const parentTag = r.u32();
  const lifetime = r.u32();
  const role = GUI_SEMANTIC_ROLES[r.u8()];
  if (role === undefined) fail("GUI semantic role");
  const nameTag = r.u8();
  let name: string | undefined;
  if (nameTag === 1) name = r.string();
  else if (nameTag !== 0) fail("GUI semantic name option");
  const value = readGuiControlValue(r);
  if (
    value.kind !== "none" &&
    value.kind !== "bool" &&
    value.kind !== "scalar" &&
    value.kind !== "text"
  )
    fail("GUI semantic value");
  const revision = r.u32();
  const bounds: [number, number, number, number] = [
    r.f32(),
    r.f32(),
    r.f32(),
    r.f32(),
  ];
  const enabled = r.boolean();
  const visible = r.boolean();
  const available = r.boolean();
  const actionCount = r.u8();
  if (actionCount > 5) fail("GUI semantic actions");
  const actions: GuiSemanticActionKind[] = [];
  for (let i = 0; i < actionCount; i++) {
    const action = GUI_SEMANTIC_ACTION_KINDS[r.u8()];
    if (action === undefined) fail("GUI semantic action kind");
    actions.push(action);
  }
  return {
    id,
    lifetime,
    ...(parentTag === 0 ? {} : { parent: parentTag }),
    role,
    ...(name === undefined ? {} : { name }),
    value,
    revision,
    bounds,
    enabled,
    visible,
    available,
    actions,
  };
}

/** Decode one bounded semantic snapshot with its observed focus. */
export function decodeGuiSemanticSnapshot(bytes: Uint8Array): GuiSemanticTree {
  const r = new Reader(bytes);
  if (r.u8() !== 1) fail("GUI semantic snapshot version");
  const entity = r.u64();
  if (entity === 0n) fail("GUI semantic snapshot entity");
  const rootIncarnation = r.u64();
  const evaluationTick = r.u64();
  const count = r.count(256);
  const nodes: GuiSemanticNode[] = [];
  for (let i = 0; i < count; i++) nodes.push(readGuiSemanticNode(r));
  const focusTag = r.u8();
  let focused: GuiSemanticFocus | undefined;
  if (focusTag === 1) {
    const id = r.u32();
    if (id === 0) fail("GUI semantic focus identity");
    focused = { id, lifetime: r.u32() };
  } else if (focusTag !== 0) fail("GUI semantic focus option");
  r.done();
  return {
    entity,
    rootIncarnation,
    evaluationTick,
    nodes,
    ...(focused === undefined ? {} : { focused }),
  };
}
