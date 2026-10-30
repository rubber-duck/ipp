/** Native CEF sources share the development bridge with desktop input. */
import {
  bindNativeHtmlSources,
  type GuiClientInput,
  type HtmlSource,
} from "@ipp/client";
import { NativeWindowChannel } from "./window.js";

export class NativeHtmlSources {
  private next = 0;
  private stopped = false;
  private readonly records = new Map<
    number,
    {
      fail(error: Error): void;
      navigate(url: string): void;
      releaseInput?: (() => void) | undefined;
    }
  >();
  private readonly timer: NodeJS.Timeout;
  private polling = false;

  constructor(
    host: object,
    private readonly channel: NativeWindowChannel,
  ) {
    bindNativeHtmlSources(host, (page, fail, navigate) =>
      this.register(page, fail, navigate),
    );
    this.timer = setInterval(() => {
      if (!this.polling) void this.poll();
    }, 30);
  }

  private async register(
    page: string,
    fail: (error: Error) => void,
    navigate: (url: string) => void,
  ): Promise<HtmlSource> {
    if (this.stopped || this.records.size >= 32)
      throw new Error("Native CEF source unavailable");
    const id = ++this.next;
    const record = {
      fail,
      navigate,
      releaseInput: undefined as (() => void) | undefined,
    };
    this.records.set(id, record);
    try {
      const source = (
        await this.channel.cef(1, id, Buffer.from(page))
      ).toString();
      if (this.stopped) throw new Error("Native CEF source closed");
      return {
        source,
        bindInput: (_target) => {
          if (!this.records.has(id))
            throw new Error("Native CEF source released");
          record.releaseInput?.();
          let released = false;
          const release = () => {
            if (!released) {
              released = true;
              this.send(id, 7);
            }
          };
          record.releaseInput = release;
          return {
            input: (input) => {
              if (!released) this.input(id, input);
            },
            release,
          };
        },
        release: () => {
          if (!this.records.delete(id)) return;
          record.releaseInput?.();
          void this.channel.cef(2, id).catch(fail);
        },
      };
    } catch (error) {
      this.records.delete(id);
      throw error;
    }
  }

  private send(
    id: number,
    kind: number,
    a = 0,
    b = 0,
    values: readonly number[] = [0, 0, 0, 0],
    text = "",
  ) {
    const bytes = Buffer.alloc(41 + Buffer.byteLength(text));
    bytes[0] = kind;
    bytes.writeUInt32LE(a, 1);
    bytes.writeUInt32LE(b, 5);
    for (let i = 0; i < 4; i++) bytes.writeDoubleLE(values[i] ?? 0, 9 + i * 8);
    bytes.write(text, 41);
    void this.channel
      .cef(3, id, bytes)
      .catch((error) => this.records.get(id)?.fail(error));
  }

  private input(id: number, input: GuiClientInput) {
    if (input.kind === "pointer") {
      if (input.phase === "cancel") {
        this.send(id, 7);
        return;
      }
      this.send(
        id,
        input.phase === "down" ? 2 : input.phase === "up" ? 3 : 1,
        input.button === "secondary" ? 1 : input.button === "auxiliary" ? 2 : 0,
        0,
        input.point,
      );
    } else if (input.kind === "wheel")
      this.send(id, 4, 0, 0, [...input.point, ...input.delta]);
    else if (input.kind === "text")
      this.send(id, 6, 0, 0, undefined, input.text);
    else if (input.pressed) {
      const keys: Record<string, number> = {
        Backspace: 8,
        Tab: 9,
        Enter: 13,
        Escape: 27,
        " ": 32,
        PageUp: 33,
        PageDown: 34,
        End: 35,
        Home: 36,
        ArrowLeft: 37,
        ArrowUp: 38,
        ArrowRight: 39,
        ArrowDown: 40,
        Delete: 46,
      };
      const key =
        keys[input.key] ??
        (input.key.length === 1 ? input.key.toUpperCase().charCodeAt(0) : 0);
      // IPP shift/control/alt/meta to CEF's EventFlags.
      const modifiers =
        (input.modifiers & 1 ? 2 : 0) |
        (input.modifiers & 2 ? 4 : 0) |
        (input.modifiers & 4 ? 8 : 0) |
        (input.modifiers & 8 ? 128 : 0);
      if (key) this.send(id, 5, key, modifiers);
    }
  }

  private async poll() {
    if (this.stopped) return;
    this.polling = true;
    try {
      const bytes = await this.channel.cef(4);
      let offset = 0;
      while (offset < bytes.length) {
        if (offset + 12 > bytes.length) throw new Error("Truncated CEF event");
        const id = bytes.readUInt32LE(offset),
          kind = bytes.readUInt32LE(offset + 4),
          length = bytes.readUInt32LE(offset + 8);
        offset += 12;
        if (length > bytes.length - offset)
          throw new Error("Invalid CEF event length");
        const text = bytes.subarray(offset, offset + length).toString();
        offset += length;
        if (kind === 2) this.records.get(id)?.fail(new Error(text));
        if (kind === 3) this.records.get(id)?.navigate(text);
      }
    } catch (error) {
      if (!this.stopped)
        for (const record of this.records.values()) record.fail(error as Error);
    } finally {
      this.polling = false;
    }
  }

  close() {
    this.stopped = true;
    clearInterval(this.timer);
    this.records.clear();
  }
}
