/** Browser fixture calls shared by page scenarios. */
import type { Page } from "playwright";

export async function invoke<T>(
  page: Page,
  moduleUrl: string,
  exportName: string,
  arguments_: readonly unknown[] = [],
): Promise<T> {
  return page.evaluate(
    async ({ url, name, values }) => {
      const module = (await import(url)) as Record<
        string,
        ((...arguments_: unknown[]) => unknown) | undefined
      >;
      const operation = module[name];
      if (!operation) throw new Error(`Missing browser fixture export ${name}`);
      return (await operation(...values)) as T;
    },
    { url: moduleUrl, name: exportName, values: [...arguments_] },
  );
}

/**
 * Call fixture exports through `invoke`, recording the result of every export
 * whose name matches `measured` as an image measurement. The recorded values
 * show the margin of each pixel threshold at the canvas size rendered.
 */
export function measuredInvoke(
  page: Page,
  moduleUrl: string,
  evidence: { record(kind: string, value: unknown): Promise<void> },
  measured: RegExp,
) {
  return async <T>(
    exportName: string,
    arguments_: readonly unknown[] = [],
  ): Promise<T> => {
    const result = await invoke<T>(page, moduleUrl, exportName, arguments_);
    if (measured.test(exportName))
      await evidence.record("image-measurement", {
        name: exportName,
        args: arguments_,
        result,
      });
    return result;
  };
}
