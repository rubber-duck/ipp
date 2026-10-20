import {
  Terminal,
  initBashkit,
  type BashOptions,
} from "@everruns/bashkit-wasm";
import {
  createCoreRuntime,
  KeyModifiers,
  type CoreRuntime,
  type CoreTerminal,
  type CoreRuntimeOptions,
  type RenderFrame,
} from "@gespenst/core/headless";
import type { GuiClientInput } from "@ipp/client";

export interface TerminalEngineOptions extends CoreRuntimeOptions {
  bashkit: Parameters<typeof initBashkit>[0];
  shell?: BashOptions;
  cols: number;
  rows: number;
  cellWidth: number;
  cellHeight: number;
}

/** One Worker owns both Wasm instances. All shell bytes remain inside that Worker. */
export class TerminalEngine {
  private readonly pending: Uint8Array[] = [];
  private pendingBytes = 0;
  private stopped = false;
  private pointerPressed = false;
  private constructor(
    private readonly runtime: CoreRuntime,
    readonly terminal: CoreTerminal,
    private readonly shell: Terminal,
  ) {
    terminal.on("input", ({ data: bytes }) => {
      if (this.stopped || !bytes.length) return;
      if (this.pendingBytes + bytes.length > 1024 * 1024)
        throw new Error("Terminal input exceeded its bounded queue");
      this.pending.push(bytes.slice());
      this.pendingBytes += bytes.length;
    });
  }

  static async create(options: TerminalEngineOptions): Promise<TerminalEngine> {
    await initBashkit(options.bashkit);
    const runtime = await createCoreRuntime(options);
    try {
      const terminal = runtime.createTerminal({
        cols: options.cols,
        rows: options.rows,
        cellWidthPx: options.cellWidth,
        cellHeightPx: options.cellHeight,
        scrollbackLines: 2000,
        defaultCursorStyle: "bar",
      });
      const shell = new Terminal({
        profile: "interactive",
        maxMemory: 64 * 1024 * 1024,
        timeoutMs: 5000,
        maxCommands: 10000,
        maxLoopIterations: 100000,
        username: "pilot",
        hostname: "ipp",
        ...options.shell,
        rows: options.rows,
        cols: options.cols,
      });
      return new TerminalEngine(runtime, terminal, shell);
    } catch (error) {
      runtime.dispose();
      throw error;
    }
  }

  input(event: GuiClientInput): void {
    if (this.stopped || this.exitCode !== undefined) return;
    switch (event.kind) {
      case "text":
        if (event.paste) this.terminal.paste(event.text);
        else this.terminal.text(event.text);
        break;
      case "key": {
        const modifiers =
          (event.modifiers & 1 ? KeyModifiers.shift : 0) |
          (event.modifiers & 2 ? KeyModifiers.control : 0) |
          (event.modifiers & 4 ? KeyModifiers.alt : 0) |
          (event.modifiers & 8 ? KeyModifiers.meta : 0);
        this.terminal.key({
          code: event.code,
          action: !event.pressed
            ? "release"
            : event.repeat
              ? "repeat"
              : "press",
          modifiers,
          ...(Array.from(event.key).length === 1 ? { text: event.key } : {}),
        });
        break;
      }
      case "pointer":
        if (event.phase === "down") this.pointerPressed = true;
        if (event.phase === "up" || event.phase === "cancel")
          this.pointerPressed = false;
        this.terminal.pointer({
          action:
            event.phase === "down"
              ? "press"
              : event.phase === "move"
                ? "motion"
                : "release",
          ...(event.button === "none"
            ? {}
            : {
                button:
                  event.button === "primary"
                    ? "left"
                    : event.button === "secondary"
                      ? "right"
                      : "middle",
              }),
          x: event.point[0],
          y: event.point[1],
          anyButtonPressed: this.pointerPressed,
        });
        break;
      case "wheel":
        this.terminal.wheel(
          { action: "motion", x: event.point[0], y: event.point[1] },
          Math.sign(event.delta[1]) * 3,
        );
        break;
    }
  }

  focus(focused: boolean): void {
    if (!this.stopped) this.terminal.focus(focused);
  }

  resize(cols: number, rows: number): void {
    if (this.stopped) return;
    this.terminal.resize(cols, rows);
    this.shell.resize(rows, cols);
  }

  get exitCode(): number | undefined {
    return this.shell.exitCode;
  }

  /** Consume partial shell writes and every terminal reply before publishing damage. */
  async run(): Promise<RenderFrame> {
    if (this.stopped) throw new Error("Terminal is closed");
    for (let round = 0; round < 4096; round++) {
      let accepted = 0;
      while (this.pending.length) {
        const bytes = this.pending[0]!;
        const count = this.shell.sendBytes(bytes);
        if (count < 0 || count > bytes.length)
          throw new Error("Invalid shell write count");
        accepted += count;
        this.pendingBytes -= count;
        if (count === bytes.length) this.pending.shift();
        else {
          this.pending[0] = bytes.subarray(count);
          break;
        }
      }
      await this.shell.runUntilIdle();
      const output = this.shell.takeOutput();
      if (output.length > 16 * 1024 * 1024)
        throw new Error("Terminal output exceeded its bounded drain");
      if (output.length) this.terminal.write(output);
      if (!this.pending.length || this.exitCode !== undefined)
        return this.terminal.render();
      if (!accepted && !output.length)
        throw new Error("Shell input made no progress");
    }
    throw new Error("Terminal reply loop exceeded its work bound");
  }

  dispose(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.pending.length = 0;
    this.pendingBytes = 0;
    this.terminal.dispose();
    this.runtime.dispose();
    // BashKit has no public Terminal.dispose: the owner terminates this Worker.
  }
}
