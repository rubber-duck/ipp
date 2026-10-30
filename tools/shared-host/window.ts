/** Native development-window events. Rendering and input use the ordinary Host API. */
import type {
  GuiPhysicalContext,
  GuiPhysicalInput,
  GuiPhysicalKey,
} from "@ipp/client";

export interface WindowEvent {
  kind: number;
  code: number;
  shift: boolean;
  x: number;
  y: number;
  dx: number;
  dy: number;
  text: string;
}

export class NativeWindowChannel {
  private serial = 0;
  private cefSerial = 0x80000000;
  private pending:
    | {
        id: number;
        resolve: (events: WindowEvent[]) => void;
        reject: (error: Error) => void;
        timer: NodeJS.Timeout;
      }
    | undefined;
  private failed: Error | undefined;
  private readonly cefRequests = new Map<
    number,
    {
      resolve(bytes: Buffer): void;
      reject(error: Error): void;
      timer: NodeJS.Timeout;
    }
  >();

  private constructor(private readonly socket: WebSocket) {
    socket.binaryType = "arraybuffer";
    socket.addEventListener("message", ({ data }) => {
      try {
        const bytes = Buffer.from(data as ArrayBuffer);
        if (bytes[0] === 1 || bytes[0] === 2) return;
        if (bytes[0] === 4) throw new Error(bytes.subarray(1).toString());
        if (bytes[0] === 7 && bytes.length >= 6) {
          const request = this.cefRequests.get(bytes.readUInt32LE(1));
          if (!request) throw new Error("Unknown native CEF reply");
          this.cefRequests.delete(bytes.readUInt32LE(1));
          clearTimeout(request.timer);
          if (bytes[5]) request.reject(new Error(bytes.subarray(6).toString()));
          else request.resolve(bytes.subarray(6));
          return;
        }
        if (bytes[0] !== 6 || bytes.length < 9)
          throw new Error("Invalid native window reply");
        const id = bytes.readUInt32LE(1),
          count = bytes.readUInt32LE(5);
        if (
          !this.pending ||
          this.pending.id !== id ||
          count > 256 ||
          bytes.length !== 9 + count * 176
        )
          throw new Error("Invalid native window event batch");
        const events: WindowEvent[] = [];
        for (let i = 0; i < count; i++) {
          const offset = 9 + i * 176;
          const length = bytes.readUInt32LE(offset + 12);
          if (length > 128) throw new Error("Invalid native text length");
          events.push({
            kind: bytes.readUInt32LE(offset),
            code: bytes.readUInt32LE(offset + 4),
            shift: (bytes.readUInt32LE(offset + 8) & 1) !== 0,
            x: bytes.readDoubleLE(offset + 16),
            y: bytes.readDoubleLE(offset + 24),
            dx: bytes.readDoubleLE(offset + 32),
            dy: bytes.readDoubleLE(offset + 40),
            text: bytes
              .subarray(offset + 48, offset + 48 + length)
              .toString("utf8"),
          });
        }
        clearTimeout(this.pending.timer);
        const { resolve } = this.pending;
        this.pending = undefined;
        resolve(events);
      } catch (error) {
        this.fail(error as Error);
      }
    });
    socket.addEventListener("error", () =>
      this.fail(new Error("Native window socket failed")),
    );
    socket.addEventListener("close", () =>
      this.fail(new Error("Native window channel closed")),
    );
  }

  static async open(url: string) {
    const socket = new WebSocket(url);
    const channel = new NativeWindowChannel(socket);
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        socket.close();
        reject(new Error("Native window connection timed out"));
      }, 10_000);
      socket.addEventListener(
        "open",
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
      socket.addEventListener(
        "error",
        () => {
          clearTimeout(timer);
          reject(new Error("Native window connection failed"));
        },
        { once: true },
      );
    });
    return channel;
  }

  private fail(error: Error) {
    this.failed ??= error;
    for (const request of this.cefRequests.values()) {
      clearTimeout(request.timer);
      request.reject(error);
    }
    this.cefRequests.clear();
    if (this.pending) {
      clearTimeout(this.pending.timer);
      this.pending.reject(error);
      this.pending = undefined;
    }
  }

  cef(
    operation: number,
    source = 0,
    payload = Buffer.alloc(0),
  ): Promise<Buffer> {
    if (this.failed) return Promise.reject(this.failed);
    if (this.cefRequests.size >= 256 || payload.length > 8182)
      return Promise.reject(new Error("Native CEF command capacity exceeded"));
    const id = ++this.cefSerial;
    const bytes = Buffer.alloc(10 + payload.length);
    bytes[0] = 12;
    bytes.writeUInt32LE(id, 1);
    bytes[5] = operation;
    bytes.writeUInt32LE(source, 6);
    payload.copy(bytes, 10);
    return new Promise((resolve, reject) => {
      this.cefRequests.set(id, {
        resolve,
        reject,
        timer: setTimeout(
          () => this.fail(new Error("Native CEF request timed out")),
          10_000,
        ),
      });
      this.socket.send(bytes);
    });
  }

  poll(): Promise<WindowEvent[]> {
    if (this.failed) return Promise.reject(this.failed);
    if (this.pending)
      return Promise.reject(new Error("Native window poll already pending"));
    const id = ++this.serial;
    return new Promise((resolve, reject) => {
      this.pending = {
        id,
        resolve,
        reject,
        timer: setTimeout(
          () => this.fail(new Error("Native window poll timed out")),
          10_000,
        ),
      };
      const bytes = Buffer.alloc(5);
      bytes[0] = 10;
      bytes.writeUInt32LE(id, 1);
      this.socket.send(bytes);
    });
  }

  /** Instrumentation-only context lifecycle control used by maintained drivers. */
  testShutdown() {
    this.socket.send(new Uint8Array([13]));
  }

  testContextLoss() {
    this.socket.send(new Uint8Array([7]));
    this.socket.send(new Uint8Array([8]));
  }

  /** Instrumentation builds only: deliver through the platform's actual event queue. */
  testEvent(kind: number, x = 0, y = 0) {
    const bytes = Buffer.alloc(10);
    bytes[0] = 11;
    bytes[1] = kind;
    bytes.writeFloatLE(x, 2);
    bytes.writeFloatLE(y, 6);
    this.socket.send(bytes);
  }

  close() {
    this.fail(new Error("Native window channel disposed"));
    this.socket.close();
  }
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
  "contextMenu",
  "f10",
];
const editKeys = new Map<
  number,
  | "enter"
  | "left"
  | "right"
  | "home"
  | "end"
  | "backspace"
  | "delete"
  | "selectAll"
>([
  [3, "enter"],
  [6, "left"],
  [7, "right"],
  [10, "home"],
  [11, "end"],
  [14, "backspace"],
  [15, "delete"],
  [16, "selectAll"],
]);

export async function dispatchWindowInput(
  event: WindowEvent,
  context: GuiPhysicalContext,
  send: (input: GuiPhysicalInput) => Promise<void>,
) {
  const native = context.nativeText;
  const point = [event.x, event.y] as const;
  if (event.kind === 6) {
    if (native?.clientInput)
      await send({
        kind: "clientInput",
        fence: native.fence,
        input: { kind: "text", text: event.text },
      });
    else if (native)
      await context.editText(native.fence, { kind: "text", text: event.text });
    else if (event.text === " ") await send({ kind: "key", key: "space" });
  } else if (event.kind === 5) {
    const edit = editKeys.get(event.code);
    if (native?.clientInput) {
      const key = [
        "Tab",
        "Tab",
        "Enter",
        " ",
        "Escape",
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Backspace",
        "Delete",
        "a",
      ][event.code - 1];
      if (key)
        await send({
          kind: "clientInput",
          fence: native.fence,
          input: {
            kind: "key",
            key,
            code: key,
            modifiers: event.code === 16 ? 8 : event.shift ? 1 : 0,
            repeat: false,
            pressed: true,
          },
        });
    } else if (native && !native.clientInput && edit)
      await context.editText(native.fence, { kind: "key", key: edit });
    else if (keys[event.code - 1])
      await send({
        kind: "key",
        key: keys[event.code - 1]!,
        shift: event.shift,
      });
  } else if (event.kind === 1)
    await send({ kind: "pointerMove", pointer: 1n, point });
  else if (event.kind === 2 || event.kind === 3)
    await send({
      kind: event.kind === 2 ? "pointerDown" : "pointerUp",
      pointer: 1n,
      point,
      button:
        event.code === 1
          ? "secondary"
          : event.code === 2
            ? "auxiliary"
            : "primary",
    });
  else if (event.kind === 4)
    await send({
      kind: "wheel",
      point,
      delta: [event.dx, event.dy],
      shift: event.shift,
    });
  else if (event.kind === 7) await send({ kind: "blur" });
}
