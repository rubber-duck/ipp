/** Host surface configuration and completed-draw fences, never World evaluation ticks. */
import { HostWireReader, HostWireWriter } from "./host-protocol.js";
import {
  outputProducer,
  readOutputReference,
  writeOutputReference,
} from "./references.js";
import type { OutputReference, PublicationReference } from "./types.js";

/** `MAX_PRESENTATION_SOURCES` of `ipp-protocol` (`presentation.rs`), checked by `tools/check_repo.py`. */
const maxPresentationSources = Math.floor((1_048_576 - 1024) / 65);

/** Capture chunks are full `MAX_FIELD_BYTES` pages of `ipp-protocol` (`lib.rs`), checked by `tools/check_repo.py`. */
const captureChunkBytes = 65_536;
/**
 * Capture reads in flight. The Host answers reads between its frames, so a busy
 * Host answers about one window per frame; 16 chunks keep a 1714x1259 capture
 * to nine windows while in-flight replies stay near 1 MiB of the connection's
 * reliable output.
 */
const captureTransferWindow = 16;

function outputKey(output: OutputReference): string {
  const producer = outputProducer(output);
  return `${output.world.id}:${output.world.incarnation}:${output.kind}:${producer?.entity ?? ""}:${producer?.incarnation ?? ""}`;
}

export interface PresentedSource {
  readonly output: OutputReference;
  readonly minimumTick: bigint;
  readonly publication: PublicationReference;
  readonly tick: bigint;
}

export interface PresentationViewport {
  readonly width: number;
  readonly height: number;
  readonly devicePixelRatio: number;
}

export interface RootBinding {
  readonly output: OutputReference;
  readonly viewport: PresentationViewport;
  readonly generation: { readonly host: bigint; readonly serial: bigint };
}

export interface PresentationSurface {
  readonly id: bigint;
  readonly context: bigint;
  readonly maxWidth: number;
  readonly maxHeight: number;
}

export interface PresentationView {
  readonly surface: PresentationSurface;
  readonly selection: bigint;
  readonly binding: RootBinding;
}

export interface PresentedFrame {
  readonly view: PresentationView;
  readonly sequence: bigint;
  readonly publication: PublicationReference;
  readonly drawCalls: number;
  readonly triangles: number;
  readonly failedDrawCalls: number;
  readonly sources: readonly PresentedSource[];
}

/** Immutable top-left RGBA8 snapshot, not authority over the current display. */
export interface PresentedCapture extends PresentedFrame {
  readonly pixels: ArrayBuffer;
}

export interface PresentationFrameOptions {
  /** Observe post-admission content of exact outputs, without forcing evaluation or repaint. */
  readonly afterOutputs?: readonly OutputReference[];
  readonly afterSequence?: bigint;
  /** Constrains an actual authorized draw; does not request historical replay. */
  readonly publication?: PublicationReference;
}

export type PresentationFailure =
  | "unsupported"
  | "unavailable"
  | "staleView"
  | "invalidViewport"
  | "obsoletePublication"
  | "capacity"
  | "timeout"
  | "drawFailed";

export class PresentationError extends Error {
  constructor(readonly reason: PresentationFailure) {
    super(`Presentation failed: ${reason}`);
    this.name = "PresentationError";
  }
}

/** A completed snapshot whose exact connection-owned transfer may still need release. */
export class CaptureTransferError extends Error {
  constructor(
    readonly capture: bigint,
    readonly frame: PresentedFrame,
    cause: unknown,
  ) {
    super(
      "Capture transfer failed; release this exact capture if the Host remains live",
      { cause },
    );
    this.name = "CaptureTransferError";
  }
}

export function writeRootBinding(
  writer: HostWireWriter,
  binding: RootBinding,
): void {
  writeOutputReference(writer, binding.output);
  writer.u32(binding.viewport.width);
  writer.u32(binding.viewport.height);
  writer.f64(binding.viewport.devicePixelRatio);
  writer.u64(binding.generation.host);
  writer.u64(binding.generation.serial);
}

export function readRootBinding(reader: HostWireReader): RootBinding {
  return {
    output: readOutputReference(reader),
    viewport: {
      width: reader.u32(),
      height: reader.u32(),
      devicePixelRatio: reader.f64(),
    },
    generation: { host: reader.u64(), serial: reader.u64() },
  };
}

function writeSurface(
  writer: HostWireWriter,
  surface: PresentationSurface,
): void {
  writer.u64(surface.id);
  writer.u64(surface.context);
  writer.u32(surface.maxWidth);
  writer.u32(surface.maxHeight);
}

function readSurface(reader: HostWireReader): PresentationSurface {
  return {
    id: reader.u64(),
    context: reader.u64(),
    maxWidth: reader.u32(),
    maxHeight: reader.u32(),
  };
}

export function writeView(
  writer: HostWireWriter,
  view: PresentationView,
): void {
  writeSurface(writer, view.surface);
  writer.u64(view.selection);
  writeRootBinding(writer, view.binding);
}

function readView(reader: HostWireReader): PresentationView {
  return {
    surface: readSurface(reader),
    selection: reader.u64(),
    binding: readRootBinding(reader),
  };
}

function readFrame(reader: HostWireReader): PresentedFrame {
  return {
    view: readView(reader),
    sequence: reader.u64(),
    publication: { host: reader.u64(), revision: reader.u64() },
    drawCalls: reader.u32(),
    triangles: reader.u32(),
    failedDrawCalls: reader.u32(),
    sources: (() => {
      const count = reader.u32();
      if (count > maxPresentationSources)
        throw new Error("Presentation source limit");
      return Array.from({ length: count }, () => ({
        output: readOutputReference(reader),
        minimumTick: reader.u64(),
        publication: { host: reader.u64(), revision: reader.u64() },
        tick: reader.u64(),
      }));
    })(),
  };
}

type Send = (
  tag: number,
  encode: (writer: HostWireWriter) => void,
) => Promise<HostWireReader>;

/** One actual surface; configuration survives authoring-session and connection closure. */
export class HostPresentation {
  constructor(
    private readonly send: Send,
    private readonly tag: (name: string) => number,
  ) {}

  private async request(
    name: string,
    expected: string,
    encode?: (writer: HostWireWriter) => void,
  ): Promise<HostWireReader> {
    const reader = await this.send(
      this.tag("HOST_REQUEST_PRESENTATION"),
      (writer) => {
        writer.u8(this.tag(`PRESENTATION_REQUEST_${name}`));
        encode?.(writer);
      },
    );
    if (reader.u8() !== this.tag("HOST_RESPONSE_PRESENTATION"))
      throw new Error("Unexpected Host presentation response");
    const kind = reader.u8();
    if (kind === this.tag("PRESENTATION_RESPONSE_ERROR")) {
      const code = reader.u8();
      reader.end();
      const reasons = [
        ["UNSUPPORTED", "unsupported"],
        ["UNAVAILABLE", "unavailable"],
        ["STALE_VIEW", "staleView"],
        ["INVALID_VIEWPORT", "invalidViewport"],
        ["OBSOLETE_PUBLICATION", "obsoletePublication"],
        ["CAPACITY", "capacity"],
        ["TIMEOUT", "timeout"],
        ["DRAW_FAILED", "drawFailed"],
      ] as const;
      const found = reasons.find(
        ([name]) => this.tag(`PRESENTATION_ERROR_${name}`) === code,
      );
      if (!found) throw new Error("Invalid presentation failure");
      throw new PresentationError(found[1]);
    }
    if (kind !== this.tag(`PRESENTATION_RESPONSE_${expected}`))
      throw new Error("Unexpected presentation result");
    return reader;
  }

  async surface(): Promise<PresentationSurface> {
    const reader = await this.request("SURFACE", "SURFACE");
    const surface = readSurface(reader);
    reader.end();
    return surface;
  }

  /** Select exactly the binding viewport; an unsupported extent rejects without clamping. */
  async select(
    surface: PresentationSurface,
    binding: RootBinding,
  ): Promise<PresentationView> {
    const reader = await this.request("SELECT", "VIEW", (writer) => {
      writeSurface(writer, surface);
      writeRootBinding(writer, binding);
    });
    const view = readView(reader);
    reader.end();
    return view;
  }

  /** Compare-and-clear. A stale view never clears a replacement selection. */
  async clear(expected: PresentationView): Promise<void> {
    (
      await this.request("CLEAR", "COMPLETE", (writer) =>
        writeView(writer, expected),
      )
    ).end();
  }

  private async completed(
    view: PresentationView,
    options: PresentationFrameOptions,
    capture: boolean,
  ): Promise<{
    frame: PresentedFrame;
    reader: HostWireReader;
    validate(): void;
  }> {
    const expected = new HostWireWriter();
    writeView(expected, view);
    const viewBytes = expected.finish();
    const afterSequence = options.afterSequence;
    const unique = new Map<string, OutputReference>();
    for (const output of options.afterOutputs ?? []) {
      const key = outputKey(output);
      if (unique.has(key)) continue;
      if (unique.size === maxPresentationSources)
        throw new RangeError("Presentation source limit");
      unique.set(key, structuredClone(output));
    }
    const outputs = [...unique.values()];
    const publication = options.publication
      ? { ...options.publication }
      : undefined;
    const reader = await this.request(
      "FRAME",
      capture ? "CAPTURE" : "FRAME",
      (writer) => {
        writer.raw(viewBytes);
        writer.u8(afterSequence === undefined ? 0 : 1);
        if (afterSequence !== undefined) writer.u64(afterSequence);
        writer.u8(publication === undefined ? 0 : 1);
        if (publication) {
          writer.u64(publication.host);
          writer.u64(publication.revision);
        }
        writer.u8(capture ? 1 : 0);
        writer.u32(outputs.length);
        for (const output of outputs) writeOutputReference(writer, output);
      },
    );
    const frame = readFrame(reader);
    return {
      frame,
      reader,
      validate() {
        const requested = new Set(outputs.map(outputKey));
        if (frame.sources.length !== requested.size)
          throw new Error("Presentation source count mismatch");
        for (const source of frame.sources) {
          if (
            !requested.delete(outputKey(source.output)) ||
            source.minimumTick === 0n ||
            source.tick < source.minimumTick ||
            source.publication.host !== frame.publication.host ||
            source.publication.revision === 0n
          ) {
            throw new Error(
              "Completed source does not match requested output cut",
            );
          }
        }
        const actual = new HostWireWriter();
        writeView(actual, frame.view);
        const actualBytes = actual.finish();
        if (
          actualBytes.length !== viewBytes.length ||
          actualBytes.some((byte, index) => byte !== viewBytes[index]) ||
          frame.sequence <= (afterSequence ?? 0n) ||
          (publication &&
            (publication.host !== frame.publication.host ||
              publication.revision !== frame.publication.revision))
        ) {
          throw new Error(
            "Completed frame does not match requested presentation fence",
          );
        }
      },
    };
  }

  async frame(
    view: PresentationView,
    options: PresentationFrameOptions = {},
  ): Promise<PresentedFrame> {
    const { frame, reader, validate } = await this.completed(
      view,
      options,
      false,
    );
    reader.end();
    validate();
    return frame;
  }

  /**
   * Pending captures are fenced at completion; completed bytes survive later rebind/loss.
   * A decoded transfer ID is journaled before fence validation and released even on failure.
   * Truncation before that ID cannot establish ownership: server expiry or connection
   * teardown reclaims the unknown transfer instead of guessing a cleanup identity.
   */
  async capture(
    view: PresentationView,
    options: PresentationFrameOptions = {},
  ): Promise<PresentedCapture> {
    const { frame, reader, validate } = await this.completed(
      view,
      options,
      true,
    );
    const capture = reader.u64();
    let failure: unknown;
    let pixels: Uint8Array<ArrayBuffer> | undefined;
    try {
      const total = reader.u64();
      reader.end();
      validate();
      const viewport = frame.view.binding.viewport;
      if (
        total > 67_108_864n ||
        total !== BigInt(viewport.width) * BigInt(viewport.height) * 4n
      )
        throw new Error("Invalid capture byte length");
      pixels = new Uint8Array(Number(total));
      const target = pixels;
      const read = async (offset: number) => {
        const chunk = await this.request("READ_CAPTURE", "CHUNK", (writer) => {
          writer.u64(capture);
          writer.u64(BigInt(offset));
        });
        if (chunk.u64() !== capture || chunk.u64() !== BigInt(offset))
          throw new Error("Capture chunk identity mismatch");
        const bytes = chunk.bytes();
        chunk.end();
        if (
          bytes.length !== Math.min(captureChunkBytes, target.length - offset)
        )
          throw new Error("Invalid capture chunk length");
        target.set(bytes, offset);
      };
      // Every submitted read settles before release.
      for (
        let start = 0;
        start < target.length;
        start += captureChunkBytes * captureTransferWindow
      ) {
        const reads: Promise<void>[] = [];
        for (let index = 0; index < captureTransferWindow; index++) {
          const offset = start + index * captureChunkBytes;
          if (offset >= target.length) break;
          reads.push(read(offset));
        }
        const results = await Promise.allSettled(reads);
        const rejected = results.find(
          (result): result is PromiseRejectedResult =>
            result.status === "rejected",
        );
        if (rejected) throw rejected.reason;
      }
    } catch (error) {
      failure = error;
    }
    try {
      await this.releaseCapture(capture);
    } catch (error) {
      failure =
        failure === undefined
          ? error
          : new AggregateError([failure, error], "Capture and release failed");
    }
    if (failure !== undefined)
      throw new CaptureTransferError(capture, frame, failure);
    if (!pixels)
      throw new CaptureTransferError(
        capture,
        frame,
        new Error("Missing capture"),
      );
    return { ...frame, pixels: pixels.buffer };
  }

  async releaseCapture(capture: bigint): Promise<void> {
    (
      await this.request("RELEASE_CAPTURE", "COMPLETE", (writer) =>
        writer.u64(capture),
      )
    ).end();
  }
}
