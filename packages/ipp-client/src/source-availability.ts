type SourceWaiter = {
  resolve(): void;
  reject(error: Error): void;
  pending(): void;
};

/** Source-provider status channel, independent of World commands and evaluation. */
export class SourceAvailability {
  private readonly socket: WebSocket;
  private readonly waits = new Map<string, Set<SourceWaiter>>();
  private failure?: Error;

  constructor(private readonly url: URL) {
    this.socket = new WebSocket(url);
    this.socket.onopen = () => {
      for (const source of this.waits.keys()) this.send(source, true);
    };
    this.socket.onmessage = (event) => this.receive(event.data);
    this.socket.onerror = () =>
      this.close(new Error("Source availability connection failed"));
    this.socket.onclose = () =>
      this.close(new Error("Source producer disconnected"));
  }

  wait(source: string, signal: AbortSignal): Promise<void> {
    if (this.failure) return Promise.reject(this.failure);
    if (signal.aborted) return Promise.reject(asError(signal.reason));
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined = setTimeout(
        () =>
          waiter.reject(
            new Error("Source availability made no progress for 30 seconds"),
          ),
        30_000,
      );
      const remove = () => {
        clearTimeout(timer);
        timer = undefined;
        signal.removeEventListener("abort", abort);
        const waits = this.waits.get(source);
        if (!waits) return;
        waits.delete(waiter);
        if (waits.size === 0) {
          this.waits.delete(source);
          this.send(source, false);
        }
      };
      const waiter: SourceWaiter = {
        // A declared pending source is intentionally idle and owns no active
        // transfer slot, so network inactivity does not apply while pending.
        pending: () => {
          clearTimeout(timer);
          timer = undefined;
        },
        resolve: () => {
          remove();
          resolve();
        },
        reject: (error) => {
          remove();
          reject(error);
        },
      };
      const abort = () => waiter.reject(asError(signal.reason));
      const waits = this.waits.get(source) ?? new Set<SourceWaiter>();
      const first = waits.size === 0;
      waits.add(waiter);
      this.waits.set(source, waits);
      signal.addEventListener("abort", abort, { once: true });
      if (first) this.send(source, true);
    });
  }

  private receive(data: unknown): void {
    try {
      const message: unknown = JSON.parse(String(data));
      if (!isAvailabilityMessage(message, this.url))
        throw new Error("Invalid source availability message");
      for (const asset of message.assets) {
        const waits = this.waits.get(asset.source);
        if (!waits) continue;
        if (asset.state === "pending") {
          for (const waiter of waits) waiter.pending();
        } else if (asset.state === "ready") {
          for (const waiter of [...waits]) waiter.resolve();
        } else if (asset.state === "failed") {
          for (const waiter of [...waits])
            waiter.reject(new Error(asset.error));
        }
      }
    } catch (error) {
      this.close(asError(error));
    }
  }

  private send(source: string, watch: boolean): void {
    if (this.socket.readyState === WebSocket.OPEN)
      this.socket.send(JSON.stringify({ source, watch }));
  }

  close(error = new Error("Source provider closed")): void {
    if (this.failure) return;
    this.failure = error;
    for (const waits of [...this.waits.values()])
      for (const waiter of [...waits]) waiter.reject(error);
    if (
      this.socket.readyState === WebSocket.OPEN ||
      this.socket.readyState === WebSocket.CONNECTING
    )
      this.socket.close();
  }
}

type AvailabilityMessage = {
  session: string;
  assets: (
    | { source: string; state: "pending" | "ready" }
    | { source: string; state: "failed"; error: string }
  )[];
};

function isAvailabilityMessage(
  value: unknown,
  url: URL,
): value is AvailabilityMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  if (
    message.session !== url.searchParams.get("session") ||
    !Array.isArray(message.assets) ||
    message.assets.length > 128
  )
    return false;
  return message.assets.every((value) => {
    if (!value || typeof value !== "object") return false;
    const asset = value as Record<string, unknown>;
    if (
      typeof asset.source !== "string" ||
      (asset.state !== "pending" &&
        asset.state !== "ready" &&
        asset.state !== "failed")
    )
      return false;
    return asset.state !== "failed" || typeof asset.error === "string";
  });
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
