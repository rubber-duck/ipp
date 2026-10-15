import { createContext } from "react";
import type { PlotSkinAppearance } from "./skin-types.js";

export const PlotSkinContext = createContext<{
  readonly container: unknown;
  readonly appearance: PlotSkinAppearance | undefined;
} | null>(null);
