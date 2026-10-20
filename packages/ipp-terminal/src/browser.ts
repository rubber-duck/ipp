import { openWorkerPort } from "@ipp/client";
import type { TerminalConfiguration } from "./types.js";
import type { TerminalWorkerStart } from "./worker.js";

export interface BrowserTerminal {
  readonly ready: Promise<void>;
  close(): Promise<void>;
}

/** Explicit shell startup. The same Host supplies an ordinary transferred connection. */
export function startBrowserTerminal(
  host: object,
  workerUrl: string | URL,
  generatedModuleUrl: string,
  configuration: TerminalConfiguration,
  onError: (error: Error) => void,
): BrowserTerminal {
  const endpoint = openWorkerPort(host);
  let worker: Worker;
  try {
    worker = new Worker(workerUrl, { type: "module", name: "ipp-terminal" });
  } catch (error) {
    endpoint.dispose();
    throw error;
  }
  let resolveReady!: () => void, rejectReady!: (error: Error) => void;
  const ready = new Promise<void>((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  let finishClose: (() => void) | undefined;
  let closing: Promise<void> | undefined;
  let terminated = false;
  const terminate = () => {
    if (terminated) return;
    terminated = true;
    worker.terminate();
    endpoint.dispose();
  };
  const fail = (error: Error) => {
    if (terminated) return;
    rejectReady(error);
    try {
      onError(error);
    } finally {
      terminate();
      finishClose?.();
    }
  };
  worker.addEventListener("error", (event) =>
    fail(new Error(event.message || "Terminal Worker failed")),
  );
  worker.addEventListener("messageerror", () =>
    fail(new Error("Terminal Worker message error")),
  );
  worker.addEventListener("message", (event) => {
    if (event.data.type === "ready") resolveReady();
    else if (event.data.type === "closed") finishClose?.();
    else if (event.data.type === "error") fail(new Error(event.data.error));
    else fail(new Error("Invalid terminal Worker response"));
  });
  const start: TerminalWorkerStart = {
    type: "start",
    port: endpoint.port,
    connection: endpoint.connection,
    generatedModuleUrl,
    configuration,
  };
  const timer = setTimeout(
    () => fail(new Error("Terminal Worker startup timed out")),
    30000,
  );
  void ready.finally(() => clearTimeout(timer)).catch(() => {});
  try {
    worker.postMessage(start, [endpoint.port]);
  } catch (error) {
    fail(error instanceof Error ? error : new Error(String(error)));
  }
  return {
    ready,
    close() {
      return (closing ??= (async () => {
        if (terminated) return;
        rejectReady(new Error("Terminal closed during startup"));
        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => {
            terminate();
            reject(new Error("Terminal Worker close timed out"));
          }, 10000);
          finishClose = () => {
            clearTimeout(timeout);
            resolve();
          };
          worker.postMessage({ type: "close" });
        }).finally(terminate);
      })());
    },
  };
}
