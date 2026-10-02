/**
 * The kit's selection controls: the `kit-select` panel's dropdown,
 * searchable dropdown, multi-select and autocomplete, declared by a React
 * root. Their lists are light overlays of rows that take no focus, driven by
 * the runtime's active item.
 */
import {
  kitSelectPanel,
  type KitSelectPanel,
  type SelectSnapshot,
} from "./kit-select-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./page.js";
export { nativePresentationTransport, workerTransport } from "./page.js";

const PANELS = [kitSelectPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("kit-select");
  const kit = panel.build as KitSelectPanel;
  /** A control's box in the parent canvas. */
  const box = async (symbol: string) => {
    const [x, y, width, height] = await kit.bounds(symbol);
    return [panel.origin[0] + x, panel.origin[1] + y, width, height] as const;
  };
  return {
    ...page.steps,
    /** A point at a fraction of a control's box, in the parent canvas. */
    async selectPoint(symbol: string, fraction: readonly [number, number]) {
      const [x, y, width, height] = await box(symbol);
      return [x + width * fraction[0], y + height * fraction[1]] as const;
    },
    selectBounds: box,
    /**
     * Wait until the field `symbol` takes typed text: the context's native
     * text target, its native buffer holding DOM focus.
     */
    async expectSelectField(symbol: string) {
      return page.expectNativeBuffer(await kit.entity(symbol));
    },
    /**
     * Wait until every field of `expected` matches the panel's state: focus,
     * active item, open lists, values and reports; returns the state.
     */
    async expectSelect(expected: Partial<SelectSnapshot>) {
      let last: SelectSnapshot | undefined;
      await page.until(
        async () => {
          last = await kit.snapshot();
          return Object.entries(expected).every(
            ([key, value]) =>
              json(last?.[key as keyof SelectSnapshot]) === json(value),
          );
        },
        () => `Select panel ${json(last)}, expected ${json(expected)}`,
      );
      return last!;
    },
  };
}
