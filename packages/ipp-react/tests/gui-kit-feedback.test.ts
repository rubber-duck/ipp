/**
 * Declarations of the GUI kit's feedback (inline alert, status badge, progress
 * bar, empty state, arcs, spinner and circular progress): which entities,
 * components, links, themes and animation each composition writes, through the
 * real reconciler against the recording World in `gui-kit-support.ts`.
 * Rendered appearance is the skin lab's evidence
 * (`tests/gui/skin-lab/specimens/`); these tests pin the structure, the theme
 * references and the token arithmetic.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h } from "react";
import { Entity } from "../src/index.js";
import {
  EmptyState,
  GUI_KIT_ICONS,
  InlineAlert,
  ProgressBar,
  StatusBadge,
  type ProgressBarProps,
  CircularProgress,
  Spinner,
  type CircularProgressProps,
} from "../src/gui-kit.js";
import {
  TOKENS,
  PART,
  CHECK_MARK,
  DESCRIPTORS,
  rowOffset,
  KitWorld,
  FONT,
  render,
  themeRows,
  skinRows,
  ring,
} from "./gui-kit-support.js";

test("InlineAlert draws its severity's frame, icon and tone with optional action", async () => {
  const { world, draw } = await render(
    h(InlineAlert, {
      id: "alert",
      severity: "warning",
      text: "Connection lost.",
      action: { label: "Reconnect" },
    }),
    { fontSize: 32 },
  );
  // Every length is twice its design size at twice the body size.
  const layout = world.fields("alert", "GuiLayout");
  assert.equal(layout.get("kind"), 1);
  assert.equal(layout.get("height"), 80);
  assert.equal(layout.get("padding_left"), 32);
  assert.equal(layout.get("padding_right"), 16);
  // Children centre on the row's content height, not on the tallest child.
  assert.equal(world.fields("alert/strut", "GuiLayout").get("height"), 80);
  assert.equal(world.fields("alert/strut", "GuiLayout").get("width"), 0);
  assert.equal(world.skin("alert"), "alertWarning");
  assert.deepEqual(world.children("alert"), [
    "alert/strut",
    "alert/icon",
    "alert/text",
    "alert/action",
  ]);
  assert.equal(
    world.fields("alert/icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.warning,
  );
  assert.equal(world.fields("alert/icon", "CanvasText").get("font_size"), 48);
  assert.deepEqual(world.tone("alert/icon"), TOKENS.amber);
  assert.deepEqual(world.tone("alert/text"), TOKENS.text);
  assert.equal(world.fields("alert/text", "CanvasText").get("source"), FONT);
  assert.equal(world.fields("alert/text", "CanvasText").get("font_size"), 32);
  assert.equal(
    world.fields("alert/action", "GuiButton").get("label"),
    "Reconnect",
  );
  assert.equal(world.skin("alert/action"), "secondarySmall");
  assert.equal(world.fields("alert/action", "GuiFont").get("font_size"), 26);
  const action = world.fields("alert/action", "GuiLayout");
  assert.equal(action.get("height"), 64);
  assert.ok(
    Math.abs((action.get("width") as number) - (9 * 0.54 * 26 + 64)) < 1e-9,
  );

  await draw(
    h(InlineAlert, { id: "alert", severity: "error", text: "Failed." }),
  );
  assert.equal(world.skin("alert"), "alertError");
  assert.equal(
    world.fields("alert/icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("alert/icon"), TOKENS.error);
  assert.equal(world.entities.has("alert/action"), false);
  assert.equal(world.fields("alert", "GuiLayout").get("padding_right"), 32);

  await draw(
    h(InlineAlert, { id: "alert", severity: "information", text: "Note." }),
  );
  assert.equal(world.skin("alert"), "alertInformation");
  assert.deepEqual(world.tone("alert/icon"), TOKENS.accent);
});

test("StatusBadge gives each status its frame, marker shape and tone, hugging its label", async () => {
  const { world } = await render([
    h(StatusBadge, { key: 1, id: "active", status: "active", label: "Online" }),
    h(StatusBadge, { key: 2, id: "busy", status: "busy", label: "Syncing" }),
    h(StatusBadge, {
      key: 3,
      id: "inactive",
      status: "inactive",
      label: "Offline",
    }),
    h(StatusBadge, {
      key: 4,
      id: "warning",
      status: "warning",
      label: "Degraded",
    }),
    h(StatusBadge, { key: 5, id: "error", status: "error", label: "Error" }),
  ]);
  const expected = {
    active: { frame: "badgeAccent", tone: TOKENS.accent, marker: "markerLit" },
    busy: {
      frame: "badgeAccent",
      tone: TOKENS.accent,
      marker: GUI_KIT_ICONS.sync,
    },
    inactive: {
      frame: "badgeNeutral",
      tone: TOKENS.neutral,
      marker: "markerUnlit",
    },
    warning: {
      frame: "badgeAmber",
      tone: TOKENS.amber,
      marker: GUI_KIT_ICONS.warning,
    },
    error: {
      frame: "badgeError",
      tone: TOKENS.error,
      marker: GUI_KIT_ICONS.error,
    },
  };
  for (const [id, { frame, tone, marker }] of Object.entries(expected)) {
    assert.equal(world.skin(id), frame, id);
    assert.equal(world.fields(id, "GuiLayout").get("height"), 32);
    assert.deepEqual(world.tone(`${id}/label`), tone, id);
    assert.equal(
      world.fields(`${id}/label`, "CanvasText").get("font_size"),
      13,
    );
    if (marker.startsWith("marker")) {
      assert.equal(world.skin(`${id}/marker`), marker, id);
      assert.equal(world.fields(`${id}/marker`, "GuiLayout").get("width"), 12);
    } else {
      assert.equal(
        world.fields(`${id}/marker`, "CanvasText").get("text"),
        marker,
      );
      assert.deepEqual(world.tone(`${id}/marker`), tone, id);
    }
    // Badges are not controls.
    assert.equal(world.entity(id).components.has("GuiButton"), false);
  }
  const label = "Online".length * 0.54 * 13;
  assert.ok(
    Math.abs(
      (world.fields("active", "GuiLayout").get("width") as number) -
        (10 + 12 + 8 + 10 + label),
    ) < 1e-9,
  );
});

const upload = (props: Partial<ProgressBarProps>) =>
  h(ProgressBar, { id: "upload", label: "Uploading", ...props });

const scan = (props: Partial<ProgressBarProps>) =>
  h(ProgressBar, { id: "scan", label: "Scanning", ...props });

/** A fill piece's own row: its cut corners, and the leading section's rest. */
function piece(cut: readonly number[], lead = false) {
  return [
    [
      0,
      {
        corner_cut: cut,
        ...(lead ? { opacity: 0.6 } : {}),
        part: PART.background,
      },
    ],
  ];
}

const START = [4, 0, 0, 0];
const END = [0, 0, 4, 0];
const BOTH = [4, 0, 4, 0];
const NONE = [0, 0, 0, 0];

test("ProgressBar fills the reported fraction and labels each outcome", async () => {
  const { world, draw } = await render(upload({ value: 0.656 }));
  assert.equal(world.fields("upload", "GuiLayout").get("height"), 24 + 4 + 32);
  assert.equal(world.skin("upload/frame"), "frame");
  // The frame holds the track in its clearance: a stack of its own, so
  // alignment in it stays inside the clearance.
  const frame = world.fields("upload/frame", "GuiLayout");
  assert.equal(frame.get("kind"), 4);
  assert.equal(frame.get("height"), 32);
  assert.equal(frame.get("padding_left"), 4);
  assert.deepEqual(world.children("upload/frame"), ["upload/track"]);
  assert.equal(world.fields("upload/track", "GuiLayout").get("kind"), 3);
  assert.equal(world.fields("upload/track", "GuiLayout").get("height"), 24);
  assert.deepEqual(world.children("upload/track"), ["upload/fills"]);
  assert.equal(world.fields("upload/fills", "GuiLayout").get("kind"), 1);
  assert.equal(world.fields("upload/fills", "GuiLayout").get("height"), 24);
  // The fill, the leading section just ahead of it, and the rest of the track.
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/lead",
    "upload/rest",
  ]);
  assert.equal(world.skin("upload/fill/0"), "valueAccent");
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 0.656);
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("height"), 24);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(START));
  // The leading section is the language's bar wide and ends the run's cut.
  assert.equal(world.skin("upload/lead"), "valueAccent");
  assert.equal(world.fields("upload/lead", "GuiLayout").get("width"), 8);
  assert.equal(world.fields("upload/lead", "GuiLayout").get("flex"), undefined);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));
  assert.ok(
    Math.abs(
      (world.fields("upload/rest", "GuiLayout").get("flex") as number) - 0.344,
    ) < 1e-9,
  );
  // The readout never runs ahead of the report.
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "65%");
  assert.deepEqual(world.tone("upload/readout"), TOKENS.text);
  assertPulsing(world);

  await draw(upload({ value: 0.999 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "99%");
  // The reported value itself, not one less from rounding.
  await draw(upload({ value: 0.57 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "57%");
  await draw(upload({ segments: [{ value: 0.1 }, { value: 0.7 }] }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "80%");

  // Finished states have no leading section and no animation.
  await draw(upload({ status: "complete" }));
  assert.deepEqual(world.children("upload/fills"), ["upload/fill/0"]);
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 1);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(BOTH));
  assert.deepEqual(world.children("upload/track"), [
    "upload/fills",
    "upload/check",
  ]);
  assert.equal(world.skin("upload/check"), "check");
  const check = world.fields("upload/check", "GuiLayout");
  assert.equal(check.get("width"), 16);
  assert.equal(check.get("align_x"), 1);
  assert.equal(check.get("margin_right"), 4);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Complete",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.accent);
  assert.equal(world.controllers.size, 0);

  await draw(upload({ value: 0.4, status: "cancelled" }));
  assert.equal(world.skin("upload/fill/0"), "valueNeutral");
  assert.equal(world.fields("upload/fill/0", "GuiLayout").get("flex"), 0.4);
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(BOTH));
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/rest",
  ]);
  assert.equal(world.entities.has("upload/check"), false);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Cancelled",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.neutral);
  assert.equal(world.controllers.size, 0);

  await draw(upload({ value: 0.4, status: "failed" }));
  assert.equal(world.skin("upload/fill/0"), "valueError");
  assert.equal(
    world.fields("upload/outcome-icon", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "Failed",
  );
  assert.deepEqual(world.tone("upload/readout"), TOKENS.error);
  assert.equal(world.controllers.size, 0);

  // At zero the leading section sits at the start of the track, both cuts its.
  await draw(upload({ value: 0 }));
  assert.deepEqual(world.children("upload/fills"), [
    "upload/lead",
    "upload/rest",
  ]);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(BOTH, true));
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 1);
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "0%");
  assertPulsing(world);
});

test("ProgressBar draws its parts in order in their tones, within the track", async () => {
  // Parts past the track's end are clamped; empty parts draw nothing.
  const { world, draw } = await render(
    upload({
      segments: [
        { value: 0.25 },
        { value: 0, tone: "amber" },
        { value: 0.125, tone: "neutral" },
      ],
    }),
  );
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/2",
    "upload/lead",
    "upload/rest",
  ]);
  assert.equal(world.skin("upload/fill/0"), "valueAccent");
  assert.equal(world.skin("upload/fill/2"), "valueNeutral");
  // Square joints inside the run, the part cut on its outer ends.
  assert.deepEqual(skinRows(world, "upload/fill/0"), piece(START));
  assert.deepEqual(skinRows(world, "upload/fill/2"), piece(NONE));
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 0.625);
  // The readout is the floored total.
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "37%");

  await draw(
    upload({
      segments: [
        { value: 0.5 },
        { value: 0.25, tone: "amber" },
        { value: 0.5, tone: "error" },
      ],
    }),
  );
  assert.equal(world.skin("upload/fill/1"), "valueAmber");
  assert.equal(world.skin("upload/fill/2"), "valueError");
  assert.equal(world.fields("upload/fill/2", "GuiLayout").get("flex"), 0.25);
  // A full track leaves no rest; the fill shares it with the leading section.
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/1",
    "upload/fill/2",
    "upload/lead",
  ]);
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("text"),
    "100%",
  );
  assertPulsing(world);

  // Complete fills the track with the parts in proportion.
  await draw(
    upload({
      segments: [{ value: 0.25 }, { value: 0.125, tone: "amber" }],
      status: "complete",
    }),
  );
  assert.deepEqual(world.children("upload/fills"), [
    "upload/fill/0",
    "upload/fill/1",
  ]);
  assert.ok(
    Math.abs(
      (world.fields("upload/fill/0", "GuiLayout").get("flex") as number) -
        2 / 3,
    ) < 1e-9,
  );
  assert.equal(world.skin("upload/fill/1"), "valueAmber");
  assert.deepEqual(skinRows(world, "upload/fill/1"), piece(END));
  assert.equal(world.controllers.size, 0);

  // An outcome recolours the reached parts.
  await draw(
    upload({
      segments: [{ value: 0.25 }, { value: 0.125, tone: "amber" }],
      status: "failed",
    }),
  );
  assert.equal(world.skin("upload/fill/0"), "valueError");
  assert.equal(world.skin("upload/fill/1"), "valueError");
  assert.equal(world.fields("upload/rest", "GuiLayout").get("flex"), 0.625);
});

test("the leading section pulses only while the task runs and the bar is declared", async () => {
  const bar = (props: Partial<ProgressBarProps>) =>
    upload({ value: 0.3, ...props });
  const { world, draw } = await render(bar({}), { reducedMotion: true });
  // Reduced motion: present and steady at its rest.
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/lead"), piece(END, true));

  await draw(bar({}));
  assertPulsing(world);
  await draw(bar({ status: "complete" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("upload/lead"), false);
  await draw(bar({}));
  assert.equal(world.controllers.size, 1);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("upload/lead"), false);
});

test("an unknown duration moves its segment across the frame on the Host clock", async () => {
  const { world, draw } = await render(scan({}));
  assert.equal(world.entities.has("scan/readout"), false);
  assert.deepEqual(world.children("scan/track"), ["scan/segment"]);
  assert.equal(world.skin("scan/segment"), "valueAccent");
  const segment = world.fields("scan/segment", "GuiLayout");
  assert.equal(segment.get("width"), 96);
  assert.equal(segment.get("height"), 24);
  // It rests at the start; the animation adds its change to the alignment.
  assert.equal(segment.get("align_x"), -1);
  // Its clearance from the frame's ends is the frame's padding: it moves in
  // the track, a stack of its own inside it, since a stack aligns its
  // children within its outer box.
  const frame = world.fields("scan/frame", "GuiLayout");
  assert.equal(frame.get("kind"), 4);
  assert.equal(frame.get("padding_left"), 4);
  assert.equal(frame.get("padding_right"), 4);
  assert.equal(segment.get("margin_left"), undefined);

  // One looping controller drives the segment's layout alignment from one
  // end of the frame to the other and back.
  const [controller] = [...world.controllers.values()];
  assert.ok(controller, "no animation controller");
  assert.equal(controller.looping, true);
  assert.equal(world.controllers.size, 1);
  assert.deepEqual(world.controls, [{ action: "play" }]);
  const layout = DESCRIPTORS.GuiLayout!;
  assert.deepEqual(controller.drivers[0]!.property, {
    component: layout.id,
    offsets: [layout.fields.align_x!.offset],
  });
  const [clip] = [...world.clips.values()] as {
    duration: number;
    tracks: { keys: { time: number; value: { value: number } }[] }[];
  }[];
  assert.equal(clip!.duration, 2);
  assert.deepEqual(
    clip!.tracks[0]!.keys.map((key) => [key.time, key.value.value]),
    [
      [0, -1],
      [1, 1],
      [2, -1],
    ],
  );

  // Reduced motion holds it centred; a reported value replaces it.
  await draw(scan({}), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.equal(world.fields("scan/segment", "GuiLayout").get("align_x"), 0);
  await draw(scan({ value: 0.2 }));
  assert.equal(world.entities.has("scan/segment"), false);
  assert.deepEqual(world.children("scan/track"), ["scan/fills"]);
});

test("EmptyState centres its message in a content frame", async () => {
  const { world } = await render(
    h(EmptyState, { id: "empty", text: "No records" }),
  );
  assert.equal(world.skin("empty"), "frame");
  assert.equal(world.fields("empty", "GuiLayout").get("kind"), 3);
  assert.equal(world.fields("empty", "GuiLayout").get("height"), 40);
  assert.equal(
    world.fields("empty/text", "CanvasText").get("text"),
    "No records",
  );
  assert.equal(world.fields("empty/text", "GuiLayout").get("align_x"), 0);
  assert.deepEqual(world.tone("empty/text"), TOKENS.neutral);
  assert.equal(world.fields("empty/text", "CanvasText").get("font_size"), 13);
});

/** The leading arcs' sweeps: 8 units along the middle of each ring. */
const LEAD_LARGE = 8 / (Math.PI * 116);
const LEAD_SMALL = 8 / (Math.PI * 56);

/** The one turning arc's controller: a looping turn a second of its start. */
interface RowClip {
  readonly duration: number;
  readonly tracks: readonly {
    readonly property: { readonly offsets: readonly number[] };
    readonly keys: readonly {
      readonly time: number;
      readonly value: unknown;
    }[];
  }[];
}

/** The one controller, looping, and the clip it plays on an own-row `property`. */
function loopingRowClip(world: KitWorld, property: string): RowClip {
  assert.equal(world.controllers.size, 1);
  const [controller] = [...world.controllers.values()];
  assert.equal(controller!.looping, true);
  const offset = rowOffset(0, property);
  assert.deepEqual(controller!.drivers[0]!.property, {
    component: DESCRIPTORS.GuiSkin!.id,
    offsets: [offset],
  });
  const clip = ([...world.clips.values()] as RowClip[]).find(
    (candidate) => candidate.tracks[0]?.property.offsets[0] === offset,
  );
  assert.ok(clip, `no clip on ${property}`);
  return clip;
}

/** A row property's key: a dynamic value. */
function dynamic(value: number) {
  return { kind: "dynamic", value: { kind: "f32", value } };
}

/** The one turning arc's controller: a looping turn a second of its start. */
function assertTurning(world: KitWorld) {
  const clip = loopingRowClip(world, "arc_start");
  assert.equal(clip.duration, 1);
  assert.ok(clip.tracks[0]!.keys.every((key) => !("interpolation" in key)));
  assert.deepEqual(
    clip.tracks[0]!.keys.map((key) => [key.time, key.value]),
    [
      [0, dynamic(0)],
      [1, dynamic(1)],
    ],
  );
}

/**
 * The one pulsing section's controller: its opacity eases from its rest of
 * 0.6 down to 0.1 and back every 1.5 seconds, so it stays within 0..1.
 */
function assertPulsing(world: KitWorld) {
  const clip = loopingRowClip(world, "opacity");
  assert.equal(clip.duration, 1.5);
  const keys = clip.tracks[0]!.keys;
  assert.equal(keys.length, 9);
  // Linear by the encoder's default: an interpolation on the last key, which
  // has no next, fails to encode.
  assert.deepEqual(keys[0], { time: 0, value: dynamic(0) });
  assert.ok(keys.every((key) => !("interpolation" in key)));
  assert.equal(keys[4]!.time, 0.75);
  assert.deepEqual(keys[4]!.value, dynamic(-0.5));
  assert.equal(keys[8]!.time, 1.5);
  for (const key of keys) {
    const change = (key.value as { value: { value: number } }).value.value;
    assert.ok(change <= 1e-12 && 0.6 + change >= 0.1 - 1e-9, `${change}`);
  }
}

test("the arc themes are the ring shape in role colours, and the lit check", async () => {
  const { world } = await render(h(Entity, { id: "empty" }));
  for (const [name, color] of [
    ["arcTrack", TOKENS.line],
    ["arcAccent", TOKENS.accent],
    ["arcError", TOKENS.error],
  ] as const)
    assert.deepEqual(themeRows(world, name).rows, [
      [0, { part: PART.background, shape: 2, color }],
    ]);
  assert.deepEqual(themeRows(world, "checkLit").rows, [
    [0, { ...CHECK_MARK, part: PART.background, color: TOKENS.accent }],
  ]);
});

test("Spinner turns a lit quarter over a quiet track once a second beside its text", async () => {
  const { world } = await render(
    h(Spinner, { id: "busy", label: "Preparing..." }),
    { fontSize: 32 },
  );
  const layout = world.fields("busy", "GuiLayout");
  assert.equal(layout.get("kind"), 1);
  assert.equal(layout.get("height"), 48);
  // The text and a hundredth of its size, so rounding never wraps it.
  assert.ok(
    Math.abs((layout.get("width") as number) - (64 + 12 * 0.54 * 32 + 0.32)) <
      1e-9,
  );
  assert.equal(world.fields("busy", "GuiFont").get("font_size"), 32);
  assert.deepEqual(world.children("busy"), [
    "busy/strut",
    "busy/track",
    "busy/label",
  ]);
  assert.deepEqual(world.children("busy/track"), ["busy/arc"]);
  assert.equal(world.skin("busy/track"), "arcTrack");
  assert.equal(world.skin("busy/arc"), "arcAccent");
  for (const symbol of ["busy/track", "busy/arc"]) {
    const ringLayout = world.fields(symbol, "GuiLayout");
    assert.equal(ringLayout.get("width"), 48);
    assert.equal(ringLayout.get("height"), 48);
    assert.equal(ringLayout.get("align_y"), 0);
  }
  // Thickness is absolute in the World's units; the start is present to bind.
  assert.deepEqual(skinRows(world, "busy/track"), ring(6, 1));
  assert.deepEqual(skinRows(world, "busy/arc"), ring(6, 0.25));
  assert.equal(
    world.fields("busy/label", "CanvasText").get("text"),
    "Preparing...",
  );
  assert.equal(world.fields("busy/label", "CanvasText").get("font_size"), 32);
  assert.equal(world.fields("busy/label", "GuiLayout").get("margin_left"), 16);
  assert.deepEqual(world.tone("busy/label"), TOKENS.text);
  assert.equal(world.entity("busy").components.has("GuiButton"), false);

  assertTurning(world);
  assert.deepEqual(world.controls, [{ action: "play" }]);
});

test("a spinner's animation exists only while it is declared and moving", async () => {
  const busy = () => h(Spinner, { id: "busy", label: "Preparing..." });
  const { world, draw } = await render(busy(), { reducedMotion: true });
  // Reduced motion: the same symbol, frozen at its rest.
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "busy/arc"), ring(3, 0.25));

  await draw(busy(), { reducedMotion: false });
  assertTurning(world);
  await draw(busy(), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.ok(world.entities.has("busy/arc"));

  // Removing a turning spinner removes its animation with it.
  await draw(busy());
  assert.equal(world.controllers.size, 1);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
  assert.equal(world.entities.has("busy"), false);
  assert.equal(world.entities.has("busy/arc"), false);
});

const ringProgress = (props: Partial<CircularProgressProps>) =>
  h(CircularProgress, { id: "upload", label: "Uploading", ...props });

test("CircularProgress draws the reported fraction from twelve o'clock and its outcomes", async () => {
  const { world, draw } = await render(ringProgress({ value: 0.656 }));
  const layout = world.fields("upload", "GuiLayout");
  assert.equal(layout.get("kind"), 2);
  assert.equal(layout.get("width"), 128);
  assert.equal(layout.get("height"), 128 + 8 + 24);
  assert.deepEqual(world.children("upload"), ["upload/ring", "upload/caption"]);
  assert.equal(world.skin("upload/ring"), "arcTrack");
  assert.equal(world.fields("upload/ring", "GuiLayout").get("width"), 128);
  assert.equal(world.fields("upload/ring", "GuiLayout").get("align_x"), 0);
  assert.deepEqual(skinRows(world, "upload/ring"), ring(12, 1));
  assert.deepEqual(world.children("upload/ring"), [
    "upload/value",
    "upload/lead",
    "upload/readout",
  ]);
  // The lit arc shares the ring with the leading arc just ahead of it.
  assert.equal(world.skin("upload/value"), "arcAccent");
  const reached = 0.656 * (1 - LEAD_LARGE);
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, reached));
  assert.equal(world.skin("upload/lead"), "arcAccent");
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(12, LEAD_LARGE, reached, true),
  );
  // The readout is floored, so it never runs ahead of the report.
  const readout = world.fields("upload/readout", "CanvasText");
  assert.equal(readout.get("text"), "65%");
  assert.equal(readout.get("font_size"), 24);
  assert.deepEqual(world.tone("upload/readout"), TOKENS.text);
  assert.equal(world.fields("upload/readout", "GuiLayout").get("align_x"), 0);
  // The caption centres under the ring and hugs the task.
  const caption = world.fields("upload/caption", "GuiLayout");
  assert.equal(caption.get("align_x"), 0);
  assert.equal(caption.get("margin_top"), 8);
  assert.ok(
    Math.abs((caption.get("width") as number) - (9 * 0.54 * 16 + 0.16)) < 1e-9,
  );
  assert.deepEqual(world.children("upload/caption"), [
    "upload/caption/strut",
    "upload/label",
  ]);
  assert.equal(
    world.fields("upload/label", "CanvasText").get("text"),
    "Uploading",
  );
  // Read-only: no control; only the leading arc's pulse moves.
  assert.equal(world.entity("upload").components.has("GuiButton"), false);
  assert.equal(world.entity("upload").components.has("GuiBehavior"), false);
  assertPulsing(world);

  await draw(ringProgress({ value: 0.999 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "99%");
  await draw(ringProgress({ value: 0.57 }));
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "57%");

  // Zero is the bare track, the leading arc at twelve o'clock.
  await draw(ringProgress({ value: 0 }));
  assert.equal(world.entities.has("upload/value"), false);
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(12, LEAD_LARGE, 0, true),
  );
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "0%");
  await draw(ringProgress({ value: 0 }), { reducedMotion: true });
  assert.equal(world.controllers.size, 0);
  assert.ok(world.entities.has("upload/lead"));

  // Complete: the whole ring lit, the check in the centre, 100% beside the task.
  await draw(ringProgress({ status: "complete" }));
  assert.equal(world.entities.has("upload/lead"), false);
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, 1));
  assert.equal(world.skin("upload/value"), "arcAccent");
  assert.equal(world.entities.has("upload/readout"), false);
  assert.equal(world.skin("upload/symbol"), "checkLit");
  const check = world.fields("upload/symbol", "GuiLayout");
  assert.equal(check.get("width"), 48);
  // The checkbox's 16-unit mark drawn three times larger, stroke and all.
  assert.equal(world.fields("upload/symbol", "GuiFont").get("font_size"), 48);
  assert.equal(check.get("align_x"), 0);
  assert.equal(check.get("align_y"), 0);
  assert.equal(
    world.fields("upload/outcome", "CanvasText").get("text"),
    "100%",
  );
  assert.deepEqual(world.tone("upload/outcome"), TOKENS.accent);
  assert.equal(
    world.fields("upload/outcome", "GuiLayout").get("margin_left"),
    8,
  );
  assert.ok(
    Math.abs(
      (world.fields("upload/caption", "GuiLayout").get("width") as number) -
        (13 * 0.54 * 16 + 8 + 0.16),
    ) < 1e-9,
  );

  // Failed: the reached arc in the error colour, the error icon in its centre.
  await draw(ringProgress({ value: 0.4, status: "failed" }));
  assert.equal(world.entities.has("upload/lead"), false);
  assert.equal(world.skin("upload/value"), "arcError");
  assert.deepEqual(skinRows(world, "upload/value"), ring(12, 0.4));
  assert.equal(
    world.fields("upload/symbol", "CanvasText").get("text"),
    GUI_KIT_ICONS.error,
  );
  assert.deepEqual(world.tone("upload/symbol"), TOKENS.error);
  assert.ok(
    Math.abs(
      (world.fields("upload/symbol", "CanvasText").get("font_size") as number) -
        32 / 0.54,
    ) < 1e-9,
  );
  assert.equal(
    world.fields("upload/outcome", "CanvasText").get("text"),
    "Failed",
  );
  assert.deepEqual(world.tone("upload/outcome"), TOKENS.error);
  assert.equal(world.controllers.size, 0);
});

test("an idle ring is the quiet track alone above its task", async () => {
  const { world, draw } = await render(
    ringProgress({ label: "PULSE", size: "small", idle: true, value: 0.5 }),
  );
  assert.equal(world.skin("upload/ring"), "arcTrack");
  assert.deepEqual(skinRows(world, "upload/ring"), ring(8, 1));
  // No arc, no leading cue and no percentage, so nothing moves.
  assert.deepEqual(world.children("upload/ring"), []);
  assert.equal(world.controllers.size, 0);
  assert.equal(world.fields("upload/label", "CanvasText").get("text"), "PULSE");
  // An outcome given with it is not shown either.
  await draw(
    ringProgress({
      label: "PULSE",
      size: "small",
      idle: true,
      status: "complete",
    }),
  );
  assert.deepEqual(world.children("upload/ring"), []);
  assert.equal(world.entities.has("upload/outcome"), false);
  // Running again draws the arc and the percentage.
  await draw(ringProgress({ label: "PULSE", size: "small", value: 0.5 }));
  assert.deepEqual(world.children("upload/ring"), [
    "upload/value",
    "upload/lead",
    "upload/readout",
  ]);
});

test("a ring of unknown total turns the spinner's quarter and shows no percentage", async () => {
  const { world, draw } = await render(
    ringProgress({ label: "Scanning", size: "small" }),
  );
  const layout = world.fields("upload", "GuiLayout");
  // The caption is wider than the small ring, so it sets the width.
  assert.ok(
    Math.abs((layout.get("width") as number) - (8 * 0.54 * 16 + 0.16)) < 1e-9,
  );
  assert.equal(layout.get("height"), 64 + 8 + 24);
  assert.equal(world.fields("upload/ring", "GuiLayout").get("width"), 64);
  assert.deepEqual(skinRows(world, "upload/ring"), ring(8, 1));
  assert.deepEqual(world.children("upload/ring"), ["upload/value"]);
  assert.deepEqual(skinRows(world, "upload/value"), ring(8, 0.25));
  assert.deepEqual(world.children("upload/caption"), [
    "upload/caption/strut",
    "upload/label",
  ]);
  assertTurning(world);

  await draw(ringProgress({ label: "Scanning", size: "small" }), {
    reducedMotion: true,
  });
  assert.equal(world.controllers.size, 0);
  assert.deepEqual(skinRows(world, "upload/value"), ring(8, 0.25));

  // A report replaces the turning arc with the leading arc's pulse.
  await draw(ringProgress({ size: "small", value: 0.5 }));
  assertPulsing(world);
  assert.deepEqual(
    skinRows(world, "upload/value"),
    ring(8, 0.5 * (1 - LEAD_SMALL)),
  );
  assert.deepEqual(
    skinRows(world, "upload/lead"),
    ring(8, LEAD_SMALL, 0.5 * (1 - LEAD_SMALL), true),
  );
  assert.equal(world.fields("upload/readout", "CanvasText").get("text"), "50%");
  assert.equal(
    world.fields("upload/readout", "CanvasText").get("font_size"),
    16,
  );

  await draw(ringProgress({ size: "small" }));
  assertTurning(world);
  await draw(h(Entity, { id: "done" }));
  assert.equal(world.controllers.size, 0);
});
