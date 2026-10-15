/**
 * Cybertron colours as linear RGBA. Specimens share the client skin's tokens;
 * the skin-lab README describes their visual roles.
 */
import { cybertronTokens as T } from "@ipp/react/gui-cybertron";

/** Linear RGBA. */
export type Color = readonly [number, number, number, number];

/** The page and every container's fill: panels, sections, title bars. */
export const page: Color = T.page;

/** The lit colour; also the information and success status. */
export const accent: Color = T.accent;

/** Content text. */
export const text: Color = T.text;

/** The unlit colour; also the inactive status. */
export const neutral: Color = T.neutral;

/** Quiet lines: list row separators, grid lines and rail outlines. */
export const line: Color = T.line;
