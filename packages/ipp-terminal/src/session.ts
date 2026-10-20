import type {
  GuiClientInput,
  GuiEffectSubscription,
  LifecycleTargetWatch,
} from "@ipp/client";
import { TerminalEngine } from "./engine.js";
import { TerminalPaint } from "./paint.js";
import { apply, component, handle, TerminalBatchError } from "./commands.js";
import type { TerminalFont, TerminalSessionOptions } from "./types.js";

/** A transport-independent terminal connected to one exact GUI target lifetime. */
export class TerminalSession {
  private engine?: TerminalEngine;
  private paint?: TerminalPaint;
  private input?: GuiEffectSubscription;
  private lifetime?: LifecycleTargetWatch;
  private layout?: LifecycleTargetWatch;
  private closing?: Promise<void>;
  private initializing?: Promise<void>;
  private stopped = false;
  private initialized = false;
  private work: Promise<void> = Promise.resolve();
  private running = false;
  private queue: ({ input: GuiClientInput } | { focus: boolean })[] = [];
  private queuedBytes = 0;
  private dimensions: readonly [number, number] = [80, 24];
  private resizePending = false;
  private constructor(private readonly options: TerminalSessionOptions) {}

  static async start(
    options: TerminalSessionOptions,
  ): Promise<TerminalSession> {
    const session = new TerminalSession(options);
    try {
      session.initializing = session.initialize();
      await session.initializing;
      return session;
    } catch (error) {
      await session.close().catch(() => {});
      throw error;
    }
  }

  private async initialize(): Promise<void> {
    const { client, target } = this.options;
    const descriptor = client.components.GuiInputTarget!;
    if (!descriptor)
      throw new Error("This Host has no client-managed input capability");
    this.lifetime = await client.watchLifecycle(
      [
        {
          target: {
            kind: "component",
            entity: target,
            component: descriptor.id,
          },
          kinds: 32 | 64,
        },
      ],
      () => {
        void this.close().catch(this.options.onError);
      },
    );
    const baseline = this.lifetime.baselines[0]?.lifetime;
    if (baseline?.kind !== "component" || baseline.incarnation === null)
      throw new Error("Terminal input target is absent");
    const fontResponse = await fetch(this.options.fontMetricsUrl);
    if (!fontResponse.ok)
      throw new Error(`Terminal font metadata: HTTP ${fontResponse.status}`);
    const font = (await fontResponse.json()) as TerminalFont;
    if (
      ![
        font.advance,
        font.unitsPerEm,
        font.ascent,
        this.options.fontSize,
        this.options.lineHeight,
      ].every((v) => Number.isFinite(v) && v > 0) ||
      !font.glyphs
    )
      throw new Error("Invalid terminal font metrics");
    const cellWidth = (this.options.fontSize * font.advance) / font.unitsPerEm;
    const layout = client.components.GuiLayout!;
    const width = layout.fields.width!.offset,
      height = layout.fields.height!.offset;
    const setSize = (w: number, h: number) => {
      if (!(w > 0 && h > 0))
        throw new Error(
          "Terminal requires explicit positive layout dimensions",
        );
      const dimensions = [
        Math.max(2, Math.min(160, Math.floor(w / cellWidth))),
        Math.max(2, Math.min(80, Math.floor(h / this.options.lineHeight))),
      ] as const;
      if (dimensions.some((v, i) => v !== this.dimensions[i])) {
        this.dimensions = dimensions;
        this.resizePending = true;
        this.schedule();
      }
    };
    const snapshot = await client.inspectPage({
      collection: "entities",
      target,
      limit: 1,
    });
    const fields = snapshot.entities
      .find((entity) => entity.id === target)
      ?.components.find((value) => value.component === layout.id)?.fields;
    if (!fields) throw new Error("Terminal target requires GuiLayout");
    setSize(Number(fields.width), Number(fields.height));
    this.layout = await client.watchLifecycle(
      [
        {
          target: {
            kind: "value",
            entity: target,
            component: layout.id,
            fields: [width, height].sort((a, b) => a - b),
          },
          kinds: 128,
        },
      ],
      (event) => {
        if (event.kind !== "value") return;
        if (!event.values) {
          void this.close().catch(this.options.onError);
          return;
        }
        try {
          setSize(
            Number(event.values.find((f) => f.offset === width)?.value),
            Number(event.values.find((f) => f.offset === height)?.value),
          );
        } catch (error) {
          this.fail(error);
        }
      },
    );
    this.engine = await TerminalEngine.create({
      wasm: this.options.wasm,
      callbacksWasm: this.options.callbacksWasm,
      bashkit: this.options.bashkit,
      ...(this.options.shell ? { shell: this.options.shell } : {}),
      cols: this.dimensions[0],
      rows: this.dimensions[1],
      cellWidth,
      cellHeight: this.options.lineHeight,
    });
    if (this.stopped)
      throw new Error("Terminal target disappeared during startup");
    this.paint = new TerminalPaint(
      client,
      this.options.contract,
      target,
      this.options.fontSource,
      font,
      this.options.fontSize,
      cellWidth,
      this.options.lineHeight,
      this.options.transparentBackground ?? false,
    );
    await this.paint.initialize();
    if (this.stopped)
      throw new Error("Terminal target disappeared during startup");
    this.input = await client.subscribeGuiEffects(
      (event) => {
        if (this.stopped) return;
        if (this.queue.length >= 4096) {
          this.fail(new Error("Terminal event queue exceeded capacity"));
          return;
        }
        if (event.effect.kind === "input") {
          const input = event.effect.input;
          this.queuedBytes +=
            input.kind === "text"
              ? new TextEncoder().encode(input.text).length
              : 128;
          if (this.queuedBytes > 1024 * 1024) {
            this.fail(new Error("Terminal event queue exceeded capacity"));
            return;
          }
          this.queue.push({ input });
        } else if (event.effect.kind === "focusChanged")
          this.queue.push({ focus: event.effect.focused });
        else return;
        this.schedule();
      },
      {
        inputTarget: {
          world: this.options.world,
          entity: target,
          component: descriptor.id,
          incarnation: baseline.incarnation,
        },
      },
    );
    if (this.stopped)
      throw new Error("Terminal target disappeared during startup");
    void this.input.closed.then((result) => {
      if (!this.stopped && result.kind === "closed") this.fail(result.reason);
    });
    void client.closed.then((result) => {
      if (!this.stopped) this.fail(result.reason);
    });
    await this.paint.update(await this.engine.run());
    this.initialized = true;
    this.schedule();
  }

  private schedule(): void {
    if (
      !this.initialized ||
      !this.engine ||
      !this.paint ||
      !this.input ||
      this.stopped ||
      this.running ||
      (!this.queue.length && !this.resizePending)
    )
      return;
    this.running = true;
    this.work = this.work
      .then(async () => {
        while (!this.stopped && (this.queue.length || this.resizePending)) {
          if (this.resizePending) {
            this.engine!.resize(...this.dimensions);
            this.resizePending = false;
          }
          const queued = this.queue;
          this.queue = [];
          this.queuedBytes = 0;
          for (const event of queued) {
            if ("input" in event) this.engine!.input(event.input);
            else this.engine!.focus(event.focus);
          }
          const frame = await this.engine!.run();
          if (this.stopped) break;
          await this.paint!.update(frame);
          if (this.engine!.exitCode !== undefined) {
            await apply(this.options.client, [
              component(
                this.options.client,
                "GuiBehavior",
                handle(this.options.target),
                {
                  enabled: false,
                  semantic_label: `Terminal exited (${this.engine!.exitCode})`,
                },
                true,
              ),
            ]);
          }
        }
      })
      .catch(async (error: unknown) => {
        if (this.stopped) return;
        if (
          error instanceof TerminalBatchError &&
          error.reason === "InvalidEntity"
        ) {
          const current = await this.options.client.inspectPage({
            collection: "entities",
            target: this.options.target,
            limit: 1,
          });
          if (
            !current.entities.some(
              (entity) => entity.id === this.options.target,
            )
          ) {
            void this.close().catch(this.options.onError);
            return;
          }
        }
        this.fail(error);
      })
      .catch((error: unknown) => this.fail(error))
      .finally(() => {
        this.running = false;
        this.schedule();
      });
  }

  private fail(error: unknown): void {
    if (this.stopped) return;
    const failure = error instanceof Error ? error : new Error(String(error));
    // The browser owner terminates the Worker when it receives this error.
    // Reclaim our subscriptions and Canvas subtree before sending it.
    void this.close().then(
      () => this.options.onError(failure),
      (cleanup: unknown) =>
        this.options.onError(
          new AggregateError(
            [failure, cleanup],
            "Terminal failed and cleanup was incomplete",
          ),
        ),
    );
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.stopped = true;
    this.queue = [];
    this.queuedBytes = 0;
    return (this.closing = (async () => {
      await this.initializing?.catch(() => {});
      await this.work;
      const results = await Promise.allSettled([
        this.input?.unsubscribe(),
        this.layout?.remove(),
        this.lifetime?.remove(),
      ]);
      this.engine?.dispose();
      if (!this.options.client.closure) await this.paint?.close();
      const failures = results.filter(
        (result): result is PromiseRejectedResult =>
          result.status === "rejected",
      );
      if (failures.length && !this.options.client.closure)
        throw new AggregateError(
          failures.map((result) => result.reason),
          "Terminal subscriptions did not close",
        );
    })());
  }
}
