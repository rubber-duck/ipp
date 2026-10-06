/**
 * The kit's looks: theme tables built from the design-language tokens and the
 * built-in looks of the receiving runtime's generated contract, never from
 * copied values. `GuiKit` declares every table once per World as an ordinary
 * theme entity with the tokens' `em`, so lengths follow the font each skinned
 * entity inherits; kit components reference them through `useGuiKit().theme`.
 *
 * Entities that are not controls paint only their Background part's base row,
 * so their tables are one row. A kit component's tables live in its section
 * below; the rules they follow are the skin lab's design language
 * (`tests/gui/skin-lab/README.md`).
 */
import type {
  GuiKitColor,
  GuiKitContract,
  GuiKitLook,
  GuiKitRow,
} from "./kit.js";

type Corners = readonly [number, number, number, number];

/** No cut: what sits on a container's lines takes its square corners. */
const SQUARE: Corners = [0, 0, 0, 0];

/** Fully transparent: a part that paints nothing at rest. */
const CLEAR: GuiKitColor = [0, 0, 0, 0];

/** One table of rows and the font size their lengths were designed at. */
interface KitTheme {
  readonly rows: readonly GuiKitRow[];
  readonly em: number;
}

/** The one cut pattern: `length` on the paired top-left and bottom-right corners. */
function paired(length: number): Corners {
  return [length, 0, length, 0];
}

/** `color` at `alpha`: a tint of a role colour. */
function tint(color: GuiKitColor, alpha: number): GuiKitColor {
  return [color[0], color[1], color[2], alpha];
}

/**
 * The opaque colour of `color` tinted over `base` at `alpha`, blended in
 * linear light as the renderer blends: a tint for a part that floats over
 * other content, which must not show through it.
 */
function over(
  base: GuiKitColor,
  color: GuiKitColor,
  alpha: number,
): GuiKitColor {
  const mix = (channel: 0 | 1 | 2) =>
    base[channel] * (1 - alpha) + color[channel] * alpha;
  return [mix(0), mix(1), mix(2), 1];
}

/**
 * Alpha of an alert interior's tint. Blended in linear light, the row tint's
 * 4% over a whole alert would outshine its text; the sheets' alert
 * interiors measure under 1%.
 */
const ALERT_TINT = 0.01;

/**
 * A built-in look's rows with `overrides` merged property by property at
 * their paint keys, as a theme sits on a default look.
 */
function extend(
  look: GuiKitLook,
  overrides: readonly GuiKitRow[],
): readonly GuiKitRow[] {
  const rows = new Map(look.parts.map((row) => [row.part, row]));
  for (const row of overrides)
    rows.set(row.part, { ...rows.get(row.part), ...row });
  return [...rows.values()];
}

/** Rows with every cut removed: the docked form of a look. */
function square(rows: readonly GuiKitRow[]): readonly GuiKitRow[] {
  return rows.map((row) =>
    "corner_cut" in row ? { ...row, corner_cut: SQUARE } : row,
  );
}

function catalogue(contract: GuiKitContract) {
  const t = contract.GUI_SKIN_TOKENS;
  const looks = contract.GUI_SKIN_LOOKS;
  const background = contract.guiPaintPartIndex({ part: "background" });
  const state = (name: "hovered" | "pressed") =>
    contract.guiPaintPartIndex({ part: "background", state: name });
  const pressed = state("pressed");
  const hovered = state("hovered");
  const checked = (name: "idle" | "hovered" | "pressed" | "disabled") =>
    contract.guiPaintPartIndex({
      part: "background",
      state: name,
      variant: "checked",
    });
  const icon = contract.guiPaintPartIndex({ part: "icon" });
  const box = (row: Omit<GuiKitRow, "part">): KitTheme => ({
    rows: [{ ...row, part: background }],
    em: t.em,
  });
  // A look drawn at a font size other than the body's keeps its lengths: its
  // em is scaled with the text that sets the control's font size.
  const at = (look: GuiKitLook, size: number, rows = look.parts) => ({
    rows,
    em: look.em * (size / t.textBody),
  });

  // Containers: uncut, a thin frame with corner accents on all four corners,
  // filled with the page; unlit when minimised.
  const container = (line: GuiKitColor) =>
    box({
      color: t.page,
      border_width: t.lineWidth,
      border_color: line,
      corner_accent: [
        t.cornerAccent,
        t.cornerAccent,
        t.cornerAccent,
        t.cornerAccent,
      ],
      corner_accent_width: t.cornerAccentWidth,
    });

  // A value mark: a solid part of the part cut, as the switch block.
  const value = (color: GuiKitColor) =>
    box({ color, corner_cut: paired(t.partCut) });

  // Status frames take their role colour for the idle line, as the amber
  // variant replaces the accent and the idle line; an alert's interior is
  // tinted with its role colour so that it reads as a message rather than a
  // field.
  const alert = (role: GuiKitColor) =>
    box({
      color: tint(role, ALERT_TINT),
      border_width: t.lineWidth,
      border_color: role,
      corner_cut: paired(t.cut),
    });
  const badge = (role: GuiKitColor) =>
    box({
      color: t.surface,
      border_width: t.lineWidth,
      border_color: role,
      corner_cut: paired(t.partCut),
    });

  // The language's one check mark: the checkbox's stroke mark, drawn as a
  // skinned entity's Background over its box.
  const checkMark = looks.checkbox.parts.find((row) => row.part === icon);
  if (!checkMark) throw new Error("The checkbox look has no check mark");

  // The selected row: the row tint across the row and the accent bar as the
  // first `selectionGutter + lineWidth / 2` units of the same box, through a
  // linear gradient whose stops sit a hundredth of a unit apart, so the bar
  // covers the row's first column line.
  const bar = t.selectionGutter + t.lineWidth / 2;
  const selection = (lit: GuiKitColor) => ({
    fill_mode: 1,
    gradient_start: [bar, 0],
    gradient_end: [bar + 0.01, 0],
    gradient_color0: lit,
    gradient_color1: t.rowTint,
  });

  // A toast's body is a button framed as an inline alert of its role: the
  // role colour lights its hover edge, and a press keeps the tint under it,
  // so its content stays legible. It floats over content, so the tint lies
  // on an opaque control interior rather than over what is beneath.
  const toast = (role: GuiKitColor): KitTheme => ({
    rows: [
      {
        part: background,
        color: over(t.surface, role, ALERT_TINT),
        border_color: role,
      },
      { part: hovered, border_color: role, glow_color: role },
      {
        part: pressed,
        color: over(t.surface, role, ALERT_TINT),
        border_color: role,
        glow_color: role,
      },
    ],
    em: looks.button.em,
  });

  return {
    // Frame and Separator. A slider scale's ticks repeat between its labels,
    // so they take the quiet line, as the dial's tick ring does, and its
    // origin's mark the division's neutral. The slider composites' controls
    // paint the runtime's slider and dial looks and need no table.
    container: container(t.accent),
    containerUnlit: container(t.neutral),
    division: box({ color: t.neutral }),
    rule: box({ color: t.accent }),
    quiet: box({ color: t.line }),

    // Buttons whose label is small text take the secondary look at the small
    // type size, so its lengths keep their size against the body text; the
    // amber variant, for a destructive action, swaps the accent and the idle
    // line for amber.
    secondarySmall: at(looks.secondary, t.textSmall),
    secondarySmallAmber: at(looks.secondaryAmber, t.textSmall),

    // Window controls: the icon glyph sets their font size. Docked in a
    // header strip or title bar they are square; free-standing they take
    // the part cut. The amber variant swaps the accent and idle line.
    dockedIcon: at(looks.docked, t.icon),
    dockedIconAmber: at(
      looks.secondaryAmber,
      t.icon,
      square(looks.secondaryAmber.parts),
    ),
    secondaryIcon: at(looks.secondary, t.icon),
    secondaryIconAmber: at(looks.secondaryAmber, t.icon),

    // ProgressBar and EmptyState: a read-only frame is a control frame at
    // rest, and a progress fill value marks in the roles of its parts or its
    // state; each piece's own row keeps the cut on the outer ends of the run
    // only.
    frame: box({
      color: t.surface,
      border_width: t.lineWidth,
      border_color: t.neutral,
      corner_cut: paired(t.cut),
    }),
    valueAccent: value(t.accent),
    valueNeutral: value(t.neutral),
    valueError: value(t.error),
    valueAmber: value(t.amber),
    check: {
      rows: [{ ...checkMark, part: background }],
      em: looks.checkbox.em,
    },

    // InlineAlert.
    alertInformation: alert(t.accent),
    alertWarning: alert(t.amber),
    alertError: alert(t.error),

    // StatusBadge, and its square markers: lit and filled while active,
    // unlit and outlined while inactive.
    badgeAccent: badge(t.accent),
    badgeNeutral: badge(t.neutral),
    badgeAmber: badge(t.amber),
    badgeError: badge(t.error),
    markerLit: box({ color: t.accent, corner_cut: paired(t.partCut) }),
    markerUnlit: box({
      color: [0, 0, 0, 0],
      border_width: t.lineWidth,
      border_color: t.neutral,
      corner_cut: paired(t.partCut),
    }),

    // Expander: its header is a button whose content is child entities that
    // do not follow its paint states, so a press keeps the surface under the
    // hover edge, as the switch rail does, and the content stays legible.
    expanderHeader: {
      rows: [{ part: pressed, color: t.surface }],
      em: looks.button.em,
    },

    // DataGrid. A row is a docked button that paints nothing at rest, so its
    // grid lines show; hover and focus light its square edge, a press keeps
    // its content legible, and selected rows take the row tint and bar. The
    // focused cell is the focus edge on the cell's lines; the cell editor is
    // the text input look, square because it sits on those lines; the
    // scrolling body is the scroll look without a frame, since the grid's
    // lines are its frame.
    gridRow: {
      rows: extend(looks.docked, [
        { part: background, color: CLEAR, border_width: 0 },
        { part: pressed, color: CLEAR },
        ...(["idle", "hovered", "pressed"] as const).map((name) => ({
          part: checked(name),
          ...selection(t.accent),
        })),
        { part: checked("disabled"), ...selection(t.neutral) },
      ]),
      em: looks.docked.em,
    },
    cellFocus: box({
      color: CLEAR,
      border_width: t.litLineWidth,
      border_color: t.accent,
      glow_color: t.accent,
      glow_intensity: t.focusGlowIntensity,
      glow_radius: t.frameGlowReach,
      glow_inner_radius: t.frameGlowReach,
      glow_falloff: t.glowFalloff,
    }),
    cellEditor: {
      rows: square(looks.textInput.parts),
      em: looks.textInput.em,
    },
    gridBody: {
      rows: extend(looks.scroll, [
        { part: background, color: CLEAR, border_width: 0 },
      ]),
      em: looks.scroll.em,
    },

    // Toast bodies by role.
    toastAccent: toast(t.accent),
    toastAmber: toast(t.amber),
    toastError: toast(t.error),

    ...overlayThemes(contract),

    // ConfirmationDialog: Cancel is a secondary button and the action a
    // primary one, in the amber variant when it is destructive, both at the
    // body size of the dialog's text.
    secondary: at(looks.secondary, t.textBody),
    amber: at(looks.amber, t.textBody),

    // Spinner and CircularProgress: functional circles, the arc shape (2)
    // filled with a colour. The track is a whole ring in the quiet line, as
    // rails are outlined; the lit arc over it is a value mark in its role
    // colour. Each ring's own row gives its thickness, start and sweep. A
    // check standing on the page rather than on a fill is lit.
    arcTrack: box({ shape: 2, color: t.line }),
    arcAccent: box({ shape: 2, color: t.accent }),
    arcError: box({ shape: 2, color: t.error }),
    checkLit: {
      rows: [{ ...checkMark, part: background, color: t.accent }],
      em: looks.checkbox.em,
    },

    ...choiceThemes(contract),

    ...selectThemes(contract),
  } satisfies Readonly<Record<string, KitTheme>>;
}

/**
 * Floating surfaces and the rows of menus and option lists.
 */
function overlayThemes(contract: GuiKitContract) {
  const t = contract.GUI_SKIN_TOKENS;
  const looks = contract.GUI_SKIN_LOOKS;
  const key = (
    state?: "idle" | "hovered" | "pressed" | "disabled",
    variant?: "checked",
  ) =>
    contract.guiPaintPartIndex({
      part: "background",
      ...(state ? { state } : {}),
      ...(variant ? { variant } : {}),
    });
  const background = key();

  // A floating surface is a content frame at rest: the idle line round the
  // surface, the frame cut on menus, option lists, popovers and dialogs and
  // the part cut on a tooltip, a small surface of small text. A resting
  // accent line would read as lit.
  const surface = (cut: number): KitTheme => ({
    rows: [
      {
        part: background,
        color: t.surface,
        border_width: t.lineWidth,
        border_color: t.neutral,
        corner_cut: paired(cut),
      },
    ],
    em: t.em,
  });

  // A menu row or option is a docked button that paints nothing at rest.
  // The active row, which the runtime paints as hovered whether the pointer
  // or the arrows moved it there, and a pressed row take the row tint under
  // the hover edge, so the content stays legible. A selected row takes the
  // row tint and the accent bar of a data grid's or tree's selected row. The
  // amber variant, for destructive commands, lights its row in amber.
  const bar = t.selectionGutter + t.lineWidth / 2;
  const row = (look: GuiKitLook, lit: GuiKitColor): KitTheme => {
    const active = tint(lit, t.rowTint[3]);
    const selected = (color: GuiKitColor) => ({
      fill_mode: 1,
      gradient_start: [bar, 0],
      gradient_end: [bar + 0.01, 0],
      gradient_color0: color,
      gradient_color1: active,
    });
    return {
      rows: extend(look, [
        { part: background, color: CLEAR, border_width: 0 },
        { part: key("hovered"), color: active },
        { part: key("pressed"), color: active },
        ...(["idle", "hovered", "pressed"] as const).map((state) => ({
          part: key(state, "checked"),
          ...selected(lit),
        })),
        { part: key("disabled", "checked"), ...selected(t.neutral) },
      ]),
      em: look.em,
    };
  };

  return {
    floating: surface(t.cut),
    floatingSmall: surface(t.partCut),
    menuRow: row(looks.docked, t.accent),
    menuRowAmber: row(
      {
        em: looks.secondaryAmber.em,
        parts: square(looks.secondaryAmber.parts),
      },
      t.amber,
    ),
  } satisfies Readonly<Record<string, KitTheme>>;
}

/**
 * Choice composites: radio options, segments, tabs and tree chevrons, all
 * Buttons whose selection paints through the checked variants. Each table
 * extends a built-in look, so the states it does not restyle stay the
 * language's own.
 */
function choiceThemes(contract: GuiKitContract) {
  const t = contract.GUI_SKIN_TOKENS;
  const looks = contract.GUI_SKIN_LOOKS;
  type Part = "background" | "icon" | "label" | "focusRing";
  type State = "idle" | "hovered" | "pressed" | "disabled";
  const key = (part: Part, state?: State, variant?: "checked") =>
    contract.guiPaintPartIndex({
      part,
      ...(state ? { state } : {}),
      ...(variant ? { variant } : {}),
    });
  const background = key("background");
  const enabled = ["idle", "hovered", "pressed"] as const;
  const checked = (part: Part, states: readonly State[], row: object) =>
    states.map((state) => ({ ...row, part: key(part, state, "checked") }));

  // The radio mark: a functional circle, the box rounded to half its side
  // around a dark interior, the focus ring on the same circle. Selected, the
  // ring is lit and the Icon, the Button's centred square of 0.55 its side,
  // is a filled dot: the arc shape (2) thicker than its radius. Pressed, the
  // circle fills as every pressed part does and the dot takes the surface.
  const round = [t.icon / 2, t.icon / 2];
  const radio = extend(looks.button, [
    { part: background, corner_cut: SQUARE, corner_radius: round },
    ...checked("background", ["idle", "hovered", "disabled"], {
      color: t.surface,
    }),
    { part: key("icon"), shape: 2, border_width: t.icon, color: CLEAR },
    ...checked("icon", ["idle", "hovered"], { color: t.accent }),
    ...checked("icon", ["pressed"], { color: t.surface }),
    ...checked("icon", ["disabled"], { color: t.neutral }),
    { part: key("focusRing"), corner_cut: SQUARE, corner_radius: round },
  ]);

  // A bare button: only its label, which `hover` lights while the pointer
  // is over it; disabled, neutral.
  const bare = (hover: boolean): KitTheme => ({
    rows: [
      { part: background, color: CLEAR, border_width: 0, glow_intensity: 0 },
      { part: key("label"), color: t.text },
      ...(hover
        ? (["hovered", "pressed"] as const).map((state) => ({
            part: key("label", state),
            color: t.accent,
          }))
        : []),
      { part: key("label", "disabled"), color: t.neutral },
    ],
    em: t.em,
  });

  // Segments sit in the segmented control's frame: clear at rest, lit by
  // hover and focus on their own contour, filled while selected. The whole
  // reads as one control because only the frame's paired cut corners are
  // cut, on the first and last segments; a segment after another starts
  // with a quiet line, a hard-stop gradient one idle line wide.
  const solid = { fill_mode: 0 };
  const divided = {
    fill_mode: 1,
    gradient_start: [t.lineWidth, 0],
    gradient_end: [t.lineWidth + 0.01, 0],
    gradient_color0: t.line,
    gradient_color1: CLEAR,
  };
  const segment = (
    cut: readonly [number, number, number, number],
    after: boolean,
  ): KitTheme => ({
    rows: extend(looks.secondary, [
      {
        part: background,
        color: CLEAR,
        border_width: 0,
        corner_cut: cut,
        ...(after ? divided : solid),
      },
      { part: key("background", "pressed"), ...solid },
      ...checked("background", [...enabled, "disabled"], solid),
      { part: key("focusRing"), corner_cut: cut },
    ]),
    em: looks.secondary.em,
  });

  // Tabs: clear at rest, lit by hover and focus on their part-cut contour.
  // Selected, the label is lit and an accent bar the selection gutter thick
  // runs along the tab's bottom edge, on the strip's line, so selection
  // reads apart from focus; a filled tab would read as a segment.
  const bar = (color: GuiKitColor) => ({
    fill_mode: 1,
    gradient_start: [0, t.controlHeight - t.selectionGutter],
    gradient_end: [0, t.controlHeight - t.selectionGutter + 0.01],
    gradient_color0: CLEAR,
    gradient_color1: color,
  });

  return {
    radio: { rows: radio, em: looks.button.em },
    radioLabel: bare(false),
    segmentFirst: segment([t.cut, 0, 0, 0], false),
    segment: segment(SQUARE, true),
    segmentLast: segment([0, 0, t.cut, 0], true),
    segmentOnly: segment(paired(t.cut), false),
    tab: {
      rows: extend(looks.secondary, [
        { part: background, color: CLEAR, border_width: 0 },
        ...checked("background", ["idle", "hovered"], bar(t.accent)),
        ...checked("background", ["disabled"], bar(t.neutral)),
        ...checked("label", ["idle", "hovered"], { color: t.accent }),
        ...checked("label", ["disabled"], { color: t.neutral }),
      ]),
      em: looks.secondary.em,
    },
    treeChevron: bare(true),
  } satisfies Readonly<Record<string, KitTheme>>;
}

/**
 * Points of a chevron pointing down in a unit square: from the start of its
 * left arm to its tip to the end of its right arm, twelve units wide and six
 * high in a square of sixteen.
 */
const CHEVRON: readonly (readonly [number, number])[] = [
  [2 / 16, 5 / 16],
  [8 / 16, 11 / 16],
  [14 / 16, 5 / 16],
];

/**
 * Selection controls: the trigger of a dropdown, its chevron and the rows of
 * an option list whose options toggle.
 */
function selectThemes(contract: GuiKitContract) {
  const t = contract.GUI_SKIN_TOKENS;
  const looks = contract.GUI_SKIN_LOOKS;
  type State = "idle" | "hovered" | "pressed" | "disabled";
  const key = (
    part: "background" | "icon",
    state?: State,
    variant?: "checked",
  ) =>
    contract.guiPaintPartIndex({
      part,
      ...(state ? { state } : {}),
      ...(variant ? { variant } : {}),
    });
  const background = key("background");
  const enabled = ["idle", "hovered", "pressed"] as const;

  // The trigger is a field-like button whose value and chevron are child
  // entities, which do not follow its paint states: a press keeps the
  // surface under the hover edge, as the expander header does, and its open
  // look, the checked variant, is the button's own hover edge on the
  // surface in every enabled state, so the value stays legible and focus,
  // the full glow, still reads above it.
  const hovered = key("background", "hovered");
  const edge = looks.button.parts.find((row) => row.part === hovered);
  const trigger = extend(looks.button, [
    { part: key("background", "pressed"), color: t.surface },
    ...enabled.map((state) => ({
      ...edge,
      part: key("background", state, "checked"),
      color: t.surface,
    })),
    {
      part: key("background", "disabled", "checked"),
      color: t.surface,
      border_color: t.neutral,
    },
  ]);

  const box = (row: Omit<GuiKitRow, "part">): KitTheme => ({
    rows: [{ ...row, part: background }],
    em: t.em,
  });

  // The chevron: two strokes of the lit line's weight in the text tone, or
  // neutral while the trigger is disabled, each running half its thickness
  // past the tip so the corner closes, as the check mark's strokes do. It
  // points down while the list is closed and up while it is open.
  const chevron = (up: boolean, color: GuiKitColor): KitTheme => {
    const weight = t.litLineWidth;
    // Half the weight along each arm's 45-degree direction, in the square's
    // units of sixteen.
    const past = weight / 2 / Math.SQRT2 / 16;
    const [start, tip, end] = CHEVRON.map(
      ([x, y]) => [x, up ? 1 - y : y] as const,
    ) as [
      readonly [number, number],
      readonly [number, number],
      readonly [number, number],
    ];
    const toward = up ? -past : past;
    return box({
      shape: 1,
      color,
      border_width: weight,
      stroke_a: [start[0], start[1], tip[0] + past, tip[1] + toward],
      stroke_b: [tip[0] - past, tip[1] + toward, end[0], end[1]],
    });
  };

  // An option that toggles, such as a multi-select's: a menu row whose
  // selection is the language's check mark, the checkbox's stroke, at the
  // start of the row, lit while selected and neutral while also disabled,
  // instead of the bar and tint of a single selection, so several selected
  // options read as a set of marks rather than a selected range, and the
  // tint stays the active option's alone.
  const icon = key("icon");
  const checkMark = looks.checkbox.parts.find((row) => row.part === icon);
  if (!checkMark) throw new Error("The checkbox look has no check mark");
  const menuRow = overlayThemes(contract).menuRow;
  const checkedBackground = new Set(
    (["idle", "hovered", "pressed", "disabled"] as const).map((state) =>
      key("background", state, "checked"),
    ),
  );
  const optionCheck: KitTheme = {
    rows: [
      ...menuRow.rows.filter((row) => !checkedBackground.has(row.part)),
      { ...checkMark, color: CLEAR },
      ...enabled.map((state) => ({
        part: key("icon", state, "checked"),
        color: t.accent,
      })),
      { part: key("icon", "disabled", "checked"), color: t.neutral },
    ],
    em: menuRow.em,
  };

  return {
    selectTrigger: { rows: trigger, em: looks.button.em },
    selectChevron: chevron(false, t.text),
    selectChevronOpen: chevron(true, t.text),
    selectChevronDisabled: chevron(false, t.neutral),
    optionCheckRow: optionCheck,
  } satisfies Readonly<Record<string, KitTheme>>;
}

/** Names of the kit's themes. */
export type KitThemeName = keyof ReturnType<typeof catalogue>;

/** Every kit theme as its encoded `GuiTheme` fields, rows in paint-key order. */
export function encodeKitThemes(
  contract: GuiKitContract,
): Readonly<
  Record<KitThemeName, { parts: Uint8Array<ArrayBuffer>; em: number }>
> {
  const themes = catalogue(contract);
  return Object.fromEntries(
    Object.entries(themes).map(([name, { rows, em }]) => {
      const ordered = [...rows].sort((left, right) => left.part - right.part);
      return [
        name,
        {
          parts: contract.GuiTheme.encodeParts({
            nextSlot: ordered.length,
            rows: new Map(ordered.map((row, slot) => [slot, row])),
          }),
          em,
        },
      ];
    }),
  ) as Record<KitThemeName, { parts: Uint8Array<ArrayBuffer>; em: number }>;
}
