import { spawn } from "node:child_process";
import {
  createServer,
  request as forwardRequest,
  type IncomingMessage,
  type ServerResponse,
} from "node:http";
import { createInterface } from "node:readline";
import type { BrowserEnvironmentContext } from "../browser/environment.js";
import { ownProcess } from "../integration/owned-processes.js";

export interface GalleryResponseOverride {
  readonly status: number;
  readonly body?: string;
}

export interface GalleryServerOptions {
  /** Gate or replace a response while preserving the production gallery server. */
  readonly beforeResponse?: (
    url: URL,
    signal: AbortSignal,
  ) => Promise<GalleryResponseOverride | void>;
}

/** Launch the same loopback server as the public example, with owned cleanup. */
export async function startGalleryServer(
  workspace: string,
  environment: BrowserEnvironmentContext,
  options: GalleryServerOptions = {},
): Promise<string> {
  environment.signal.throwIfAborted();
  const child = spawn(
    process.env.PYTHON_BIN ??
      (process.platform === "win32" ? "python" : "python3"),
    ["tools/ipp.py", "_operation", "serve", "gallery"],
    {
      cwd: workspace,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  ownProcess(child);
  const exited = new Promise<void>((done) => {
    child.once("close", (code, signal) => {
      void environment.evidence.record("gallery_server_exit", { code, signal });
      done();
    });
  });
  let proxy: { readonly origin: string; close(): Promise<void> } | undefined;
  environment.own({
    async close() {
      try {
        await proxy?.close();
      } finally {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill("SIGTERM");
        }
        const force = setTimeout(() => child.kill("SIGKILL"), 1000);
        try {
          await exited;
        } finally {
          clearTimeout(force);
        }
      }
    },
  });
  child.stderr.on("data", (data: Buffer) => {
    void environment.evidence.record("gallery_server_stderr", data.toString());
  });
  child.on("error", (error) => {
    void environment.evidence.record("gallery_server_error", error);
  });
  const lines = createInterface({ input: child.stdout });
  let cleanup = () => {};
  const ready = new Promise<string>((done, reject) => {
    const onLine = (line: string) => {
      void environment.evidence.record("gallery_server_stdout", line);
      const match = /^IPP scene gallery: (http:\/\/127\.0\.0\.1:\d+)\//.exec(
        line,
      );
      if (match?.[1]) done(match[1]);
    };
    const onExit = () =>
      reject(new Error("Gallery server exited before ready"));
    lines.on("line", onLine);
    child.once("error", reject);
    child.once("close", onExit);
    cleanup = () => {
      lines.close();
      child.off("error", reject);
      child.off("close", onExit);
    };
  });
  try {
    const origin = await environment.execute(
      "start gallery server",
      {},
      () => ready,
    );
    proxy = options.beforeResponse
      ? await startResponseProxy(origin, options.beforeResponse, environment)
      : undefined;
    return proxy?.origin ?? origin;
  } finally {
    cleanup();
  }
}

async function startResponseProxy(
  upstream: string,
  beforeResponse: NonNullable<GalleryServerOptions["beforeResponse"]>,
  environment: BrowserEnvironmentContext,
): Promise<{ readonly origin: string; close(): Promise<void> }> {
  const server = createServer((request, response) => {
    void proxyResponse(upstream, request, response, beforeResponse).catch(
      (failure: unknown) => {
        void environment.evidence.record("gallery_proxy_error", {
          error: failure instanceof Error ? failure.message : String(failure),
        });
        if (!response.headersSent) response.writeHead(502);
        response.end("Gallery proxy failed\n");
      },
    );
  });
  await new Promise<void>((resolve, reject) => {
    const failed = (error: Error) => reject(error);
    server.once("error", failed);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", failed);
      resolve();
    });
  });
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("Gallery response proxy did not bind a port");
  return {
    origin: `http://127.0.0.1:${address.port}`,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      });
    },
  };
}

async function proxyResponse(
  upstream: string,
  incoming: IncomingMessage,
  outgoing: ServerResponse,
  beforeResponse: NonNullable<GalleryServerOptions["beforeResponse"]>,
): Promise<void> {
  const url = new URL(incoming.url ?? "/", upstream);
  const controller = new AbortController();
  const cancel = () => controller.abort();
  incoming.once("aborted", cancel);
  outgoing.once("close", cancel);
  try {
    const replacement = await beforeResponse(url, controller.signal);
    if (controller.signal.aborted) return;
    if (replacement) {
      const body = Buffer.from(replacement.body ?? "Unavailable\n");
      outgoing.writeHead(replacement.status, {
        "Content-Type": "text/plain; charset=utf-8",
        "Content-Length": body.byteLength,
        "Cache-Control": "no-store",
      });
      if (incoming.method === "HEAD") outgoing.end();
      else outgoing.end(body);
      return;
    }
    await new Promise<void>((resolve, reject) => {
      const forwarded = forwardRequest(
        url,
        { method: incoming.method, headers: incoming.headers },
        (response) => {
          outgoing.writeHead(response.statusCode ?? 502, response.headers);
          response.pipe(outgoing);
          response.once("end", resolve);
          response.once("error", reject);
        },
      );
      controller.signal.addEventListener(
        "abort",
        () => forwarded.destroy(new Error("Gallery response cancelled")),
        { once: true },
      );
      forwarded.once("error", (error) => {
        if (controller.signal.aborted) resolve();
        else reject(error);
      });
      incoming.pipe(forwarded);
    });
  } finally {
    incoming.off("aborted", cancel);
    outgoing.off("close", cancel);
  }
}
