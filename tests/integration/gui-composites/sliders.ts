/**
 * Sliders: the `sliders` panel's vertical slider and the horizontal slider
 * in its scroll view, beside the `dial` panel's dial in a scroll view of its
 * own.
 */
import { canvasOf, openCompositePage, type CompositeSetup } from "./page.js";
import { PANEL, dialPanel, slidersPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./page.js";

const PANELS = [slidersPanel([0, 0]), dialPanel([PANEL, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  return (await openCompositePage(setup, PANELS)).steps;
}
