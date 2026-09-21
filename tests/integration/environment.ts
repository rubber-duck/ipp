import type { ChildProcessByStdio } from "node:child_process";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream, createWriteStream } from "node:fs";
import { copyFile, mkdir, mkdtemp, stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import type { Readable } from "node:stream";
import { Transform } from "node:stream";
import { finished } from "node:stream/promises";
import type { HarnessDriver, HarnessDriverFactory } from "./driver.js";
import { EvidenceRecorder } from "./evidence.js";

const DEFAULT_READINESS_TIMEOUT_MS = 5_000;
const DEFAULT_OPERATION_TIMEOUT_MS = 5_000;
const DEFAULT_CLOSE_TIMEOUT_MS = 2_000;
const DEFAULT_LOG_LIMIT_BYTES = 256 * 1024;
const MAXIMUM_READINESS_LINE_BYTES = 64 * 1024;

export interface NativeServerConfiguration {
  readonly executable: string;
  readonly schemaArtifact: string;
  readonly workingDirectory: string;
  readonly extraArguments?: readonly string[];
  readonly readinessTimeoutMs?: number;
  readonly operationTimeoutMs?: number;
  readonly closeTimeoutMs?: number;
  readonly logLimitBytes?: number;
  readonly evidenceParent?: string;
  readonly environment?: Readonly<Record<string, string>>;
}

export interface NativeEnvironmentContext {
  readonly signal: AbortSignal;
  readonly url: string;
  readonly evidence: EvidenceRecorder;
  track<T extends Pick<HarnessDriver, "close">>(
    pending: Promise<T>,
  ): Promise<T>;
  execute<T>(
    label: string,
    input: unknown,
    action: () => Promise<T>,
  ): Promise<T>;
}

export interface ScenarioContext
  extends Omit<NativeEnvironmentContext, "track"> {
  readonly driver: HarnessDriver;
  connectFresh(): Promise<HarnessDriver>;
}

export interface ScenarioResult<T> {
  readonly value: T;
  readonly evidenceDirectory: string;
}

export class HarnessRunError extends Error {
  readonly evidenceDirectory: string;

  constructor(message: string, evidenceDirectory: string, cause: unknown) {
    super(message, { cause });
    this.name = "HarnessRunError";
    this.evidenceDirectory = evidenceDirectory;
  }
}

interface Readiness {
  readonly event: "ready";
  readonly url: string;
}

interface OwnedResource {
  close(): Promise<void>;
}

type NativeServerProcess = ChildProcessByStdio<null, Readable, Readable>;

class CappedLog extends Transform {
  readonly #maximumBytes: number;
  #writtenBytes = 0;
  #marked = false;

  constructor(maximumBytes: number) {
    super();
    this.#maximumBytes = maximumBytes;
  }

  override _transform(
    chunk: Buffer,
    _encoding: BufferEncoding,
    callback: (error?: Error | null) => void,
  ): void {
    let truncated = false;
    if (this.#writtenBytes < this.#maximumBytes) {
      const remaining = this.#maximumBytes - this.#writtenBytes;
      const selected = chunk.subarray(0, remaining);
      this.push(selected);
      this.#writtenBytes += selected.byteLength;
      truncated = selected.byteLength < chunk.byteLength;
    } else {
      truncated = chunk.byteLength > 0;
    }
    if (!this.#marked && truncated) {
      this.#marked = true;
      this.push("\n[IPP harness log truncated]\n");
    }
    callback();
  }
}

class NativeServerEnvironment {
  readonly #configuration: NativeServerConfiguration;
  readonly #evidence: EvidenceRecorder;
  readonly #resources: OwnedResource[] = [];
  readonly #pendingResources: Promise<null>[] = [];
  readonly #logCompletions: Promise<unknown | null>[] = [];
  readonly #lateCloses: Promise<unknown | null>[] = [];
  #child: NativeServerProcess | null = null;
  #exit: { code: number | null; signal: NodeJS.Signals | null } | null = null;
  #closing = false;

  constructor(
    configuration: NativeServerConfiguration,
    evidence: EvidenceRecorder,
  ) {
    this.#configuration = configuration;
    this.#evidence = evidence;
  }

  async start(signal: AbortSignal): Promise<string> {
    signal.throwIfAborted();
    await this.#captureBuildIdentity();
    const arguments_ = [
      "--bind",
      "127.0.0.1:0",
      ...(this.#configuration.extraArguments ?? []),
    ];
    await this.#evidence.record("server_spawn", {
      executable: this.#configuration.executable,
      arguments: arguments_,
      workingDirectory: this.#configuration.workingDirectory,
      environment: Object.keys(this.#configuration.environment ?? {}).sort(),
    });
    signal.throwIfAborted();

    const child = spawn(this.#configuration.executable, arguments_, {
      cwd: this.#configuration.workingDirectory,
      env: { ...process.env, ...this.#configuration.environment },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    this.#child = child;
    child.once("exit", (code, exitSignal) => {
      this.#exit = { code, signal: exitSignal };
    });
    this.#captureLog(child.stdout, "server-stdout.log");
    this.#captureLog(child.stderr, "server-stderr.log");

    const readiness = await waitForReadiness(
      child,
      signal,
      this.#configuration.readinessTimeoutMs ?? DEFAULT_READINESS_TIMEOUT_MS,
    );
    const url = validateReadiness(readiness);
    await this.#evidence.record("server_ready", readiness);
    return url;
  }

  own(resource: OwnedResource): void {
    if (!this.#closing) {
      this.#resources.push(resource);
      return;
    }
    this.#lateCloses.push(
      this.#closeResource(resource, "late socket close").then(
        () => null,
        (error: unknown) => error,
      ),
    );
  }

  track<T extends OwnedResource>(pending: Promise<T>): Promise<T> {
    const tracked = pending.then((resource) => {
      this.own(resource);
      return resource;
    });
    this.#pendingResources.push(
      tracked.then(
        () => null,
        () => null,
      ),
    );
    return tracked;
  }

  async stop(): Promise<readonly unknown[]> {
    const closeTimeoutMs =
      this.#configuration.closeTimeoutMs ?? DEFAULT_CLOSE_TIMEOUT_MS;
    const failures: unknown[] = [];
    this.#closing = true;

    for (const resource of this.#resources.reverse()) {
      try {
        await this.#closeResource(resource, "socket close");
      } catch (error) {
        failures.push(error);
      }
    }

    try {
      await withTimeout(
        Promise.all(this.#pendingResources),
        closeTimeoutMs,
        "pending socket ownership drain",
      );
    } catch (error) {
      failures.push(error);
    }

    const child = this.#child;
    if (
      child !== null &&
      child.exitCode === null &&
      child.signalCode === null
    ) {
      child.kill("SIGTERM");
      try {
        await waitForExit(child, closeTimeoutMs);
      } catch (error) {
        failures.push(error);
        child.kill("SIGKILL");
        try {
          await waitForExit(child, closeTimeoutMs);
        } catch (killError) {
          failures.push(killError);
        }
      }
    }

    try {
      const logResults = await withTimeout(
        Promise.all(this.#logCompletions),
        closeTimeoutMs,
        "server log drain",
      );
      for (const result of logResults) {
        if (result !== null) {
          failures.push(result);
        }
      }
    } catch (error) {
      failures.push(error);
    }

    try {
      const lateCloseResults = await withTimeout(
        Promise.all(this.#lateCloses),
        closeTimeoutMs,
        "late socket close drain",
      );
      for (const result of lateCloseResults) {
        if (result !== null) {
          failures.push(result);
        }
      }
    } catch (error) {
      failures.push(error);
    }
    await this.#evidence.record("environment_stopped", {
      exit: this.#exit,
      cleanupFailures: failures,
    });
    return failures;
  }

  async #closeResource(resource: OwnedResource, label: string): Promise<void> {
    await withTimeout(
      resource.close(),
      this.#configuration.closeTimeoutMs ?? DEFAULT_CLOSE_TIMEOUT_MS,
      label,
    );
  }

  #captureLog(stream: Readable, name: string): void {
    const capped = new CappedLog(
      this.#configuration.logLimitBytes ?? DEFAULT_LOG_LIMIT_BYTES,
    );
    const destination = createWriteStream(join(this.#evidence.directory, name));
    stream.pipe(capped).pipe(destination);
    this.#logCompletions.push(
      finished(destination).then(
        () => null,
        (error: unknown) => error,
      ),
    );
  }

  async #captureBuildIdentity(): Promise<void> {
    const [executableStat, schemaStat, executableHash, schemaHash] =
      await Promise.all([
        stat(this.#configuration.executable),
        stat(this.#configuration.schemaArtifact),
        hashFile(this.#configuration.executable),
        hashFile(this.#configuration.schemaArtifact),
      ]);

    const schemaCopyName = `schema-${basename(this.#configuration.schemaArtifact)}`;
    await copyFile(
      this.#configuration.schemaArtifact,
      join(this.#evidence.directory, schemaCopyName),
    );
    await this.#evidence.writeJson("build-identity.json", {
      node: process.version,
      platform: process.platform,
      architecture: process.arch,
      executable: {
        path: this.#configuration.executable,
        bytes: executableStat.size,
        modifiedAt: executableStat.mtime.toISOString(),
        sha256: executableHash,
      },
      schema: {
        path: this.#configuration.schemaArtifact,
        copy: schemaCopyName,
        bytes: schemaStat.size,
        modifiedAt: schemaStat.mtime.toISOString(),
        sha256: schemaHash,
      },
    });
  }
}

export async function runNativeScenario<T>(
  name: string,
  configuration: NativeServerConfiguration,
  driverFactory: HarnessDriverFactory,
  parentSignal: AbortSignal,
  scenario: (context: ScenarioContext) => Promise<T>,
): Promise<ScenarioResult<T>> {
  return runNativeEnvironment(
    name,
    configuration,
    parentSignal,
    async (context) => {
      const connect = (): Promise<HarnessDriver> =>
        context.execute("client connect", { url: context.url }, () =>
          context.track(
            driverFactory.connect(context.url, {
              signal: context.signal,
              record: context.evidence.record.bind(context.evidence),
            }),
          ),
        );
      const driver = await connect();
      return scenario({ ...context, driver, connectFresh: connect });
    },
  );
}

/** Process lifecycle and evidence independent of a particular scene/client driver. */
export async function runNativeEnvironment<T>(
  name: string,
  configuration: NativeServerConfiguration,
  parentSignal: AbortSignal,
  scenario: (context: NativeEnvironmentContext) => Promise<T>,
): Promise<ScenarioResult<T>> {
  const evidenceParent =
    configuration.evidenceParent ?? resolve("target/integration-artifacts");
  await mkdir(evidenceParent, { recursive: true });
  const evidenceDirectory = await mkdtemp(
    join(evidenceParent, `ipp-integration-${safeName(name)}-`),
  );
  const evidence = await EvidenceRecorder.create(evidenceDirectory);
  const environment = new NativeServerEnvironment(configuration, evidence);
  const scenarioController = new AbortController();
  const operationTimeoutMs =
    configuration.operationTimeoutMs ?? DEFAULT_OPERATION_TIMEOUT_MS;
  const onParentAbort = (): void =>
    scenarioController.abort(parentSignal.reason);
  if (parentSignal.aborted) {
    onParentAbort();
  } else {
    parentSignal.addEventListener("abort", onParentAbort, { once: true });
  }

  let result: ScenarioResult<T> | undefined;
  let scenarioError: unknown;
  try {
    await evidence.record("scenario_start", { name });
    const url = await environment.start(scenarioController.signal);
    const execute = async <R>(
      label: string,
      input: unknown,
      action: () => Promise<R>,
    ): Promise<R> => {
      await evidence.record("operation_input", { label, input });
      try {
        const result = await withAbortAndTimeout(
          action(),
          scenarioController.signal,
          operationTimeoutMs,
          label,
        );
        await evidence.record("operation_result", { label, result });
        return result;
      } catch (error) {
        await evidence.record("operation_error", { label, error });
        throw error;
      }
    };

    const value = await scenario({
      signal: scenarioController.signal,
      url,
      evidence,
      track: (pending) => environment.track(pending),
      execute,
    });
    await evidence.record("scenario_success", { name });
    result = {
      value,
      evidenceDirectory,
    };
  } catch (error) {
    scenarioError = error;
    try {
      await evidence.record("scenario_failure", { name, error });
    } catch (evidenceError) {
      // Artifact failures must never bypass ownership cleanup.
      scenarioError = new AggregateError(
        [error, evidenceError],
        "scenario and evidence recording failed",
      );
    }
  }

  scenarioController.abort(new Error("scenario cleanup"));
  parentSignal.removeEventListener("abort", onParentAbort);
  let cleanupFailures: readonly unknown[];
  try {
    cleanupFailures = await environment.stop();
  } catch (error) {
    cleanupFailures = [error];
  }
  try {
    await evidence.flush();
  } catch (error) {
    cleanupFailures = [...cleanupFailures, error];
  }

  if (scenarioError !== undefined || cleanupFailures.length > 0) {
    throw new HarnessRunError(
      `integration scenario '${name}' failed; evidence retained at ${evidenceDirectory}`,
      evidenceDirectory,
      scenarioError ??
        new AggregateError(cleanupFailures, "environment cleanup failed"),
    );
  }
  if (result === undefined) {
    throw new HarnessRunError(
      `integration scenario '${name}' produced no result; evidence retained at ${evidenceDirectory}`,
      evidenceDirectory,
      new Error("scenario produced no result"),
    );
  }
  // Keep bounded success evidence as well as failures for CI diagnosis.
  return result;
}

async function hashFile(path: string): Promise<string> {
  const hash = createHash("sha256");
  const stream = createReadStream(path);
  for await (const chunk of stream) {
    hash.update(chunk as Buffer);
  }
  return hash.digest("hex");
}

function safeName(name: string): string {
  return name
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-|-$/g, "");
}

async function waitForReadiness(
  child: NativeServerProcess,
  signal: AbortSignal,
  timeoutMs: number,
): Promise<unknown> {
  return await new Promise((resolve, reject) => {
    let buffered = Buffer.alloc(0);
    const timer = setTimeout(
      () =>
        finish(new Error(`server readiness timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
    timer.unref();

    const finish = (error?: unknown, value?: unknown): void => {
      clearTimeout(timer);
      child.stdout.off("data", onData);
      child.off("error", onError);
      child.off("exit", onExit);
      signal.removeEventListener("abort", onAbort);
      if (error === undefined) {
        resolve(value);
      } else {
        reject(error);
      }
    };
    const onData = (chunk: Buffer): void => {
      buffered = Buffer.concat([buffered, chunk]);
      if (buffered.byteLength > MAXIMUM_READINESS_LINE_BYTES) {
        finish(
          new Error(
            `server readiness line exceeded ${MAXIMUM_READINESS_LINE_BYTES} bytes`,
          ),
        );
        return;
      }
      const newline = buffered.indexOf(0x0a);
      if (newline < 0) {
        return;
      }
      const line = buffered.subarray(0, newline).toString("utf8").trim();
      try {
        finish(undefined, JSON.parse(line) as unknown);
      } catch (error) {
        finish(
          new Error(`invalid server readiness JSON: ${line}`, { cause: error }),
        );
      }
    };
    const onError = (error: Error): void => finish(error);
    const onExit = (
      code: number | null,
      exitSignal: NodeJS.Signals | null,
    ): void =>
      finish(
        new Error(
          `server exited before readiness (code=${String(code)}, signal=${String(exitSignal)})`,
        ),
      );
    const onAbort = (): void =>
      finish(signal.reason ?? new Error("server readiness cancelled"));

    child.stdout.on("data", onData);
    child.once("error", onError);
    child.once("exit", onExit);
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

function validateReadiness(value: unknown): string {
  if (
    typeof value !== "object" ||
    value === null ||
    !("event" in value) ||
    value.event !== "ready" ||
    !("url" in value) ||
    typeof value.url !== "string"
  ) {
    throw new Error(
      "server readiness must be {event:'ready',url:'ws://127.0.0.1:PORT'}",
    );
  }
  const readiness = value as Readiness;
  const url = new URL(readiness.url);
  if (
    url.protocol !== "ws:" ||
    url.hostname !== "127.0.0.1" ||
    url.port === "" ||
    Number(url.port) <= 0 ||
    url.pathname !== "/"
  ) {
    throw new Error(
      `server emitted an invalid loopback WebSocket URL: ${readiness.url}`,
    );
  }
  return readiness.url;
}

async function waitForExit(
  child: NativeServerProcess,
  timeoutMs: number,
): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  await withTimeout(
    new Promise<void>((resolve, reject) => {
      child.once("exit", () => resolve());
      child.once("error", reject);
    }),
    timeoutMs,
    "server exit",
  );
}

async function withAbortAndTimeout<T>(
  promise: Promise<T>,
  signal: AbortSignal,
  timeoutMs: number,
  label: string,
): Promise<T> {
  if (signal.aborted) {
    throw signal.reason;
  }
  let rejectAbort: (reason: unknown) => void = () => undefined;
  const aborted = new Promise<never>((_resolve, reject) => {
    rejectAbort = reject;
  });
  const onAbort = (): void =>
    rejectAbort(signal.reason ?? new Error(`${label} cancelled`));
  signal.addEventListener("abort", onAbort, { once: true });
  let rejectTimeout: (reason: unknown) => void = () => undefined;
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
  let rejectTimeout: (reason: unknown) => void = () => undefined;
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
