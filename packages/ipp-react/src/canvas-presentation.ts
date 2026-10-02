import type {
  Client,
  HostClientBase,
  OutputReference,
  PresentationFrameOptions,
  PresentationSurface,
  PresentationView,
  PresentationViewport,
  PresentedCapture,
  PresentedFrame,
  RootBinding,
} from "@ipp/client";
import { attachmentIdentity } from "./attachment-identity.js";

export type CanvasHost = HostClientBase<Client>;

export interface CanvasSize {
  readonly width: number;
  readonly height: number;
  readonly devicePixelRatio: number;
}

export interface CanvasPresentationJournal {
  readonly bindings: readonly RootBinding[];
  readonly views: readonly PresentationView[];
  readonly captures: readonly bigint[];
  readonly unknownMutations: readonly unknown[];
}

export class CanvasPresentation {
  private generation = 0;
  private closing = false;
  private tail: Promise<void> = Promise.resolve();
  private desired = "";
  private request:
    | { output: OutputReference | null; size: CanvasSize }
    | undefined;
  private readonly operations = new Set<Promise<unknown>>();
  private readonly bindings = new Set<RootBinding>();
  private readonly views = new Set<PresentationView>();
  private readonly captures = new Set<bigint>();
  private readonly unknownMutations: unknown[] = [];
  private current:
    | {
        binding: RootBinding;
        surface: PresentationSurface;
        key: string;
        recovering?: boolean;
      }
    | undefined;
  view: PresentationView | null = null;

  constructor(
    private readonly host: CanvasHost,
    private readonly changed: (view: PresentationView | null) => void,
  ) {}

  get journal(): CanvasPresentationJournal {
    return Object.freeze({
      bindings: Object.freeze([...this.bindings]),
      views: Object.freeze([...this.views]),
      captures: Object.freeze([...this.captures]),
      unknownMutations: Object.freeze([...this.unknownMutations]),
    });
  }

  get viewport(): PresentationViewport | null {
    return this.view?.binding.viewport ?? null;
  }

  select(output: OutputReference | null, size: CanvasSize): Promise<void> {
    if (this.closing)
      return Promise.reject(new Error("Canvas presentation is closed"));
    if (this.unknownMutations.length)
      return Promise.reject(
        new AggregateError(
          this.unknownMutations,
          "Canvas presentation has unknown submitted effects",
        ),
      );
    const key = attachmentIdentity({ output, size });
    if (key === this.desired) return this.tail;
    const previousOutput = this.request?.output;
    this.desired = key;
    const generation = ++this.generation;
    const target = output && structuredClone(output);
    const dimensions = { ...size };
    this.request = { output: target, size: dimensions };
    const current = () => !this.closing && generation === this.generation;
    const work = this.enqueue(async () => {
      if (!current()) return;
      if (!target) {
        this.view = null;
        this.current = undefined;
        this.changed(null);
        await this.release();
        return;
      }
      await this.host.resolveOutput(target);
      if (!current()) return;
      const surface = await this.host.presentation.surface();
      if (!current()) return;
      const viewport = canvasViewport(dimensions, surface);
      // Only the viewport of the selected output changes: one Host request
      // rebinds and reselects it, so no draw sees the root unselected.
      const selected = this.view;
      const resizing =
        !!selected &&
        !!this.current &&
        this.current.key !== key &&
        !this.current.recovering &&
        attachmentIdentity(selected.binding) ===
          attachmentIdentity(this.current.binding) &&
        attachmentIdentity(selected.binding.output) ===
          attachmentIdentity(target);
      if (
        this.current &&
        (this.current.key === key ||
          resizing ||
          attachmentIdentity(previousOutput) === attachmentIdentity(target))
      ) {
        await this.requireCurrentBinding();
        if (
          surface.context !== this.current.surface.context ||
          surface.id !== this.current.surface.id
        )
          throw new Error(
            "Canvas context changed; call recoverPresentation explicitly",
          );
      }
      if (!current()) return;
      if (resizing) {
        await this.resize(selected!, viewport, key, current);
        return;
      }
      const binding =
        this.current?.key === key
          ? this.current.binding
          : await this.mutation(() =>
              this.host.setRootOutput(target, viewport),
            );
      freezeIdentity(binding);
      this.bindings.add(binding);
      if (!current()) {
        await this.release();
        return;
      }
      const selection = {
        binding,
        surface,
        key,
        recovering:
          this.current?.key === key && this.current.recovering === true,
      };
      this.current = selection;
      if (
        this.view &&
        attachmentIdentity(this.view.binding) === attachmentIdentity(binding)
      ) {
        await this.release(this.view, binding);
        return;
      }
      this.view = null;
      const view = await this.mutation(() =>
        this.host.presentation.select(surface, binding),
      );
      freezeIdentity(view);
      this.views.add(view);
      if (!current()) {
        await this.release();
        return;
      }
      this.view = view;
      selection.recovering = false;
      this.changed(view);
      await this.release(view, binding);
    });
    void work.catch(() => {
      if (current() && !this.unknownMutations.length) this.desired = "";
    });
    return work;
  }

  recover(): Promise<void> {
    if (this.closing)
      return Promise.reject(new Error("Canvas presentation is closed"));
    if (this.unknownMutations.length)
      return Promise.reject(
        new AggregateError(
          this.unknownMutations,
          "Canvas presentation has unknown submitted effects",
        ),
      );
    const generation = ++this.generation;
    const work = this.enqueue(async () => {
      await this.requireCurrentBinding();
      const selected = this.current!;
      const surface = await this.host.presentation.surface();
      if (this.closing || generation !== this.generation) return;
      if (
        surface.id === selected.surface.id &&
        surface.context === selected.surface.context &&
        !selected.recovering
      )
        throw new Error(
          "Presentation recovery requires a fresh context, not a stale selection",
        );
      const desired = this.request;
      const viewport =
        desired &&
        attachmentIdentity(desired.output) ===
          attachmentIdentity(selected.binding.output)
          ? canvasViewport(desired.size, surface)
          : canvasViewport(selected.binding.viewport, surface, false);
      await this.requireCurrentBinding();
      if (this.closing || generation !== this.generation) return;
      let binding = selected.binding;
      if (
        attachmentIdentity(viewport) !== attachmentIdentity(binding.viewport)
      ) {
        binding = await this.mutation(() =>
          this.host.setRootOutput(selected.binding.output, viewport),
        );
        freezeIdentity(binding);
        this.bindings.add(binding);
        if (this.closing || generation !== this.generation) {
          await this.release();
          return;
        }
      }
      const key =
        desired &&
        attachmentIdentity(desired.output) ===
          attachmentIdentity(binding.output)
          ? attachmentIdentity(desired)
          : selected.key;
      this.current = { binding, surface, key, recovering: true };
      this.view = null;
      const view = await this.mutation(() =>
        this.host.presentation.select(surface, binding),
      );
      freezeIdentity(view);
      this.views.add(view);
      if (this.closing || generation !== this.generation) {
        await this.release();
        return;
      }
      this.desired = key;
      this.current = { binding, surface, key };
      this.view = view;
      this.changed(view);
      await this.release(view, binding);
    });
    void work.catch(() => {
      if (
        !this.closing &&
        generation === this.generation &&
        !this.unknownMutations.length
      )
        this.desired = "";
    });
    return work;
  }

  /** Rebind the selected root at `viewport` and select it in one Host request. */
  private async resize(
    selected: PresentationView,
    viewport: PresentationViewport,
    key: string,
    current: () => boolean,
  ): Promise<void> {
    const previous = this.current!;
    if (
      attachmentIdentity(viewport) ===
      attachmentIdentity(selected.binding.viewport)
    ) {
      // A CSS change that rounds to the same pixels and ratio changes nothing.
      previous.key = key;
      return;
    }
    const view = await this.mutation(() =>
      this.host.presentation.resize(selected, viewport),
    );
    freezeIdentity(view);
    this.views.add(view);
    this.bindings.add(view.binding);
    // The Host replaced the previous binding and view in that one step, and
    // neither can become current again, so neither needs cleanup.
    this.views.delete(selected);
    this.bindings.delete(previous.binding);
    this.current = { binding: view.binding, surface: view.surface, key };
    if (this.closing) return;
    // The new view is the Host's selection even when a later request has
    // superseded this one; that request resizes or replaces it in turn.
    this.view = view;
    this.changed(view);
    if (current()) await this.release(view, view.binding);
  }

  private async requireCurrentBinding(): Promise<void> {
    if (!this.current) throw new Error("No acknowledged Canvas root binding");
    const binding = await this.host.getRootOutputBinding(
      this.current.binding.output.world,
    );
    if (
      attachmentIdentity(binding) !== attachmentIdentity(this.current.binding)
    )
      throw new Error("Canvas root binding has been replaced or cleared");
  }

  private enqueue(work: () => Promise<void>): Promise<void> {
    const result = this.tail.catch(() => {}).then(work);
    this.tail = result;
    void result.catch(() => {});
    return result;
  }

  private async mutation<T>(operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      if (!knownPresentationRejection(error)) this.unknownMutations.push(error);
      throw error;
    }
  }

  async frame(options: PresentationFrameOptions = {}): Promise<PresentedFrame> {
    await this.tail;
    const view = this.requireView();
    return this.track(this.host.presentation.frame(view, options));
  }

  async capture(
    options: PresentationFrameOptions = {},
  ): Promise<PresentedCapture> {
    await this.tail;
    const view = this.requireView();
    return this.track(
      this.host.presentation.capture(view, options).catch((error: unknown) => {
        if (
          error &&
          typeof error === "object" &&
          "capture" in error &&
          typeof error.capture === "bigint"
        )
          this.captures.add(error.capture);
        throw error;
      }),
    );
  }

  private track<T>(operation: Promise<T>): Promise<T> {
    this.operations.add(operation);
    void operation
      .finally(() => {
        this.operations.delete(operation);
      })
      .catch(() => {});
    return operation;
  }

  private requireView(): PresentationView {
    if (this.closing || !this.view)
      throw new Error("Canvas has no selected presentation view");
    return this.view;
  }

  fence(): void {
    if (this.closing) return;
    this.closing = true;
    this.generation++;
    this.view = null;
  }

  async close(): Promise<void> {
    this.fence();
    await this.tail.catch(() => {});
    await Promise.allSettled([...this.operations]);
    await this.release();
  }

  private async release(
    keepView?: PresentationView,
    keepBinding?: RootBinding,
  ): Promise<void> {
    const errors: unknown[] = [];
    for (const view of this.views) {
      if (view === keepView) continue;
      try {
        await this.host.presentation.clear(view);
        this.views.delete(view);
        if (this.view === view) this.view = null;
      } catch (error) {
        errors.push(error);
      }
    }
    for (const binding of this.bindings) {
      if (binding === keepBinding) continue;
      try {
        await this.host.clearRootOutput(binding);
        this.bindings.delete(binding);
        if (this.current?.binding === binding) this.current = undefined;
      } catch (error) {
        errors.push(error);
      }
    }
    for (const capture of this.captures) {
      try {
        await this.host.presentation.releaseCapture(capture);
        this.captures.delete(capture);
      } catch (error) {
        errors.push(error);
      }
    }
    errors.push(...this.unknownMutations);
    if (errors.length)
      throw new AggregateError(
        errors,
        "Canvas presentation cleanup is incomplete",
      );
  }
}

export function canvasViewport(
  size: CanvasSize,
  surface: PresentationSurface,
  css = true,
): PresentationViewport {
  if (
    ![size.width, size.height, size.devicePixelRatio].every(
      (value) => Number.isFinite(value) && value > 0,
    )
  )
    throw new RangeError(
      "Canvas dimensions and pixel ratio must be positive and finite",
    );
  const ratio = css
    ? Math.min(
        size.devicePixelRatio,
        surface.maxWidth / size.width,
        surface.maxHeight / size.height,
      )
    : 1;
  const width = Math.max(1, Math.round(size.width * ratio));
  const height = Math.max(1, Math.round(size.height * ratio));
  if (width > surface.maxWidth || height > surface.maxHeight)
    throw new RangeError("Canvas viewport exceeds the current surface limits");
  return {
    width,
    height,
    devicePixelRatio: css ? ratio : size.devicePixelRatio,
  };
}

function knownPresentationRejection(error: unknown): boolean {
  return (
    !!error &&
    typeof error === "object" &&
    (("code" in error &&
      (error.code === "IPP_REQUEST_NOT_SENT" ||
        error.code === "IPP_REQUEST_REJECTED")) ||
      ("reason" in error &&
        typeof error.reason === "string" &&
        [
          "unsupported",
          "unavailable",
          "staleView",
          "invalidViewport",
          "obsoletePublication",
          "capacity",
          "timeout",
          "drawFailed",
        ].includes(error.reason)))
  );
}

function freezeIdentity(value: object): void {
  for (const field of Object.values(value))
    if (field && typeof field === "object") freezeIdentity(field);
  Object.freeze(value);
}
