import { HostWireReader, HostWireWriter } from "./host-protocol.js";
import { writeView, type PresentationView } from "./host-presentation.js";
import { RequestNotSentError } from "./client.js";
import type { GuiTarget } from "./gui-types.js";

/** Exact published PickingGeometry incarnation; ordinary renderables are transparent to input. */
export type GuiPickingBlocker = Omit<GuiTarget, "component">;

export interface GuiPhysicalContextOptions {
  readonly blockers?: readonly GuiPickingBlocker[];
}

/**
 * The native text state an edit was made against: its target and generation.
 * The generation advances on every native edit, every commit to the focused
 * target and every external change of the committed text.
 */
export interface GuiTextFence {
  readonly target: GuiTarget;
  readonly generation: bigint;
}

export interface GuiNativeTextState {
  readonly fence: GuiTextFence;
  readonly text: string;
  readonly selectionStart: number;
  readonly selectionEnd: number;
  readonly composition?: {
    readonly text: string;
    readonly caretStart: number;
    readonly caretEnd: number;
  };
}

export type GuiNativeEdit =
  | { kind: "text"; text: string }
  | { kind: "selection"; start: number; end: number }
  | { kind: "composition"; text: string; caretStart: number; caretEnd: number }
  | { kind: "commitComposition" | "cancelComposition" }
  | {
      kind: "key";
      key:
        | "backspace"
        | "delete"
        | "left"
        | "right"
        | "home"
        | "end"
        | "selectAll"
        | "enter";
    };

function readNative(reader: HostWireReader): GuiNativeTextState {
  const world = { id: reader.u64(), incarnation: reader.u64() };
  const entity = reader.u64();
  const component = reader.u8() | (reader.u8() << 8);
  const incarnation = reader.u64();
  const generation = reader.u64();
  const text = reader.string();
  const selectionStart = reader.u32();
  const selectionEnd = reader.u32();
  const composition = reader.boolean()
    ? {
        text: reader.string(),
        caretStart: reader.u32(),
        caretEnd: reader.u32(),
      }
    : undefined;
  reader.end();
  const valid = (text: string, offsets: readonly number[]) => {
    const length = new TextEncoder().encode(text).length;
    return offsets.every((offset) => offset <= length);
  };
  if (
    !generation ||
    !incarnation ||
    !valid(text, [selectionStart, selectionEnd]) ||
    (composition &&
      !valid(composition.text, [composition.caretStart, composition.caretEnd]))
  )
    throw new Error("Invalid native text state");
  return {
    fence: {
      target: { world, entity, component, incarnation },
      generation,
    },
    text,
    selectionStart,
    selectionEnd,
    ...(composition ? { composition } : {}),
  };
}

export class GuiPhysicalRejection extends Error {
  constructor(readonly reason: string) {
    super(`Physical input rejected: ${reason}`);
    this.name = "GuiPhysicalRejection";
  }
}

export type GuiPhysicalKey =
  | "tab"
  | "backTab"
  | "enter"
  | "space"
  | "escape"
  | "left"
  | "right"
  | "up"
  | "down"
  | "home"
  | "end";
export type GuiPhysicalInput =
  | {
      kind: "pointerDown" | "pointerUp";
      button?: "primary" | "secondary" | "auxiliary";
      pointer: bigint;
      point: readonly [number, number];
    }
  | {
      kind: "pointerMove";
      pointer: bigint;
      point: readonly [number, number];
    }
  | { kind: "pointerCancel"; pointer: bigint }
  | {
      kind: "wheel";
      point: readonly [number, number];
      delta: readonly [number, number];
    }
  | { kind: "key"; key: GuiPhysicalKey }
  | { kind: "blur" };

/** Correlated physical routing settlement, not an effect stream or completed frame. */
export interface GuiInputRoutingOutcome {
  readonly disposition: "routed" | "miss" | "blocked" | "unhandled";
  readonly applied: number;
  readonly rejected: number;
  readonly cancelled: number;
  readonly error?: string;
  readonly remaining?: readonly [number, number];
}

export interface GuiInputCancellation {
  readonly pointers: readonly bigint[];
  readonly focus: boolean;
}

const keys: readonly GuiPhysicalKey[] = [
  "tab",
  "backTab",
  "enter",
  "space",
  "escape",
  "left",
  "right",
  "up",
  "down",
  "home",
  "end",
];

type Request = (
  tag: number,
  encode: (writer: HostWireWriter) => void,
  accept: (reader: HostWireReader) => void,
) => Promise<HostWireReader>;

/** One physical connection's explicitly selected input ownership, not an authoring session. */
export class HostPhysicalInput {
  private readonly contexts = new Map<bigint, GuiPhysicalContext>();
  private stopped = false;

  /**
   * `tag` and `limit` resolve Host contract tags and physical input bounds
   * (`GUI_PHYSICAL_POINTERS`, `GUI_PHYSICAL_BLOCKERS`) of the connected target.
   */
  constructor(
    private readonly request: Request,
    private readonly tag: (name: string) => number,
    private readonly limit: (name: string) => number,
  ) {}

  async open(
    view: PresentationView,
    options: GuiPhysicalContextOptions = {},
  ): Promise<GuiPhysicalContext> {
    if (this.stopped) throw new Error("Physical input connection closed");
    let context: GuiPhysicalContext | undefined;
    await this.control(
      (writer) => {
        writer.u8(this.tag("GUI_PHYSICAL_REQUEST_OPEN"));
        writeView(writer, view);
        const blockers = options.blockers ?? [];
        if (blockers.length > this.limit("GUI_PHYSICAL_BLOCKERS"))
          throw new Error("Physical blocker list exceeds framing limit");
        writer.u32(blockers.length);
        for (const blocker of blockers) {
          writer.u64(blocker.world.id);
          writer.u64(blocker.world.incarnation);
          writer.u64(blocker.entity);
          writer.u64(blocker.incarnation);
        }
      },
      (reader, kind) => {
        if (kind !== this.tag("GUI_PHYSICAL_RESPONSE_OPENED"))
          throw new Error("Invalid physical context acknowledgement");
        const identity = reader.u64();
        reader.end();
        if (identity === 0n || this.contexts.has(identity))
          throw new Error("Invalid physical context identity");
        context = new GuiPhysicalContext(
          identity,
          view,
          (encode, activate) => this.control(encode, activate),
          () => this.contexts.delete(identity),
          this.tag,
        );
        this.contexts.set(identity, context);
      },
    );
    if (!context || this.stopped || context.isClosed)
      throw new Error("Physical input connection closed during acquisition");
    return context;
  }

  notification(reader: HostWireReader): void {
    const payload = this.payload(reader);
    const kind = payload.u8();
    if (
      ![
        this.tag("GUI_PHYSICAL_RESPONSE_REVOKED"),
        this.tag("GUI_PHYSICAL_RESPONSE_CANCELLED"),
        this.tag("GUI_PHYSICAL_RESPONSE_NATIVE"),
      ].includes(kind)
    )
      throw new Error("Invalid physical context notification");
    const identity = payload.u64();
    if (identity === 0n) throw new Error("Invalid physical context identity");
    if (kind === this.tag("GUI_PHYSICAL_RESPONSE_NATIVE")) {
      const state = payload.boolean()
        ? readNative(new HostWireReader(payload.raw(payload.u32())))
        : null;
      payload.end();
      this.contexts.get(identity)?.observeText(state);
    } else if (kind === this.tag("GUI_PHYSICAL_RESPONSE_REVOKED")) {
      payload.end();
      this.contexts
        .get(identity)
        ?.stop(new Error("Physical input context revoked"));
    } else {
      const count = payload.u8();
      if (count > this.limit("GUI_PHYSICAL_POINTERS"))
        throw new Error("Invalid physical cancellation count");
      const pointers = Array.from({ length: count }, () => payload.u64());
      const focus = payload.boolean();
      payload.end();
      this.contexts.get(identity)?.cancel({ pointers, focus });
    }
  }

  stop(reason: Error): void {
    this.stopped = true;
    for (const context of this.contexts.values()) context.stop(reason);
    this.contexts.clear();
  }

  private payload(reader: HostWireReader): HostWireReader {
    if (reader.u8() !== this.tag("HOST_RESPONSE_GUI_INPUT"))
      throw new Error("Invalid physical input response");
    const payload = reader.raw(reader.u32());
    reader.end();
    return new HostWireReader(payload);
  }

  private async control(
    encode: (writer: HostWireWriter) => void,
    activate?: (reader: HostWireReader, kind: number) => void,
  ): Promise<HostWireReader> {
    const body = new HostWireWriter();
    encode(body);
    let rejected: GuiPhysicalRejection | undefined;
    const response = await this.request(
      this.tag("HOST_REQUEST_GUI_INPUT"),
      (writer) => {
        const bytes = body.finish();
        writer.u32(bytes.length);
        writer.raw(bytes);
      },
      (reader) => {
        const result = this.payload(reader);
        const kind = result.u8();
        if (kind === this.tag("GUI_PHYSICAL_RESPONSE_REJECTED")) {
          rejected = new GuiPhysicalRejection(result.string());
          result.end();
        } else activate?.(result, kind);
      },
    );
    if (rejected) throw rejected;
    const payload = this.payload(response);
    return payload;
  }
}

/** Exact input generation. Releasing it never clears the root or closes its Worlds. */
export class GuiPhysicalContext {
  private text: GuiNativeTextState | null = null;
  private readonly textListeners = new Set<
    (
      state: GuiNativeTextState | null,
      origin: "terminal" | "notification",
    ) => void
  >();

  get nativeText(): GuiNativeTextState | null {
    return this.text;
  }

  onText(
    listener: (
      state: GuiNativeTextState | null,
      origin: "terminal" | "notification",
    ) => void,
  ): () => void {
    if (!this.stopped) this.textListeners.add(listener);
    listener(this.text, "notification");
    return () => this.textListeners.delete(listener);
  }

  observeText(
    state: GuiNativeTextState | null,
    origin: "terminal" | "notification" = "notification",
  ): void {
    if (this.stopped) return;
    const previous = this.text;
    if (
      state &&
      previous &&
      state.fence.target.world.id === previous.fence.target.world.id &&
      state.fence.target.world.incarnation ===
        previous.fence.target.world.incarnation &&
      state.fence.generation <= previous.fence.generation
    )
      return;
    if (state === null && previous === null) return;
    this.text = state;
    for (const listener of [...this.textListeners]) {
      if (this.stopped) break;
      try {
        listener(state, origin);
      } catch (error) {
        queueMicrotask(() => {
          throw error;
        });
      }
    }
  }
  private stopped: Error | undefined;
  private closing: Promise<void> | undefined;
  private readonly listeners = new Set<(reason: Error) => void>();
  private readonly cancellations = new Set<
    (event: GuiInputCancellation) => void
  >();

  constructor(
    readonly identity: bigint,
    readonly view: PresentationView,
    private readonly request: (
      encode: (writer: HostWireWriter) => void,
      activate?: (reader: HostWireReader, kind: number) => void,
    ) => Promise<HostWireReader>,
    private readonly released: () => void,
    private readonly tag: (name: string) => number,
  ) {}

  get isClosed(): boolean {
    return this.stopped !== undefined;
  }

  onClose(listener: (reason: Error) => void): () => void {
    if (this.stopped) listener(this.stopped);
    else this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onCancel(listener: (event: GuiInputCancellation) => void): () => void {
    if (!this.stopped) this.cancellations.add(listener);
    return () => this.cancellations.delete(listener);
  }

  cancel(event: GuiInputCancellation): void {
    if (this.stopped) return;
    if (event.focus) this.observeText(null);
    for (const listener of [...this.cancellations]) {
      if (this.stopped) break;
      try {
        listener(event);
      } catch (error) {
        queueMicrotask(() => {
          throw error;
        });
      }
    }
  }

  stop(reason: Error): void {
    if (this.stopped) return;
    this.stopped = reason;
    const hadText = this.text !== null;
    this.text = null;
    this.released();
    if (hadText) {
      for (const listener of [...this.textListeners]) {
        try {
          listener(null, "notification");
        } catch (error) {
          queueMicrotask(() => {
            throw error;
          });
        }
      }
    }
    for (const listener of [...this.listeners]) {
      try {
        listener(reason);
      } catch (error) {
        queueMicrotask(() => {
          throw error;
        });
      }
    }
    this.listeners.clear();
    this.cancellations.clear();
    this.textListeners.clear();
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.stop(new Error("Physical input context closed"));
    this.closing = this.request((writer) => {
      writer.u8(this.tag("GUI_PHYSICAL_REQUEST_CLOSE"));
      writer.u64(this.identity);
    })
      .then((reader) => {
        if (reader.u8() !== this.tag("GUI_PHYSICAL_RESPONSE_CLOSED"))
          throw new Error("Invalid physical input release");
        reader.end();
      })
      .catch((error: unknown) => {
        if (error instanceof RequestNotSentError) this.closing = undefined;
        throw error;
      });
    return this.closing;
  }

  async send(input: GuiPhysicalInput): Promise<GuiInputRoutingOutcome> {
    if (this.stopped) throw this.stopped;
    const body = new HostWireWriter();
    body.u8(this.tag("GUI_PHYSICAL_REQUEST_EVENT"));
    body.u64(this.identity);
    const point = (values: readonly [number, number]) => {
      for (const value of values) {
        if (!Number.isFinite(value))
          throw new Error("Invalid physical input coordinate");
        body.f32(value);
      }
    };
    switch (input.kind) {
      case "pointerDown":
      case "pointerMove":
      case "pointerUp":
        body.u8(
          this.tag(
            {
              pointerDown: "GUI_PHYSICAL_EVENT_POINTER_DOWN",
              pointerMove: "GUI_PHYSICAL_EVENT_POINTER_MOVE",
              pointerUp: "GUI_PHYSICAL_EVENT_POINTER_UP",
            }[input.kind],
          ),
        );
        body.u64(input.pointer);
        point(input.point);
        if (input.kind !== "pointerMove") {
          const button = ["primary", "secondary", "auxiliary"].indexOf(
            input.button ?? "primary",
          );
          if (button < 0) throw new Error("Invalid physical pointer button");
          body.u8(
            this.tag(
              `GUI_PHYSICAL_BUTTON_${["PRIMARY", "SECONDARY", "AUXILIARY"][button]}`,
            ),
          );
        }
        break;
      case "pointerCancel":
        body.u8(this.tag("GUI_PHYSICAL_EVENT_POINTER_CANCEL"));
        body.u64(input.pointer);
        break;
      case "wheel":
        body.u8(this.tag("GUI_PHYSICAL_EVENT_WHEEL"));
        point(input.point);
        point(input.delta);
        break;
      case "key": {
        const index = keys.indexOf(input.key);
        if (index < 0) throw new Error("Invalid physical key");
        body.u8(this.tag("GUI_PHYSICAL_EVENT_KEY"));
        body.u8(
          this.tag(
            `GUI_PHYSICAL_KEY_${input.key === "backTab" ? "BACK_TAB" : input.key.toUpperCase()}`,
          ),
        );
        break;
      }
      case "blur":
        body.u8(this.tag("GUI_PHYSICAL_EVENT_BLUR"));
        break;
    }
    return this.submit(body);
  }

  /** Exact native-buffer edit. A delayed clipboard/composition retains its original fence. */
  async editText(
    fence: GuiTextFence,
    edit: GuiNativeEdit,
  ): Promise<GuiInputRoutingOutcome> {
    if (this.stopped) throw this.stopped;
    const body = new HostWireWriter();
    body.u8(this.tag("GUI_PHYSICAL_REQUEST_TEXT"));
    body.u64(this.identity);
    body.u64(fence.target.world.id);
    body.u64(fence.target.world.incarnation);
    body.u64(fence.target.entity);
    if (
      !Number.isInteger(fence.target.component) ||
      fence.target.component < 0 ||
      fence.target.component > 65535
    )
      throw new Error("Invalid text component");
    body.u8(fence.target.component & 255);
    body.u8(fence.target.component >>> 8);
    body.u64(fence.target.incarnation);
    body.u64(fence.generation);
    switch (edit.kind) {
      case "text":
        body.u8(this.tag("GUI_NATIVE_EDIT_INSERT"));
        body.string(edit.text);
        break;
      case "selection":
        body.u8(this.tag("GUI_NATIVE_EDIT_SELECTION"));
        body.u32(edit.start);
        body.u32(edit.end);
        break;
      case "composition":
        body.u8(this.tag("GUI_NATIVE_EDIT_COMPOSE"));
        body.string(edit.text);
        body.u32(edit.caretStart);
        body.u32(edit.caretEnd);
        break;
      case "commitComposition":
        body.u8(this.tag("GUI_NATIVE_EDIT_COMMIT_COMPOSITION"));
        break;
      case "cancelComposition":
        body.u8(this.tag("GUI_NATIVE_EDIT_CANCEL_COMPOSITION"));
        break;
      case "key": {
        const index = [
          "backspace",
          "delete",
          "left",
          "right",
          "home",
          "end",
          "selectAll",
          "enter",
        ].indexOf(edit.key);
        if (index < 0) throw new Error("Invalid native key");
        body.u8(
          this.tag(
            `GUI_NATIVE_EDIT_${["BACKSPACE", "DELETE", "LEFT", "RIGHT", "HOME", "END", "SELECT_ALL", "SUBMIT"][index]}`,
          ),
        );
        break;
      }
    }
    return this.submit(body);
  }

  private async submit(body: HostWireWriter): Promise<GuiInputRoutingOutcome> {
    try {
      let outcome: GuiInputRoutingOutcome | undefined;
      await this.request(
        (writer) => writer.raw(body.finish()),
        (reader, kind) => {
          if (kind !== this.tag("GUI_PHYSICAL_RESPONSE_ROUTED"))
            throw new Error("Invalid physical routing terminal");
          outcome = this.readRouting(reader);
        },
      );
      if (!outcome) throw new Error("Missing routing outcome");
      return outcome;
    } catch (error) {
      if (
        !(error instanceof RequestNotSentError) &&
        !(error instanceof GuiPhysicalRejection)
      )
        this.stop(error instanceof Error ? error : new Error(String(error)));
      throw error;
    }
  }

  private readRouting(reader: HostWireReader): GuiInputRoutingOutcome {
    const dispositionTag = reader.u8();
    const disposition = (
      ["routed", "miss", "blocked", "unhandled"] as const
    ).find(
      (kind) =>
        this.tag(`GUI_PHYSICAL_DISPOSITION_${kind.toUpperCase()}`) ===
        dispositionTag,
    );
    if (disposition === undefined)
      throw new Error("Invalid routing disposition");
    const applied = reader.u32();
    const rejected = reader.u32();
    const cancelled = reader.u32();
    const error = reader.boolean() ? reader.string() : undefined;
    const remaining = reader.boolean()
      ? ([reader.f32(), reader.f32()] as const)
      : undefined;
    const native = reader.boolean()
      ? readNative(new HostWireReader(reader.raw(reader.u32())))
      : undefined;
    reader.end();
    if (native) this.observeText(native, "terminal");
    return {
      disposition,
      applied,
      rejected,
      cancelled,
      ...(error === undefined ? {} : { error }),
      ...(remaining === undefined ? {} : { remaining }),
    };
  }
}
