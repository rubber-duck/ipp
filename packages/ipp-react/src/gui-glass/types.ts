import type { GuiKitColor, GuiKitLook, GuiKitRow } from "../gui-kit/kit.js";

export type GlassColor = GuiKitColor;
export type GlassRow = GuiKitRow;
export type GlassThemeName =
  | "button"
  | "textInput"
  | "checkbox"
  | "switch"
  | "slider"
  | "panel"
  | "progress";

/** Public palette inputs are sRGB hex colours; resolved colours are linear RGBA. */
export interface GlassPaletteInput {
  readonly primary: string;
  readonly secondary: string;
  readonly surface: string;
  readonly background: string;
  readonly text: string;
  readonly mutedText: string;
  readonly neutral: string;
  readonly success: string;
  readonly warning: string;
  readonly error: string;
}

export type GlassPalette = {
  readonly [K in keyof GlassPaletteInput]: GlassColor;
};

export interface GlassTheme {
  readonly rows: readonly GlassRow[];
  readonly em: number;
  readonly motion?: readonly GlassRow[] | undefined;
  readonly metrics?: import("../gui/theme.js").MetricsProps | undefined;
}

/** The receiving Host's generated contract module, never a copied schema. */
export interface GlassContract {
  guiPaintPartIndex(key: {
    readonly part: string;
    readonly state?: string;
    readonly variant?: string;
  }): number;
  readonly GuiThemeMotion: {
    encodeParts(table: {
      readonly nextSlot: number;
      readonly rows: ReadonlyMap<number, GlassRow>;
    }): Uint8Array<ArrayBuffer>;
  };
  readonly GuiTheme: {
    encodeParts(table: {
      readonly nextSlot: number;
      readonly rows: ReadonlyMap<number, GlassRow>;
    }): Uint8Array<ArrayBuffer>;
  };
}

export interface GlassLight {
  readonly x: number;
  readonly y: number;
  readonly height: number;
  readonly intensity: number;
}

/** Physical optical controls; edge scales the polished cross-section. */
export interface GlassMaterial {
  readonly ior: number;
  readonly roughness: number;
  readonly thickness: number;
  readonly edge: number;
}

/** Complete inputs for pure Glass recipes, kept separate from the runtime contract. */
export interface GlassRecipeInputs extends GlassContract {
  readonly tokens: import("../gui-kit/kit.js").GuiKitTokens;
  readonly looks: Readonly<
    Record<
      string,
      GuiKitLook & { readonly motion?: readonly GlassRow[] | undefined }
    >
  >;
}
