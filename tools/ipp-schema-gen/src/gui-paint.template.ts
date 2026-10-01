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
