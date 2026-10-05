/** Observe errors without coupling headless declarations to Canvas sessions. */
export function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

export function notify(
  callback: () => void | Promise<void>,
  report: (error: Error) => void,
): void {
  try {
    void Promise.resolve(callback()).catch((error: unknown) =>
      report(asError(error)),
    );
  } catch (error) {
    report(asError(error));
  }
}
