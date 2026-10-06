import { clientAssetSource } from "../../../packages/ipp-client/src/asset-sources.js";
import type {
  AnimationWorldClient,
  PickingWorldClient,
  WorldPersistenceHostClient,
  PresentedCapture,
  Command,
  WorldReference,
} from "@ipp/client";
import { AnimationFixture } from "../../fixtures/animation.js";
import { check } from "../../harness/page/checks.js";
import { hierarchyLifecycle } from "../../runtime/scenarios/hierarchy.js";
import { successfulBatch, insertComponent } from "../../fixtures/commands.js";
import {
  affinePoseTransform,
  affinePosePosition,
  aimedPoseVector,
  poseMesh,
} from "../../fixtures/mesh-poses.js";
import {
  compareImages,
  summarizeImage,
  type FramePixels,
  rgbaDataUrl,
} from "../../harness/page/images.js";
import {
  RootPresentation,
  capturedImage,
} from "../../harness/page/presentation.js";
import {
  CONSTRAINTS,
  LIFECYCLE,
  SKINNING,
  SCENE,
  selectSystems,
} from "../../fixtures/system-selections.js";

type Client = AnimationWorldClient & PickingWorldClient;

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
  const viewport = { width: canvas.width, height: canvas.height };
  const created = await host.createWorld({
    selectedSystems: selectSystems(SCENE, SKINNING, CONSTRAINTS, LIFECYCLE),
    symbolicId: "hierarchy-render",
  });
  const worlds: WorldReference[] = [created.reference];
  let client = await host.openWorld(created.reference);
  let presentation: RootPresentation | undefined;
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
  const place = (entity: bigint, parent: bigint | null): Command => ({
    kind: "placeEntity",
    entity: { kind: "handle", id: entity },
    placement: {
      parent: parent === null ? null : { kind: "handle", id: parent },
      before: null,
    },
  });
  const frames = new Map<string, FramePixels>();
  const capture = async (label: string) => {
    check(presentation, "Camera presentation is not selected");
    const start = await client.inspect();
    const deadline = performance.now() + 15000;
    let frame: PresentedCapture;
    do {
      frame = await presentation.capture();
      check(
        presentation.sourceTick(frame) >= start.tick,
        `${label}: capture does not include the inspected state`,
      );
      check(
        performance.now() < deadline,
        `${label}: scene did not reach two draws`,
      );
    } while (frame.drawCalls !== 2);
    const image = capturedImage(frame);
    frames.set(label, image);
    await record("capture", {
      label,
      dataUrl: rgbaDataUrl(image),
      summary: summarizeImage(image),
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
      dataUrl: rgbaDataUrl({ ...a, pixels: pixels.buffer }),
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
    presentation = await RootPresentation.camera(
      host,
      created.reference,
      camera,
      viewport,
    );
    const parent = await fixture.create("hierarchy-render-parent", {
      Transform: affinePoseTransform,
    });
    const targetPoint = affinePosePosition([1, 0, -1]);
    const target = await fixture.create("hierarchy-render-target", {
      Transform: { x: targetPoint[0]!, y: targetPoint[1]!, z: targetPoint[2]! },
    });
    const tracker = await fixture.create("hierarchy-render-tracker", {
      Transform: {},
      LookAt: { target, enabled: false },
      MeshInstance: {
        source: clientAssetSource(client.session, 1, 601n).source,
      },
      UnlitMaterial: { r: 0.1, g: 0.6, b: 1 },
    });
    const tip = await fixture.create("hierarchy-render-tip", {
      Transform: { z: -1, sx: 0.12, sy: 0.12, sz: 0.12 },
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
    await batch([place(tracker, parent), place(tip, tracker)]);
    const plainTip = affinePosePosition([0, 0, -1]);
    await capture("hierarchy-affine");
    // Compare with independently baked positions and a flat child placement.
    await batch([
      place(tracker, null),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 602n).source,
      }),
      place(tip, parent),
      ...fixture.set(tip, "Transform", { z: -1 }),
    ]);
    await capture("hierarchy-baked");
    await same("hierarchy-affine", "hierarchy-baked");
    await batch([
      place(tracker, parent),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 601n).source,
      }),
      ...fixture.set(tracker, "LookAt", { enabled: true }),
      place(tip, tracker),
    ]);
    await capture("aimed-affine");
    const tipPoint = affinePosePosition(aimedPoseVector([0, 0, -1]));
    // Freeze the same independently known local orientation for the reference tip.
    const tipQ = { qy: -Math.sin(Math.PI / 8), qw: Math.cos(Math.PI / 8) };
    const tipParent = await fixture.create("hierarchy-tip-reference", {
      Transform: tipQ,
    });
    await batch([
      place(tipParent, parent),
      ...fixture.set(tracker, "LookAt", { enabled: false }),
      place(tracker, null),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 603n).source,
      }),
      place(tip, tipParent),
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
      place(tracker, parent),
      ...fixture.set(tracker, "MeshInstance", {
        source: clientAssetSource(client.session, 1, 601n).source,
      }),
      ...fixture.set(tracker, "LookAt", { enabled: true }),
      place(tip, tracker),
    ]);
    const query = {
      view: { kind: "bound" as const, binding: presentation.binding },
      x: 0.5 + tipPoint[0]! / ((4 * 4) / 3),
      y: 0.5 - tipPoint[1]! / 4,
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
    await batch([place(camera, parent)]);
    await capture("parented-camera");
    check(
      compareImages(frames.get("aimed-affine")!, frames.get("parented-camera")!)
        .changedFraction > 0.01,
      "camera ignored parent",
    );
    await batch([place(camera, null)]);
    {
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
      });
      await batch([
        place(reference, parent),
        insertComponent(
          client,
          "ParentJoint",
          { kind: "handle", id: tracker },
          { ordinal: 0xffff_ffff },
        ),
      ]);
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
          place(tracker, parent),
          ...fixture.set(tracker, "ParentJoint", { ordinal: 1 }),
          ...fixture.set(reference, "Transform", {
            qz: Math.sin(((time % 1) * Math.PI) / 4),
            qw: Math.cos(((time % 1) * Math.PI) / 4),
          }),
        ]);
        await capture(`bone-${time}`);
        await batch([
          ...fixture.set(tracker, "ParentJoint", { ordinal: 0xffff_ffff }),
          place(tracker, reference),
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
        place(tracker, parent),
        ...fixture.set(tracker, "ParentJoint", { ordinal: 1 }),
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
    const saved = await host.saveWorld(client.session);
    await presentation.close();
    presentation = undefined;
    await client.close();
    const graph = await host.loadWorld(saved, {
      symbolicId: "hierarchy-restored",
    });
    worlds.push(...graph.created.values());
    client = await host.openWorld(graph.root);
    fixture = new AnimationFixture(client, contract, record);
    await uploads();
    const restored = await client.inspect();
    camera = restored.entities.find(
      (v) => v.metadata.symbolicId === "hierarchy-camera",
    )!.id;
    presentation = await RootPresentation.camera(
      host,
      graph.root,
      camera,
      viewport,
    );
    await capture("restored");
    await same("restored", "before-save");
    await hierarchyLifecycle(client, contract, record);
    return { comparisons: 3, picking: true, persistence: true, plainTip };
  } finally {
    try {
      await presentation?.close();
      for (const session of host.sessions.values()) await session.close();
      for (const world of worlds) await host.destroyWorld(world);
    } finally {
      await host.close();
    }
  }
}
