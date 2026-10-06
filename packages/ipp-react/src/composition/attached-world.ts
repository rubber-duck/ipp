import {
  createContext,
  createElement,
  Fragment,
  useContext,
  useState,
  useSyncExternalStore,
  type ReactNode,
  type Ref,
} from "react";
import type {
  Client,
  AttachmentReceipt,
  CameraOutputReference,
  HostClientBase,
  OutputReference,
  WorldCreateOptions,
  WorldReference,
} from "@ipp/client";
import {
  reconciler,
  type ReactWorldContainer,
} from "../reconciler/host-config.js";
import { attachmentIdentity } from "./attachment-identity.js";

export const ATTACHED_WORLD_HOST_TYPE = "ipp-attached-world";

export type ReactCompositionHost = Pick<
  HostClientBase<Client>,
  | "sessions"
  | "createWorld"
  | "openWorld"
  | "destroyWorld"
  | "bindOutput"
  | "resolveOutput"
  | "listWorlds"
>;

export type AttachedWorldChild =
  | { borrow: WorldReference; create?: never }
  | { create: WorldCreateOptions; borrow?: never };

/**
 * How the child World appears on the parent anchor. SurfaceCanvas presents the
 * child World's canvas, which names no entity; SurfaceCamera names one of the
 * child's Camera outputs, as a reference or a declared entity.
 */
export type AttachedWorldAttachment =
  | { mode: "spatial"; output?: never }
  | { mode: "surface-canvas"; output?: never }
  | {
      mode: "surface-camera";
      output: CameraOutputReference | { entity: string | bigint };
    };

export interface AttachedWorldHandle {
  readonly world: WorldReference;
  /** The presented output: the child's canvas or its selected camera. */
  readonly output: OutputReference | undefined;
  readonly closed: Promise<void>;
}

export interface AttachedWorldCleanupJournal {
  readonly parent: WorldReference;
  readonly receiptSession: bigint | undefined;
  readonly child: WorldReference | undefined;
  readonly childSession: bigint | undefined;
  readonly creatorOwned: boolean;
  readonly receipts: readonly AttachmentReceipt[];
  readonly unknownAttachmentOutcome: unknown;
  readonly preparationFailure: unknown;
}

export interface AttachedWorldCleanupRecovery {
  readonly journal: AttachedWorldCleanupJournal;
  retry(): Promise<void>;
  abandon(): Promise<AttachedWorldCleanupJournal>;
}

export class AttachedWorldCleanupError extends AggregateError {
  constructor(
    errors: readonly unknown[],
    readonly journal: AttachedWorldCleanupJournal,
    readonly recovery: AttachedWorldCleanupRecovery,
  ) {
    super(errors, "Attached World cleanup is incomplete");
    this.name = "AttachedWorldCleanupError";
  }
}

export interface AttachedWorldProps {
  anchor: string | bigint;
  child: AttachedWorldChild;
  attachment: AttachedWorldAttachment;
  children?: ReactNode;
  ref?: Ref<AttachedWorldHandle>;
  onReady?: (handle: AttachedWorldHandle) => void;
  onError?: (error: Error) => void;
}

/**
 * Canvas state a CanvasWorld asks of the World it creates: the initial state
 * at creation, and later values sent as Canvas state updates. An omitted value
 * keeps the World's current value.
 */
export interface AttachedCanvasRequest {
  readonly extent?: readonly [number, number];
  readonly unitsPerMetre?: number;
}

export class AttachedWorldSlot {
  container: ReactWorldContainer | undefined;
  error: Error | undefined;
  private revision = 0;
  private readonly listeners = new Set<() => void>();
  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  readonly snapshot = (): number => this.revision;

  update(container: ReactWorldContainer | undefined, error?: Error): void {
    this.container = container;
    this.error = error;
    this.revision++;
    for (const listener of this.listeners) listener();
  }
}

export const AttachmentContext = createContext<ReactWorldContainer | null>(
  null,
);

export interface AttachedWorldDescription {
  slot: AttachedWorldSlot;
  anchor: string | bigint;
  child: AttachedWorldChild;
  attachment: AttachedWorldAttachment;
  signature: string;
  /** Canvas state of a created canvas child; not part of its identity. */
  canvas: AttachedCanvasRequest | undefined;
  suspended: boolean;
  portal: ReactWorldContainer | undefined;
  attachmentRef: Ref<AttachedWorldHandle> | undefined;
  onReady: AttachedWorldProps["onReady"];
  onError: AttachedWorldProps["onError"];
}

export function describeAttachedWorld(
  props: Readonly<Record<string, unknown>>,
): AttachedWorldDescription {
  const {
    slot,
    anchor,
    child,
    attachment,
    canvas,
    portal,
    attachmentRef,
    onReady,
    onError,
  } = props;
  if (
    !(slot instanceof AttachedWorldSlot) ||
    (typeof anchor !== "string" && typeof anchor !== "bigint")
  )
    throw new Error("AttachedWorld requires an explicit parent anchor");
  if (
    !child ||
    typeof child !== "object" ||
    "borrow" in child === "create" in child
  )
    throw new Error(
      "AttachedWorld requires exactly one borrowed or created child",
    );
  if (
    !attachment ||
    typeof attachment !== "object" ||
    !("mode" in attachment) ||
    !["spatial", "surface-camera", "surface-canvas"].includes(
      String(attachment.mode),
    )
  )
    throw new Error("AttachedWorld requires an explicit attachment mode");
  if (attachment.mode === "surface-camera" && !("output" in attachment))
    throw new Error("SurfaceCamera attachments require an explicit camera");
  if (attachment.mode !== "surface-camera" && "output" in attachment)
    throw new Error(
      "Only SurfaceCamera attachments name an output; SurfaceCanvas presents the child World's canvas",
    );
  if (canvas !== undefined && !("create" in child))
    throw new Error("Only a created child World takes canvas state");
  const values = structuredClone({ anchor, child, attachment }) as Pick<
    AttachedWorldDescription,
    "anchor" | "child" | "attachment"
  >;
  return {
    ...values,
    canvas:
      canvas === undefined
        ? undefined
        : (structuredClone(canvas) as AttachedCanvasRequest),
    slot,
    suspended: false,
    signature: attachmentIdentity(values),
    portal: portal as ReactWorldContainer | undefined,
    attachmentRef: attachmentRef as Ref<AttachedWorldHandle> | undefined,
    onReady: onReady as AttachedWorldProps["onReady"],
    onError: onError as AttachedWorldProps["onError"],
  };
}

/** The attached-World boundary; a CanvasWorld also passes its canvas state. */
export function AttachedWorldBoundary({
  children,
  ref,
  ...props
}: AttachedWorldProps & { canvas?: AttachedCanvasRequest }): ReactNode {
  const parent = useContext(AttachmentContext);
  if (!parent?.attachmentHost)
    throw new Error("AttachedWorld requires createRoot(client, { host })");
  const [slot] = useState(() => new AttachedWorldSlot());
  useSyncExternalStore(slot.subscribe, slot.snapshot, slot.snapshot);
  if (slot.error) throw slot.error;
  return createElement(
    Fragment,
    null,
    createElement(ATTACHED_WORLD_HOST_TYPE, {
      ...props,
      slot,
      portal: slot.container,
      attachmentRef: ref,
    }),
    slot.container
      ? (reconciler.createPortal(
          createElement(AttachmentContext, { value: slot.container }, children),
          slot.container,
          null,
        ) as unknown as ReactNode)
      : null,
  );
}

export const AttachedWorld: (props: AttachedWorldProps) => ReactNode =
  AttachedWorldBoundary;
