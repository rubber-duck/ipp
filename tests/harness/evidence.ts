/** Scenario evidence: the event log and completed-frame artifacts. */
import { appendFile, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import type { Page } from "playwright";
import { invoke } from "./page-calls.js";

const DEFAULT_EVENT_LIMIT = 1024 * 1024;

function serialize(value: unknown): string {
  const seen = new WeakSet<object>();
  return JSON.stringify(value, (_key, item: unknown) => {
    if (typeof item === "bigint") {
      return { $bigint: item.toString() };
    }
    if (item !== null && typeof item === "object") {
      if (seen.has(item)) return { $reference: "previously recorded object" };
      seen.add(item);
    }
    if (item instanceof Error) {
      return {
        name: item.name,
        message: item.message,
        stack: item.stack,
        cause: item.cause,
      };
    }
    return item;
  });
}

export class EvidenceRecorder {
  readonly directory: string;
  readonly #eventsPath: string;
  readonly #maximumBytes: number;
  #writtenBytes = 0;
  #truncated = false;
  #tail: Promise<void> = Promise.resolve();

  private constructor(directory: string, maximumBytes: number) {
    this.directory = directory;
    this.#eventsPath = join(directory, "events.jsonl");
    this.#maximumBytes = maximumBytes;
  }

  static async create(
    directory: string,
    maximumBytes = DEFAULT_EVENT_LIMIT,
  ): Promise<EvidenceRecorder> {
    await mkdir(directory, { recursive: true });
    await writeFile(join(directory, "events.jsonl"), "", "utf8");
    return new EvidenceRecorder(directory, maximumBytes);
  }

  record(kind: string, value: unknown): Promise<void> {
    const line = `${serialize({
      at: new Date().toISOString(),
      kind,
      value,
    })}\n`;
    const byteLength = Buffer.byteLength(line);

    this.#tail = this.#tail.then(async () => {
      if (this.#truncated) {
        return;
      }
      if (this.#writtenBytes + byteLength > this.#maximumBytes) {
        this.#truncated = true;
        await appendFile(
          this.#eventsPath,
          `${serialize({
            at: new Date().toISOString(),
            kind: "evidence_truncated",
            value: { maximumBytes: this.#maximumBytes },
          })}\n`,
          "utf8",
        );
        return;
      }
      await appendFile(this.#eventsPath, line, "utf8");
      this.#writtenBytes += byteLength;
    });

    return this.#tail;
  }

  async writeJson(name: string, value: unknown): Promise<void> {
    await writeFile(
      join(this.directory, name),
      `${serialize(value)}\n`,
      "utf8",
    );
  }

  async flush(): Promise<void> {
    await this.#tail;
  }
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
