/**
 * The kit's overlays: the `kit-overlays` panel's context menu, confirmation
 * dialog, popover and tooltip, declared by a React root from the kit's
 * components alone.
 */
import {
  kitOverlaysPanel,
  type KitOverlayReports,
  type KitOverlaysPanel,
} from "./kit-overlays-panel.js";
import {
  canvasOf,
  json,
  openCompositePage,
  type CompositeSetup,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [kitOverlaysPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const panel = page.panel("kit-overlays");
  const kit = panel.build as KitOverlaysPanel;
  return {
    ...page.steps,
    /** A point at a fraction of a kit control's box, in the parent canvas. */
    async overlayKitPoint(symbol: string, fraction: readonly [number, number]) {
      const [x, y, width, height] = await kit.bounds(symbol);
      return [
        panel.origin[0] + x + width * fraction[0],
        panel.origin[1] + y + height * fraction[1],
      ] as const;
    },
    /** Wait for the panel's focus on `symbol` and its ring, or none. */
    async expectOverlayKitFocus(symbol: string | null, visible?: boolean) {
      let last: Awaited<ReturnType<typeof kit.focus>> = null;
      await page.until(
        async () => {
          last = await kit.focus();
          return symbol === null
            ? last === null
            : last?.symbol === symbol &&
                (visible === undefined || last.visible === visible);
        },
        () => `Kit overlay focus ${json(last)}, expected ${symbol}/${visible}`,
      );
      return last;
    },
    /** Wait until the overlay `symbol` is open or not. */
    async expectOverlayKitOpen(symbol: string, open: boolean) {
      let last: boolean | undefined;
      await page.until(
        async () => (last = await kit.open(symbol)) === open,
        () => `Kit overlay ${symbol} open ${last}, expected ${open}`,
      );
    },
    /** Wait until the panel's active items are exactly `expected`. */
    async expectOverlayKitActive(expected: readonly string[]) {
      let last: string[] = [];
      await page.until(
        async () => json((last = await kit.active())) === json(expected),
        () => `Kit active items ${json(last)}, expected ${json(expected)}`,
      );
      return last;
    },
    /** Wait until `present` are declared in the panel and `absent` are not. */
    async expectOverlayKitDeclared(
      present: readonly string[],
      absent: readonly string[] = [],
    ) {
      let last: string[] = [];
      await page.until(
        async () =>
          json((last = await kit.declared([...present, ...absent]))) ===
          json(present),
        () => `Kit overlays declare ${json(last)}, expected ${json(present)}`,
      );
    },
    /** Wait until the application heard exactly `expected` from `name`. */
    async expectOverlayKitReports(
      name: keyof KitOverlayReports,
      expected: readonly unknown[],
    ) {
      await page.until(
        () => json(kit.reports[name]) === json(expected),
        () =>
          `Kit overlays ${name} reported ${json(kit.reports[name])}, expected ${json(expected)}`,
      );
      return [...kit.reports[name]];
    },
    /** Frames until settled, then what the application heard from `name`. */
    async overlayKitReports(name: keyof KitOverlayReports) {
      await page.host.presentation.frame(page.view);
      await page.host.presentation.frame(page.view);
      return [...kit.reports[name]];
    },
    /** `prepareHint` for a kit control and its tooltip, by symbol. */
    async prepareKitHint(parent: string, hint: string) {
      await page.prepareHintOn(
        panel,
        await kit.entity(parent),
        await kit.entity(hint),
      );
    },
  };
}
