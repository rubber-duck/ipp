type GuiPaintKeyInput<Entry> = Entry extends {
  part: infer Part;
  state: infer State;
  variant: infer Variant;
}
  ? { readonly part: Part } & (State extends null
      ? { readonly state?: never }
      : { readonly state: State }) &
      (Variant extends null
        ? { readonly variant?: never }
        : { readonly variant: Variant })
  : never;

/** Valid base/state/variant combinations exported by the compiled GUI capability. */
export type GuiPaintPartKey = GuiPaintKeyInput<
  (typeof GUI_PAINT_PART_KEYS)[number]
>;
/** Per-control GuiSkin overrides accept only unqualified parts. */
export type GuiPaintBasePartKey = Extract<
  GuiPaintPartKey,
  { readonly state?: never }
>;

export function guiPaintPartIndex(key: GuiPaintPartKey): number {
  const entry = GUI_PAINT_PART_KEYS.find(
    (candidate) =>
      candidate.part === key.part &&
      candidate.state === (key.state ?? null) &&
      candidate.variant === (key.variant ?? null),
  );
  if (!entry) throw new Error("Invalid GUI paint part key");
  return entry.index;
}

/** Names of the built-in skin looks. */
export type GuiSkinLookName = keyof typeof GUI_SKIN_LOOKS;

/**
 * A built-in look's rows as a `GuiTheme.parts` table. An ordinary theme entity
 * takes them with the look's `em`, so its lengths follow the font of each
 * control it skins: `GuiTheme.encodeParts(guiSkinLookTable("switch"))` and
 * `GUI_SKIN_LOOKS.switch.em`. A control paints its kind's default look without
 * a theme; a look used as a theme still sits on that default property by
 * property.
 */
export function guiSkinLookTable(name: GuiSkinLookName) {
  const rows = GUI_SKIN_LOOKS[name].parts;
  return {
    nextSlot: rows.length,
    rows: new Map(rows.map((row, slot) => [slot, row] as const)),
  };
}

/**
 * A built-in look's transition timing as a `GuiThemeMotion.parts` table, for
 * the same theme entity as its `guiSkinLookTable` rows:
 * `GuiThemeMotion.encodeParts(guiSkinLookMotionTable("switch"))`. A theme
 * with these rows moves between interaction states as the look does in a
 * World that selects animation; its kind's default look times what they
 * leave out.
 */
export function guiSkinLookMotionTable(name: GuiSkinLookName) {
  const rows = GUI_SKIN_LOOKS[name].motion;
  return {
    nextSlot: rows.length,
    rows: new Map(rows.map((row, slot) => [slot, row] as const)),
  };
}
