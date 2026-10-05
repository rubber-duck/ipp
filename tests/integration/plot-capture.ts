/** Completed Plot frames and motion samples share one maintained presentation driver. */
import type {
  Client,
  HostClientBase,
  PresentationSurface,
  PresentationView,
  RootBinding,
} from "@ipp/client";
import { image, settled } from "../../tools/shared-host/presentation.js";

export interface PlotFrame {
  width: number;
  height: number;
  pixels: Uint8Array<ArrayBuffer>;
}

type Clock = { tick: bigint; time: number };
export interface TimedPlotFrame {
  frame: PlotFrame;
  before: Clock;
  after: Clock;
  sequence: bigint;
}

export interface PlotMotionFrames {
  origin: { before: Clock; after: Clock };
  start: TimedPlotFrame;
  middle: TimedPlotFrame;
  interrupted?: TimedPlotFrame;
  end: TimedPlotFrame;
}

/** Multiple observations of one continuously selected presentation view. */
export interface PlotViewCapture {
  (label: string): Promise<PlotFrame>;
  afterMotion(label: string, client: Client): Promise<PlotFrame>;
  motion(
    label: string,
    client: Client,
    change: () => Promise<void>,
    middleAt?: number,
    interrupt?: () => Promise<void>,
  ): Promise<PlotMotionFrames>;
}

export interface PlotCapture {
  (label: string, binding: RootBinding): Promise<PlotFrame>;
  afterMotion(
    label: string,
    binding: RootBinding,
    client: Client,
  ): Promise<PlotFrame>;
  motion(
    label: string,
    binding: RootBinding,
    client: Client,
    change: () => Promise<void>,
    middleAt?: number,
    interrupt?: () => Promise<void>,
  ): Promise<PlotMotionFrames>;
  session<T>(
    binding: RootBinding,
    run: (capture: PlotViewCapture) => Promise<T>,
  ): Promise<T>;
}

/** All retained samples fit inside four negotiated viewports; waits never advance time. */
export function createPlotCapture(
  host: HostClientBase<Client>,
  surface: PresentationSurface,
  save: (label: string, frame: PlotFrame) => Promise<void>,
  record: (label: string, value: unknown) => Promise<void>,
): PlotCapture {
  const select = async <T>(
    binding: RootBinding,
    run: (view: PresentationView) => Promise<T>,
  ) => {
    const view = await host.presentation.select(surface, binding);
    try {
      return await run(view);
    } finally {
      await host.presentation.clear(view);
    }
  };
  const wait = async (client: Client, target: number) => {
    const deadline = performance.now() + 30_000;
    let clock = await client.waitForFrame();
    for (let frames = 0; clock.time < target; frames++) {
      if (frames >= 600 || performance.now() >= deadline)
        throw new Error(
          `Plot presentation clock did not reach ${target}: ${clock.time}`,
        );
      clock = await client.waitForFrame(clock.tick);
    }
    return clock;
  };
  const sample = async (
    client: Client,
    view: PresentationView,
    afterSequence?: bigint,
  ): Promise<TimedPlotFrame> => {
    const beforeInspection = await client.inspect(),
      before = { tick: beforeInspection.tick, time: beforeInspection.time };
    const capture = await host.presentation.capture(view, {
      afterOutputs: [view.binding.output],
      ...(afterSequence === undefined ? {} : { afterSequence }),
    });
    const afterInspection = await client.inspect(),
      after = { tick: afterInspection.tick, time: afterInspection.time };
    return { frame: image(capture), before, after, sequence: capture.sequence };
  };
  const inView = (view: PresentationView): PlotViewCapture => {
    const capture = (async (label: string) => {
      const frame = image(await settled(host, view, [view.binding.output]));
      await save(label, frame);
      return frame;
    }) as PlotViewCapture;
    capture.afterMotion = async (label, client) => {
      const start = await sample(client, view);
      // HostSession advances presentation_time and every unpaused World's time
      // by the same dt. Waiting from the post-draw upper bound prevents an early
      // pair of identical eased-animation pixels from declaring completion.
      await wait(client, start.after.time + 2);
      const frame = image(await settled(host, view, [view.binding.output]));
      await save(label, frame);
      await record(`${label}-clock`, {
        start: { before: start.before, after: start.after },
        end: await client.waitForFrame(),
      });
      return frame;
    };
    capture.motion = async (label, client, change, middleAt = 1, interrupt) => {
      if (!(middleAt > 0 && middleAt < 2))
        throw new Error(
          "Plot intermediate capture must fall inside the2s motion",
        );
      // Prepare this camera's retained placement before changing its target.
      await settled(host, view, [view.binding.output]);
      const originBefore = await client
        .inspect()
        .then(({ tick, time }) => ({ tick, time }));
      await change();
      const originAfter = await client
        .inspect()
        .then(({ tick, time }) => ({ tick, time }));
      const origin = { before: originBefore, after: originAfter };
      const start = await sample(client, view);
      await wait(client, origin.after.time + middleAt);
      const middle = await sample(client, view, start.sequence);
      let interrupted: TimedPlotFrame | undefined;
      if (interrupt) {
        await interrupt();
        interrupted = await sample(client, view, middle.sequence);
      }
      await wait(client, origin.after.time + 2);
      const endInspection = await client.inspect(),
        endBefore = { tick: endInspection.tick, time: endInspection.time };
      const completed = await settled(host, view, [view.binding.output]);
      const end: TimedPlotFrame = {
        frame: image(completed),
        before: endBefore,
        after: await client
          .inspect()
          .then(({ tick, time }) => ({ tick, time })),
        sequence: completed.sequence,
      };
      // Immutable images are collected before PNG encoding or a
      // remote evidence callback can consume the transition's capture window.
      for (const [phase, sample] of Object.entries({
        start,
        middle,
        ...(interrupted ? { interrupted } : {}),
        end,
      }))
        await save(`${label}-${phase}`, sample.frame);
      await record(`${label}-clock`, {
        origin,
        start: {
          before: start.before,
          after: start.after,
          sequence: start.sequence,
        },
        middle: {
          before: middle.before,
          after: middle.after,
          sequence: middle.sequence,
        },
        ...(interrupted
          ? {
              interrupted: {
                before: interrupted.before,
                after: interrupted.after,
                sequence: interrupted.sequence,
              },
            }
          : {}),
        end: { before: end.before, after: end.after, sequence: end.sequence },
      });
      return {
        origin,
        start,
        middle,
        ...(interrupted ? { interrupted } : {}),
        end,
      };
    };
    return capture;
  };
  const capture = (async (label: string, binding: RootBinding) =>
    select(binding, (view) => inView(view)(label))) as PlotCapture;
  capture.afterMotion = (label, binding, client) =>
    select(binding, (view) => inView(view).afterMotion(label, client));
  capture.motion = (label, binding, client, change, middleAt, interrupt) =>
    select(binding, (view) =>
      inView(view).motion(label, client, change, middleAt, interrupt),
    );
  capture.session = (binding, run) =>
    select(binding, (view) => run(inView(view)));
  return capture;
}
