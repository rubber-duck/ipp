/**
 * Items and groups: the `groups` panel's segmented group and tab group, and
 * the option list of its query field, whose rows take no focus and follow
 * the runtime's active item.
 */
import {
  canvasOf,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
import { groupsPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [groupsPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  return (await openCompositePage(setup, PANELS)).steps;
}
