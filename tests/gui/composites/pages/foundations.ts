/**
 * Input foundations: client focus steering the keyboard target across panel
 * Worlds, client blur, and context requests by pointer and key. The
 * `buttons` and `sliders` panels sit side by side above `groups`, whose
 * segmented group is the next Tab stop after the sliders.
 */
import {
  canvasOf,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
import { PANEL, buttonsPanel, groupsPanel, slidersPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [
  buttonsPanel([0, 0]),
  slidersPanel([PANEL, 0]),
  groupsPanel([0, PANEL]),
];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  return (await openCompositePage(setup, PANELS)).steps;
}
