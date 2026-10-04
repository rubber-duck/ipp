import type {
  Client,
  LogLevel,
  OutputReference,
  PresentationView,
  ResourceUrlMapping,
} from "@ipp/client";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type HTMLAttributes,
} from "react";
import {
  CanvasWorldSession,
  asError,
  notify,
  type IppCanvasHandle,
} from "./canvas-world-session.js";
import { CanvasLifetime, type CanvasWorldSource } from "./canvas-lifetime.js";
import type { CanvasHost, CanvasSize } from "./canvas-presentation.js";
import { attachmentIdentity } from "./attachment-identity.js";
import { CanvasGuiInput, type CanvasGuiInputOptions } from "./gui/input.js";
import { CanvasContext } from "./canvas-context.js";

export { CanvasWorldSession } from "./canvas-world-session.js";
export { CanvasCleanupError } from "./canvas-lifetime.js";
export type {
  IppCanvasHandle,
  CanvasSessionOptions,
} from "./canvas-world-session.js";
export type {
  CanvasCleanupJournal,
  CanvasCleanupRecovery,
  CanvasWorldSource,
} from "./canvas-lifetime.js";
export type {
  CanvasHost,
  CanvasSize,
  CanvasPresentationJournal,
} from "./canvas-presentation.js";

export interface CanvasRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs?: number;
  readonly logLevel?: LogLevel;
  /** Unused-asset cache target in bytes; omitted keeps the Host default (64 MiB), 0 evicts on release. */
  readonly assetCacheBytes?: number;
  readonly resourceUrls?: readonly ResourceUrlMapping[];
}

type CanvasProps = Omit<
  ComponentPropsWithoutRef<"canvas">,
  "children" | "ref" | "width" | "height"
>;

export interface IppCanvasProps
  extends Omit<HTMLAttributes<HTMLDivElement>, "onError"> {
  readonly runtime: CanvasRuntimeConfiguration;
  readonly world: CanvasWorldSource;
  /**
   * Explicit root selection, independent of the authoring session: a Camera
   * output or `canvasOutput(world)`. Null presents nothing. Omit it when a
   * root-presented `CanvasWorld` supplies the root.
   */
  readonly output?: OutputReference | null;
  readonly initialize?: (
    client: Client,
    signal: AbortSignal,
    host: CanvasHost,
  ) => void | Promise<void>;
  readonly width?: number;
  readonly height?: number;
  readonly canvasProps?: CanvasProps;
  readonly guiInput?: Omit<CanvasGuiInputOptions, "onError">;
  readonly onReady?: (handle: IppCanvasHandle) => void;
  readonly onViewChange?: (view: PresentationView | null) => void;
  readonly onError?: (error: Error) => void;
}

interface GeneratedCanvasModule {
  readonly IppHostClient: {
    connectWorker(
      worker: string,
      wasm: string,
      options: {
        canvas: OffscreenCanvas;
        signal: AbortSignal;
        timeoutMs: number;
        logLevel: LogLevel;
        assetCacheBytes?: number;
        resourceUrls?: readonly ResourceUrlMapping[];
      },
    ): Promise<CanvasHost>;
  };
}

interface CanvasRenderSurface {
  readonly canvas: HTMLCanvasElement;
  readonly configuration: string;
  renew(): void;
}

const transferred = new WeakSet<HTMLCanvasElement>();
const configurationFunctions = new WeakMap<object, number>();
let nextConfigurationFunction = 1;

function sourceIdentity(source: CanvasWorldSource): string {
  if (source.create) return attachmentIdentity(source);
  const names = source.load.options?.worldNames;
  let identity: unknown = names;
  if (typeof names === "function") {
    let id = configurationFunctions.get(names);
    if (id === undefined) {
      id = nextConfigurationFunction++;
      configurationFunctions.set(names, id);
    }
    identity = { callback: id };
  } else if (names)
    identity = [...names.entries()].sort(([left], [right]) => left - right);
  return attachmentIdentity({
    load: {
      ...source.load,
      options: { ...source.load.options, worldNames: identity },
    },
  });
}

/** DOM ownership and explicit presentation; World boundaries remain same-World scopes. */
export function IppCanvas({
  runtime,
  world,
  output,
  initialize,
  width = 640,
  height = 480,
  canvasProps,
  guiInput,
  onReady,
  onViewChange,
  onError,
  children,
  ...domProps
}: IppCanvasProps) {
  if (![width, height].every((value) => Number.isFinite(value) && value > 0))
    throw new RangeError("Canvas CSS dimensions must be positive and finite");
  const configuration = attachmentIdentity({
    runtime,
    source: sourceIdentity(world),
  });
  const [surface, setSurface] = useState<CanvasRenderSurface>();
  const [ready, setReady] = useState<{
    surface: CanvasRenderSurface;
    session: CanvasWorldSession;
  }>();
  const [error, setError] = useState<{
    surface: CanvasRenderSurface;
    error: Error;
  }>();
  const callbacks = useRef({ initialize, onReady, onViewChange, onError });
  const inputs = useRef({ runtime, world, output, guiInput });
  const activeInput = useRef<
    | {
        owner: CanvasGuiInput;
        report: (failure: unknown) => void;
      }
    | undefined
  >(undefined);
  const dimensions = useRef<CanvasSize>({ width, height, devicePixelRatio: 1 });
  useLayoutEffect(() => {
    callbacks.current = { initialize, onReady, onViewChange, onError };
    inputs.current = { runtime, world, output, guiInput };
    const active = activeInput.current;
    if (active)
      void active.owner
        .update({ ...guiInput, onError: active.report })
        .catch(active.report);
  });

  useEffect(() => {
    if (!surface || surface.configuration !== configuration) return;
    if (transferred.has(surface.canvas)) {
      surface.renew();
      return;
    }
    let disposed = false;
    const startup = new AbortController();
    let lifetime: CanvasLifetime | undefined;
    let session: CanvasWorldSession | undefined;
    let input: CanvasGuiInput | undefined;
    const capturedError = callbacks.current.onError;
    const report = (failure: unknown): void => {
      const value = asError(failure);
      const fallback = (error: Error): void => {
        if (!disposed) setError({ surface, error });
        else console.error("Canvas lifecycle error", error);
      };
      const observer = disposed ? capturedError : callbacks.current.onError;
      if (observer) notify(() => observer(value), fallback);
      else fallback(value);
    };
    const close = async (): Promise<void> => {
      if (activeInput.current?.owner === input) activeInput.current = undefined;
      try {
        await input?.close();
      } finally {
        if (session) await session.close();
        else await lifetime?.close();
      }
    };
    const work = (async () => {
      const { runtime: configuration, world: source } = inputs.current;
      try {
        const module = (await import(
          configuration.generatedModuleUrl
        )) as GeneratedCanvasModule;
        if (disposed) return;
        surface.canvas.width = Math.max(
          1,
          Math.round(dimensions.current.width),
        );
        surface.canvas.height = Math.max(
          1,
          Math.round(dimensions.current.height),
        );
        const canvas = surface.canvas.transferControlToOffscreen();
        transferred.add(surface.canvas);
        const host = await module.IppHostClient.connectWorker(
          configuration.workerScriptUrl,
          configuration.wasmUrl,
          {
            canvas,
            signal: startup.signal,
            timeoutMs: configuration.timeoutMs ?? 10_000,
            logLevel: configuration.logLevel ?? "info",
            ...(configuration.assetCacheBytes !== undefined
              ? { assetCacheBytes: configuration.assetCacheBytes }
              : {}),
            ...(configuration.resourceUrls
              ? { resourceUrls: configuration.resourceUrls }
              : {}),
          },
        );
        lifetime = new CanvasLifetime(host, true);
        startup.signal.throwIfAborted();
        const client = await lifetime.open(source, startup.signal);
        startup.signal.throwIfAborted();
        input = new CanvasGuiInput(surface.canvas, host.input, {
          ...inputs.current.guiInput,
          onError: report,
        });
        activeInput.current = { owner: input, report };
        session = new CanvasWorldSession(
          {
            host,
            client,
            onError: report,
            onViewChange: (view) => {
              if (!disposed) void input?.select(view).catch(report);
              if (!disposed) callbacks.current.onViewChange?.(view);
            },
          },
          lifetime,
        );
        await callbacks.current.initialize?.(client, startup.signal, host);
        startup.signal.throwIfAborted();
        if (session.isClosing)
          throw new Error(
            "Canvas authoring session closed during initialization",
          );
        setReady({ surface, session });
        notify(() => {
          if (!disposed) callbacks.current.onReady?.(session!);
        }, report);
      } catch (failure) {
        try {
          await close();
        } catch (cleanup) {
          if (!session) report(cleanup);
        }
        if (!disposed) report(failure);
      }
    })();
    return () => {
      disposed = true;
      startup.abort();
      if (activeInput.current?.owner === input) activeInput.current = undefined;
      // Drain physical ownership before session cleanup can dispose the Host.
      void close().catch(report);
      void work.then(close).catch((error) => {
        if (!session) report(error);
      });
    };
  }, [surface, configuration]);

  const session =
    ready?.surface === surface && surface?.configuration === configuration
      ? ready?.session
      : undefined;
  const outputIdentity = attachmentIdentity(output);
  useLayoutEffect(() => {
    if (!surface) return;
    const canvas = surface.canvas;
    let cssWidth = canvas.clientWidth;
    let cssHeight = canvas.clientHeight;
    let detached = false;
    const resize = () => {
      if (detached || session?.isClosing || cssWidth <= 0 || cssHeight <= 0)
        return;
      dimensions.current = {
        width: cssWidth,
        height: cssHeight,
        devicePixelRatio: window.devicePixelRatio,
      };
      if (session)
        void session
          .selectOutput(inputs.current.output, dimensions.current)
          .catch((error) => {
            if (!detached && !session.isClosing) session.report(asError(error));
          });
    };
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      cssWidth = entry.contentRect.width;
      cssHeight = entry.contentRect.height;
      resize();
    });
    observer.observe(canvas);
    let density = window.matchMedia(
      `(resolution: ${window.devicePixelRatio}dppx)`,
    );
    const densityChanged = () => {
      density.removeEventListener("change", densityChanged);
      density = window.matchMedia(
        `(resolution: ${window.devicePixelRatio}dppx)`,
      );
      density.addEventListener("change", densityChanged);
      resize();
    };
    density.addEventListener("change", densityChanged);
    window.addEventListener("resize", resize);
    resize();
    const detach = () => {
      detached = true;
      observer.disconnect();
      density.removeEventListener("change", densityChanged);
      window.removeEventListener("resize", resize);
    };
    const unsubscribe = session?.onClosing(detach);
    return () => {
      unsubscribe?.();
      detach();
    };
  }, [surface, session, width, height, outputIdentity]);

  if (error?.surface === surface && surface?.configuration === configuration)
    throw error!.error;
  return (
    <CanvasContext value={session ?? null}>
      <div {...domProps}>
        <CanvasSurface
          key={configuration}
          configuration={configuration}
          width={width}
          height={height}
          canvasProps={canvasProps}
          onSurface={setSurface}
        />
        {children}
      </div>
    </CanvasContext>
  );
}

function CanvasSurface({
  configuration,
  width,
  height,
  canvasProps,
  onSurface,
}: {
  configuration: string;
  width: number;
  height: number;
  canvasProps: CanvasProps | undefined;
  onSurface: (surface: CanvasRenderSurface) => void;
}) {
  const [generation, setGeneration] = useState(0);
  const renew = useCallback(() => setGeneration((value) => value + 1), []);
  const reference = useCallback(
    (canvas: HTMLCanvasElement | null) => {
      if (canvas) onSurface({ canvas, configuration, renew });
    },
    [configuration, onSurface, renew],
  );
  return (
    <canvas
      {...canvasProps}
      style={{
        width,
        height,
        aspectRatio: `${width} / ${height}`,
        ...canvasProps?.style,
      }}
      key={generation}
      ref={reference}
      width={1}
      height={1}
    />
  );
}

export { World, useIppCanvas } from "./canvas-scope.js";
export type { WorldProps } from "./canvas-scope.js";
