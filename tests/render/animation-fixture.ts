import type {
  AnimationWorldClient,
  CameraWorldClient,
  FrameCapture,
} from "@ipp/client";
import {
  AnimationFixture,
  check,
  propertyAnimationScenario,
} from "../integration/animation-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

/** Same production assets/client path in a lean worker and a WebGL worker. */
export async function run(
  configuration: { generated: string; workerScript: string; wasm: string },
  rendering: boolean,
) {
  const contract = await import(configuration.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 240;
  document.body.replaceChildren(canvas);
  const client: AnimationWorldClient & CameraWorldClient =
    await contract.IppClient.connectWorker(
      configuration.workerScript,
      configuration.wasm,
      {
        ...(rendering ? { canvas: canvas.transferControlToOffscreen() } : {}),
        timeoutMs: 10_000,
      },
    );
  const record = (kind: string, value: unknown) =>
    (
      globalThis as unknown as {
        recordAnimation(kind: string, value: unknown): Promise<void>;
      }
    ).recordAnimation(
      kind,
      JSON.parse(
        JSON.stringify(value, (_key, item: unknown) =>
          typeof item === "bigint" ? { $bigint: item.toString() } : item,
        ),
      ),
    );
  try {
    if (!rendering)
      return {
        contract: await propertyAnimationScenario(client, contract, record),
        captures: [],
        differences: [],
      };
    check(client.presentation, "animation renderer missing");
    const fixture = new AnimationFixture(client, contract, record);
    const camera = await fixture.create("animation-camera", {
      Transform: { z: 6 },
      Camera: { projection: 1, focus_distance: 6, ortho_height: 4 },
    });
    client.sendCommand({ type: "CameraActivateCommand", entity: camera });
    const mesh = "ipp://mesh/cube?width=2&height=2&length=2";
    const target = await fixture.create("animated-cube", {
      Transform: { sx: 0.4, sy: 0.4, sz: 0.4 },
      MeshInstance: { source: mesh },
      UnlitMaterial: { r: 0, g: 0, b: 1 },
    });
    const transform = client.components.Transform!;
    const material = client.components.UnlitMaterial!;
    const source = await fixture.upload({
      duration: 2,
      tracks: [
        {
          property: {
            component: transform.id,
            offsets: [transform.fields.x!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32", value: -1 },
              interpolation: {
                kind: "bezier",
                time1: 0,
                time2: 0.5,
                value1: { kind: "f32", value: -1 },
                value2: { kind: "f32", value: 1 },
              },
            },
            { time: 2, value: { kind: "f32", value: 1 } },
          ],
        },
        ...(["r", "g", "b"] as const).map((field, index) => ({
          property: {
            component: material.id,
            offsets: [material.fields[field]!.offset],
          },
          keys: [
            {
              time: 0,
              value: { kind: "f32" as const, value: index === 0 ? 1 : 0 },
              interpolation: { kind: "linear" as const },
            },
            {
              time: 2,
              value: { kind: "f32" as const, value: index === 1 ? 1 : 0 },
            },
          ],
        })),
      ],
    });
    const controller = await fixture.controller([
      fixture.driver(target, source, 0, "Transform", ["x"]),
      ...(["r", "g", "b"] as const).map((field, index) =>
        fixture.driver(target, source, index + 1, "UnlitMaterial", [field]),
      ),
    ]);
    const deadline = performance.now() + 10_000;
    for (;;) {
      const inspection = await fixture.inspect();
      if (
        inspection.resources.some(
          (resource) =>
            resource.source === mesh && resource.status === "loaded",
        )
      )
        break;
      check(performance.now() < deadline, "render mesh did not become ready");
      await client.waitForFrame(inspection.tick);
    }
    const frames: FrameCapture[] = [];
    const captures = [];
    for (const [label, time] of [
      ["left-red", 0],
      ["bezier-middle", 0.4375],
      ["right-green", 2],
      ["paused-green", 2],
      ["stopped-blue", null],
    ] as const) {
      if (time === null) client.playback(controller, { action: "stop" });
      else if (label !== "paused-green")
        await fixture.seekPaused(controller, time);
      const inspection = await fixture.inspect();
      const frame = await client.presentation.capture(inspection.tick);
      check(
        frame.tick > inspection.tick && frame.session === client.session,
        "capture is not a completed session frame",
      );
      frames.push(frame);
      const { pixels: _pixels, ...metadata } = frame;
      const captured = {
        label,
        metadata,
        summary: summarizeImage(frame),
        dataUrl: dataUrl(frame),
      };
      captures.push(captured);
      // Persist each frame before later assertions or operations can fail.
      await record("animation.capture", captured);
    }
    return {
      captures,
      differences: [
        compareImages(frames[0]!, frames[2]!),
        compareImages(frames[2]!, frames[3]!),
      ],
    };
  } finally {
    await client.close();
    canvas.remove();
  }
}

function dataUrl(frame: FrameCapture) {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const context = canvas.getContext("2d");
  check(context, "PNG evidence needs Canvas 2D");
  context.putImageData(
    new ImageData(
      new Uint8ClampedArray(frame.pixels),
      frame.width,
      frame.height,
    ),
    0,
    0,
  );
  return canvas.toDataURL("image/png");
}
