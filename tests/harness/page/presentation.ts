import type { RenderDiagnostics } from "@ipp/client/diagnostics";
import { renderDiagnostics } from "../../../packages/ipp-client/src/diagnostics.js";
import { sameOutputReference } from "../../../packages/ipp-client/src/references.js";
/**
 * Explicit root Camera presentation for browser render fixtures.
 *
 * A fixture binds one Camera entity as its World's root output, selects that
 * exact binding on the worker's graphics surface and captures completed draws
 * that include content admitted before each request. Selection never follows
 * an authoring session; replacing the camera or viewport is an explicit rebind.
 */
import type {
  Client,
  HostClientBase,
  OutputReference,
  PresentationFrameOptions,
  PresentationView,
  PresentedCapture,
  PresentedFrame,
  RootBinding,
  WorldReference,
} from "@ipp/client";

export type PresentationHost = HostClientBase<Client>;

export interface RootViewport {
  readonly width: number;
  readonly height: number;
  readonly devicePixelRatio?: number;
}

export function worldReference(client: Client): WorldReference {
  const reference = client.worldReference;
  if (!reference) throw new Error("World client has no World reference");
  return reference;
}

export class RootPresentation {
  private constructor(
    readonly host: PresentationHost,
    private selected: {
      readonly output: OutputReference;
      readonly binding: RootBinding;
      readonly view: PresentationView;
    },
  ) {}

  /** Bind `camera` in `world` and select it on the Host's graphics surface. */
  static async camera(
    host: PresentationHost,
    world: WorldReference,
    camera: bigint,
    viewport: RootViewport,
  ): Promise<RootPresentation> {
    const output = await host.bindOutput(world, camera, "camera");
    return new RootPresentation(
      host,
      await RootPresentation.bind(host, output, viewport),
    );
  }

  private static async bind(
    host: PresentationHost,
    output: OutputReference,
    viewport: RootViewport,
  ) {
    const binding = await host.setRootOutput(output, {
      width: viewport.width,
      height: viewport.height,
      devicePixelRatio: viewport.devicePixelRatio ?? 1,
    });
    const view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    return { output, binding, view };
  }

  get output(): OutputReference {
    return this.selected.output;
  }

  get binding(): RootBinding {
    return this.selected.binding;
  }

  get view(): PresentationView {
    return this.selected.view;
  }

  get viewport() {
    return this.selected.binding.viewport;
  }

  get diagnostics(): RenderDiagnostics {
    const diagnostics = renderDiagnostics(this.host);
    if (!diagnostics) throw new Error("Render diagnostics are unavailable");
    return diagnostics;
  }

  /** Explicitly select another Camera of the same World, or another viewport. */
  async select(camera: bigint, viewport: RootViewport = this.viewport) {
    const output = await this.host.bindOutput(
      this.selected.output.world,
      camera,
      "camera",
    );
    if (sameOutputReference(output, this.selected.output))
      await this.resize(viewport);
    else
      this.selected = await RootPresentation.bind(this.host, output, viewport);
  }

  /** Rebind the selected output at `viewport` and keep it selected, in one request. */
  async resize(viewport: RootViewport) {
    const view = await this.host.presentation.resize(this.selected.view, {
      width: viewport.width,
      height: viewport.height,
      devicePixelRatio: viewport.devicePixelRatio ?? 1,
    });
    this.selected = {
      output: this.selected.output,
      binding: view.binding,
      view,
    };
  }

  /** Select the unchanged binding on the graphics context restored after loss. */
  async recover() {
    const surface = await this.host.presentation.surface();
    if (surface.context === this.selected.view.surface.context)
      throw new Error("Presentation recovery requires a restored context");
    this.selected = {
      ...this.selected,
      view: await this.host.presentation.select(surface, this.selected.binding),
    };
  }

  /** A completed draw including this output's content admitted before the request. */
  frame(options: PresentationFrameOptions = {}): Promise<PresentedFrame> {
    return this.host.presentation.frame(this.selected.view, {
      afterOutputs: [this.selected.output],
      ...options,
    });
  }

  /** Read back a completed draw including content admitted before the request. */
  capture(options: PresentationFrameOptions = {}): Promise<PresentedCapture> {
    return this.host.presentation.capture(this.selected.view, {
      afterOutputs: [this.selected.output],
      ...options,
    });
  }

  /**
   * Read back a later completed draw without an output-inclusion witness.
   * A draw that skips failed resources completes but never witnesses its
   * output's inclusion, so scenes with failed draws use the draw sequence.
   */
  captureDraw(after?: PresentedFrame): Promise<PresentedCapture> {
    return this.host.presentation.capture(
      this.selected.view,
      after ? { afterSequence: after.sequence } : {},
    );
  }

  /** The evaluated tick of this output in a completed draw. */
  sourceTick(frame: PresentedFrame): bigint {
    const source = frame.sources.find((source) =>
      sameOutputReference(source.output, this.selected.output),
    );
    if (!source) throw new Error("Completed draw omitted the selected output");
    return source.tick;
  }

  async close() {
    try {
      await this.host.presentation.clear(this.selected.view);
    } finally {
      await this.host.clearRootOutput(this.selected.binding);
    }
  }
}

/** Evidence fields of a completed capture, without its pixel buffer. */
export function captureSummary(frame: PresentedFrame) {
  const { width, height, devicePixelRatio } = frame.view.binding.viewport;
  return {
    width,
    height,
    devicePixelRatio,
    sequence: frame.sequence,
    context: frame.view.surface.context,
    drawCalls: frame.drawCalls,
    triangles: frame.triangles,
    failedDrawCalls: frame.failedDrawCalls,
  };
}

/** Top-left RGBA8 pixels with the dimensions of the exact presented view. */
export function capturedImage(frame: PresentedCapture) {
  const { width, height } = frame.view.binding.viewport;
  return { width, height, pixels: frame.pixels };
}

/**
 * Await the graphics context replacing the selected view's lost context, then
 * select the unchanged binding on it. The surface is unavailable while lost.
 */
export async function recoverRestoredContext(
  presentation: RootPresentation,
  timeoutMs = 10_000,
): Promise<void> {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const surface = await presentation.host.presentation
      .surface()
      .catch((error: unknown) => {
        if (
          error instanceof Error &&
          "reason" in error &&
          error.reason === "unavailable"
        )
          return undefined;
        throw error;
      });
    if (surface && surface.context !== presentation.view.surface.context) break;
    if (performance.now() >= deadline)
      throw new Error("Graphics context was not restored");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  await presentation.recover();
}
