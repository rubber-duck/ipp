/** Diagnostic call counts on real generated clients; every original call still executes. */
import type { Page } from "playwright";

export interface ChartWorkObservation {
  calls: Record<string, number>;
  active: Record<string, number>;
  maxActive: Record<string, number>;
}

export async function startChartWorkObservation(page: Page) {
  await page.evaluate(() => {
    type Method = (...args: unknown[]) => unknown;
    type Target = Record<string, Method>;
    const observedWindow = window as unknown as Window & {
      ippWorldCanvas?: {
        client: Target;
        host: Target & { datasets: Target };
      };
      ippGalleryScene?: { subscribe(listener: () => void): () => void };
      ippChartWork?: ChartWorkObservation & { restore(): void };
    };
    if (observedWindow.ippChartWork)
      throw new Error("Chart work observation is already active");
    const handle = observedWindow.ippWorldCanvas;
    if (!handle) throw new Error("Gallery canvas is not ready");
    const restores: (() => void)[] = [];
    const state: ChartWorkObservation = {
      calls: {},
      active: {},
      maxActive: {},
    };
    const wrap = (
      target: Target,
      name: string,
      label: (args: unknown[]) => string,
    ) => {
      const original = target[name];
      if (typeof original !== "function")
        throw new Error(`Missing observed public method ${name}`);
      const descriptor = Object.getOwnPropertyDescriptor(target, name);
      target[name] = function (...args: unknown[]) {
        const key = label(args);
        state.calls[key] = (state.calls[key] ?? 0) + 1;
        state.active[key] = (state.active[key] ?? 0) + 1;
        state.maxActive[key] = Math.max(
          state.maxActive[key] ?? 0,
          state.active[key],
        );
        const completed = () => {
          state.active[key]!--;
        };
        try {
          return Promise.resolve(original.apply(this, args)).finally(completed);
        } catch (error) {
          completed();
          throw error;
        }
      };
      restores.push(() => {
        if (descriptor) Object.defineProperty(target, name, descriptor);
        else delete target[name];
      });
    };
    const owner = (target: Target, name: string): Target => {
      let current: Target | null = target;
      while (current && !Object.hasOwn(current, name))
        current = Object.getPrototypeOf(current) as Target | null;
      if (!current) throw new Error(`Missing public method owner ${name}`);
      return current;
    };
    try {
      const unsubscribe = observedWindow.ippGalleryScene?.subscribe(() => {
        state.calls["scene.notify"] = (state.calls["scene.notify"] ?? 0) + 1;
      });
      if (unsubscribe) restores.push(unsubscribe);
      for (const name of ["read", "bindingView", "update"])
        wrap(handle.host.datasets, name, () => `datasets.${name}`);
      wrap(handle.host, "getRootOutputBinding", () => "host.rootBinding");
      for (const name of ["inspect", "navigateCamera", "waitForFrame"])
        wrap(owner(handle.client, name), name, () => `world.${name}`);
      wrap(owner(handle.client, "inspectPage"), "inspectPage", (args) => {
        const query = args[0] as { collection?: string; target?: unknown };
        return `world.inspectPage.${query?.collection ?? "summary"}${query?.target === undefined ? "" : ".targeted"}`;
      });
      wrap(owner(handle.client, "query"), "query", (args) => {
        const query = args[0] as { type?: string };
        return `world.query.${query?.type ?? "unknown"}`;
      });
      observedWindow.ippChartWork = {
        ...state,
        restore() {
          for (const restore of restores.reverse()) restore();
          delete observedWindow.ippChartWork;
        },
      };
    } catch (error) {
      for (const restore of restores.reverse()) restore();
      throw error;
    }
  });
}

export async function finishChartWorkObservation(
  page: Page,
): Promise<ChartWorkObservation> {
  return page.evaluate(() => {
    const observedWindow = window as Window & {
      ippChartWork?: ChartWorkObservation & { restore(): void };
    };
    const state = observedWindow.ippChartWork;
    if (!state) throw new Error("No active chart work observation");
    const snapshot = {
      calls: { ...state.calls },
      active: { ...state.active },
      maxActive: { ...state.maxActive },
    };
    state.restore();
    return snapshot;
  });
}

export async function observeChartWork(
  page: Page,
  operation: () => Promise<unknown>,
) {
  await startChartWorkObservation(page);
  try {
    await operation();
  } catch (error) {
    await finishChartWorkObservation(page);
    throw error;
  }
  return finishChartWorkObservation(page);
}

export async function waitForChartUpdates(page: Page, count: number) {
  await page.waitForFunction(
    (count) =>
      ((window as Window & { ippChartWork?: ChartWorkObservation }).ippChartWork
        ?.calls["datasets.update"] ?? 0) >= count,
    count,
  );
}
