import type { HtmlElementImage } from "./html-content.js";

interface HtmlRuntime {
  memory: WebAssembly.Memory;
  ipp_html_register(source: number): number;
  ipp_html_revoke(source: number): void;
  ipp_html_changed(source: number): void;
  ipp_html_requests_ptr(): number;
  ipp_html_requests_len(): number;
  ipp_html_complete(ticket: number, image: number): number;
  ipp_html_error_ptr(): number;
  ipp_html_error_len(): number;
}

/** Host control messages carry images; generated World commands carry names only. */
export class HtmlWorkerService {
  constructor(
    private readonly runtime: HtmlRuntime,
    private readonly device: {
      stageHtmlImage(image: HtmlElementImage): number;
      releaseWebImage(id: number): void;
      isContextLost(): boolean;
    },
    private readonly post: (message: unknown, transfer: Transferable[]) => void,
  ) {}

  poll(): void {
    const pointer = this.runtime.ipp_html_requests_ptr() >>> 0;
    const length = this.runtime.ipp_html_requests_len() >>> 0;
    if (!length) return;
    const records = new Uint32Array(
      this.runtime.memory.buffer,
      pointer,
      length,
    ).slice();
    for (let i = 0; i < records.length; i += 4)
      this.post(
        {
          type: "html-capture",
          ticket: records[i],
          source: records[i + 1],
          width: records[i + 2],
          height: records[i + 3],
        },
        [],
      );
  }

  receive(data: Record<string, unknown>): boolean {
    if (data.type === "html-register") {
      const source = Number(data.source);
      const accepted = this.runtime.ipp_html_register(source) === 1;
      this.post(
        {
          type: "html-registered",
          source,
          ...(accepted ? {} : { error: "HTML source registration failed" }),
        },
        [],
      );
      return true;
    }
    if (data.type === "html-changed") {
      this.runtime.ipp_html_changed(Number(data.source));
      return true;
    }
    if (data.type === "html-revoke") {
      this.runtime.ipp_html_revoke(Number(data.source));
      return true;
    }
    if (data.type !== "html-frame") return false;
    if (this.device.isContextLost()) {
      (data.image as HtmlElementImage | undefined)?.close();
      this.runtime.ipp_html_complete(Number(data.ticket), 0);
      this.post({ type: "html-completed", ticket: data.ticket }, []);
      return true;
    }
    let stage = 0;
    let error = data.error;
    try {
      if (data.image)
        stage = this.device.stageHtmlImage(data.image as HtmlElementImage);
      const accepted = this.runtime.ipp_html_complete(
        Number(data.ticket),
        stage,
      );
      if (accepted === 2) error = undefined;
      if (accepted === 0 && !error && !this.device.isContextLost())
        error =
          new TextDecoder().decode(
            new Uint8Array(
              this.runtime.memory.buffer,
              this.runtime.ipp_html_error_ptr() >>> 0,
              this.runtime.ipp_html_error_len() >>> 0,
            ),
          ) || "HTML capture could not be imported";
    } catch (failure) {
      error = this.device.isContextLost() ? undefined : String(failure);
      this.runtime.ipp_html_complete(Number(data.ticket), 0);
    } finally {
      if (stage) this.device.releaseWebImage(stage);
    }
    this.post(
      {
        type: "html-completed",
        ticket: data.ticket,
        ...(error ? { error } : {}),
      },
      [],
    );
    return true;
  }
}
