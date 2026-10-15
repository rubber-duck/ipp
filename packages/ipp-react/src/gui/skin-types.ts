import type { ReactNode } from "react";
import type { PaintProps } from "./components.js";
import type { MetricsProps } from "./theme.js";
import type {
  GuiKitControlName,
  GuiKitRow,
  GuiKitTokens,
} from "../gui-kit/kit.js";
import type { KitThemeName } from "../gui-kit/themes.js";

/** Shared native data, declared once for any number of controls. */
export interface GuiSkinTheme {
  readonly em: number;
  readonly rows: readonly GuiKitRow[];
  readonly motion?: readonly GuiKitRow[] | undefined;
  readonly metrics?: MetricsProps | undefined;
  readonly animation?:
    | string
    | import("../assets/declarations.js").AssetReference
    | undefined;
  readonly paint?: (scope: string) => PaintProps;
}

/** A client skin contains every appearance it uses; core supplies no look. */
export interface GuiSkinDefinition {
  readonly tokens: GuiKitTokens;
  readonly themes: Readonly<Record<string, GuiSkinTheme>>;
  readonly controls: Readonly<Record<GuiKitControlName, string>>;
  readonly kit: Readonly<Record<KitThemeName, string>>;
  /** IDs relative to the resource scope, required before the skin becomes visible. */
  readonly assetIds?: readonly string[];
  /** Immutable resources. Keep this function stable until the bundle changes. */
  readonly assets?: (scope: string) => ReactNode;
  /** Optional chart defaults using the same prepared resource bundle. */
  readonly plots?: (
    scope: string,
  ) => import("../plots/skin-types.js").PlotSkinAppearance;
}
