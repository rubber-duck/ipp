import type { ResourceUrlMapping } from "@ipp/client";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type HTMLAttributes,
  type ReactNode,
} from "react";
import {
  MAX_CAPTURE_DIMENSION,
  type Client,
  type GuiWorldClient,
  type LogLevel,
} from "@ipp/client";
import {
  CanvasWorldSession,
  asError,
  notify,
  type CanvasWorldBinding,
} from "./canvas-world-session.js";
import {
  attachCanvasGuiInput,
  canvasViewportPoint,
  createGuiInputSink,
  type AttachCanvasGuiInputOptions,
  type GuiViewportPoint,
} from "./gui/input.js";
import {
  attachTextBridge,
  createTextBridgeModel,
  observeTextBridgeBatch,
  viewportToBridgeOffset,
} from "./gui/text-bridge.js";
import type { IppCanvasHandle } from "./canvas-world-session.js";

export type { IppCanvasHandle } from "./canvas-world-session.js";

export interface CanvasRuntimeConfiguration {
  readonly generatedModuleUrl: string;
  readonly workerScriptUrl: string;
  readonly wasmUrl: string;
  readonly timeoutMs?: number;
  readonly logLevel?: LogLevel;
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
  /** Opt-in browser input: attach the GUI relay to the canvas once connected.
   *
   * Pointer and wheel events originate on the canvas. By default one hidden
   * native editor owns keyboard, `beforeinput`, IME and clipboard events and
   * forwards them through the session client's `submitGuiInput` in DOM order;
   * capture, focus scopes and gesture arbitration stay in core. While
   * attached the canvas uses
   * `touch-action: none` (opt out per attach with
   * `enableTouchActionNone: false`), wheel input is always
   * `preventDefault`ed and Tab is prevented. Disable `textBridge` to keep
   * keyboard ownership on the canvas or an explicit `keyboardTarget`.
   * Requires a GUI-capable client, otherwise `onError` fires and no listeners
   * attach.
   */
  readonly guiInput?: AttachCanvasGuiInputOptions;
  /** Fetch and deserialize a saved World instead of creating an empty one. */
  readonly worldUrl?: string;
  /** Runs once per startup before World bindings and onReady; honor cancellation. */
  readonly initialize?: (
    client: Client,
    signal: AbortSignal,
  ) => void | Promise<void>;
  /** Default CSS dimensions; the drawing buffer follows layout and display density. */
  readonly width?: number;
  readonly height?: number;
  readonly canvasProps?: CanvasProps;
  readonly onReady?: (handle: IppCanvasHandle) => void;
  readonly onError?: (error: Error) => void;
}

export interface WorldProps {
  readonly children?: ReactNode;
  readonly onCommit?: () => void;
  readonly onError?: (error: Error) => void;
}

interface CanvasConnectionOptions {
  canvas: OffscreenCanvas;
  timeoutMs: number;
  signal: AbortSignal;
  logLevel: LogLevel;
  assetCacheBytes?: number;
  resourceUrls?: readonly ResourceUrlMapping[];
}

interface GeneratedCanvasHost {
  readonly capabilities: { readonly snapshot?: boolean };
  loadWorld?(
    bytes: Uint8Array,
    options: { signal: AbortSignal },
  ): Promise<Client>;
  ownWorldConnection(): void;
  close(): Promise<void>;
}

interface GeneratedWorldClientModule {
  readonly IppHostClient?: {
    connectWorker(
      worker: string,
      wasm: string,
      options: CanvasConnectionOptions,
    ): Promise<GeneratedCanvasHost>;
  };
  readonly IppClient: {
    connectWorker(
      worker: string,
      wasm: string,
      options: CanvasConnectionOptions,
    ): Promise<Client>;
  };
}

interface CanvasRenderSurface {
  readonly canvas: HTMLCanvasElement;
  readonly configuration: string;
  renew(): void;
}

const CanvasContext = createContext<CanvasWorldSession | null | undefined>(
  undefined,
);
const transferred = new WeakSet<HTMLCanvasElement>();

/** A normal DOM component. Only descendant World boundaries use the IPP renderer. */
export function IppCanvas({
  runtime,
  guiInput,
  worldUrl,
  initialize,
  width = 640,
  height = 480,
  canvasProps,
  onReady,
  onError,
  children,
  ...domProps
}: IppCanvasProps) {
  if (![width, height].every((value) => Number.isFinite(value) && value > 0)) {
    throw new RangeError("Canvas CSS dimensions must be positive and finite");
  }
  const {
    generatedModuleUrl,
    workerScriptUrl,
    wasmUrl,
    timeoutMs = 10_000,
    logLevel = "info",
  } = runtime;
  const configuration = JSON.stringify([
    generatedModuleUrl,
    workerScriptUrl,
    wasmUrl,
    timeoutMs,
    logLevel,
    runtime.assetCacheBytes,
    worldUrl,
    runtime.resourceUrls,
  ]);
  const [surface, setSurface] = useState<CanvasRenderSurface>();
  const [ready, setReady] = useState<{
    surface: CanvasRenderSurface;
    session: CanvasWorldSession;
  }>();
  const [error, setError] = useState<{
    surface: CanvasRenderSurface;
    error: Error;
  }>();
  const callbacks = useRef({ onReady, onError, initialize });
  const guiInputRef = useRef(guiInput);
  const dimensions = useRef({ width: 1, height: 1 });
  useLayoutEffect(() => {
    callbacks.current = { onReady, onError, initialize };
    guiInputRef.current = guiInput;
  });

  useEffect(() => {
    if (!surface || surface.configuration !== configuration) return;
    if (transferred.has(surface.canvas)) {
      // A replay after transfer needs a new DOM canvas; an OffscreenCanvas
      // cannot be transferred a second time. React owns the replacement node.
      surface.renew();
      return;
    }
    let disposed = false;
    const startup = new AbortController();
    let session: CanvasWorldSession | undefined;
    let client: Client | undefined;
    let host: GeneratedCanvasHost | undefined;
    const close = (): Promise<void> => {
      setReady((current) =>
        current?.surface === surface ? undefined : current,
      );
      if (session) return session.close();
      if (host) return host.close();
      return client?.close() ?? Promise.resolve();
    };
    const originalErrorHandler = callbacks.current.onError;
    const report = (failure: unknown): void => {
      const error = asError(failure);
      const fallback = (callbackError: Error): void => {
        if (!disposed) setError({ surface, error: callbackError });
        else console.error("IPP canvas lifecycle error", callbackError);
      };
      const observer = disposed
        ? originalErrorHandler
        : callbacks.current.onError;
      if (observer) notify(() => observer(error), fallback);
      else fallback(error);
    };
    void (async () => {
      try {
        const module = (await import(
          generatedModuleUrl
        )) as GeneratedWorldClientModule;
        if (disposed) return;
        const size = dimensions.current;
        surface.canvas.width = size.width;
        surface.canvas.height = size.height;
        const offscreen = surface.canvas.transferControlToOffscreen();
        transferred.add(surface.canvas);
        const options = {
          canvas: offscreen,
          timeoutMs,
          logLevel,
          ...(runtime.assetCacheBytes !== undefined
            ? { assetCacheBytes: runtime.assetCacheBytes }
            : {}),
          signal: startup.signal,
          ...(runtime.resourceUrls
            ? { resourceUrls: runtime.resourceUrls }
            : {}),
        };
        if (worldUrl !== undefined) {
          if (!module.IppHostClient?.connectWorker)
            throw new Error(
              "IppCanvas worldUrl requires generated Host snapshot support",
            );
          host = await module.IppHostClient.connectWorker(
            workerScriptUrl,
            wasmUrl,
            options,
          );
          if (disposed) {
            await close();
            return;
          }
          if (!host.capabilities.snapshot || !host.loadWorld)
            throw new Error("IppCanvas worldUrl requires snapshot support");
          const response = await fetch(worldUrl, { signal: startup.signal });
          if (!response.ok)
            throw new Error(
              `Unable to load IPP World: HTTP ${response.status}`,
            );
          const bytes = new Uint8Array(await response.arrayBuffer());
          startup.signal.throwIfAborted();
          // This correlated Host reply confirms reconstruction and publication.
          // Resource readiness is independent of the deserialization boundary.
          client = await host.loadWorld(bytes, { signal: startup.signal });
          if (disposed) {
            await close();
            return;
          }
          host.ownWorldConnection();
        } else {
          client = await module.IppClient.connectWorker(
            workerScriptUrl,
            wasmUrl,
            options,
          );
        }
        if (disposed) {
          await close();
          return;
        }
        if (
          !client.capabilities.spatial ||
          !client.capabilities.stateOverlays ||
          !client.presentation
        ) {
          throw new Error(
            "IppCanvas requires spatial, state overlays, and browser presentation",
          );
        }
        await callbacks.current.initialize?.(client, startup.signal);
        if (disposed) {
          await close();
          return;
        }
        session = new CanvasWorldSession(
          client,
          report,
          () => dimensions.current,
        );
        client.presentation.resize(
          dimensions.current.width,
          dimensions.current.height,
        );
        setReady({ surface, session });
        const connected = session;
        notify(() => callbacks.current.onReady?.(connected), report);
      } catch (failure) {
        if (!disposed)
          setReady((current) =>
            current?.surface === surface ? undefined : current,
          );
        const errors = [failure];
        try {
          await close();
        } catch (cleanup) {
          errors.push(cleanup);
        }
        if (!disposed)
          report(
            errors.length === 1
              ? failure
              : new AggregateError(errors, "IPP canvas startup failed"),
          );
        else if (errors.length > 1) report(errors[1]);
      }
    })();
    return () => {
      disposed = true;
      startup.abort();
      void close().catch(report);
    };
  }, [
    surface,
    configuration,
    generatedModuleUrl,
    workerScriptUrl,
    wasmUrl,
    timeoutMs,
    logLevel,
    worldUrl,
  ]);

  const session =
    ready &&
    ready.surface === surface &&
    surface?.configuration === configuration
      ? ready.session
      : undefined;
  useLayoutEffect(() => {
    if (!surface) return;
    const canvas = surface.canvas;
    let cssWidth = canvas.clientWidth;
    let cssHeight = canvas.clientHeight;
    let detached = false;
    const resize = () => {
      if (detached || session?.isClosing) return;
      if (cssWidth <= 0 || cssHeight <= 0) return;
      const ratio = Math.min(
        window.devicePixelRatio,
        MAX_CAPTURE_DIMENSION / cssWidth,
        MAX_CAPTURE_DIMENSION / cssHeight,
      );
      const next = {
        width: Math.max(1, Math.round(cssWidth * ratio)),
        height: Math.max(1, Math.round(cssHeight * ratio)),
      };
      if (
        next.width === dimensions.current.width &&
        next.height === dimensions.current.height
      )
        return;
      dimensions.current = next;
      session?.client.presentation!.resize(next.width, next.height);
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
  }, [surface, session, width, height]);

  useEffect(() => {
    const inputOptions = guiInputRef.current;
    if (!surface || !session || !inputOptions || session.isClosing) return;
    const target = session.client as GuiWorldClient;
    if (typeof target.submitGuiInput !== "function") {
      callbacks.current.onError?.(
        new Error("IppCanvas guiInput requires a GUI-capable client"),
      );
      return;
    }
    const sink = createGuiInputSink(target, {
      onError: (error) => callbacks.current.onError?.(error),
      ...(inputOptions.unhandledInputGate === undefined
        ? {}
        : { unhandledInputGate: inputOptions.unhandledInputGate }),
    });
    // The native editable buffer gives the OS IME, soft keyboard and
    // clipboard a focusable target; canvas routing keeps sole ownership of
    // captures, focus and gesture decisions. The bridge model folds
    // authoritative transient focus observations into the sync readers, so delayed
    // paste reads and selection echoes cancel across focus moves,
    // replacements and teardown instead of writing into a new target.
    let detachTextBridge: (() => void) | undefined;
    let nativeKeyboardTarget: HTMLElement | null | undefined =
      inputOptions.keyboardTarget;
    let blurOnKeyboardTarget = true;
    let keyboardInput = true;
    if (inputOptions.textBridge !== false && surface.canvas.parentElement) {
      const container = surface.canvas.parentElement;
      const canvas = surface.canvas;
      const model = createTextBridgeModel(target.session);
      const reportBridge = (error: Error): void => {
        callbacks.current.onError?.(error);
      };
      const bridge = attachTextBridge(container, sink, {
        onError: reportBridge,
        getFocusToken: () => model.token(),
        readCommitted: () => model.committed(),
        onLocalSelection: (selection) => {
          model.noteLocalSelection(selection.start, selection.end);
        },
      });
      nativeKeyboardTarget = bridge.element;
      keyboardInput = false;
      // The editor mirrors runtime focus. Blurring it during an internal
      // canvas/editor transfer must not clear the runtime focus that the
      // pointer event just selected. Window blur remains authoritative.
      blurOnKeyboardTarget = false;
      const toViewport: (point: GuiViewportPoint) => GuiViewportPoint = (
        point,
      ) => guiInputRef.current?.toViewport?.(point) ?? point;
      let pendingActivation:
        | { readonly x: number; readonly y: number; reported: boolean }
        | undefined;
      const unsubscribeObservations =
        typeof target.subscribeGuiObservations === "function"
          ? target.subscribeGuiObservations((batch) => {
              try {
                if (observeTextBridgeBatch(model, batch)) bridge.syncFromCore();
                if (pendingActivation && batch.textFocus !== undefined) {
                  if (batch.textFocus === null) {
                    pendingActivation = undefined;
                  } else {
                    const activation = (
                      navigator as Navigator & {
                        userActivation?: { readonly isActive: boolean };
                      }
                    ).userActivation;
                    if (activation?.isActive === true) {
                      bridge.placeAt(pendingActivation.x, pendingActivation.y);
                      bridge.focusFromGesture({
                        trigger: "pointerDown",
                        isTrusted: true,
                      });
                      pendingActivation = undefined;
                    } else if (!pendingActivation.reported) {
                      pendingActivation.reported = true;
                      reportBridge(
                        new Error(
                          "Native text activation expired before the authoritative GUI focus result; tap the text input again",
                        ),
                      );
                    }
                  }
                }
              } catch (error) {
                reportBridge(asError(error));
              }
            })
          : undefined;
      // Keep the actual trusted gesture pending until core publishes an
      // authoritative text-focus result. Non-text taps never focus or open the
      // editor. Platforms that expire transient activation before the result
      // report the failure and require a later valid gesture.
      const onPointerDown = (event: PointerEvent): void => {
        model.noteActivation();
        if (!event.isTrusted) return;
        const rect = canvas.getBoundingClientRect();
        const raw = canvasViewportPoint(event.clientX, event.clientY, rect);
        if (raw === null) return;
        const offset = viewportToBridgeOffset(
          toViewport(raw),
          rect,
          container.getBoundingClientRect(),
        );
        pendingActivation = { ...offset, reported: false };
      };
      const onKeyDown = (event: KeyboardEvent): void => {
        if (!event.isTrusted) return;
        if (event.key === "Tab" || event.key === "Escape") {
          model.noteActivation();
        }
      };
      // Losing the window drops the buffer focus view; the relay already
      // sends the core blur. Stale paste reads cancel on the cleared token.
      const onWindowBlur = (): void => {
        model.clear();
        bridge.syncFromCore();
      };
      canvas.addEventListener("pointerdown", onPointerDown);
      bridge.element.addEventListener("keydown", onKeyDown);
      window.addEventListener("blur", onWindowBlur);
      detachTextBridge = (): void => {
        canvas.removeEventListener("pointerdown", onPointerDown);
        bridge.element.removeEventListener("keydown", onKeyDown);
        window.removeEventListener("blur", onWindowBlur);
        unsubscribeObservations?.();
        pendingActivation = undefined;
        model.clear();
        bridge.dispose();
      };
    }
    const detachRelay = attachCanvasGuiInput(surface.canvas, sink, {
      ...inputOptions,
      toViewport: (point) => guiInputRef.current?.toViewport?.(point) ?? point,
      blurOnKeyboardTarget,
      keyboardInput,
      onError: (error) => {
        guiInputRef.current?.onError?.(error);
        callbacks.current.onError?.(error);
      },
      ...(nativeKeyboardTarget === undefined
        ? {}
        : { keyboardTarget: nativeKeyboardTarget }),
    });
    const detach = (): void => {
      detachTextBridge?.();
      detachTextBridge = undefined;
      detachRelay();
    };
    const unsubscribe = session.onClosing(detach);
    return () => {
      unsubscribe();
      detach();
    };
  }, [
    surface,
    session,
    guiInput !== undefined,
    guiInput?.keyboardTarget,
    guiInput?.textBridge,
    guiInput?.blurOnKeyboardTarget,
    guiInput?.preventDefaultPointer,
    guiInput?.enableTouchActionNone,
    guiInput?.blockers,
    guiInput?.panelDistance,
    guiInput?.unhandledInputGate,
  ]);

  if (
    error &&
    error.surface === surface &&
    surface?.configuration === configuration
  ) {
    throw error.error;
  }
  // Canvas-only keyboard input needs a focusable canvas; the default never
  // overrides an author-supplied tab index.
  const inputCanvasProps =
    guiInput && canvasProps?.tabIndex === undefined
      ? { tabIndex: 0, ...canvasProps }
      : canvasProps;
  return (
    <CanvasContext value={session ?? null}>
      <div {...domProps}>
        <CanvasSurface
          key={configuration}
          configuration={configuration}
          width={width}
          height={height}
          canvasProps={inputCanvasProps}
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
  // DOM width/height must not be mutated after OffscreenCanvas transfer.
  // Subsequent drawing-buffer sizes travel through the presentation channel.
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

/** Undefined while the nearest canvas is connecting; throws without a canvas. */
export function useIppCanvas(): IppCanvasHandle | undefined {
  return useCanvasSession() ?? undefined;
}

function useCanvasSession(): CanvasWorldSession | null {
  const session = useContext(CanvasContext);
  if (session === undefined)
    throw new Error("World and useIppCanvas require an ancestor IppCanvas");
  return session;
}

/** Render children into the nearest canvas, through any ordinary DOM wrappers. */
export function World({ children, onCommit, onError }: WorldProps) {
  const session = useCanvasSession();
  const binding = useRef<CanvasWorldBinding | undefined>(undefined);
  const callbacks = useRef({ onCommit, onError });
  const [error, setError] = useState<{
    session: CanvasWorldSession;
    error: Error;
  }>();
  useLayoutEffect(() => {
    callbacks.current = { onCommit, onError };
  });
  useLayoutEffect(() => {
    if (!session) return;
    let active = true;
    const fallback = (error: Error): void => {
      if (active) setError({ session, error });
      else session.report(error);
    };
    const report = (error: Error): void => {
      if (!active) session.report(error);
      else if (callbacks.current.onError) {
        const observer = callbacks.current.onError;
        notify(() => observer(error), fallback);
      } else fallback(error);
    };
    const world = session.attach(report);
    binding.current = world;
    return () => {
      active = false;
      binding.current = undefined;
      void world.close().catch(() => {
        // ReactWorldRoot failures already use report; canvas.closed also observes teardown.
      });
    };
  }, [session]);
  useLayoutEffect(() => {
    const world = binding.current;
    if (!session || !world) return;
    world.render(
      <CanvasContext value={session}>{children}</CanvasContext>,
      callbacks.current.onCommit,
    );
  }, [children, session]);
  if (error?.session === session) throw error.error;
  return null;
}
