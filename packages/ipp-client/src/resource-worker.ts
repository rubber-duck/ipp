import { SourceAvailability } from "./source-availability.js";
import { resourceFetchUrl, type ResourceUrlMapping } from "./resource-urls.js";
/** Host I/O bridge. Only the synchronous host pump calls WASM exports. */
import { DiagnosticLogger, type LogLevel } from "./logging.js";

const CHUNK_BYTES = 64 << 10;
const MAX_ACTIVE = 8;
const FETCH_INACTIVITY_MS = 30_000;

export interface AssetHostExports {
  readonly memory: WebAssembly.Memory;
  ipp_progress_resources(): number;
  ipp_service_resources(): number;
  ipp_resource_poll(): number;
  ipp_asset_error_max_bytes(): number;
  ipp_resource_buffered_bytes(): number;
  ipp_resource_chunk(session: bigint, id: bigint, length: number): number;
  ipp_resource_end(
    session: bigint,
    id: bigint,
    success: number,
    length: number,
  ): number;
  ipp_resource_input_reserve(length: number): number;
  ipp_output_ptr(): number;
  ipp_output_len(): number;
}

interface Acquisition {
  readonly id: bigint;
  readonly source: string;
  readonly recovery: boolean;
  readonly controller: AbortController;
  started: boolean;
  waiting: boolean;
  reading: boolean;
  received: number;
  reader?: ReadableStreamBYOBReader;
  chunk?: Uint8Array<ArrayBuffer>;
  done?: boolean;
  error?: string;
  timer?: ReturnType<typeof setTimeout>;
}

export class AssetWorkerService {
  private readonly requests = new Map<bigint, Acquisition>();
  // Strong HTTP validators pin recovery to the original content. Evicted or
  // absent validators make recovery fail explicitly instead of refreshing data.
  private readonly validators = new Map<string, string | null>();
  private readonly availability = new Map<string, SourceAvailability>();
  private closed = false;
  private pumpTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly logger: DiagnosticLogger;

  constructor(
    private readonly runtime: AssetHostExports,
    private readonly session: bigint,
    logLevel: LogLevel = "info",
    private readonly counters: Record<string, number> = {},
    private readonly resourceUrls: readonly ResourceUrlMapping[] = [],
    private readonly failed: (error: Error) => void = (error) => {
      throw error;
    },
    private readonly prepareProgress: () => void = () => {},
  ) {
    this.logger = new DiagnosticLogger("resources", logLevel);
    counters.sourceBytes = 0;
    counters.sourcePeakBufferedBytes = 0;
  }

  /** Real retained JS staging; each active reader holds at most one bounded chunk. */
  get bufferedBytes(): number {
    let bytes = 0;
    for (const request of this.requests.values())
      bytes += request.reading
        ? CHUNK_BYTES
        : (request.chunk?.buffer.byteLength ?? 0);
    return bytes;
  }

  pump(): void {
    if (this.closed) return;
    this.measureBuffers();
    this.pollRequests();
    let receivedInput = false;

    for (const request of this.requests.values()) {
      if (request.error) {
        const limit = this.runtime.ipp_asset_error_max_bytes();
        const encoded = new TextEncoder().encode(request.error.slice(0, limit));
        let end = Math.min(limit, encoded.length);
        while (end < encoded.length && (encoded[end]! & 0xc0) === 0x80) end--;
        const bytes = encoded.slice(0, end);
        this.copyInput(bytes);
        this.check(
          this.runtime.ipp_resource_end(
            this.session,
            request.id,
            0,
            bytes.byteLength,
          ),
        );
        receivedInput = true;
        this.release(request);
        continue;
      }
      if (request.chunk) {
        this.copyInput(request.chunk);
        let result = this.runtime.ipp_resource_chunk(
          this.session,
          request.id,
          request.chunk.byteLength,
        );
        if (result === 2) {
          this.progressResources();
          this.copyInput(request.chunk);
          result = this.runtime.ipp_resource_chunk(
            this.session,
            request.id,
            request.chunk.byteLength,
          );
          if (result === 2) {
            this.schedulePump(4);
            continue;
          }
        }
        this.check(result);
        receivedInput = true;
        this.counters.sourceBytes =
          (this.counters.sourceBytes ?? 0) + request.chunk.byteLength;
        delete request.chunk;
      }
      if (request.done) {
        this.check(
          this.runtime.ipp_resource_end(this.session, request.id, 1, 0),
        );
        receivedInput = true;
        this.logger.log("debug", "resource.delivered", () => ({
          session: this.session,
          resource: request.id,
          bytes: request.received,
          success: true,
        }));
        this.release(request);
      } else if (request.reader && !request.reading && !request.chunk) {
        void this.readNext(request);
      }
    }

    if (receivedInput) {
      this.progressResources();
      this.pollRequests();
    }

    let active = 0;
    for (const request of this.requests.values()) {
      if (request.started && !request.waiting) active++;
    }
    for (const request of this.requests.values()) {
      if (active >= MAX_ACTIVE) break;
      if (request.started) continue;
      request.started = true;
      active++;
      void this.acquire(request);
    }
  }

  /** Expose requests opened by the frame loader phase without polling it again. */
  pumpAfterFrame(): void {
    if (this.closed) return;
    this.check(this.runtime.ipp_service_resources());
    this.pump();
  }

  private pollRequests(): void {
    while (this.runtime.ipp_resource_poll() === 1) {
      const bytes = this.output();
      if (bytes.byteLength < 13)
        throw new Error("Invalid resource request header");
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      const operation = view.getUint8(0);
      const id = view.getBigUint64(1, true);
      const length = view.getUint32(9, true);
      if (id === 0n || operation > 2 || length !== bytes.length - 13)
        throw new Error("Invalid resource request bounds");
      if (operation === 0) {
        const request = this.requests.get(id);
        if (request) this.release(request);
        continue;
      }
      if (this.requests.has(id))
        throw new Error("Duplicate resource input request");
      this.requests.set(id, {
        id,
        source: new TextDecoder("utf-8", { fatal: true }).decode(
          bytes.subarray(13),
        ),
        recovery: operation === 2,
        controller: new AbortController(),
        started: false,
        waiting: false,
        reading: false,
        received: 0,
      });
    }
  }

  private schedulePump(delay = 0): void {
    if (this.closed || this.pumpTimer !== undefined) return;
    this.pumpTimer = setTimeout(() => {
      this.pumpTimer = undefined;
      try {
        this.pump();
      } catch (error) {
        this.failed(error instanceof Error ? error : new Error(String(error)));
      }
    }, delay);
  }

  private progressResources(): void {
    this.prepareProgress();
    this.check(this.runtime.ipp_progress_resources());
  }

  private awaitProgress(request: Acquisition): void {
    clearTimeout(request.timer);
    request.timer = setTimeout(
      () =>
        request.controller.abort(
          new Error("Resource made no progress for 30 seconds"),
        ),
      FETCH_INACTIVITY_MS,
    );
  }

  private measureBuffers(): void {
    this.counters.sourceBufferedBytes =
      this.bufferedBytes + this.runtime.ipp_resource_buffered_bytes();
    this.counters.sourcePeakBufferedBytes = Math.max(
      this.counters.sourcePeakBufferedBytes ?? 0,
      this.counters.sourceBufferedBytes,
    );
  }

  close(): void {
    this.closed = true;
    clearTimeout(this.pumpTimer);
    for (const request of this.requests.values()) this.release(request);
    this.validators.clear();
    for (const monitor of this.availability.values()) monitor.close();
    this.availability.clear();
  }

  private release(request: Acquisition): void {
    clearTimeout(request.timer);
    request.controller.abort();
    void request.reader?.cancel().catch(() => {});
    this.requests.delete(request.id);
  }

  private isCurrent(request: Acquisition): boolean {
    return !this.closed && this.requests.get(request.id) === request;
  }

  private async acquire(request: Acquisition): Promise<void> {
    this.logger.log("debug", "resource.requested", () => ({
      session: this.session,
      resource: request.id,
      source: request.source,
    }));
    this.awaitProgress(request);
    try {
      const url = new URL(request.source);
      if (url.protocol !== "http:" && url.protocol !== "https:")
        throw new Error(
          `Resource provider ${url.protocol} is unavailable in this worker`,
        );
      const pinned = this.validators.get(request.source);
      if (request.recovery && !pinned)
        throw new Error(
          "Immutable resource recovery is unavailable without a strong HTTP ETag",
        );
      const response = await fetch(resourceFetchUrl(url, this.resourceUrls), {
        signal: request.controller.signal,
        credentials: "same-origin",
        ...(pinned ? { headers: { "If-Match": pinned } } : {}),
      });
      if (response.status === 202) {
        const declaration: unknown = await response.json();
        if (!isPendingSourceDeclaration(declaration))
          throw new Error("Invalid pending source declaration");
        const fetchUrl = new URL(resourceFetchUrl(url, this.resourceUrls));
        const monitorUrl = new URL(declaration.monitor, fetchUrl);
        if (
          monitorUrl.origin !== fetchUrl.origin ||
          declaration.source !== fetchUrl.pathname ||
          !monitorUrl.searchParams.get("session")
        )
          throw new Error("Invalid pending source declaration");
        monitorUrl.protocol = fetchUrl.protocol === "https:" ? "wss:" : "ws:";
        let monitor = this.availability.get(monitorUrl.href);
        if (!monitor) {
          monitor = new SourceAvailability(monitorUrl);
          this.availability.set(monitorUrl.href, monitor);
        }
        clearTimeout(request.timer);
        request.waiting = true;
        this.schedulePump();
        await monitor.wait(declaration.source, request.controller.signal);
        if (this.isCurrent(request)) {
          request.waiting = false;
          request.started = false;
        }
        return;
      }
      if (!response.ok) {
        await response.body?.cancel();
        throw new Error(`Resource fetch failed: ${response.status}`);
      }
      const etag = response.headers.get("etag");
      if (pinned && etag !== pinned) {
        await response.body?.cancel();
        throw new Error("Resource source changed; immutable recovery refused");
      }
      if (!response.body) throw new Error("Resource response is empty");
      if (!this.isCurrent(request)) {
        await response.body.cancel();
        return;
      }
      if (!request.recovery) {
        this.validators.set(
          request.source,
          etag && !etag.startsWith("W/") ? etag : null,
        );
      }
      // BYOB bounds application-owned chunks instead of retaining arbitrarily
      // sized response chunks. No following read starts until this one drains.
      request.reader = response.body.getReader({ mode: "byob" });
      void this.readNext(request);
    } catch (error) {
      if (this.isCurrent(request))
        request.error = error instanceof Error ? error.message : String(error);
    } finally {
      this.schedulePump();
    }
  }

  private async readNext(request: Acquisition): Promise<void> {
    request.reading = true;
    this.awaitProgress(request);
    try {
      const result = await request.reader!.read(new Uint8Array(CHUNK_BYTES));
      if (!this.isCurrent(request)) return;
      if (result.value?.byteLength) {
        request.received += result.value.byteLength;
        request.chunk = result.value;
      }
      if (result.done) request.done = true;
    } catch (error) {
      if (this.isCurrent(request))
        request.error = error instanceof Error ? error.message : String(error);
    } finally {
      request.reading = false;
      // A ready chunk is intentional local backpressure, not network inactivity.
      clearTimeout(request.timer);
      this.schedulePump();
    }
  }

  private copyInput(bytes: Uint8Array<ArrayBuffer>): void {
    // Wasm i32 results are signed in JS even for addresses above 2 GiB.
    const pointer =
      this.runtime.ipp_resource_input_reserve(bytes.byteLength) >>> 0;
    if (pointer === 0) throw new Error("Resource input reservation failed");
    new Uint8Array(this.runtime.memory.buffer, pointer, bytes.byteLength).set(
      bytes,
    );
  }

  private check(result: number): void {
    if (result !== 1) throw new Error(new TextDecoder().decode(this.output()));
  }

  private output(): Uint8Array<ArrayBuffer> {
    const length = this.runtime.ipp_output_len() >>> 0;
    if (length > 1_048_576) throw new Error("Resource output exceeds bounds");
    return new Uint8Array(
      this.runtime.memory.buffer,
      this.runtime.ipp_output_ptr() >>> 0,
      length,
    ).slice();
  }
}

function isPendingSourceDeclaration(
  value: unknown,
): value is { source: string; monitor: string } {
  if (!value || typeof value !== "object") return false;
  const declaration = value as Record<string, unknown>;
  return (
    typeof declaration.source === "string" &&
    typeof declaration.monitor === "string"
  );
}
