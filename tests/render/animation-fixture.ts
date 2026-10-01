import type { AnimationWorldClient, PresentedCapture } from "@ipp/client";
import {
  AnimationFixture,
  check,
  propertyAnimationScenario,
} from "../integration/animation-fixtures.js";
import type { HostedWorldClient } from "../integration/camera-fixtures.js";
import { compareImages, summarizeImage } from "./image-assertions.js";
import {
  RootPresentation,
  captureSummary,
  capturedImage,
  worldReference,
} from "./root-presentation.js";
import {
  CONSTRAINTS,
  SCENE,
  selectSystems,
} from "../integration/system-selections.js";

/** Same production assets/client path in a headless worker and a WebGL worker. */
export async function run(
  configuration: { generated: string; workerScript: string; wasm: string },
  rendering: boolean,
) {
  const contract = await import(configuration.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 240;
  document.body.replaceChildren(canvas);
  const client: HostedWorldClient<AnimationWorldClient> =
    await contract.IppClient.connectWorker(
      configuration.workerScript,
      configuration.wasm,
      {
        selectedSystems: selectSystems(SCENE, CONSTRAINTS),
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
    const fixture = new AnimationFixture(client, contract, record);
    const camera = await fixture.create("animation-camera", {
      Transform: { z: 6 },
      Camera: { projection: 1, focus_distance: 6, ortho_height: 4 },
    });
    const presentation = await RootPresentation.camera(
      client.host,
      worldReference(client),
      camera,
      { width: canvas.width, height: canvas.height },
    );
    const mesh = "ipp://mesh/cube?width=2&height=2&length=2";
    // Authored where the clip starts: the controller adds the clip's change,
    // and stopping returns the cube to this left red base.
    const target = await fixture.create("animated-cube", {
      Transform: { x: -1, sx: 0.4, sy: 0.4, sz: 0.4 },
      MeshInstance: { source: mesh },
      UnlitMaterial: { r: 1, g: 0, b: 0 },
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
    const frames: PresentedCapture[] = [];
    const captures = [];
    for (const [label, time] of [
      ["left-red", 0],
      ["bezier-middle", 0.4375],
      ["right-green", 2],
      ["paused-green", 2],
      ["stopped-red", null],
    ] as const) {
      if (time === null) client.playback(controller, { action: "stop" });
      else if (label !== "paused-green")
        await fixture.seekPaused(controller, time);
      const inspection = await fixture.inspect();
      const frame = await presentation.capture();
      check(
        presentation.sourceTick(frame) > inspection.tick,
        "capture does not include the inspected animation state",
      );
      frames.push(frame);
      const captured = {
        label,
        metadata: captureSummary(frame),
        summary: summarizeImage(capturedImage(frame)),
        dataUrl: dataUrl(frame),
      };
      captures.push(captured);
      // Persist each frame before later assertions or operations can fail.
      await record("animation.capture", captured);
    }
    return {
      captures,
      differences: [
        compareImages(capturedImage(frames[0]!), capturedImage(frames[2]!)),
        compareImages(capturedImage(frames[2]!), capturedImage(frames[3]!)),
      ],
    };
  } finally {
    await client.close();
    canvas.remove();
  }
}

function dataUrl(frame: PresentedCapture) {
  const { width, height, pixels } = capturedImage(frame);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  check(context, "PNG evidence needs Canvas 2D");
  context.putImageData(
    new ImageData(new Uint8ClampedArray(pixels), width, height),
    0,
    0,
  );
  return canvas.toDataURL("image/png");
}
