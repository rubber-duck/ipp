import type { SharedBufferPublication } from "./buffer-source.js";
import { SourceAvailability } from "./source-availability.js";
import { resourceFetchUrl, type ResourceUrlMapping } from "./resource-urls.js";
/** Host I/O bridge. Only the synchronous host pump calls WASM exports. */
import { DiagnosticLogger, type LogLevel } from "./logging.js";
import type { IngressStatistics } from "./presentation.js";

const CHUNK_BYTES = 64 << 10;
const MAX_ACTIVE = 8;
const FETCH_INACTIVITY_MS = 30_000;

export interface AssetHostExports {
  readonly memory: WebAssembly.Memory;
  ipp_progress_resources(): number;
  ipp_service_resources(): number;
  ipp_resource_poll(): number;
  ipp_asset_error_max_bytes(): number;
  /** Rust-owned source staging, for ingress statistics. */
  ipp_resource_buffered_bytes(): number;
  ipp_resource_chunk_reserve(
    session: bigint,
    id: bigint,
    length: number,
  ): number;
  ipp_resource_chunk_ptr(): number;
  ipp_resource_chunk(session: bigint, id: bigint, length: number): number;
  ipp_resource_end(
    session: bigint,
    id: bigint,
    success: number,
    length: number,
  ): number;
  ipp_resource_input_reserve(length: number): number;
  ipp_buffer_source_register(session: bigint, length: number): number;
  ipp_buffer_source_revoke(session: bigint, length: number): number;
  ipp_output_ptr(): number;
  ipp_output_len(): number;
}

export interface AssetWorkerOptions {
  logLevel?: LogLevel;
  resourceUrls?: readonly ResourceUrlMapping[];
  failed?: (error: Error) => void;
  /** Called before the Host progresses resources. */
  prepareProgress?: () => void;
  /** Counters of a diagnostics build; without them the service measures nothing. */
  statistics?: IngressStatistics;
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
  chunk?: Uint8Array<ArrayBufferLike>;
  done?: boolean;
  error?: string;
  timer?: ReturnType<typeof setTimeout>;
  bufferSource?: {
    readonly bytes: Uint8Array<ArrayBufferLike>;
    readonly control?: Int32Array<SharedArrayBuffer>;
    readonly generation?: number;
  };
}

export class AssetWorkerService {
  private readonly requests = new Map<bigint, Acquisition>();
  // Strong HTTP validators pin recovery to the original content. Evicted or
  // absent validators make recovery fail explicitly instead of refreshing data.
  private readonly publishedBufferNames = new Set<string>();
  private readonly buffers = new Map<
    string,
    {
      readonly bytes: Uint8Array<ArrayBufferLike>;
      readonly control?: Int32Array<SharedArrayBuffer>;
      readonly generation?: number;
    }
  >();
  private readonly validators = new Map<string, string | null>();
  private readonly availability = new Map<string, SourceAvailability>();
  private closed = false;
  private pumpTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly logger: DiagnosticLogger;
  private readonly resourceUrls: readonly ResourceUrlMapping[];
  private readonly failed: (error: Error) => void;
  private readonly prepareProgress: () => void;
  private readonly statistics: IngressStatistics | undefined;

  constructor(
    private readonly runtime: AssetHostExports,
    private readonly session: bigint,
    options: AssetWorkerOptions = {},
  ) {
    this.logger = new DiagnosticLogger("resources", options.logLevel ?? "info");
    this.resourceUrls = options.resourceUrls ?? [];
    this.failed =
      options.failed ??
      ((error) => {
        throw error;
      });
    this.prepareProgress = options.prepareProgress ?? (() => {});
    this.statistics = options.statistics;
  }

  /** Install transferred ownership or a cooperatively sealed external SAB source. */
  mountBuffer(
    source: string,
    publication: ArrayBuffer | SharedBufferPublication,
  ): void {
    if (
      this.closed ||
      !/^js-buffer:[0-9a-f]{32}$/.test(source) ||
      this.publishedBufferNames.has(source)
    )
      throw new Error("Invalid or duplicate generated source");
    let control: Int32Array<SharedArrayBuffer> | undefined;
    let bytes: Uint8Array<ArrayBufferLike>;
    if (publication instanceof ArrayBuffer) bytes = new Uint8Array(publication);
    else {
      if (
        !(publication.buffer instanceof SharedArrayBuffer) ||
        !(publication.control instanceof SharedArrayBuffer) ||
        publication.control.byteLength !== 8
      )
        throw new Error("Invalid shared publication guard");
      if (
        !Number.isSafeInteger(publication.offset) ||
        !Number.isSafeInteger(publication.length) ||
        publication.offset < 0 ||
        publication.length < 0 ||
        publication.offset > publication.buffer.byteLength ||
        publication.length > publication.buffer.byteLength - publication.offset
      )
        throw new Error("Invalid shared publication range");
      control = new Int32Array(publication.control);
      if (
        !Number.isInteger(publication.generation) ||
        publication.generation <= 0 ||
        publication.generation > 0x7fffffff ||
        Atomics.load(control, 1) !== publication.generation
      )
        throw new Error("Stale shared publication generation");
      if (Atomics.compareExchange(control, 0, 2, 3) !== 2)
        throw new Error("Shared bytes have not been exclusively published");
      bytes = new Uint8Array(
        publication.buffer,
        publication.offset,
        publication.length,
      );
    }
    try {
      const name = new TextEncoder().encode(source);
      this.copyInput(name);
      this.check(
        this.runtime.ipp_buffer_source_register(this.session, name.length),
      );
      this.buffers.set(source, {
        bytes,
        ...(control
          ? {
              control,
              generation: (publication as SharedBufferPublication).generation,
            }
          : {}),
      });
      this.publishedBufferNames.add(source);
      if (this.statistics)
        this.statistics.sourceBackingBytes = this.sourceBackingBytes;
    } catch (error) {
      if (control) Atomics.store(control, 0, 0);
      throw error;
    }
  }

  /** Existing source cursors are fenced before releasing producer write access. */
  revokeBuffer(source: string): void {
    const buffer = this.buffers.get(source);
    if (!buffer) return;
    const name = new TextEncoder().encode(source);
    this.copyInput(name);
    this.prepareProgress();
    this.check(
      this.runtime.ipp_buffer_source_revoke(this.session, name.length),
    );
    for (const request of this.requests.values())
      if (request.source === source) this.release(request);
    this.buffers.delete(source);
    if (this.statistics)
      this.statistics.sourceBackingBytes = this.sourceBackingBytes;
    if (buffer.control) Atomics.store(buffer.control, 0, 0);
  }

  /** Original external storage is separate from bounded transport staging. */
  get sourceBackingBytes(): number {
    let bytes = 0;
    for (const source of this.buffers.values())
      bytes += source.bytes.buffer.byteLength;
    return bytes;
  }

  /** Real retained JS staging; each active reader holds at most one bounded chunk. */
  get bufferedBytes(): number {
    let bytes = 0;
    for (const request of this.requests.values())
      bytes += request.reading
        ? CHUNK_BYTES
        : request.bufferSource
          ? 0
          : (request.chunk?.buffer.byteLength ?? 0);
    return bytes;
  }

  pump(): void {
    if (this.closed) return;
    if (this.statistics) this.measureBuffers(this.statistics);
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
        const admission = this.runtime.ipp_resource_chunk_reserve(
          this.session,
          request.id,
          request.chunk.byteLength,
        );
        if (admission === 2) continue;
        if (admission === 3) {
          this.release(request);
          continue;
        }
        this.check(admission);
        const pointer = this.runtime.ipp_resource_chunk_ptr() >>> 0;
        // Reservation owns the original storage across memory.grow and JS entry.
        // Reacquire the current linear-memory buffer immediately before copying.
        new Uint8Array(
          this.runtime.memory.buffer,
          pointer,
          request.chunk.byteLength,
        ).set(request.chunk);
        this.check(
          this.runtime.ipp_resource_chunk(
            this.session,
            request.id,
            request.chunk.byteLength,
          ),
        );
        receivedInput = true;
        if (this.statistics)
          this.statistics.sourceBytes += request.chunk.byteLength;
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
      } else if (
        (request.reader || request.bufferSource) &&
        !request.reading &&
        !request.chunk
      ) {
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

  /** Presenting workers sample staging at each pump to record its peak. */
  private measureBuffers(statistics: IngressStatistics): void {
    statistics.sourceJsBufferedBytes = this.bufferedBytes;
    statistics.sourceRuntimeBufferedBytes =
      this.runtime.ipp_resource_buffered_bytes();
    statistics.sourceJsPeakBufferedBytes = Math.max(
      statistics.sourceJsPeakBufferedBytes ?? 0,
      statistics.sourceJsBufferedBytes,
    );
    statistics.sourceRuntimePeakBufferedBytes = Math.max(
      statistics.sourceRuntimePeakBufferedBytes ?? 0,
      statistics.sourceRuntimeBufferedBytes,
    );
    statistics.sourceBufferedBytes =
      statistics.sourceJsBufferedBytes + statistics.sourceRuntimeBufferedBytes;
    statistics.sourcePeakBufferedBytes = Math.max(
      statistics.sourcePeakBufferedBytes,
      statistics.sourceBufferedBytes,
    );
  }

  close(): void {
    this.closed = true;
    clearTimeout(this.pumpTimer);
    for (const request of this.requests.values()) this.release(request);
    this.validators.clear();
    for (const source of this.buffers.values())
      if (source.control) Atomics.store(source.control, 0, 0);
    this.buffers.clear();
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
    const buffer = this.buffers.get(request.source);
    if (buffer) {
      request.bufferSource = buffer;
      if (buffer.bytes.byteLength === 0) request.done = true;
      else void this.readNext(request);
      this.schedulePump();
      return;
    }
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
    if (request.bufferSource) {
      if (
        request.bufferSource.control &&
        (Atomics.load(request.bufferSource.control, 0) !== 3 ||
          Atomics.load(request.bufferSource.control, 1) !==
            request.bufferSource.generation)
      ) {
        request.error = "Shared publication guard changed before release";
        this.schedulePump();
        return;
      }
      const bytes = request.bufferSource.bytes;
      const end = Math.min(bytes.byteLength, request.received + CHUNK_BYTES);
      // This view allocates no backing and remains under the publication guard.
      request.chunk = bytes.subarray(request.received, end);
      request.received = end;
      request.done = end === bytes.byteLength;
      this.schedulePump();
      return;
    }
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

  private copyInput(bytes: Uint8Array<ArrayBufferLike>): void {
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
    // `ipp_protocol::MAX_MESSAGE_BYTES`, checked by `tools/check_repo.py`.
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
