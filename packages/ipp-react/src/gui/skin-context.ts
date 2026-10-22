import { createContext } from "react";
import type { AssetReference } from "../assets/declarations.js";
import type { GuiKitAppearance, GuiKitContract } from "../gui-kit/kit.js";

export interface GuiSkinContextValue {
  readonly appearance: GuiKitAppearance;
  readonly emptyParts: Uint8Array<ArrayBuffer>;
  readonly contract: GuiKitContract;
  readonly font: string | AssetReference;
  readonly iconFont: string | AssetReference;
  readonly fontSize: number;
  /** The setting given to this kit or the nearest enclosing one. */
  readonly reducedMotion: boolean | undefined;
  /** The World container whose kit themes this kit references. */
  readonly container: unknown;
}

export const GuiSkinContext = createContext<GuiSkinContextValue | null>(null);
