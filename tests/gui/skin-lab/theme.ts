/**
 * Theme part rows written in the target contract's own field names. A row
 * names its paint key (part, optional state, optional checked/unchecked
 * variant); every other property is copied unchanged into the generated
 * `GuiTheme.parts` row, so new row fields of the connected runtime need no
 * change here; the generated encoder rejects a name the Host does not know.
 * `GuiThemePartsRow` in the Host's generated client lists the fields, for
 * example `corner_cut`, `corner_accent`, `glow_inner_radius`, the stroke mark
 * (`shape: 1`, `stroke_a`, `stroke_b`), the ring arc (`shape: 2`,
 * `arc_start`, `arc_sweep`, `arc_dashes`), the colour fields (`fill_mode: 3`
 * hue along the gradient axis, `fill_mode: 4` saturation-value of `fill_hue`)
 * and the checker (`checker_size`, `checker_color0`, `checker_color1`);
 * `tests/react/gui-authoring/pages/gui-paint.tsx` shows each in use. Colours are linear RGBA: use
 * `srgb()` for sheet colours.
 *
 * Rows sit on the runtime's default look of the control they skin, property
 * by property, and their lengths are absolute logical units. A theme may
 * instead name a built-in look of the Host's generated contract
 * (`GUI_SKIN_LOOKS`), such as `{ look: "switch" }`: that look's rows with its
 * `em`, so its lengths follow each control's font, or, with a `scale`, its
 * rows drawn at that scale in absolute units, as a specimen of a sheet drawn
 * at another scale needs them. `rows` replace the look's properties at their
 * paint keys. Such a theme also carries the look's motion rows, so its
 * controls move between states as the look does; plain rows leave timing to
 * the default look of each control's kind.
 */
import { LENGTHS } from "./scale.js";

export type SkinState = "idle" | "hovered" | "pressed" | "disabled";
export type SkinVariant = "checked" | "unchecked";

export type SkinRow = {
  /** Paint part, such as `background`, `fill`, `label`, `icon` or `focusRing`. */
  readonly part: string;
  readonly state?: SkinState;
  /** A variant without a state applies to every state of that variant. */
  readonly variant?: SkinVariant;
} & { readonly [field: string]: unknown };

/** Rows in absolute logical units. */
export type SkinRows = readonly SkinRow[];

/** A built-in look of the connected runtime, by its contract name. */
export interface BuiltInLook {
  readonly look: string;
  /** Draw the look's lengths this many times larger, in absolute units. */
  readonly scale?: number;
  /** Rows whose properties replace the look's at the same paint keys. */
  readonly rows?: SkinRows;
}

export type SkinTheme = SkinRows | BuiltInLook;

/** Named themes of one theme module; specimens reference them by name. */
export type SkinThemes = Readonly<Record<string, SkinTheme>>;

/** One theme as its encoded `GuiTheme` fields and `GuiThemeMotion` rows. */
export interface EncodedTheme {
  readonly parts: Uint8Array<ArrayBuffer>;
  readonly em: number;
  /** The look's motion rows, for a theme that names a look with any. */
  readonly motion?: Uint8Array<ArrayBuffer>;
}

type ContractRow = { readonly part: number } & {
  readonly [field: string]: unknown;
};

interface RowsEncoder {
  encodeParts(table: {
    nextSlot: number;
    rows: ReadonlyMap<number, { part: number }>;
  }): Uint8Array<ArrayBuffer>;
}

/** The generated contract exports that encode theme rows. */
export interface ThemeContract {
  readonly GUI_PAINT_PART_KEYS: readonly {
    readonly index: number;
    readonly part: string;
    readonly state: string | null;
    readonly variant: string | null;
  }[];
  readonly GuiTheme: RowsEncoder;
  readonly GuiThemeMotion: RowsEncoder;
  readonly GUI_SKIN_LOOKS: Readonly<
    Record<
      string,
      {
        readonly em: number;
        readonly parts: readonly ContractRow[];
        readonly motion: readonly ContractRow[];
      }
    >
  >;
}

const STATES: readonly SkinState[] = ["idle", "hovered", "pressed", "disabled"];

function paintIndex(
  contract: ThemeContract,
  part: string,
  state: string | null,
  variant: string | null,
): number {
  const entry = contract.GUI_PAINT_PART_KEYS.find(
    (key) =>
      key.part === part && key.state === state && key.variant === variant,
  );
  if (!entry) {
    const parts = [
      ...new Set(contract.GUI_PAINT_PART_KEYS.map((key) => key.part)),
    ];
    throw new Error(
      `Unknown paint key ${[part, state, variant].filter(Boolean).join("/")}; parts: ${parts.join(", ")}`,
    );
  }
  return entry.index;
}

/** Rows keyed by paint index; a variant without a state names every state. */
function keyed(contract: ThemeContract, theme: SkinRows) {
  const rows = new Map<number, ContractRow>();
  for (const { part, state, variant, ...fields } of theme) {
    const states = variant && !state ? STATES : [state ?? null];
    for (const each of states) {
      const index = paintIndex(contract, part, each, variant ?? null);
      if (rows.has(index))
        throw new Error(
          `Theme repeats paint key ${[part, each, variant].filter(Boolean).join("/")}`,
        );
      rows.set(index, { ...fields, part: index });
    }
  }
  return rows;
}

/** One row with every length multiplied by `factor`. */
function scaled(row: ContractRow, factor: number): ContractRow {
  const result: Record<string, unknown> = { ...row };
  for (const field of LENGTHS) {
    const value = row[field];
    if (typeof value === "number") result[field] = value * factor;
    else if (Array.isArray(value))
      result[field] = value.map((length: number) => length * factor);
  }
  return result as ContractRow;
}

/** A built-in look's rows, scaled and replaced as `theme` asks. */
function look(contract: ThemeContract, theme: BuiltInLook) {
  const look = contract.GUI_SKIN_LOOKS[theme.look];
  if (!look)
    throw new Error(
      `Unknown built-in look ${theme.look}; looks: ${Object.keys(contract.GUI_SKIN_LOOKS).join(", ")}`,
    );
  const rows = new Map<number, ContractRow>();
  for (const row of look.parts)
    rows.set(row.part, theme.scale ? scaled(row, theme.scale) : row);
  for (const [index, row] of keyed(contract, theme.rows ?? []))
    rows.set(index, { ...rows.get(index), ...row });
  return { rows, em: theme.scale ? 0 : look.em, motion: look.motion };
}

/** Rows in slot order, encoded with `encoder`. */
function encodeRows(
  encoder: RowsEncoder,
  rows: readonly ContractRow[],
): Uint8Array<ArrayBuffer> {
  return encoder.encodeParts({
    nextSlot: rows.length,
    rows: new Map(rows.map((row, slot) => [slot, row])),
  });
}

/**
 * Encode one theme's rows in paint-key order with its `em` and a look's
 * motion rows, or nothing for an empty theme.
 */
export function encodeTheme(
  contract: ThemeContract,
  theme: SkinTheme,
): EncodedTheme | undefined {
  const { rows, em, motion } =
    "look" in theme
      ? look(contract, theme)
      : { rows: keyed(contract, theme), em: 0, motion: [] };
  if (!rows.size) return undefined;
  const ordered = [...rows.entries()].sort(([left], [right]) => left - right);
  return {
    parts: encodeRows(
      contract.GuiTheme,
      ordered.map(([, row]) => row),
    ),
    em,
    ...(motion.length
      ? { motion: encodeRows(contract.GuiThemeMotion, motion) }
      : {}),
  };
}
