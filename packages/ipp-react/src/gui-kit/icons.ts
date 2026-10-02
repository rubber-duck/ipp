/**
 * Icon glyphs of the shared GUI font, Shure Tech Mono Nerd Font, by meaning.
 * Each is the font's Material Design glyph named beside it; its ink fills its
 * advance and is centred in the line box. Every code point was checked against
 * the character map of the font source the `font-assets` product builds.
 */
export const GUI_KIT_ICONS = {
  /** nf-md-information: a filled circle with an i. */
  information: "\u{f02fc}",
  /** nf-md-alert: a filled triangle with an exclamation mark. */
  warning: "\u{f0026}",
  /** nf-md-alert_octagon: a filled octagon with an exclamation mark. */
  error: "\u{f0029}",
  /** nf-md-sync: two arrows chasing each other. */
  sync: "\u{f04e6}",
  /** nf-md-chevron_right: collapsed. */
  collapsed: "\u{f0142}",
  /** nf-md-chevron_down: expanded. */
  expanded: "\u{f0140}",
  // Window controls: plain outline shapes in one stroke weight, about
  // 0.06 em, the sheets' weight. The Codicons chrome set (U+EAB8 to U+EABB)
  // has the same shapes at about half the weight.
  /** nf-md-close. */
  close: "\u{f0156}",
  /** nf-md-square_outline. */
  maximize: "\u{f0763}",
  /** nf-md-minus. */
  minimize: "\u{f0374}",
  /** nf-md-checkbox_multiple_blank_outline. */
  restore: "\u{f0137}",
  // Sort markers: sharp-cornered triangles 0.88 as high as wide, their ink
  // 0.085 to 0.558 em above the baseline, so they centre on the capitals.
  /** nf-md-triangle_small_down: sorted descending. */
  sortDescending: "\u{f1a09}",
  /** nf-md-triangle_small_up: sorted ascending. */
  sortAscending: "\u{f1a0a}",
  // Scroll buttons of a strip that overflows.
  /** nf-md-chevron_left: towards the start. */
  previous: "\u{f0141}",
  /** nf-md-chevron_right: towards the end. */
  next: "\u{f0142}",
  /** nf-md-magnify: a search field. */
  search: "\u{f0349}",
} as const;

export type GuiKitIcon = keyof typeof GUI_KIT_ICONS;
