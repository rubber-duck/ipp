/**
 * The colour control: the `colour` panel's one control, declared by a React
 * root, whose field, hue rail and alpha rail are focus parts with their own
 * keys and drags, and whose reported colours the application records.
 */
import { colourPanel, type ColourPanel } from "./colour-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [colourPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

type Hsva = {
  hue: number;
  saturation: number;
  value: number;
  alpha: number;
};

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("colour");
  const colour = panel.build as ColourPanel;
  return {
    ...page.steps,
    /** The colour control's evaluated box in the parent canvas. */
    async colourBounds() {
      const [x, y, width, height] = await colour.bounds();
      return [panel.origin[0] + x, panel.origin[1] + y, width, height] as const;
    },
    /** The colour the control's fields hold. */
    colourValue() {
      return colour.value();
    },
    /** Frames until the colour's channels are within `tolerance` of `expected`. */
    async expectColour(expected: Hsva, tolerance = 1e-5) {
      let last: unknown;
      await page.until(
        async () => {
          const value = await colour.value();
          last = value;
          return (["hue", "saturation", "value", "alpha"] as const).every(
            (channel) =>
              Math.abs(value[channel] - expected[channel]) <= tolerance,
          );
        },
        () => `Colour ${json(last)}, expected ${json(expected)}`,
      );
      return last as Hsva;
    },
    /** The colours the application received since record `from`. */
    colourRecords(from = 0) {
      return colour.records.slice(from);
    },
    /** Frames until the application's latest colour is `expected`. */
    async expectColourRecorded(expected: Hsva) {
      await page.until(
        () => json(colour.records.at(-1)?.value) === json(expected),
        () =>
          `Latest colour ${json(colour.records.at(-1))}, expected ${json(expected)}`,
      );
    },
    /** Frames until the panel focuses `part` with its ring, or nothing. */
    async expectColourFocus(part: number | null, visible?: boolean) {
      let last: Awaited<ReturnType<typeof colour.focus>> = null;
      await page.until(
        async () => {
          last = await colour.focus();
          return part === null
            ? last === null
            : last?.part === part &&
                (visible === undefined || last.visible === visible);
        },
        () => `Colour focus ${json(last)}, expected ${part}/${visible}`,
      );
      return last;
    },
    /** A client's `GuiAction` on the colour control, then a frame. */
    async colourAction(action: Parameters<typeof colour.action>[0]) {
      await colour.action(action);
      await page.host.presentation.frame(page.view);
    },
  };
}
