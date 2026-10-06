/**
 * The kit's value composites: the `kit-values` panel's range slider and
 * knob, declared by a React root, whose readouts follow the committed values
 * of real drags.
 */
import {
  kitValuesPanel,
  type KitValuesPanel,
  type ValuesSnapshot,
} from "./kit-values-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [kitValuesPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("kit-values");
  const kit = panel.build as KitValuesPanel;
  return {
    ...page.steps,
    /** A control's evaluated box in the parent canvas. */
    async valuesBounds(symbol: string) {
      const [x, y, width, height] = await kit.bounds(symbol);
      return [panel.origin[0] + x, panel.origin[1] + y, width, height] as const;
    },
    /**
     * Wait until a control of the panel is the context's native text target
     * with `text` being edited and its native buffer holds DOM focus.
     */
    async expectValuesEdit(text: string) {
      const { input } = page;
      await page.until(
        () =>
          input.nativeText?.fence.target.world.id === kit.world.id &&
          input.nativeText.text === text &&
          document.activeElement?.hasAttribute("data-ipp-native-text") === true,
        () => `Values edit ${json(input.nativeText)}, expected ${text}`,
      );
      return input.nativeText!.text;
    },
    /**
     * Wait until every field of `expected` matches the panel's committed
     * values, readouts and reports; returns the state.
     */
    async expectValues(expected: Partial<ValuesSnapshot>) {
      let last: ValuesSnapshot | undefined;
      await page.until(
        async () => {
          last = await kit.snapshot();
          return Object.entries(expected).every(
            ([key, value]) =>
              json(last?.[key as keyof ValuesSnapshot]) === json(value),
          );
        },
        () => `Values panel ${json(last)}, expected ${json(expected)}`,
      );
      return last!;
    },
  };
}
