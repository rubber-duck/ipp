/**
 * Presenting one client's root on the shared Host surface. The surface
 * selects one root binding at a time, so a client presents and captures
 * inside `present`, which selects its binding, runs the section and clears
 * the selection. The Host fences every frame request by the exact view, so a
 * request whose view was replaced fails with `staleView` instead of drawing
 * another client's root; the caller's lock keeps that from happening.
 */
import type {
  Client,
  HostClientBase,
  OutputReference,
  PresentationView,
  PresentedCapture,
  RootBinding,
} from "@ipp/client";
import type { RgbaImage } from "./images.js";

/** Frames to wait for paint to settle before reporting it unsettled. */
const SETTLE_LIMIT = 240;

/** Runs a section while no other client presents or captures on the Host. */
export type Exclusive = <T>(section: () => Promise<T>) => Promise<T>;

/** Select `binding` alone on the Host surface while `section` runs. */
export async function present<T>(
  host: HostClientBase<Client>,
  exclusive: Exclusive,
  binding: RootBinding,
  section: (view: PresentationView) => Promise<T>,
): Promise<T> {
  return exclusive(async () => {
    const surface = await host.presentation.surface();
    const view = await host.presentation.select(surface, binding);
    try {
      return await section(view);
    } finally {
      await host.presentation.clear(view).catch(() => {});
    }
  });
}

/** The capture's pixels as an image. The pixels are not copied. */
export function image(capture: PresentedCapture): RgbaImage {
  const { width, height } = capture.view.binding.viewport;
  return { width, height, pixels: new Uint8Array(capture.pixels) };
}

function samePixels(left: PresentedCapture, right: PresentedCapture): boolean {
  if (left.pixels.byteLength !== right.pixels.byteLength) return false;
  const a = new Uint32Array(left.pixels);
  const b = new Uint32Array(right.pixels);
  for (let index = 0; index < a.length; index++)
    if (a[index] !== b[index]) return false;
  return true;
}

/**
 * The first capture whose next frame draws identical pixels while `quiet`
 * holds across both. The first read waits for `outputs` evaluated after the
 * request, so acknowledged writes and applied input are included. `quiet`
 * reports whether the client has writes in flight, such as declarations a
 * React root makes only after the runtime reports something.
 */
export async function settled(
  host: HostClientBase<Client>,
  view: PresentationView,
  outputs: readonly OutputReference[],
  quiet: () => boolean = () => true,
): Promise<PresentedCapture> {
  let previous = await host.presentation.capture(view, {
    afterOutputs: outputs,
  });
  let wasQuiet = quiet();
  for (let frame = 0; frame < SETTLE_LIMIT; frame++) {
    const next = await host.presentation.capture(view, {
      afterSequence: previous.sequence,
      ...(wasQuiet ? {} : { afterOutputs: outputs }),
    });
    const isQuiet = quiet();
    if (wasQuiet && isQuiet && samePixels(previous, next)) return next;
    wasQuiet = isQuiet;
    previous = next;
  }
  throw new Error(`Paint did not settle within ${SETTLE_LIMIT} frames`);
}
