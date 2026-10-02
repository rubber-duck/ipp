/**
 * Numeric text input: the `groups` panel's `number`, with its step parts,
 * and a lifecycle watch of its `value` field whose records carry the tick of
 * the frame that ended with each change.
 */
import {
  canvasOf,
  check,
  hostInterval,
  openCompositePage,
  type CompositeSetup,
} from "./page.js";
import { groupsPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./page.js";

const PANELS = [groupsPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const records = await page.watchValues("number", "GuiTextInput", ["value"]);
  const { clock } = page.panel("groups");
  return {
    ...page.steps,
    /** The numeric input's number records since `from`, one per change. */
    numberRecords(from: number) {
      return records.slice(from).map(({ values: [value] }) => Number(value));
    },
    /** How many number records arrived so far. */
    numberRecordCount() {
      return records.length;
    },
    /** Sample the Host clock before a step part is held. */
    async prepareNumberHold() {
      await clock.sample();
    },
    /**
     * The Host-clock interval from the frame that ended with the held part's
     * first step, the first record since `from`, to the one that ended with
     * its first repeat, the next record: its least and most possible length.
     */
    async numberHoldTiming(from: number) {
      const [press, repeat] = records.slice(from);
      check(press && repeat, "The held part did not repeat");
      return {
        press: press.tick,
        repeat: repeat.tick,
        ...(await hostInterval(clock, press.tick, repeat.tick)),
      };
    },
  };
}
