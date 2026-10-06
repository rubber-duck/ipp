/**
 * Overlays in the runtime: the `overlays` panel's dropdown list, popover,
 * modal dialog and tooltip beside the `buttons` panel, another World whose
 * button an outside press must not reach while a list is open and which a
 * modal dialog leaves usable.
 */
import {
  canvasOf,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
import { PANEL, buttonsPanel, overlaysPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [buttonsPanel([0, 0]), overlaysPanel([PANEL, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  return (await openCompositePage(setup, PANELS)).steps;
}
