import { createContext } from "react";
import type { CanvasWorldSession } from "./world-session.js";

/**
 * The enclosing `IppCanvas` session: undefined outside an `IppCanvas`, null
 * while it starts. `World` and root-presented `CanvasWorld` elements read it.
 */
export const CanvasContext = createContext<
  CanvasWorldSession | null | undefined
>(undefined);
