/**
 * The GUI kit's toast stack over the `buttons` panel: a React root on the
 * panel's own session declares a top-level manual overlay at the panel's top
 * edge whose toast lies over button `a`. The application owns its toasts:
 * the stack reports a dismissal and the application removes the toast.
 */
import { createElement } from "react";
import {
  createRoot,
  type ReactWorldClient,
} from "../../../../packages/ipp-react/src/index.js";
import {
  GuiKit,
  ToastStack,
  type ToastItem,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import {
  canvasOf,
  json,
  openCompositePage,
  readControl,
  type CompositeSetup,
} from "./composites.js";
import { PANEL, buttonsPanel } from "./panels.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

const PANELS = [buttonsPanel([0, 0])];

export const CANVAS = canvasOf(PANELS);

/**
 * The toasts' kit body size: three eighths of the design size, so a toast is
 * 21 units tall at a 6-unit inset from the panel's edges, over button `a`.
 */
const TOAST_FONT_SIZE = 6;
const TOAST_INSET = 6;

/** Milliseconds of the Host clock before the toast dismisses itself. */
export const TOAST_DURATION = 2000;

const TOAST: ToastItem = { key: "saved", severity: "success", text: "Saved" };
const SYMBOL = `composite-toasts/${TOAST.key}`;

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, PANELS);
  const { client, font, clock } = page.panel("buttons");

  let toasts: readonly ToastItem[] = [];
  const dismissals: string[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: page.report,
  });
  const render = (): Promise<void> =>
    root.render(
      createElement(
        GuiKit,
        { contract: setup.contract, font, fontSize: TOAST_FONT_SIZE },
        createElement(ToastStack, {
          id: "composite-toasts",
          toasts,
          side: "top",
          duration: TOAST_DURATION,
          layout: { width: PANEL - 2 * TOAST_INSET },
          onDismiss: (key) => {
            dismissals.push(key);
            toasts = toasts.filter((toast) => toast.key !== key);
            void render();
          },
        }),
      ),
    );
  await render();

  /** The toast body's entity once shown, kept after it is removed. */
  let shown: bigint | undefined;
  /** The World time at which the toast was first seen laid out. */
  let shownAt = 0;
  /**
   * The toast body's entity, while declared, and the World time of the read
   * that settled it.
   */
  const find = async () => {
    let after = 0n;
    let time = 0;
    do {
      const page = await client.inspectPage({
        collection: "entities",
        after,
        limit: 64,
      });
      time = page.time;
      const entity = page.entities.find(
        (entry) => entry.metadata.symbolicId === SYMBOL,
      )?.id;
      if (entity !== undefined) return { entity, time };
      after = page.next;
    } while (after !== 0n);
    return { entity: undefined, time };
  };
  /** The World's animation controllers: the toast's time. */
  const controllers = async () =>
    (
      (await client.inspectPage({ collection: "controllers" })).controllers ??
      []
    ).map(({ id, state, time }) => ({ id, state, time }));

  return {
    ...page.steps,
    /**
     * Show the toast and wait until it is laid out; returns its evaluated box
     * in the panel's canvas.
     */
    async showToast() {
      toasts = [TOAST];
      await render();
      let bounds: readonly number[] | undefined;
      await page.until(
        async () => {
          const { entity, time } = await find();
          shown = entity;
          shownAt = time;
          bounds =
            entity === undefined
              ? undefined
              : (await readControl(client, entity))?.bounds;
          return !!bounds && bounds[2]! > 0 && bounds[3]! > 0;
        },
        () => `The toast was not laid out: ${json(bounds)}`,
      );
      return bounds!;
    },
    /**
     * Effects of `kind` on the shown toast's body since `from`, once there
     * are at least `count`.
     */
    async toastEffects(
      kind: Parameters<typeof page.effectsOn>[2],
      from: Parameters<typeof page.effectsOn>[3],
      count = 0,
    ) {
      const panel = page.panel("buttons");
      const found = () =>
        shown === undefined ? [] : page.effectsOn(panel, shown, kind, from);
      await page.until(
        () => found().length >= count,
        () => `Expected ${count} ${kind} on the toast: ${json(found())}`,
      );
      return found();
    },
    /**
     * Frames until the panel's World time is `seconds` past the toast's
     * showing, then whether it is still declared, what was dismissed and
     * its controller's state and time.
     */
    async toastAfter(seconds: number) {
      let now = shownAt;
      await page.until(
        async () => (now = (await clock.sample()).time) >= shownAt + seconds,
        () => `The World time stayed at ${now}`,
      );
      return {
        elapsed: now - shownAt,
        present: (await find()).entity !== undefined,
        dismissals: [...dismissals],
        controllers: await controllers(),
      };
    },
    /** The World time and the toast's controllers before the pointer leaves. */
    async prepareToastLeave() {
      return {
        time: (await clock.sample()).time,
        controllers: await controllers(),
      };
    },
    /**
     * Wait until the toast dismissed itself and the application removed it;
     * returns the dismissals and the World time of the first read without
     * the toast.
     */
    async expectToastDismissed() {
      let removedAt = 0;
      await page.until(
        async () => {
          const { entity, time } = await find();
          removedAt = time;
          return dismissals.includes(TOAST.key) && entity === undefined;
        },
        () => `The toast was not dismissed: ${json(dismissals)}`,
      );
      return { dismissals: [...dismissals], removedAt };
    },
    async close() {
      await root.unmount();
      await page.steps.close();
    },
  };
}
