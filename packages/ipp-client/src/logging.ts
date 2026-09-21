/** Host diagnostics are independent of semantic protocol events. */
export const LOG_LEVELS = [
  "off",
  "error",
  "warn",
  "info",
  "debug",
  "trace",
] as const;

export type LogLevel = (typeof LOG_LEVELS)[number];
type Detail = string | number | bigint | boolean | undefined;

export function logLevelValue(level: unknown = "info"): number {
  const value = LOG_LEVELS.indexOf(level as LogLevel);
  if (value < 0)
    throw new RangeError(`Unknown IPP log level: ${String(level)}`);
  return value;
}

/** A console sink must never change runtime success or failure. */
export function writeDiagnostic(level: number, message: string): void {
  const method = LOG_LEVELS[level];
  if (!method || method === "off") return;
  try {
    // console.trace captures a stack; trace here means verbosity, not a stack dump.
    console[method === "trace" ? "debug" : method](message);
  } catch {}
}

export class DiagnosticLogger {
  private readonly threshold: number;

  constructor(
    private readonly scope: string,
    level: LogLevel = "info",
  ) {
    this.threshold = logLevelValue(level);
  }

  enabled(level: Exclude<LogLevel, "off">): boolean {
    return logLevelValue(level) <= this.threshold;
  }

  log(
    level: Exclude<LogLevel, "off">,
    event: string,
    details?: () => Record<string, Detail>,
  ): void {
    if (!this.enabled(level)) return;
    const fields = Object.entries(details?.() ?? {})
      .filter(([, value]) => value !== undefined)
      .map(([key, value]) => `${key}=${String(value)}`);
    writeDiagnostic(
      logLevelValue(level),
      `[IPP ${this.scope}] ${event}${fields.length ? ` ${fields.join(" ")}` : ""}`,
    );
  }

  /** Preserve useful existing diagnostics while applying the same level filter. */
  message(level: Exclude<LogLevel, "off">, message: () => string): void {
    if (this.enabled(level)) writeDiagnostic(logLevelValue(level), message());
  }
}
