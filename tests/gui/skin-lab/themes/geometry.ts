/**
 * The skin's lengths the specimens place their content with: the idle line,
 * control sizes, the content inset, row heights and the type scale, in units
 * of the controls sheet (sheet a), read from the design language's tokens
 * that the connected runtime exports (`GUI_SKIN_TOKENS`, written once in the
 * core's built-in looks). Specimens import these instead of measuring their
 * own; colours come from `palette.ts`. The rules that use them are in the
 * README's "Design language" section; the looks built from them are the
 * runtime's built-in looks and the GUI kit's themes.
 */
import { GUI_SKIN_TOKENS as T } from "@ipp/host-contract";

/** Every idle line: frames, separators, grid lines and rail outlines. */
export const LINE = T.lineWidth;

// Sizes.

/** Full-size controls: primary and icon buttons, text fields. */
export const CONTROL_HEIGHT = T.controlHeight;

/** Small controls: checkboxes, switch rails, secondary text buttons, badges. */
export const SMALL_HEIGHT = T.smallHeight;

/** Side of an unsized dial, which is square: the knob's housing width. */
export const DIAL = T.dial;

/** Width of a button docked in a header strip or title bar. */
export const DOCKED_WIDTH = T.dockedWidth;

/** Scroll bar width, and its inset from the frame's outer edge. */
export const BAR = T.bar;
export const BAR_INSET = T.bar;

/**
 * Content inset: text and content from the edge of the frame that holds it
 * (field text, list and grid rows, panel content).
 */
export const INSET = T.inset;

/**
 * Row heights: rows of body text (data grid, tree, menu and dropdown
 * options), and dense rows of small text (event logs).
 */
export const ROW = T.row;
export const DENSE_ROW = T.denseRow;

/**
 * Every grid row keeps this gutter before its first column, where the
 * selected row's accent bar lies.
 */
export const SELECTION_GUTTER = T.selectionGutter;

// Type scale: text sizes and the icon size.

/** Dense list rows, readouts, grid headers and counts, secondary text button labels. */
export const TEXT_SMALL = T.textSmall;

/** Field text, grid cells, panel content and titles, primary button labels. */
export const TEXT_BODY = T.textBody;

/** Icon glyphs: window controls, status marks and the spinner's ring. */
export const ICON = T.icon;
