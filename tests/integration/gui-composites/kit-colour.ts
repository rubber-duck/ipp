/**
 * The kit's colour picker: the `kit-colour` panel's picker, declared by a
 * React root, whose hex, channels and presets set the control's one colour
 * and follow it as it is dragged.
 */
import {
  kitColourPanel,
  type KitColourPanel,
  type PickerSnapshot,
} from "./kit-colour-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./page.js";
export { nativePresentationTransport, workerTransport } from "./page.js";

const PANELS = [kitColourPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("kit-colour");
  const kit = panel.build as KitColourPanel;
  return {
    ...page.steps,
    /** A control's evaluated box in the parent canvas. */
    async pickerBounds(symbol: string) {
      const [x, y, width, height] = await kit.bounds(symbol);
      return [panel.origin[0] + x, panel.origin[1] + y, width, height] as const;
    },
    /**
     * Wait until a control of the panel is the context's native text target
     * with `text` being edited and its native buffer holds DOM focus.
     */
    async expectPickerEdit(text: string) {
      const { input } = page;
      await page.until(
        () =>
          input.nativeText?.fence.target.world.id === kit.world.id &&
          input.nativeText.text === text &&
          document.activeElement?.hasAttribute("data-ipp-native-text") === true,
        () => `Picker edit ${json(input.nativeText)}, expected ${text}`,
      );
      return input.nativeText!.text;
    },
    /**
     * Wait until every field of `expected` matches the picker's state;
     * returns the state.
     */
    async expectPicker(expected: Partial<PickerSnapshot>) {
      let last: PickerSnapshot | undefined;
      await page.until(
        async () => {
          last = await kit.snapshot();
          return Object.entries(expected).every(
            ([key, value]) =>
              json(last?.[key as keyof PickerSnapshot]) === json(value),
          );
        },
        () => `Picker ${json(last)}, expected ${json(expected)}`,
      );
      return last!;
    },
    /**
     * Wait until two frames in a row show the same picker state: the
     * control's colour and everything derived from it have settled.
     */
    async expectPickerSettled() {
      let last = await kit.snapshot();
      let settled = false;
      await page.until(
        async () => {
          const next = await kit.snapshot();
          settled = json(next) === json(last);
          last = next;
          return settled;
        },
        () => `Picker still changing: ${json(last)}`,
      );
      return last;
    },
  };
}
