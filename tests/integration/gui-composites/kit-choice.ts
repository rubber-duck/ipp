/**
 * The kit's choice composites: the `kit-choice` panel's radio group, tab
 * strip and tree view, declared by a React root. Their keys come from their
 * groups, their selection reaches the application through its callbacks, and
 * the tree takes the keys the runtime returns unhandled.
 */
import {
  kitChoicePanel,
  type KitChoicePanel,
  type KitChoiceReports,
} from "./kit-choice-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./page.js";
export { nativePresentationTransport, workerTransport } from "./page.js";

const PANELS = [kitChoicePanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("kit-choice");
  const kit = panel.build as KitChoicePanel;
  return {
    ...page.steps,
    /** A point at a fraction of a kit control's box, in the parent canvas. */
    async kitPoint(symbol: string, fraction: readonly [number, number]) {
      const [x, y, width, height] = await kit.bounds(symbol);
      return [
        panel.origin[0] + x + width * fraction[0],
        panel.origin[1] + y + height * fraction[1],
      ] as const;
    },
    /** Wait for the panel's focus on `symbol` and its ring, or none. */
    async expectKitFocus(symbol: string | null, visible?: boolean) {
      let last: Awaited<ReturnType<typeof kit.focus>> = null;
      await page.until(
        async () => {
          last = await kit.focus();
          return symbol === null
            ? last === null
            : last?.symbol === symbol &&
                (visible === undefined || last.visible === visible);
        },
        () => `Kit focus ${json(last)}, expected ${symbol}/${visible}`,
      );
      return last;
    },
    /** Wait until exactly `expected` among the Buttons `symbols` are selected. */
    async expectKitSelected(
      symbols: readonly string[],
      expected: readonly string[],
    ) {
      let last: string[] = [];
      await page.until(
        async () =>
          json((last = await kit.selected(symbols))) === json(expected),
        () => `Kit selection ${json(last)}, expected ${json(expected)}`,
      );
      return last;
    },
    /** Wait until `present` are declared in the panel and `absent` are not. */
    async expectKitDeclared(
      present: readonly string[],
      absent: readonly string[] = [],
    ) {
      let last: string[] = [];
      await page.until(
        async () =>
          json((last = await kit.declared([...present, ...absent]))) ===
          json(present),
        () => `Kit declares ${json(last)}, expected ${json(present)}`,
      );
    },
    /** Wait until the composite `name` reported `expected` to the application. */
    async expectKitReports(
      name: keyof KitChoiceReports,
      expected: readonly unknown[],
    ) {
      await page.until(
        () => json(kit.reports[name]) === json(expected),
        () =>
          `Kit ${name} reported ${json(kit.reports[name])}, expected ${json(expected)}`,
      );
      return [...kit.reports[name]];
    },
  };
}
