import type { LifecycleTargetWatch } from "@ipp/client";

export interface ReactFieldWatchLease {
  readonly admitted: Promise<LifecycleTargetWatch>;
  release(): Promise<void>;
}

interface FieldWatchRecord {
  watch?: LifecycleTargetWatch;
  admission: Promise<void>;
  requested: boolean;
  removed: boolean;
  release?: Promise<void> | undefined;
}

/** Root-owned native watch admissions and exact, retryable releases. */
export class ReactFieldWatches {
  private readonly records = new Set<FieldWatchRecord>();
  private closing = false;

  constructor(private readonly report: (error: unknown) => void) {}

  track(admission: Promise<LifecycleTargetWatch>): ReactFieldWatchLease {
    const record: FieldWatchRecord = {
      admission: Promise.resolve(),
      requested: this.closing,
      removed: false,
    };
    this.records.add(record);
    const admitted = admission.then(
      (watch) => {
        this.observe(record, watch);
        return watch;
      },
      (error: unknown) => {
        if (error && typeof error === "object" && "partial" in error) {
          const partial = error.partial as LifecycleTargetWatch | undefined;
          // Generated clients carry their own class identity; retain the exact
          // returned partial handle rather than relying on instanceof.
          if (
            partial &&
            typeof partial.remove === "function" &&
            partial.closed &&
            typeof partial.closed.then === "function"
          )
            this.observe(record, partial);
        }
        record.requested = true;
        throw error;
      },
    );
    record.admission = admitted.then(
      () => {},
      () => {},
    );
    void record.admission.then(() => {
      if (record.requested) void this.backgroundRelease(record);
    });
    return {
      admitted,
      release: () => {
        record.requested = true;
        return this.backgroundRelease(record);
      },
    };
  }

  private observe(record: FieldWatchRecord, watch: LifecycleTargetWatch): void {
    record.watch = watch;
    void watch.closed.then(() => {
      record.removed = true;
      this.records.delete(record);
    });
  }

  private backgroundRelease(record: FieldWatchRecord): Promise<void> {
    const operation = this.release(record);
    void operation.catch((error) => {
      if (!this.closing) this.report(error);
    });
    return operation;
  }

  private release(record: FieldWatchRecord): Promise<void> {
    if (record.removed) return Promise.resolve();
    if (record.release) return record.release;
    const operation = (async () => {
      await record.admission;
      if (record.removed) return;
      if (record.watch) await record.watch.remove();
      record.removed = true;
      this.records.delete(record);
    })();
    record.release = operation;
    void operation
      .finally(() => {
        if (record.release === operation) record.release = undefined;
      })
      .catch(() => {});
    return operation;
  }

  fence(): void {
    this.closing = true;
    for (const record of this.records) record.requested = true;
  }

  async dispose(): Promise<void> {
    this.fence();
    const results = await Promise.allSettled(
      [...this.records].map((record) => this.release(record)),
    );
    const failures = results.flatMap((result) =>
      result.status === "rejected" ? [result.reason as unknown] : [],
    );
    if (failures.length)
      throw new AggregateError(
        failures,
        "React field watch cleanup is incomplete",
      );
  }
}
