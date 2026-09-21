import { appendFile, mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

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
