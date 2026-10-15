/**
 * The kit's per-World context: the receiving runtime's generated contract,
 * the font and the body size every kit component draws at, the shared theme
 * entities and the reduced-motion setting. One `GuiKit` declares the kit's
 * themes once in each World container; kit components reference them by
 * symbolic id and scale their own lengths by `fontSize / tokens.em`,
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
import { createElement, useContext, useMemo } from "react";
import type { AssetReference } from "../assets/declarations.js";
import { AttachmentContext } from "../composition/attached-world.js";
import { SkinProvider } from "../gui/skin-provider.js";
import { GuiSkinContext } from "../gui/skin-context.js";
import {
  guiKitSpacing,
  type GuiKitSpacing,
  type GuiKitSpacingOverrides,
} from "./spacing.js";
import type { KitThemeName } from "./themes.js";

/** Linear RGBA. */
export type GuiKitColor = readonly [number, number, number, number];

/**
 * Client skin tokens: role colours, and lengths in logical units at `em`.
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
  /** Choice hit-row side; omitted uses the icon size in custom skins. */
  readonly choiceTarget?: number;
  /** Visible choice mark side, independent of the hit row. */
  readonly choiceIndicator?: number;
  readonly switchWidth?: number;
  readonly switchHeight?: number;
  /** Side of an unsized dial, which is square. */
  readonly dial: number;
  /** Place the readout inside the dial; omitted keeps the ordinary readout row. */
  readonly dialReadoutInside?: boolean;
  /** Visual housing diameter as a fraction of the dial side; omitted uses the full side. */
  readonly dialHousingScale?: number;
  readonly dockedHeight: number;
  /** Optional progress-frame inset in em units; omitted uses the ordinary docked clearance. */
  readonly progressInset?: number;
  /** Independent progress track height; defaults to smallHeight. */
  readonly progressHeight?: number;
  readonly dockedWidth: number;
  readonly bar: number;
  readonly inset: number;
  /** Form spacing roles at em; omitted roles derive from inset. */
  readonly spacing?: GuiKitSpacingOverrides;
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

/** A client-owned look recipe. */
export interface GuiKitLook {
  readonly em: number;
  readonly parts: readonly GuiKitRow[];
  readonly motion?: readonly GuiKitRow[] | undefined;
  readonly metrics?: import("../gui/theme.js").MetricsProps | undefined;
}

/**
 * What the kit uses of the receiving runtime's generated contract module:
 * pass the module itself. Skins supply all colours, lengths and recipes.
 */
export interface GuiKitContract {
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
  readonly GuiThemeMotion: {
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

/** Optional authoring appearance; native controls continue to own their state. */
export interface GuiKitAppearance {
  readonly tokens: GuiKitTokens;
  readonly themes?: Partial<Record<KitThemeName, string>>;
  readonly controls?: Partial<Record<GuiKitControlName, string>>;
  readonly metrics?: Partial<
    Record<GuiKitControlName, import("../gui/theme.js").MetricsProps>
  >;
}

export type GuiKitControlName =
  | "button"
  | "checkbox"
  | "switch"
  | "textInput"
  | "slider"
  | "dial"
  | "scroll"
  | "color";

/** Kit configuration also provides the skin for native controls in this World. */
export type GuiKitProps = import("../gui/skin-provider.js").SkinProviderProps;

export function GuiKit(props: GuiKitProps) {
  return createElement(SkinProvider, props);
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
  /** Resolved skin/client spacing roles in World logical units. */
  readonly spacing: GuiKitSpacing;
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
  /** Optional look for a previously unskinned native control. */
  controlTheme(name: GuiKitControlName): string | undefined;
  /** Preferred native control dimensions from the selected skin, in logical units. */
  controlSize(name: GuiKitControlName): readonly [number, number] | undefined;
}

/** The kit of the enclosing `GuiKit`, which must declare its themes in this World. */
export function useGuiKit(): GuiKitScope {
  const kit = useContext(GuiSkinContext);
  const container = useContext(AttachmentContext);
  if (!kit)
    throw new Error("Kit components need an enclosing GuiKit in their World");
  if (kit.container !== container)
    throw new Error(
      "Kit components need a GuiKit inside their own World's declarations",
    );
  return useMemo(() => {
    const tokens = kit.appearance.tokens;
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
      spacing: guiKitSpacing(tokens, scale),
      reducedMotion: kit.reducedMotion ?? false,
      unit: (length) => length * scale,
      typeSize: (size) => sizes[size] * scale,
      color: (tone) => tokens[tone],
      theme: (name) => {
        const theme = kit.appearance.themes?.[name];
        if (!theme)
          throw new Error(
            `The selected skin does not define the ${name} kit theme`,
          );
        return theme;
      },
      controlTheme: (name) => kit.appearance?.controls?.[name],
      controlSize: (name) => {
        const metrics = kit.appearance.metrics?.[name];
        const width = metrics?.min_width;
        const height = metrics?.min_height;
        if (
          width === undefined ||
          height === undefined ||
          width <= 0 ||
          height <= 0
        )
          return;
        const em = metrics?.em ?? 0;
        const factor = em > 0 ? kit.fontSize / em : 1;
        return [width * factor, height * factor] as const;
      },
    };
  }, [kit]);
}
