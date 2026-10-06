import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { readFile, stat } from "node:fs/promises";
import {
  createServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from "node:http";
import { createConnection } from "node:net";
import {
  dirname,
  extname,
  isAbsolute,
  join,
  relative,
  resolve,
  sep,
} from "node:path";
import { mkdir, mkdtemp } from "node:fs/promises";
import {
  chromium,
  type Browser,
  type BrowserContext,
  type Page,
} from "playwright";
import type { HarnessDriver, ScenarioContext } from "./driver.js";
import { EvidenceRecorder } from "./evidence.js";
import type { BrowserRuntimeConfiguration } from "./drivers/browser-runtime.js";
import { BrowserDriverFactory } from "./drivers/browser-worker.js";
import { browserLaunchOptions } from "#ipp-browser-options";

const DEFAULT_OPERATION_TIMEOUT_MS = 5_000;
const DEFAULT_CLOSE_TIMEOUT_MS = 3_000;

export interface BrowserBuildConfiguration {
  readonly name:
    | "headless"
    | "render"
    | "render-instrumentation"
    | "world-host"
    | "gles";
  readonly generatedModule: string;
  readonly runtimeWasm: string;
  readonly contractArtifact: string;
}

export interface BrowserHarnessConfiguration {
  readonly workspace: string;
  readonly build: BrowserBuildConfiguration;
  readonly operationTimeoutMs?: number;
  readonly closeTimeoutMs?: number;
  readonly evidenceParent?: string;
  readonly deviceScaleFactor?: number;
  readonly hasTouch?: boolean;
  /** Whether the build presents frames; defaults to render-prefixed build names. */
  readonly rendering?: boolean;
  /** Enable shared generated buffers in an explicitly isolated worker environment. */
  readonly crossOriginIsolation?: boolean;
  readonly logLevel?: "trace" | "debug" | "info" | "warn" | "error" | "off";
  /** Gate actual HTTP responses; aborts when the requesting client disconnects. */
  readonly beforeArtifactResponse?: (
    url: URL,
    signal: AbortSignal,
  ) => Promise<void>;
  /** Gate transport chunks by byte offset, including abort on reader cancellation. */
  readonly beforeArtifactChunk?: (
    url: URL,
    offset: number,
    signal: AbortSignal,
  ) => Promise<void>;
  /** Observe real startup boundaries, for cancellation/failure scenarios. */
  readonly onStartup?: (
    stage: "server-ready" | "browser-launching",
    origin: string,
  ) => void;
}

export interface BrowserConsoleEntry {
  readonly kind: "console";
  readonly level: string;
  readonly text: string;
}

export interface BrowserUrls {
  readonly origin: string;
  readonly runtimeModule: string;
  readonly workerScript: string;
  readonly transportModule: string;
  readonly wasm: string;
  readonly generated: string;
  /** A client generated for the other host target, whose contract never matches. */
  readonly mismatchGenerated: string;
  readonly missingWasm: string;
  readonly invalidWasm: string;
}

export interface BrowserScenarioContext extends ScenarioContext {
  readonly page: Page;
  readonly factory: BrowserDriverFactory;
  readonly urls: BrowserUrls;
  browserLog(): readonly BrowserConsoleEntry[];
}

/** Browser/process lifecycle without choosing a scene client or transport driver. */
export interface BrowserEnvironmentContext
  extends Omit<BrowserScenarioContext, "driver" | "factory" | "connectFresh"> {
  own(resource: Pick<HarnessDriver, "close">): void;
}

export interface BrowserScenarioResult<T> {
  readonly value: T;
  readonly evidenceDirectory: string;
  readonly origin: string;
}

export class BrowserHarnessRunError extends Error {
  readonly evidenceDirectory: string;
  readonly origin: string;

  constructor(
    message: string,
    evidenceDirectory: string,
    origin: string,
    cause: unknown,
  ) {
    super(message, { cause });
    this.name = "BrowserHarnessRunError";
    this.evidenceDirectory = evidenceDirectory;
    this.origin = origin;
  }
}

class LoopbackArtifactServer {
  readonly #workspace: string;
  readonly #evidence: EvidenceRecorder;
  #server: Server | undefined;
  #listening: Promise<void> | undefined;

  constructor(
    workspace: string,
    evidence: EvidenceRecorder,
    private readonly beforeResponse?: BrowserHarnessConfiguration["beforeArtifactResponse"],
    private readonly beforeChunk?: BrowserHarnessConfiguration["beforeArtifactChunk"],
    private readonly isolated = false,
  ) {
    this.#workspace = workspace;
    this.#evidence = evidence;
  }

  async start(): Promise<string> {
    const server = createServer((request, response) => {
      if (this.isolated) {
        response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
        response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
      }
      void this.#respond(request, response).catch((error: unknown) => {
        void this.#evidence.record("artifact_server_error", { error });
        if (!response.headersSent) response.writeHead(500);
        response.end("Internal server error\n");
      });
    });
    this.#server = server;
    this.#listening = new Promise<void>((resolvePromise, reject) => {
      const failed = (error: Error): void => reject(error);
      server.once("error", failed);
      server.listen(0, "127.0.0.1", () => {
        server.off("error", failed);
        resolvePromise();
      });
    });
    await this.#listening;
    const address = server.address();
    if (address === null || typeof address === "string") {
      throw new Error("artifact server did not bind a TCP port");
    }
    const origin = `http://127.0.0.1:${address.port}`;
    await this.#evidence.record("artifact_server_ready", { origin });
    return origin;
  }

  async close(timeoutMs: number): Promise<void> {
    const server = this.#server;
    if (server === undefined) return;
    this.#server = undefined;
    // A cancellation may arrive while listen() is still completing.
    await this.#listening?.catch(() => undefined);
    if (!server.listening) return;
    server.closeAllConnections();
    await withTimeout(
      new Promise<void>((resolvePromise, reject) => {
        server.close((error) => {
          if (error === undefined) resolvePromise();
          else reject(error);
        });
      }),
      timeoutMs,
      "artifact server close",
    );
  }

  async #respond(
    request: IncomingMessage,
    response: ServerResponse,
  ): Promise<void> {
    const method = request.method ?? "GET";
    const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
    await this.#evidence.record("artifact_request", {
      method,
      path: requestUrl.pathname,
    });
    if (this.beforeResponse) {
      const controller = new AbortController();
      const closed = (): void => controller.abort();
      response.once("close", closed);
      try {
        await Promise.race([
          this.beforeResponse(requestUrl, controller.signal),
          new Promise<void>((done) => {
            controller.signal.addEventListener("abort", () => done(), {
              once: true,
            });
          }),
        ]);
      } finally {
        response.off("close", closed);
      }
      if (response.destroyed) return;
    }
    if (method !== "GET" && method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" });
      response.end();
      return;
    }
    if (requestUrl.pathname === "/favicon.ico") {
      response.writeHead(204);
      response.end();
      return;
    }
    if (requestUrl.pathname === "/") {
      send(
        response,
        method,
        200,
        "text/html; charset=utf-8",
        Buffer.from(
          '<!doctype html><meta charset="utf-8"><title>IPP browser harness</title>',
        ),
      );
      return;
    }
    if (requestUrl.pathname === "/__fixtures__/invalid.wasm") {
      send(
        response,
        method,
        200,
        "application/wasm",
        Buffer.from("not a WebAssembly module"),
      );
      return;
    }

    let pathname: string;
    try {
      pathname = decodeURIComponent(requestUrl.pathname);
    } catch {
      response.writeHead(400);
      response.end("Invalid path\n");
      return;
    }
    const file = resolve(this.#workspace, `.${pathname}`);
    const relativePath = relative(this.#workspace, file);
    if (
      relativePath === "" ||
      relativePath.startsWith(`..${sep}`) ||
      relativePath === ".." ||
      isAbsolute(relativePath)
    ) {
      response.writeHead(404);
      response.end("Not found\n");
      return;
    }
    try {
      const contents = await readFile(file);
      if (!this.beforeChunk || method === "HEAD") {
        send(response, method, 200, contentType(file), contents);
      } else {
        const controller = new AbortController();
        const closed = (): void => controller.abort();
        response.once("close", closed);
        response.writeHead(200, artifactHeaders(contentType(file), contents));
        const aborted = new Promise<void>((done) =>
          controller.signal.addEventListener("abort", () => done(), {
            once: true,
          }),
        );
        try {
          for (let offset = 0; offset < contents.length; offset += 64 << 10) {
            await Promise.race([
              this.beforeChunk(requestUrl, offset, controller.signal),
              aborted,
            ]);
            if (response.destroyed) return;
            if (
              !response.write(contents.subarray(offset, offset + (64 << 10)))
            ) {
              await once(response, "drain", { signal: controller.signal });
            }
          }
          response.end();
        } catch (error) {
          if (!controller.signal.aborted) throw error;
        } finally {
          response.off("close", closed);
        }
      }
    } catch (error) {
      if (isNodeError(error) && error.code === "ENOENT") {
        response.writeHead(404, {
          "Content-Type": "text/plain; charset=utf-8",
        });
        response.end("Not found\n");
        return;
      }
      throw error;
    }
  }
}

class BrowserEnvironment {
  readonly #configuration: BrowserHarnessConfiguration;
  readonly #rendering: boolean;
  readonly #evidence: EvidenceRecorder;
  readonly #server: LoopbackArtifactServer;
  readonly #drivers: Pick<HarnessDriver, "close">[] = [];
  readonly #browserLog: unknown[] = [];
  readonly #onBrowserLost: (error: Error) => void;
  #browser: Browser | undefined;
  #context: BrowserContext | undefined;
  #page: Page | undefined;
  #launching: Promise<Browser> | undefined;
  #stopped = false;
  origin = "";
  /** Set when Chromium disconnects before this environment stops. */
  browserLost: Error | undefined;

  constructor(
    configuration: BrowserHarnessConfiguration,
    evidence: EvidenceRecorder,
    onBrowserLost: (error: Error) => void,
  ) {
    this.#configuration = configuration;
    this.#rendering =
      configuration.rendering ?? configuration.build.name.startsWith("render");
    this.#evidence = evidence;
    this.#onBrowserLost = onBrowserLost;
    this.#server = new LoopbackArtifactServer(
      configuration.workspace,
      evidence,
      configuration.beforeArtifactResponse,
      configuration.beforeArtifactChunk,
      configuration.crossOriginIsolation,
    );
  }

  async start(signal: AbortSignal): Promise<{
    readonly page: Page;
    readonly urls: BrowserUrls;
  }> {
    const checkActive = () => {
      signal.throwIfAborted();
      if (this.#stopped) throw new Error("Browser environment is stopped");
    };
    checkActive();
    await this.#captureBuildIdentity();
    checkActive();
    const origin = await this.#server.start();
    this.origin = origin;
    this.#configuration.onStartup?.("server-ready", origin);
    checkActive();
    this.#launching = chromium.launch({
      ...browserLaunchOptions(this.#rendering),
      timeout:
        this.#configuration.operationTimeoutMs ?? DEFAULT_OPERATION_TIMEOUT_MS,
    });
    this.#configuration.onStartup?.("browser-launching", origin);
    const browser = await this.#launching;
    this.#browser = browser;
    browser.on("disconnected", () => {
      this.#browserLog.push({ kind: "browser_disconnected" });
      if (this.#stopped) return;
      this.browserLost = new Error("Chromium disconnected during the scenario");
      this.#onBrowserLost(this.browserLost);
    });
    checkActive();
    const context = await browser.newContext({
      ...(this.#configuration.deviceScaleFactor === undefined
        ? {}
        : { deviceScaleFactor: this.#configuration.deviceScaleFactor }),
      ...(this.#configuration.hasTouch === undefined
        ? {}
        : { hasTouch: this.#configuration.hasTouch }),
    });
    this.#context = context;
    checkActive();
    const page = await context.newPage();
    this.#page = page;
    checkActive();
    page.on("console", (message) => {
      this.#browserLog.push({
        kind: "console",
        level: message.type(),
        text: message.text(),
      });
    });
    page.on("pageerror", (error) => {
      this.#browserLog.push({ kind: "page_error", error });
    });
    page.on("worker", (worker) => {
      const entry = { kind: "worker_started", url: worker.url() };
      this.#browserLog.push(entry);
      void this.#evidence.record("browser_worker_started", entry);
      worker.on("close", () => {
        const closed = { kind: "worker_closed", url: worker.url() };
        this.#browserLog.push(closed);
        void this.#evidence.record("browser_worker_closed", closed);
      });
    });
    await page.goto(origin, { waitUntil: "load" });
    const urls = this.#urls(origin);
    await this.#evidence.record("browser_ready", {
      chromium: browser.version(),
      launchOptions: browserLaunchOptions(this.#rendering),
      page: page.url(),
      urls,
      deviceScaleFactor: await page.evaluate(() => window.devicePixelRatio),
      renderingClaimed: this.#rendering,
    });
    return { page, urls };
  }

  own(driver: Pick<HarnessDriver, "close">): void {
    this.#drivers.push(driver);
  }

  browserLog(): readonly BrowserConsoleEntry[] {
    return this.#browserLog.filter(
      (entry): entry is BrowserConsoleEntry =>
        typeof entry === "object" &&
        entry !== null &&
        "kind" in entry &&
        entry.kind === "console",
    );
  }

  async stop(): Promise<readonly unknown[]> {
    this.#stopped = true;
    // A timed-out or cancelled launch still owns a future browser. Await that
    // bounded launch and close its result before reporting environment cleanup.
    if (!this.#browser && this.#launching) {
      this.#browser = await this.#launching.catch(() => undefined);
    }
    const failures: unknown[] = [];
    const closeTimeoutMs =
      this.#configuration.closeTimeoutMs ?? DEFAULT_CLOSE_TIMEOUT_MS;
    for (const driver of this.#drivers.reverse()) {
      try {
        await withTimeout(
          driver.close(),
          closeTimeoutMs,
          "browser client close",
        );
      } catch (error) {
        failures.push(error);
      }
    }
    for (const [label, close] of [
      ["page close", () => this.#page?.close()],
      ["browser context close", () => this.#context?.close()],
      ["browser close", () => this.#browser?.close()],
    ] as const) {
      try {
        await withTimeout(
          Promise.resolve(close()).then(() => undefined),
          closeTimeoutMs,
          label,
        );
      } catch (error) {
        failures.push(error);
      }
    }
    try {
      await this.#server.close(closeTimeoutMs);
    } catch (error) {
      failures.push(error);
    }
    try {
      await this.#evidence.writeJson("browser-log.json", this.#browserLog);
      await this.#evidence.record("browser_environment_stopped", {
        cleanupFailures: failures,
      });
    } catch (error) {
      failures.push(error);
    }
    return failures;
  }

  #urls(origin: string): BrowserUrls {
    const build = this.#configuration.build;
    const nativeGenerated = `${origin}/dist/target/integration-artifacts/client/generated.js`;
    return {
      origin,
      runtimeModule: workspaceUrl(
        origin,
        this.#configuration.workspace,
        join(
          this.#configuration.workspace,
          "dist/tests/harness/drivers/browser-runtime.js",
        ),
      ),
      workerScript: workspaceUrl(
        origin,
        this.#configuration.workspace,
        join(dirname(build.runtimeWasm), "wasm-worker.js"),
      ),
      transportModule: workspaceUrl(
        origin,
        this.#configuration.workspace,
        join(
          this.#configuration.workspace,
          "dist/packages/ipp-client/src/transport.js",
        ),
      ),
      wasm: workspaceUrl(
        origin,
        this.#configuration.workspace,
        build.runtimeWasm,
      ),
      generated: workspaceUrl(
        origin,
        this.#configuration.workspace,
        build.generatedModule,
      ),
      mismatchGenerated: nativeGenerated,
      missingWasm: `${origin}/target/browser-build/missing/runtime.wasm`,
      invalidWasm: `${origin}/__fixtures__/invalid.wasm`,
    };
  }

  async #captureBuildIdentity(): Promise<void> {
    const builds = await Promise.all(
      [this.#configuration.build].map(async (build) => ({
        name: build.name,
        generated: await fileIdentity(build.generatedModule),
        runtime: await fileIdentity(build.runtimeWasm),
        contract: await fileIdentity(build.contractArtifact),
      })),
    );
    await this.#evidence.writeJson("build-identity.json", {
      node: process.version,
      platform: process.platform,
      architecture: process.arch,
      playwright: "1.63.0",
      builds,
      renderingClaimed: this.#rendering,
    });
  }
}

export async function runBrowserScenario<T>(
  name: string,
  configuration: BrowserHarnessConfiguration,
  parentSignal: AbortSignal,
  scenario: (context: BrowserScenarioContext) => Promise<T>,
): Promise<BrowserScenarioResult<T>> {
  return runBrowserEnvironment(
    name,
    configuration,
    parentSignal,
    async (context) => {
      const { page, urls, evidence } = context;
      const origin = context.url;
      const operationTimeoutMs =
        configuration.operationTimeoutMs ?? DEFAULT_OPERATION_TIMEOUT_MS;
      const runtimeConfiguration = runtimeConfigurationFor(
        urls,
        urls.generated,
        urls.wasm,
        operationTimeoutMs,
        configuration.logLevel,
      );
      const mismatchConfiguration = runtimeConfigurationFor(
        urls,
        urls.mismatchGenerated,
        urls.wasm,
        operationTimeoutMs,
        configuration.logLevel,
      );
      const factory = new BrowserDriverFactory(
        page,
        urls.runtimeModule,
        runtimeConfiguration,
        mismatchConfiguration,
      );
      const connect = async (label: string): Promise<HarnessDriver> => {
        const driver = await withAbortAndTimeout(
          factory.connect(origin, {
            signal: context.signal,
            record: evidence.record.bind(evidence),
          }),
          context.signal,
          operationTimeoutMs,
          label,
        );
        context.own(driver);
        return driver;
      };
      const driver = await connect("browser client connect");
      return scenario({
        ...context,
        driver,
        factory,
        connectFresh: () =>
          context.execute("connect fresh browser session", { origin }, () =>
            connect("fresh browser client connect"),
          ),
        browserLog: () => context.browserLog(),
      });
    },
  );
}

export async function runBrowserEnvironment<T>(
  name: string,
  configuration: BrowserHarnessConfiguration,
  parentSignal: AbortSignal,
  scenario: (context: BrowserEnvironmentContext) => Promise<T>,
): Promise<BrowserScenarioResult<T>> {
  const evidenceParent =
    configuration.evidenceParent ??
    resolve(configuration.workspace, "target/integration-artifacts/browser");
  await mkdir(evidenceParent, { recursive: true });
  const evidenceDirectory = await mkdtemp(
    join(evidenceParent, `ipp-browser-${safeName(name)}-`),
  );
  const evidence = await EvidenceRecorder.create(evidenceDirectory);
  const scenarioController = new AbortController();
  // A lost browser cancels pending operations instead of leaving them to time out.
  const environment = new BrowserEnvironment(configuration, evidence, (error) =>
    scenarioController.abort(error),
  );
  const operationTimeoutMs =
    configuration.operationTimeoutMs ?? DEFAULT_OPERATION_TIMEOUT_MS;
  const onParentAbort = (): void =>
    scenarioController.abort(parentSignal.reason);
  if (parentSignal.aborted) onParentAbort();
  else parentSignal.addEventListener("abort", onParentAbort, { once: true });

  let origin = "";
  let result: BrowserScenarioResult<T> | undefined;
  let scenarioError: unknown;
  try {
    await evidence.record("browser_scenario_start", {
      name,
      build: configuration.build.name,
    });
    const { page, urls } = await withAbortAndTimeout(
      environment.start(scenarioController.signal),
      scenarioController.signal,
      operationTimeoutMs,
      "browser environment start",
    );
    origin = urls.origin;
    const execute = async <R>(
      label: string,
      input: unknown,
      action: () => Promise<R>,
    ): Promise<R> => {
      await evidence.record("browser_operation_input", { label, input });
      try {
        const value = await withAbortAndTimeout(
          action(),
          scenarioController.signal,
          operationTimeoutMs,
          label,
        );
        await evidence.record("browser_operation_result", { label, value });
        return value;
      } catch (error) {
        await evidence.record("browser_operation_error", { label, error });
        throw error;
      }
    };
    const value = await scenario({
      page,
      signal: scenarioController.signal,
      url: origin,
      evidence,
      urls,
      execute,
      own: (resource) => environment.own(resource),
      browserLog: () => environment.browserLog(),
    });
    await evidence.record("browser_scenario_success", { name });
    result = { value, evidenceDirectory, origin };
  } catch (error) {
    scenarioError =
      environment.browserLost === undefined || error === environment.browserLost
        ? error
        : new Error(environment.browserLost.message, { cause: error });
    try {
      await evidence.record("browser_scenario_failure", {
        name,
        error: scenarioError,
      });
      await evidence.writeJson("failure.json", { name, error: scenarioError });
    } catch (evidenceError) {
      scenarioError = new AggregateError(
        [error, evidenceError],
        "browser scenario and evidence recording failed",
      );
    }
  }

  scenarioController.abort(new Error("browser scenario cleanup"));
  parentSignal.removeEventListener("abort", onParentAbort);
  let cleanupFailures: readonly unknown[];
  try {
    cleanupFailures = await environment.stop();
    origin ||= environment.origin;
  } catch (error) {
    cleanupFailures = [error];
  }
  try {
    await evidence.flush();
  } catch (error) {
    cleanupFailures = [...cleanupFailures, error];
  }

  if (scenarioError !== undefined || cleanupFailures.length > 0) {
    throw new BrowserHarnessRunError(
      `browser scenario '${name}' failed; evidence retained at ${evidenceDirectory}`,
      evidenceDirectory,
      origin,
      scenarioError ??
        new AggregateError(
          cleanupFailures,
          "browser environment cleanup failed",
        ),
    );
  }
  if (result === undefined) {
    throw new BrowserHarnessRunError(
      `browser scenario '${name}' produced no result; evidence retained at ${evidenceDirectory}`,
      evidenceDirectory,
      origin,
      new Error("browser scenario produced no result"),
    );
  }
  return result;
}

export function runtimeConfigurationFor(
  urls: BrowserUrls,
  contractModuleUrl: string,
  wasmUrl: string,
  timeoutMs = DEFAULT_OPERATION_TIMEOUT_MS,
  logLevel?: BrowserHarnessConfiguration["logLevel"],
): BrowserRuntimeConfiguration {
  return {
    workerScriptUrl: urls.workerScript,
    transportModuleUrl: urls.transportModule,
    contractModuleUrl,
    wasmUrl,
    timeoutMs,
    ...(logLevel === undefined ? {} : { logLevel }),
  };
}

export async function assertLoopbackClosed(origin: string): Promise<void> {
  assert.notEqual(origin, "");
  const url = new URL(origin);
  await new Promise<void>((resolvePromise, reject) => {
    const socket = createConnection({
      host: url.hostname,
      port: Number(url.port),
    });
    socket.setTimeout(1_000);
    socket.once("connect", () => {
      socket.destroy();
      reject(new Error("owned artifact server still accepts connections"));
    });
    socket.once("timeout", () => {
      socket.destroy();
      reject(new Error("could not verify artifact server shutdown"));
    });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      socket.destroy();
      if (error.code === "ECONNREFUSED") resolvePromise();
      else reject(error);
    });
  });
}

async function fileIdentity(path: string): Promise<unknown> {
  const [metadata, contents] = await Promise.all([stat(path), readFile(path)]);
  return {
    path,
    bytes: metadata.size,
    modifiedAt: metadata.mtime.toISOString(),
    sha256: createHash("sha256").update(contents).digest("hex"),
  };
}

function workspaceUrl(origin: string, workspace: string, path: string): string {
  const relativePath = relative(workspace, path);
  if (
    relativePath === "" ||
    relativePath.startsWith(`..${sep}`) ||
    relativePath === ".." ||
    isAbsolute(relativePath)
  ) {
    throw new Error(`artifact is outside the workspace: ${path}`);
  }
  const encoded = relativePath
    .split(sep)
    .map((part) => encodeURIComponent(part))
    .join("/");
  return `${origin}/${encoded}`;
}

function send(
  response: ServerResponse,
  method: string,
  status: number,
  type: string,
  contents: Buffer,
): void {
  response.writeHead(status, artifactHeaders(type, contents));
  if (method === "HEAD") response.end();
  else response.end(contents);
}

function artifactHeaders(type: string, contents: Buffer) {
  return {
    "Cache-Control": "no-store",
    ETag: `"${createHash("sha256").update(contents).digest("hex")}"`,
    "Content-Length": contents.byteLength,
    "Content-Type": type,
    "Cross-Origin-Resource-Policy": "same-origin",
  };
}

function contentType(path: string): string {
  switch (extname(path)) {
    case ".js":
    case ".mjs":
      return "text/javascript; charset=utf-8";
    case ".html":
      return "text/html; charset=utf-8";
    case ".css":
      return "text/css; charset=utf-8";
    case ".png":
      return "image/png";
    case ".wasm":
      return "application/wasm";
    case ".json":
    case ".map":
      return "application/json; charset=utf-8";
    default:
      return "application/octet-stream";
  }
}

function safeName(name: string): string {
  return name
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-|-$/g, "");
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

async function withAbortAndTimeout<T>(
  promise: Promise<T>,
  signal: AbortSignal,
  timeoutMs: number,
  label: string,
): Promise<T> {
  signal.throwIfAborted();
  let rejectAbort = (_reason: unknown): void => undefined;
  const aborted = new Promise<never>((_resolve, reject) => {
    rejectAbort = reject;
  });
  const onAbort = (): void =>
    rejectAbort(signal.reason ?? new Error(`${label} cancelled`));
  signal.addEventListener("abort", onAbort, { once: true });
  let rejectTimeout = (_reason: unknown): void => undefined;
  const timedOut = new Promise<never>((_resolve, reject) => {
    rejectTimeout = reject;
  });
  const timer = setTimeout(
    () => rejectTimeout(new Error(`${label} timed out after ${timeoutMs}ms`)),
    timeoutMs,
  );
  timer.unref();
  try {
    return await Promise.race([promise, aborted, timedOut]);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", onAbort);
  }
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  let rejectTimeout = (_reason: unknown): void => undefined;
  const timedOut = new Promise<never>((_resolve, reject) => {
    rejectTimeout = reject;
  });
  const timer = setTimeout(
    () => rejectTimeout(new Error(`${label} timed out after ${timeoutMs}ms`)),
    timeoutMs,
  );
  timer.unref();
  try {
    return await Promise.race([promise, timedOut]);
  } finally {
    clearTimeout(timer);
  }
}
