/**
 * The skin palette the specimens draw with: the colours of the roles they
 * use, as linear RGBA, read from the design language's tokens that the
 * connected runtime exports (`GUI_SKIN_TOKENS`; the values and their roles
 * are written once, in the core's built-in looks,
 * `crates/ipp-core/src/world/systems/gui/presentation/looks.rs`). Specimens
 * import these instead of defining their own; the rules that use them are in
 * the README's "Design language" section.
 */
import { GUI_SKIN_TOKENS as T } from "@ipp/host-contract";

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
