import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  AnimationWorldClient,
  PickingWorldClient,
  WorldPersistenceHostClient,
  FrameCapture,
  Command,
} from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import { hierarchyLifecycle } from "../integration/scenarios/hierarchy.js";
import {
  successfulBatch,
  insertComponent,
} from "../integration/camera-fixtures.js";
import {
  affinePoseTransform,
  affinePosePosition,
  aimedPoseVector,
  poseMesh,
} from "./mesh-pose-assets.js";
import { compareImages, summarizeImage } from "./image-assertions.js";

type Client = AnimationWorldClient & PickingWorldClient;

function png(frame: FrameCapture) {
  const canvas = document.createElement("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  canvas
    .getContext("2d")!
    .putImageData(
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

export async function run(configuration: {
  generated: string;
  workerScript: string;
  wasm: string;
}) {
  const contract = await import(configuration.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 400;
  canvas.height = 300;
  document.body.replaceChildren(canvas);
  const host: WorldPersistenceHostClient<Client> =
    await contract.IppHostClient.connectWorker(
      configuration.workerScript,
      configuration.wasm,
      { canvas: canvas.transferControlToOffscreen(), timeoutMs: 15000 },
    );
  let client = await host.createWorld({ symbolicId: "hierarchy-render" });
  const record = async (kind: string, value: unknown) => {
    await (
      globalThis as unknown as {
        recordHierarchy(kind: string, value: unknown): Promise<void>;
      }
    ).recordHierarchy(
      kind,
      JSON.parse(
        JSON.stringify(value, (_k, v: unknown) =>
          typeof v === "bigint" ? v.toString() : v,
        ),
      ),
    );
  };
  let fixture = new AnimationFixture(client, contract, record);
  const batch = async (operations: Command[]) =>
    successfulBatch(await client.batch(operations));
  const frames = new Map<string, FrameCapture>();
  const capture = async (label: string) => {
    const start = await client.inspect();
    const deadline = performance.now() + 15000;
    let frame: FrameCapture;
    do {
      frame = await client.presentation!.capture(start.tick);
      check(
        performance.now() < deadline,
        `${label}: scene did not reach two draws`,
      );
    } while (frame.drawCalls !== 2);
    frames.set(label, frame);
    await record("capture", {
      label,
      dataUrl: png(frame),
      summary: summarizeImage(frame),
    });
    return frame;
  };
  const same = async (actual: string, expected: string) => {
    const a = frames.get(actual)!;
    const b = frames.get(expected)!;
    const difference = compareImages(a, b);
    const pixels = new Uint8Array(a.pixels).map((v, i) =>
      i % 4 === 3
        ? 255
        : Math.min(255, Math.abs(v - new Uint8Array(b.pixels)[i]!) * 4),
    );
    await record("capture", {
      label: `${actual}-diff`,
      dataUrl: png({ ...a, pixels: pixels.buffer }),
    });
    await record("comparison", { actual, expected, ...difference });
    check(
      difference.changedFraction < 0.001,
      `${actual} disagrees with independent ${expected}: ${difference.changedFraction}`,
    );
  };
  const uploads = async () => {
    for (const [asset, options] of [
      [601n, {}],
      [602n, { affine: true }],
      [603n, { affine: true, aim: true }],
    ] as const) {
      const result = clientAssetSource(client.session, 1, asset);
      await client.registerAsset(result, poseMesh(0.5, options).buffer);
    }
  };
  try {
    await uploads();
    let camera = await fixture.create("hierarchy-camera", {
      Transform: { z: 6 },
      Camera: { projection: 1, ortho_height: 4, focus_distance: 6 },
    });
    client.sendCommand({ type: "CameraActivateCommand", entity: camera });
    const parent = await fixture.create("hierarchy-render-parent", {
      Transform: affinePoseTransform,
    });
    const targetPoint = affinePosePosition([1, 0, -1]);
    const target = await fixture.create("hierarchy-render-target", {
      Transform: { x: targetPoint[0]!, y: targetPoint[1]!, z: targetPoint[2]! },
    });
    const tracker = await fixture.create("hierarchy-render-tracker", {
      Transform: {},
      Hierarchy: { parent },
      LookAt: { target, enabled: false },
      MeshInstance: {
        source: clientAssetSource(client.session, 1, 601n).source,
      },
      UnlitMaterial: { r: 0.1, g: 0.6, b: 1 },
    });
    const tip = await fixture.create("hierarchy-render-tip", {
      Transform: { z: -1, sx: 0.12, sy: 0.12, sz: 0.12 },
      Hierarchy: { parent: tracker },
      MeshInstance: { source: "ipp://mesh/cube?width=1&height=1&length=1" },
      UnlitMaterial: { r: 1, g: 0.3, b: 0.02 },
      PickingGeometry: {
        geometry: contract.encodeBoundingShape({
          type: "box",
          min: [-0.5, -0.5, -0.5],
          max: [0.5, 0.5, 0.5],
        }),
      },
    });
    const plainTip = affinePosePosition([0, 0, -1]);
    await capture("hierarchy-affine");
    // Compare with independently baked positions and a flat child placement.
    await batch([
      ...fixture.set(tracker, "Hierarchy", { parent: 0n }),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 602n).source,
      }),
      ...fixture.set(tip, "Hierarchy", { parent: parent }),
      ...fixture.set(tip, "Transform", { z: -1 }),
    ]);
    await capture("hierarchy-baked");
    await same("hierarchy-affine", "hierarchy-baked");
    await batch([
      ...fixture.set(tracker, "Hierarchy", { parent }),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 601n).source,
      }),
      ...fixture.set(tracker, "LookAt", { enabled: true }),
      ...fixture.set(tip, "Hierarchy", { parent: tracker }),
    ]);
    await capture("aimed-affine");
    const tipPoint = affinePosePosition(aimedPoseVector([0, 0, -1]));
    // Freeze the same independently known local orientation for the reference tip.
    const tipQ = { qy: -Math.sin(Math.PI / 8), qw: Math.cos(Math.PI / 8) };
    const tipParent = await fixture.create("hierarchy-tip-reference", {
      Transform: tipQ,
      Hierarchy: { parent },
    });
    await batch([
      ...fixture.set(tracker, "LookAt", { enabled: false }),
      ...fixture.set(tracker, "Hierarchy", { parent: 0n }),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 603n).source,
      }),
      ...fixture.set(tip, "Hierarchy", { parent: tipParent }),
    ]);
    await capture("aimed-baked");
    await same("aimed-affine", "aimed-baked");
    check(
      compareImages(
        frames.get("hierarchy-affine")!,
        frames.get("aimed-affine")!,
      ).changedFraction > 0.005,
      "tracking must visibly change the surface and attachment",
    );
    await batch([
      ...fixture.set(tracker, "Hierarchy", { parent }),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 601n).source,
      }),
      ...fixture.set(tracker, "LookAt", { enabled: true }),
      ...fixture.set(tip, "Hierarchy", { parent: tracker }),
    ]);
    const query = {
      x: 0.5 + tipPoint[0]! / ((4 * 4) / 3),
      y: 0.5 - tipPoint[1]! / 4,
      width: 400,
      height: 300,
    };
    const hit = await client.query({
      type: "GeometryPickQuery",
      ...query,
      includeViewPlane: true,
    });
    await record("picking", hit);
    check(
      hit.ok && hit.hit?.entity === tip,
      "picking did not consume final child placement",
    );
    const projection = await client.query({
      type: "CameraProjectQuery",
      ...query,
      plane: { point: tipPoint as [number, number, number], normal: [0, 0, 1] },
    });
    await record("projection", projection);
    check(
      projection.ok &&
        projection.position &&
        projection.position.every((v, i) => Math.abs(v - tipPoint[i]!) < 1e-5),
      "camera projection and evaluated child disagree",
    );
    // Camera parenting changes projection/query origins in the same affine space.
    await batch([
      insertComponent(
        client,
        "Hierarchy",
        { kind: "handle", id: camera },
        { parent },
      ),
    ]);
    await capture("parented-camera");
    check(
      compareImages(frames.get("aimed-affine")!, frames.get("parented-camera")!)
        .changedFraction > 0.01,
      "camera ignored parent",
    );
    await batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: camera },
        component: client.components.Hierarchy!.id,
      },
    ]);
    if (contract.CAPABILITIES.skeletalAnimation) {
      await batch([...fixture.set(tracker, "Transform", { y: 0.35 })]);
      await batch([
        insertComponent(
          client,
          "Skeleton",
          { kind: "handle", id: parent },
          { source: "ipp://skeleton/rig-strip" },
        ),
      ]);
      const reference = await fixture.create("joint-reference", {
        Transform: { y: 1 },
        Hierarchy: { parent },
      });
      const source = await fixture.upload(
        {
          duration: 1,
          tracks: [
            {
              joints: [1],
              keys: [0, 1].map((time) => ({
                time,
                value: {
                  kind: "pose" as const,
                  value: [
                    {
                      translation: [0, 1, 0] as [number, number, number],
                      rotation: [
                        0,
                        0,
                        Math.sin((time * Math.PI) / 4),
                        Math.cos((time * Math.PI) / 4),
                      ] as [number, number, number, number],
                      scale: [1, 1, 1] as [number, number, number],
                    },
                  ],
                },
              })),
            },
          ],
        },
        990n,
      );
      const clock = await fixture.create("clip-repeat-clock", { Scalar: {} });
      const clockClip = fixture.curve("Scalar", "value", 0, 1);
      clockClip.duration = 3;
      clockClip.tracks[0]!.keys[1]!.time = 3;
      const clockSource = await fixture.upload(clockClip, 991n);
      const controller = await fixture.controller([
        {
          source,
          target: parent,
          track: 0,
          property: { joints: [1] },
          repeat: true,
        },
        fixture.driver(clock, clockSource, 0, "Scalar", ["value"]),
      ]);
      for (const time of [0, 0.5, 1.5, 2.5]) {
        await fixture.seekPaused(controller, time);
        await batch([
          ...fixture.set(tracker, "Hierarchy", { parent, parent_bone: 1 }),
          ...fixture.set(reference, "Transform", {
            qz: Math.sin(((time % 1) * Math.PI) / 4),
            qw: Math.cos(((time % 1) * Math.PI) / 4),
          }),
        ]);
        await capture(`bone-${time}`);
        await batch([
          ...fixture.set(tracker, "Hierarchy", {
            parent: reference,
            parent_bone: 0xffff_ffff,
          }),
        ]);
        await capture(`bone-reference-${time}`);
        await same(`bone-${time}`, `bone-reference-${time}`);
      }
      check(
        compareImages(frames.get("bone-0")!, frames.get("bone-0.5")!)
          .changedFraction > 0.005,
        "joint animation must move the attached geometry",
      );
      await batch([
        ...fixture.set(tracker, "Hierarchy", { parent, parent_bone: 1 }),
      ]);
      // Persist a bone attachment with a reusable selected pose, independent of temporary uploads.
      await client.deleteAnimationController(controller);
      await batch([
        ...fixture.set(parent, "Skeleton", {
          pose_source: "ipp://pose/rig-strip-bent",
        }),
      ]);
    }
    await capture("before-save");
    const saved = await host.saveWorld();
    await client.close();
    client = await host.loadWorld(saved, { symbolicId: "hierarchy-restored" });
    fixture = new AnimationFixture(client, contract, record);
    await uploads();
    const restored = await client.inspect();
    camera = restored.entities.find(
      (v) => v.metadata.symbolicId === "hierarchy-camera",
    )!.id;
    client.sendCommand({ type: "CameraActivateCommand", entity: camera });
    await capture("restored");
    await same("restored", "before-save");
    await hierarchyLifecycle(client, contract, record);
    return { comparisons: 3, picking: true, persistence: true, plainTip };
  } finally {
    await host.close();
  }
}
