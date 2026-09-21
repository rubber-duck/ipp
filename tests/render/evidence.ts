/** Browser fixture calls and completed-frame artifacts shared by render scenarios. */
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
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

export function bigintJson(_key: string, value: unknown): unknown {
  return typeof value === "bigint" ? { $bigint: value.toString() } : value;
}

export async function writeDataUrl(
  path: string,
  dataUrl: string,
): Promise<void> {
  const prefix = "data:image/png;base64,";
  if (!dataUrl.startsWith(prefix))
    throw new Error("Frame artifact is not a PNG data URL");
  await writeFile(path, Buffer.from(dataUrl.slice(prefix.length), "base64"));
}

export async function recordCapture(
  page: Page,
  moduleUrl: string,
  directory: string,
  captured: Set<string>,
  label: string,
  options: {
    dataUrlExport?: string;
    metadataExport?: string;
    canvasSelector: string;
  },
): Promise<void> {
  if (captured.has(label)) return;
  const [dataUrl, metadata] = await Promise.all([
    invoke<string>(page, moduleUrl, options.dataUrlExport ?? "captureDataUrl", [
      label,
    ]),
    invoke<unknown>(
      page,
      moduleUrl,
      options.metadataExport ?? "captureMetadata",
      [label],
    ),
  ]);
  await Promise.all([
    writeDataUrl(join(directory, `${label}-capture.png`), dataUrl),
    writeFile(
      join(directory, `${label}-frame.json`),
      `${JSON.stringify(metadata, bigintJson, 2)}\n`,
    ),
    page
      .locator(options.canvasSelector)
      .screenshot({ path: join(directory, `${label}-canvas.png`) }),
  ]);
  captured.add(label);
}
