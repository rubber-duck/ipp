/**
 * `<CanvasWorld>`: an element that owns and controls one World selecting the
 * Canvas System. It creates the World with its selection and initial canvas
 * state, sends a Canvas state update when `extent` or `unitsPerMetre` changes,
 * and presents the World's canvas as the root of the enclosing `IppCanvas` or
 * as a SurfaceCanvas child of a parent anchor.
 */
import {
  createElement,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type Ref,
} from "react";
import {
  canvasOutput,
  type CanvasOutputReference,
  type CanvasState,
  type CanvasStateUpdateCommand,
  type WorldCreateOptions,
  type WorldReference,
} from "@ipp/client";
import {
  AttachedWorldBoundary,
  type AttachedWorldHandle,
} from "./attached-world.js";
import { attachmentIdentity } from "./attachment-identity.js";
import { CanvasContext } from "./canvas-context.js";
import { notify } from "./error-reporting.js";
import type {
  CanvasWorldSession,
  OwnedCanvasWorld,
} from "./canvas-world-session.js";

/** Where a CanvasWorld presents its World's canvas. */
export type CanvasWorldPresentation =
  /** The root output of the enclosing `IppCanvas`, sized by its viewport. */
  | { readonly root: true; readonly anchor?: never }
  /** A SurfaceCanvas child of the parent World entity with this symbolic id
   * or handle; the entity's Surface supplies the physical size. */
  | { readonly anchor: string | bigint; readonly root?: never };

export interface CanvasWorldProps {
  /** Creation of the owned World. The selection must include `ipp.canvas`;
   * the initial canvas state comes from `extent` and `unitsPerMetre`. A
   * changed creation creates a new World. */
  readonly create: Omit<WorldCreateOptions, "canvas">;
  /** Logical width and height, each finite and positive; used while the
   * canvas is not presented. Omitted keeps the current value (default 1 x 1). */
  readonly extent?: readonly [number, number];
  /** Logical units per Surface metre, finite and positive. Omitted keeps the
   * current value (default 1). A root viewport ignores density. */
  readonly unitsPerMetre?: number;
  readonly presentation: CanvasWorldPresentation;
  /** Declarations mounted into the owned World. */
  readonly children?: ReactNode;
  readonly ref?: Ref<CanvasWorldHandle>;
  readonly onReady?: (handle: CanvasWorldHandle) => void;
  readonly onError?: (error: Error) => void;
}

export interface CanvasWorldHandle {
  readonly world: WorldReference;
  /** The owned World's canvas output; it lives as long as the World. */
  readonly output: CanvasOutputReference;
  readonly closed: Promise<void>;
}

const CANVAS_SYSTEM = "ipp.canvas";

function positive(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}

/** Refuse a CanvasWorld that cannot own a canvas before creating anything. */
export function validateCanvasWorld(props: CanvasWorldProps): void {
  if (!props.create?.selectedSystems?.includes(CANVAS_SYSTEM))
    throw new Error(`CanvasWorld selects ${CANVAS_SYSTEM}`);
  if ("canvas" in props.create)
    throw new Error(
      "CanvasWorld takes its canvas state from extent and unitsPerMetre",
    );
  if (
    props.extent !== undefined &&
    (props.extent.length !== 2 || !props.extent.every(positive))
  )
    throw new RangeError("Canvas extent must be two finite positive values");
  if (props.unitsPerMetre !== undefined && !positive(props.unitsPerMetre))
    throw new RangeError("Canvas density must be finite and positive");
  const presentation = props.presentation;
  if (
    !presentation ||
    (presentation.root === true) === (presentation.anchor !== undefined)
  )
    throw new Error("CanvasWorld presents at the root or at one anchor");
}

/**
 * The Canvas state update that brings `state` to `request`, naming only the
 * values that differ, and the resulting state; undefined when nothing
 * differs. An omitted request value keeps the current one.
 */
export function canvasStateUpdate(
  request: {
    readonly extent?: readonly [number, number];
    readonly unitsPerMetre?: number;
  },
  state: CanvasState,
): { command: CanvasStateUpdateCommand; state: CanvasState } | undefined {
  const extent =
    request.extent &&
    (request.extent[0] !== state.extent[0] ||
      request.extent[1] !== state.extent[1])
      ? ([request.extent[0], request.extent[1]] as const)
      : undefined;
  const unitsPerMetre =
    request.unitsPerMetre !== undefined &&
    request.unitsPerMetre !== state.unitsPerMetre
      ? request.unitsPerMetre
      : undefined;
  if (extent === undefined && unitsPerMetre === undefined) return undefined;
  return {
    command: {
      type: "CanvasStateUpdateCommand",
      ...(extent ? { extent } : {}),
      ...(unitsPerMetre !== undefined ? { unitsPerMetre } : {}),
    },
    state: {
      extent: extent ?? state.extent,
      unitsPerMetre: unitsPerMetre ?? state.unitsPerMetre,
    },
  };
}

/** The CanvasWorld handle of the World a boundary created. */
export function canvasWorldHandle(handle: {
  readonly world: WorldReference;
  readonly closed: Promise<void>;
}): CanvasWorldHandle {
  return {
    get world() {
      return handle.world;
    },
    get output() {
      return canvasOutput(handle.world);
    },
    closed: handle.closed,
  };
}

/** Assign `value` to a React ref and return its release. */
export function assignRef<Value>(
  ref: Ref<Value> | undefined,
  value: Value,
): (() => void) | undefined {
  if (typeof ref === "function") {
    const release = ref(value);
    return typeof release === "function" ? release : () => ref(null);
  }
  if (ref) {
    ref.current = value;
    return () => {
      ref.current = null;
    };
  }
  return undefined;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

/**
 * Own one World selecting the Canvas System and present its canvas. A root
 * presentation needs an ancestor `IppCanvas` and supplies its root output; an
 * anchor presentation needs a parent created with `createRoot(client, {
 * host })`. A changed `create` or presentation creates a new World; removing
 * the element destroys its World.
 */
export function CanvasWorld(props: CanvasWorldProps): ReactNode {
  validateCanvasWorld(props);
  return createElement(
    props.presentation.root ? RootCanvasWorld : AnchoredCanvasWorld,
    props,
  );
}

function RootCanvasWorld({
  create,
  extent,
  unitsPerMetre,
  children,
  ref,
  onReady,
  onError,
}: CanvasWorldProps): ReactNode {
  const session = useContext(CanvasContext);
  if (session === undefined)
    throw new Error("A root CanvasWorld requires an ancestor IppCanvas");
  const creation = attachmentIdentity(create);
  const owner = useRef<OwnedCanvasWorld | undefined>(undefined);
  const request = {
    ...(extent ? { extent } : {}),
    ...(unitsPerMetre !== undefined ? { unitsPerMetre } : {}),
  };
  const latest = useRef({ create, request, onReady, onError });
  const [ready, setReady] = useState<{
    owner: OwnedCanvasWorld;
    handle: CanvasWorldHandle;
  }>();
  const [error, setError] = useState<{
    session: CanvasWorldSession;
    error: Error;
  }>();
  useLayoutEffect(() => {
    latest.current = { create, request, onReady, onError };
  });
  useLayoutEffect(() => {
    if (!session || session.isClosing) return;
    let active = true;
    const fallback = (failure: Error): void => {
      if (active) setError({ session, error: failure });
      else session.report(failure);
    };
    const report = (failure: Error): void => {
      const observer = latest.current.onError;
      if (!active || !observer) return fallback(failure);
      try {
        observer(failure);
      } catch (thrown) {
        fallback(asError(thrown));
      }
    };
    const reportDeclaration = (failure: Error): void => {
      const observer = latest.current.onError;
      if (active && observer)
        notify(() => observer(failure), session.reportDeclaration);
      else session.reportDeclaration(failure);
    };
    let world: OwnedCanvasWorld;
    try {
      world = session.openCanvasWorld(
        latest.current.create,
        latest.current.request,
        report,
        (reference, closed) => {
          if (!active) return;
          const handle = canvasWorldHandle({ world: reference, closed });
          setReady({ owner: world, handle });
          try {
            latest.current.onReady?.(handle);
          } catch (thrown) {
            report(asError(thrown));
          }
        },
        reportDeclaration,
      );
    } catch (failure) {
      report(asError(failure));
      return;
    }
    owner.current = world;
    return () => {
      active = false;
      owner.current = undefined;
      void world.remove().catch((failure) => session.report(asError(failure)));
    };
  }, [session, creation]);
  useLayoutEffect(() => {
    owner.current?.render(children);
  }, [children, session, creation]);
  useLayoutEffect(() => {
    owner.current?.update(latest.current.request);
  }, [extent?.[0], extent?.[1], unitsPerMetre, session, creation]);
  useLayoutEffect(() => {
    if (!ready || ready.owner !== owner.current) return;
    return assignRef(ref, ready.handle);
  }, [ref, ready, session, creation]);
  if (error?.session === session) throw error.error;
  return null;
}

function AnchoredCanvasWorld({
  create,
  extent,
  unitsPerMetre,
  presentation,
  children,
  ref,
  onReady,
  onError,
}: CanvasWorldProps): ReactNode {
  const attachmentRef = useMemo(
    () =>
      ref &&
      ((handle: AttachedWorldHandle | null) =>
        handle ? assignRef(ref, canvasWorldHandle(handle)) : undefined),
    [ref],
  );
  return createElement(
    AttachedWorldBoundary,
    {
      anchor: presentation.anchor!,
      child: { create },
      attachment: { mode: "surface-canvas" },
      canvas: {
        ...(extent ? { extent } : {}),
        ...(unitsPerMetre !== undefined ? { unitsPerMetre } : {}),
      },
      ...(attachmentRef ? { ref: attachmentRef } : {}),
      ...(onReady
        ? {
            onReady: (handle: AttachedWorldHandle) =>
              onReady(canvasWorldHandle(handle)),
          }
        : {}),
      ...(onError ? { onError } : {}),
    },
    children,
  );
}
