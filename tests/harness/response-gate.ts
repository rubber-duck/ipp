/** Controls the real artifact server, including observable client cancellation. */
export function responseGate() {
  let release!: () => void;
  let requested!: () => void;
  let aborted!: () => void;
  let count = 0;
  const gate = new Promise<void>((done) => {
    release = done;
  });
  const request = new Promise<void>((done) => {
    requested = done;
  });
  const abort = new Promise<void>((done) => {
    aborted = done;
  });
  return {
    requested: request,
    aborted: abort,
    release,
    count: () => count,
    hold(signal: AbortSignal): Promise<void> {
      count += 1;
      requested();
      if (signal.aborted) aborted();
      else signal.addEventListener("abort", aborted, { once: true });
      return gate;
    },
  };
}
