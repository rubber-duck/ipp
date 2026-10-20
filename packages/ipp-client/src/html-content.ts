import { HtmlInputBinding } from "./html-input.js";
import type { GuiTarget } from "./gui-types.js";

/** Browser-owned DOM; only the source name enters a World. */
export interface HtmlSource {
  readonly source: string;
  bindInput(target: GuiTarget): HtmlInputBinding;
  release(): void;
}

export interface HtmlElementImage {
  readonly width: number;
  readonly height: number;
  close(): void;
}

interface DrawableCanvas extends HTMLCanvasElement {
  layoutSubtree: boolean;
  requestPaint(): void;
  captureElementImage(element: Element): HtmlElementImage;
}

export function htmlCanvasAvailable(): boolean {
  return (
    typeof HTMLCanvasElement !== "undefined" &&
    "layoutSubtree" in HTMLCanvasElement.prototype &&
    "captureElementImage" in HTMLCanvasElement.prototype &&
    "requestPaint" in HTMLCanvasElement.prototype &&
    "updateElementGeometry" in HTMLCanvasElement.prototype &&
    "clearElementGeometry" in HTMLCanvasElement.prototype &&
    typeof WebGL2RenderingContext !== "undefined" &&
    "texElementSubImage2D" in WebGL2RenderingContext.prototype
  );
}

type Register = (
  content: HTMLElement,
  fail: (error: Error) => void,
) => Promise<HtmlSource>;
const HTML_SOURCES = Symbol.for("ipp.html-sources");

export function bindHtmlSources<T extends object>(
  target: T,
  source: object | Register,
): T {
  const register =
    typeof source === "function"
      ? source
      : (source as { [HTML_SOURCES]?: Register })[HTML_SOURCES];
  if (register)
    Object.defineProperty(target, HTML_SOURCES, { value: register });
  return target;
}

/** DOM owned by the presentation canvas document. Paint changes publish fresh
 * images. Canvas/video and cross-origin descendants are outside this producer. */
export function registerHtml(
  host: object,
  content: HTMLElement,
  fail: (error: Error) => void,
): Promise<HtmlSource> {
  const register = (host as { [HTML_SOURCES]?: Register })[HTML_SOURCES];
  if (!register)
    return Promise.reject(
      new Error("HTML capture requires a browser presentation canvas"),
    );
  return register(content, fail);
}

/** Main-thread adapter for one associated worker presentation canvas. */
export class HtmlSources {
  #next = 0;
  #closed = false;
  #queue = Promise.resolve();
  readonly #sources = new Map<
    number,
    {
      content: HTMLElement;
      input?: HtmlInputBinding;
      fail(error: Error): void;
      ready?: { resolve(): void; reject(error: Error): void };
    }
  >();
  readonly #captures = new Map<number, number>();
  readonly #cancel = new AbortController();

  constructor(
    private readonly canvas: HTMLCanvasElement | undefined,
    private readonly post: (
      message: unknown,
      transfer?: Transferable[],
    ) => void,
  ) {
    canvas?.addEventListener(
      "paint",
      (event) => {
        const changed = (event as Event & { changedElements?: Element[] })
          .changedElements;
        for (const [source, record] of this.#sources)
          if (changed?.includes(record.content))
            this.post({ type: "html-changed", source });
      },
      { signal: this.#cancel.signal },
    );
  }

  async register(
    content: HTMLElement,
    fail: (error: Error) => void,
  ): Promise<HtmlSource> {
    if (this.#closed || !this.canvas || !htmlCanvasAvailable())
      throw new Error("HTML-in-Canvas is unavailable");
    if (
      content.ownerDocument !== this.canvas.ownerDocument ||
      content.parentNode
    )
      throw new Error(
        "HTML source must be an unattached element in the canvas document",
      );
    if (this.#sources.size >= 32 || this.#next === 0xffff_ffff)
      throw new Error("HTML source capacity exhausted");
    const id = ++this.#next;
    (this.canvas as DrawableCanvas).layoutSubtree = true;
    content.setAttribute("drawable", "");
    content.style.boxSizing = "border-box";
    content.style.overflow = "hidden";
    this.canvas.append(content);
    const release = () => {
      const record = this.#sources.get(id);
      if (!record) return;
      this.#sources.delete(id);
      record.input?.release();
      record.content.remove();
      record.ready?.reject(new Error("HTML source released"));
      if (!this.#closed) this.post({ type: "html-revoke", source: id });
    };
    try {
      await new Promise<void>((resolve, reject) => {
        this.#sources.set(id, { content, fail, ready: { resolve, reject } });
        this.post({ type: "html-register", source: id });
      });
      return {
        source: `html:${id}`,
        release,
        bindInput: (target) => {
          const record = this.#sources.get(id);
          if (!record) throw new Error("HTML source released");
          record.input?.release();
          const input = new HtmlInputBinding(this.canvas!, content, target);
          record.input = input;
          return input;
        },
      };
    } catch (error) {
      release();
      throw error;
    }
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "html-registered") {
      const record = this.#sources.get(Number(data.source));
      if (data.error) record?.ready?.reject(new Error(String(data.error)));
      else record?.ready?.resolve();
      if (record) delete record.ready;
      return true;
    }
    if (data.type === "html-capture") {
      const { ticket, source, width, height } = data as unknown as {
        ticket: number;
        source: number;
        width: number;
        height: number;
      };
      this.#captures.set(ticket, source);
      this.#queue = this.#queue.then(() =>
        this.capture(ticket, source, width, height),
      );
      return true;
    }
    if (data.type === "html-completed") {
      const source = this.#captures.get(Number(data.ticket));
      this.#captures.delete(Number(data.ticket));
      if (data.error && source !== undefined)
        this.#sources.get(source)?.fail(new Error(String(data.error)));
      return true;
    }
    return false;
  }

  private async capture(
    ticket: number,
    source: number,
    width: number,
    height: number,
  ): Promise<void> {
    let image: HtmlElementImage | undefined;
    try {
      const record = this.#sources.get(source);
      if (!record || this.#closed) throw new Error("HTML source released");
      const canvas = this.canvas as DrawableCanvas;
      record.content.style.width = `${width}px`;
      record.content.style.height = `${height}px`;
      await new Promise<void>((resolve, reject) => {
        const finish = (error?: Error) => {
          clearTimeout(timer);
          canvas.removeEventListener("paint", painted);
          this.#cancel.signal.removeEventListener("abort", aborted);
          if (error) reject(error);
          else resolve();
        };
        const painted = () => finish();
        const aborted = () => finish(new Error("HTML producer closed"));
        const timer = setTimeout(
          () => finish(new Error("HTML capture timed out")),
          10_000,
        );
        canvas.addEventListener("paint", painted, { once: true });
        this.#cancel.signal.addEventListener("abort", aborted, { once: true });
        try {
          canvas.requestPaint();
        } catch (error) {
          finish(error instanceof Error ? error : new Error(String(error)));
        }
      });
      if (this.#sources.get(source) !== record || this.#closed)
        throw new Error("HTML source released");
      image = canvas.captureElementImage(record.content);
      this.post({ type: "html-frame", ticket, image }, [
        image as unknown as Transferable,
      ]);
      image = undefined;
    } catch (error) {
      image?.close();
      if (!this.#closed)
        this.post({ type: "html-frame", ticket, error: String(error) });
    }
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#cancel.abort();
    for (const record of this.#sources.values()) {
      record.input?.release();
      record.content.remove();
      record.ready?.reject(new Error("HTML producer closed"));
    }
    this.#sources.clear();
    this.#captures.clear();
  }
}
