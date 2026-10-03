/** Persistent gallery commands through the maintained Node session launcher. */
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import type { RgbaImage } from "../../tools/shared-host/images.js";
import { decodePng } from "../../tools/shared-host/png.js";

const execute = promisify(execFile);

export interface NativeGalleryReply {
  readonly files: readonly string[];
  readonly report: Record<string, unknown>;
}

/** Process arrangement is confined to this driver; assertions use its public operations. */
export class NativeGalleryDriver {
  constructor(
    readonly workspace: string,
    readonly hostDirectory: string,
    readonly name: string,
  ) {}

  private async cli(args: readonly string[], signal?: AbortSignal) {
    const result = await execute(
      process.execPath,
      [
        resolve(this.workspace, "tools/shared-host/shared-host.mjs"),
        ...args,
        "--host",
        this.hostDirectory,
        "--json",
      ],
      {
        cwd: this.workspace,
        ...(signal ? { signal } : {}),
        maxBuffer: 16 << 20,
      },
    );
    return result.stdout;
  }

  async start(scene: string, args: readonly string[], signal: AbortSignal) {
    await this.cli(["gallery", scene, "--name", this.name, ...args], signal);
  }

  async call(
    command: string,
    args: readonly string[] = [],
    signal?: AbortSignal,
  ): Promise<NativeGalleryReply> {
    return JSON.parse(
      await this.cli(["session", command, this.name, ...args], signal),
    ) as NativeGalleryReply;
  }

  async capture(out: string, signal: AbortSignal): Promise<RgbaImage> {
    const reply = await this.call("capture", ["--out", out], signal);
    if (!reply.files[0])
      throw new Error("Native gallery did not return a PNG capture");
    return decodePng(await readFile(reply.files[0]));
  }

  async close() {
    await this.cli(["session", "stop", this.name]);
  }
}
