/**
 * Assertions and bounded polling for scenarios and pages. This module has no
 * imports, so it bundles into browser pages and runs in Node unchanged.
 */

export function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

/**
 * Read until `ready` accepts a value. Every call reads at least once; after
 * `timeoutMs` (10 s by default) it fails with `describe(last)`, by default
 * `${label} timed out`. Reads run back to back unless `intervalMs` asks for a
 * pause between them.
 */
export async function until<T>(
  read: () => Promise<T>,
  ready: (value: T) => boolean,
  label: string,
  options: {
    readonly timeoutMs?: number;
    readonly intervalMs?: number;
    readonly describe?: (last: T) => string;
  } = {},
): Promise<T> {
  const deadline = performance.now() + (options.timeoutMs ?? 10_000);
  for (;;) {
    const last = await read();
    if (ready(last)) return last;
    if (performance.now() >= deadline)
      throw new Error(options.describe?.(last) ?? `${label} timed out`);
    if (options.intervalMs)
      await new Promise((resolve) => setTimeout(resolve, options.intervalMs));
  }
}
