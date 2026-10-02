/**
 * The kit's per-World context: the receiving runtime's generated contract,
 * the font and the body size every kit component draws at, the shared theme
 * entities and the reduced-motion setting. One `GuiKit` declares the kit's
 * themes once in each World container; kit components reference them by
 * symbolic id and scale their own lengths by `fontSize / GUI_SKIN_TOKENS.em`,
 * as the themes' rows scale by their `em`.
 *
 * Reduced motion is one setting for the runtime and the kit. The `GuiKit`
 * that declares a World's themes sends it to that World as the GUI
 * preference, which snaps the runtime's skin transitions, whenever it
 * changes; like the themes, it goes with that kit, so removing the kit, or
 * the setting, from a live root turns a preference it turned on off again,
 * while unmounting the root leaves it. Every kit animation reads the setting
 * of its nearest `GuiKit`, so a nested kit can hold part of a World still
 * without changing the preference.
 */
import {
  createContext,
  createElement,
  Fragment,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  type ReactNode,
} from "react";
import type { AssetReference } from "../assets.js";
import { AttachmentContext } from "../attached-world.js";
import type { ReactWorldCommits } from "../commits.js";
import { Entity } from "../components.js";
import { Theme } from "../gui/theme.js";
import { encodeKitThemes, type KitThemeName } from "./themes.js";

/** Linear RGBA. */
export type GuiKitColor = readonly [number, number, number, number];

/**
 * The design-language tokens the kit reads from the generated contract's
 * `GUI_SKIN_TOKENS`: role colours, and lengths in logical units at `em`.
 */
export interface GuiKitTokens {
  readonly em: number;
  readonly page: GuiKitColor;
  readonly surface: GuiKitColor;
  readonly accent: GuiKitColor;
  readonly text: GuiKitColor;
  readonly neutral: GuiKitColor;
  readonly line: GuiKitColor;
  readonly amber: GuiKitColor;
  readonly error: GuiKitColor;
  readonly rowTint: GuiKitColor;
  readonly lineWidth: number;
  readonly litLineWidth: number;
  readonly cut: number;
  readonly partCut: number;
  readonly cornerAccent: number;
  readonly cornerAccentWidth: number;
  readonly focusGlowIntensity: number;
  readonly glowFalloff: number;
  readonly frameGlowReach: number;
  readonly controlHeight: number;
  readonly smallHeight: number;
  /** Side of an unsized dial, which is square. */
  readonly dial: number;
  readonly dockedHeight: number;
  readonly dockedWidth: number;
  readonly bar: number;
  readonly inset: number;
  readonly row: number;
  readonly denseRow: number;
  readonly selectionGutter: number;
  readonly textSmall: number;
  readonly textBody: number;
  readonly textDisplay: number;
  readonly icon: number;
}

/** One `GuiTheme.parts` row: a paint key index and its properties by field name. */
export type GuiKitRow = { readonly part: number } & {
  readonly [field: string]: unknown;
};

/** A built-in look of the generated contract. */
export interface GuiKitLook {
  readonly em: number;
  readonly parts: readonly GuiKitRow[];
}

/**
 * What the kit uses of the receiving runtime's generated contract module:
 * pass the module itself. Its tokens and built-in looks are the only source of
 * the kit's colours and lengths.
 */
export interface GuiKitContract {
  readonly GUI_SKIN_TOKENS: GuiKitTokens;
  readonly GUI_SKIN_LOOKS: {
    readonly button: GuiKitLook;
    readonly checkbox: GuiKitLook;
    readonly amber: GuiKitLook;
    readonly secondary: GuiKitLook;
    readonly secondaryAmber: GuiKitLook;
    readonly docked: GuiKitLook;
    readonly textInput: GuiKitLook;
    readonly scroll: GuiKitLook;
  };
  guiPaintPartIndex(key: {
    readonly part: string;
    readonly state?: string;
    readonly variant?: string;
  }): number;
  readonly GuiTheme: {
    encodeParts(table: {
      readonly nextSlot: number;
      readonly rows: ReadonlyMap<number, GuiKitRow>;
    }): Uint8Array<ArrayBuffer>;
  };
  /** An entity's own rows, such as a ring's sweep, and their animation offsets. */
  readonly GuiSkin: {
    readonly id: number;
    partsOffset(slot: number, property: string): number;
    encodeParts(table: {
      readonly nextSlot: number;
      readonly rows: ReadonlyMap<number, GuiKitRow>;
    }): Uint8Array<ArrayBuffer>;
  };
  readonly components: {
    readonly GuiLayout: {
      readonly id: number;
      readonly fields: { readonly align_x: { readonly offset: number } };
    };
    readonly CanvasStyle: {
      readonly id: number;
      readonly fields: { readonly opacity: { readonly offset: number } };
    };
  };
}

export interface GuiKitProps {
  /**
   * The receiving runtime's generated contract module. Omit it inside
   * another `GuiKit` to inherit.
   */
  readonly contract?: GuiKitContract;
  /**
   * The GUI font kit text draws with, the shared Shure Tech Mono Nerd Font:
   * a font source or an asset reference. Omit it inside another `GuiKit` to
   * inherit.
   */
  readonly font?: string | AssetReference;
  /**
   * Body text size in the World's logical units. Kit lengths are drawn
   * `fontSize / GUI_SKIN_TOKENS.em` times their design size. Omit it inside
   * another `GuiKit` to inherit.
   */
  readonly fontSize?: number;
  /**
   * Hold motion still: the World's GUI preference, which snaps the runtime's
   * skin transitions, and the kit's own animations, which stand still or
   * end at once. The kit that declares the World's themes sends it to the
   * World; a nested kit only holds the kit animations beneath it. Omit it to
   * inherit; omitted everywhere, the kit sends nothing and its animations
   * move.
   */
  readonly reducedMotion?: boolean;
  readonly children?: ReactNode;
}

interface GuiKitValue {
  readonly contract: GuiKitContract;
  readonly font: string | AssetReference;
  readonly fontSize: number;
  /** The setting given to this kit or the nearest enclosing one. */
  readonly reducedMotion: boolean | undefined;
  /** The World container whose kit themes this kit references. */
  readonly container: unknown;
}

const GuiKitContext = createContext<GuiKitValue | null>(null);

/** Symbolic id of a kit theme entity in its World. */
function themeId(name: KitThemeName): string {
  return `ipp-kit/theme/${name}`;
}

/**
 * Provide the kit to the components below it. The first `GuiKit` in a World's
 * declarations declares the kit's theme entities there, as top-level entities
 * beside its children, so place it outside any `Children`. A `GuiKit` nested
 * in the same World declares nothing and changes only what it is given, such
 * as the size of a part of the panel or reduced motion beneath it; a
 * `GuiKit` in another World (inside an `AttachedWorld` or `CanvasWorld`)
 * declares that World's themes and sends its reduced motion there.
 */
export function GuiKit({
  contract,
  font,
  fontSize,
  reducedMotion,
  children,
}: GuiKitProps) {
  const outer = useContext(GuiKitContext);
  const container = useContext(AttachmentContext);
  const resolved = {
    contract: contract ?? outer?.contract,
    font: font ?? outer?.font,
    fontSize: fontSize ?? outer?.fontSize,
    reducedMotion: reducedMotion ?? outer?.reducedMotion,
  };
  if (!resolved.contract || !resolved.font || resolved.fontSize === undefined)
    throw new Error(
      "GuiKit needs a contract, font and fontSize, or an enclosing GuiKit to inherit them from",
    );
  if (!(resolved.fontSize > 0) || !Number.isFinite(resolved.fontSize))
    throw new RangeError("GuiKit fontSize must be positive and finite");
  const declares = outer?.container !== container;
  const value = useMemo<GuiKitValue>(
    () => ({
      contract: resolved.contract!,
      font: resolved.font!,
      fontSize: resolved.fontSize!,
      reducedMotion: resolved.reducedMotion,
      container,
    }),
    [
      resolved.contract,
      resolved.font,
      resolved.fontSize,
      resolved.reducedMotion,
      container,
    ],
  );
  const themes = useMemo(
    () => (declares ? encodeKitThemes(resolved.contract!) : undefined),
    [declares, resolved.contract],
  );
  useReducedMotionPreference(
    declares ? container?.commits : undefined,
    resolved.reducedMotion,
  );
  return createElement(
    GuiKitContext,
    { value },
    themes &&
      Object.entries(themes).map(([name, { parts, em }]) =>
        createElement(
          Entity,
          { key: name, id: themeId(name as KitThemeName) },
          createElement(Theme, { parts, em }),
        ),
      ),
    createElement(Fragment, null, children),
  );
}

/**
 * Send the World's reduced-motion preference through `commits` when the
 * setting changes, and turn it off again when the setting or the kit goes
 * after turning it on. Undefined `commits`, in a kit that declares nothing,
 * sends nothing.
 */
function useReducedMotionPreference(
  commits: ReactWorldCommits | undefined,
  reducedMotion: boolean | undefined,
): void {
  /** The value this kit last sent, if any. */
  const sent = useRef<boolean | undefined>(undefined);
  useLayoutEffect(() => {
    if (!commits) return;
    return () => {
      // Unmounting the root fences it first, so this then sends nothing.
      if (sent.current) sendReducedMotion(commits, false);
      sent.current = undefined;
    };
  }, [commits]);
  useLayoutEffect(() => {
    if (!commits) return;
    const next = reducedMotion ?? (sent.current ? false : undefined);
    if (
      next !== undefined &&
      next !== sent.current &&
      sendReducedMotion(commits, next)
    )
      sent.current = next;
  }, [commits, reducedMotion]);
}

function sendReducedMotion(
  commits: ReactWorldCommits,
  reducedMotion: boolean,
): boolean {
  return commits.sendSystemCommand({
    type: "GuiPreferencesUpdateCommand",
    reducedMotion,
  });
}

/** Text sizes of the type scale. */
export type GuiKitTypeSize = "small" | "body" | "display";

/** Palette roles kit text and marks are drawn in. */
export type GuiKitTone = "accent" | "text" | "neutral" | "amber" | "error";

/** The enclosing kit, resolved for the components of one World. */
export interface GuiKitScope {
  readonly tokens: GuiKitTokens;
  readonly contract: GuiKitContract;
  readonly font: string | AssetReference;
  /** Body text size in the World's units. */
  readonly fontSize: number;
  /**
   * Hold motion still: looping animations are not declared, leaving what
   * they move at its rest, and timed changes such as a toast's fade happen
   * at once.
   */
  readonly reducedMotion: boolean;
  /** A design length (at the tokens' `em`) in the World's units. */
  unit(length: number): number;
  /** A text size of the type scale in the World's units. */
  typeSize(size: GuiKitTypeSize): number;
  color(tone: GuiKitTone): GuiKitColor;
  /** The symbolic id of a kit theme entity. */
  theme(name: KitThemeName): string;
}

/** The kit of the enclosing `GuiKit`, which must declare its themes in this World. */
export function useGuiKit(): GuiKitScope {
  const kit = useContext(GuiKitContext);
  const container = useContext(AttachmentContext);
  if (!kit)
    throw new Error("Kit components need an enclosing GuiKit in their World");
  if (kit.container !== container)
    throw new Error(
      "Kit components need a GuiKit inside their own World's declarations",
    );
  return useMemo(() => {
    const tokens = kit.contract.GUI_SKIN_TOKENS;
    const scale = kit.fontSize / tokens.em;
    const sizes = {
      small: tokens.textSmall,
      body: tokens.textBody,
      display: tokens.textDisplay,
    };
    return {
      tokens,
      contract: kit.contract,
      font: kit.font,
      fontSize: kit.fontSize,
      reducedMotion: kit.reducedMotion ?? false,
      unit: (length) => length * scale,
      typeSize: (size) => sizes[size] * scale,
      color: (tone) => tokens[tone],
      theme: themeId,
    };
  }, [kit]);
}
