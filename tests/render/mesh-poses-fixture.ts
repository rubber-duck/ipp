import { presentationTesting } from "../../packages/ipp-client/src/testing.js";
import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  AnimationWorldClient,
  Command,
  PresentedCapture,
} from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import type { HostedWorldClient } from "../integration/camera-fixtures.js";
import {
  compareImages,
  summarizeImage,
  type FramePixels,
} from "./image-assertions.js";
import {
  affinePosePosition,
  affinePoseTransform,
  poseMesh,
} from "./mesh-pose-assets.js";
import {
  RootPresentation,
  captureSummary,
  capturedImage,
  recoverRestoredContext,
  worldReference,
} from "./root-presentation.js";
import {
  SKINNING,
  SCENE,
  selectSystems,
} from "../integration/system-selections.js";

function png(frame: FramePixels) {
  const imageCanvas = document.createElement("canvas");
  imageCanvas.width = frame.width;
  imageCanvas.height = frame.height;
  imageCanvas
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
  return imageCanvas.toDataURL("image/png");
}

/**
 * The canvas is 200 x 150: every check compares whole frames or canvas-relative
 * regions, and a smaller canvas shortens each capture and comparison.
 */
const VIEW = Object.freeze({ width: 200, height: 150 });

/** The right instance's columns start at 225 of 400: past the left instance. */
const RIGHT_INSTANCE_START = 0.5625;

/**
 * Scenario intent stays independent of worker startup and artifact persistence.
 * The lit scene adds shadows and skinned poses through its World's Systems; the
 * unlit scene exercises standard unlit and textured deformation without them.
 */
export async function run(
  configuration: {
    generated: string;
    workerScript: string;
    wasm: string;
  },
  scene: "unlit" | "lit" = "lit",
) {
  const lit = scene === "lit";
  const skin = lit;
  const contract = await import(configuration.generated);
  const canvas = document.createElement("canvas");
  canvas.width = VIEW.width;
  canvas.height = VIEW.height;
  document.body.replaceChildren(canvas);
  const client: HostedWorldClient<AnimationWorldClient> =
    await contract.IppClient.connectWorker(
      configuration.workerScript,
      configuration.wasm,
      {
        selectedSystems: lit
          ? selectSystems(SCENE, SKINNING)
          : selectSystems(SCENE),
        canvas: canvas.transferControlToOffscreen(),
        timeoutMs: 10_000,
      },
    );
  const record = async (kind: string, value: unknown) => {
    await (
      globalThis as unknown as {
        recordMeshPose(kind: string, value: unknown): Promise<void>;
      }
    ).recordMeshPose(
      kind,
      JSON.parse(
        JSON.stringify(value, (_key, value: unknown) =>
          typeof value === "bigint" ? value.toString() : value,
        ),
      ),
    );
  };
  const fixture = new AnimationFixture(client, contract, record);
  const frames = new Map<string, FramePixels>();
  const comparisons: ({ actual: string; expected: string } & ReturnType<
    typeof compareImages
  >)[] = [];
  try {
    const batch = async (operations: Command[]) => {
      const outcome = await client.batch(operations);
      await record("batch", outcome);
      check(outcome.ok, "scene batch failed");
    };
    const upload = async (
      id: bigint,
      bytes: Uint8Array<ArrayBuffer>,
      kind = 1,
    ) => {
      const outcome = clientAssetSource(client.session, kind, id);
      await client.registerAsset(outcome, bytes.buffer);
      await record("asset", outcome);
      return outcome.source;
    };
    const capture = async (label: string, draws = lit ? 3 : 2) => {
      const state = await client.inspect();
      const deadline = performance.now() + 10_000;
      let frame: PresentedCapture;
      do {
        frame = await presentation.capture();
        check(
          presentation.sourceTick(frame) >= state.tick,
          `${label}: capture does not include the inspected scene`,
        );
        check(
          performance.now() < deadline,
          `${label}: expected ${draws} draws, got ${frame.drawCalls}`,
        );
      } while (frame.drawCalls !== draws);
      const image = capturedImage(frame);
      frames.set(label, image);
      await record("capture", {
        label,
        metadata: {
          ...captureSummary(frame),
          sourceTick: presentation.sourceTick(frame),
        },
        summary: summarizeImage(image),
        dataUrl: png(image),
      });
      return frame;
    };
    const same = async (
      actual: string,
      expected: string,
      tolerance = 0.0005,
    ) => {
      const difference = compareImages(
        frames.get(actual)!,
        frames.get(expected)!,
      );
      if (difference.changedFraction > tolerance) {
        const actualFrame = frames.get(actual)!;
        const a = new Uint8Array(actualFrame.pixels);
        const b = new Uint8Array(frames.get(expected)!.pixels);
        const diff = a.map((value, index) =>
          index % 4 === 3
            ? 255
            : Math.min(255, Math.abs(value - b[index]!) * 4),
        );
        await record("capture", {
          label: `${actual}-diff`,
          dataUrl: png({ ...actualFrame, pixels: diff.buffer }),
        });
      }
      comparisons.push({ actual, expected, ...difference });
      await record("comparison", comparisons.at(-1));
      check(
        difference.changedFraction <= tolerance,
        `${actual} differs from ${expected}: ${JSON.stringify(difference)}`,
      );
    };
    const camera = await fixture.create("pose-camera", {
      Transform: { z: 6 },
      Camera: { projection: 1, ortho_height: 4.5 },
    });
    const presentation = await RootPresentation.camera(
      client.host,
      worldReference(client),
      camera,
      { width: canvas.width, height: canvas.height },
    );
    const base = await upload(101n, poseMesh(0, { skin }));
    const target = await upload(102n, poseMesh(1));
    const baked = [];
    for (const [index, weight] of [0, 0.5, 1].entries())
      baked.push(await upload(BigInt(103 + index), poseMesh(weight, { skin })));
    const reversed = await upload(106n, poseMesh(1, { reversed: true }));
    const flat = await upload(107n, poseMesh(1, { normals: false }));
    const flatBaked = await upload(
      108n,
      poseMesh(0.5, { normals: false, skin }),
    );
    let skeleton = 0n;
    let skinSource = "";
    if (skin) {
      const source = await upload(
        201n,
        contract.encodeSkeletonAsset([{ parent: null }]),
        contract.WIRE.ASSET_SKELETON,
      );
      skinSource = await upload(
        202n,
        contract.encodeSkinAsset([
          {
            joint: 0,
            inverseBind: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
          },
        ]),
        contract.WIRE.ASSET_SKIN,
      );
      skeleton = await fixture.create("pose-rig", {
        Transform: { x: -1.25 },
        Skeleton: { source },
      });
      await batch(
        contract.Skeleton.patchJoints(contract.Entity.handle(skeleton), 0, {
          rotation: [0, 0, Math.sin(0.2), Math.cos(0.2)],
        }),
      );
    }
    const a = await fixture.create("pose-left", {
      Transform: { x: -1.25 },
      MeshInstance: { source: base },
      MeshPose: { source: target },
      BoundingGeometry: {},
      UnlitMaterial: { r: 0.2, g: 0.7, b: 0.95 },
      ...(lit
        ? {
            PbrMaterial: {
              r: 0.2,
              g: 0.7,
              b: 0.95,
              metallic: 0.1,
              roughness: 0.6,
              cast_shadows: true,
            },
          }
        : {}),
      ...(skin ? { Skin: { skeleton, source: skinSource } } : {}),
    });
    const b = await fixture.create("pose-right", {
      Transform: { x: 1.25 },
      MeshInstance: { source: base },
      MeshPose: { source: target, weight: 0.25 },
      BoundingGeometry: {},
      UnlitMaterial: { r: 0.95, g: 0.2, b: 0.1 },
      ...(lit
        ? { PbrMaterial: { r: 0.95, g: 0.2, b: 0.1, roughness: 0.6 } }
        : {}),
    });
    let receiver = 0n;
    if (lit) {
      await fixture.create("pose-spot", {
        Transform: { x: -1.5, y: 2, z: 4 },
        Light: {
          kind: 2,
          intensity: 90,
          range: 12,
          outer_cone: 1.1,
          inner_cone: 0.8,
          cast_shadows: true,
        },
      });
      await fixture.create("pose-fill", {
        Transform: {},
        Light: { kind: 0, intensity: 0.7 },
      });
      const plane = await upload(109n, poseMesh(0));
      receiver = await fixture.create("pose-receiver", {
        Transform: { z: -0.65, sx: 4, sy: 3 },
        MeshInstance: { source: plane },
        PbrMaterial: {
          r: 0.6,
          g: 0.6,
          b: 0.6,
          receive_shadows: true,
          cast_shadows: false,
        },
      });
    }
    for (const [index, weight] of [0, 0.5, 1].entries()) {
      await batch([
        ...fixture.set(a, "MeshInstance", { source: base }),
        ...fixture.set(a, "MeshPose", { source: target, weight }),
      ]);
      await capture(`pose-${weight}`);
      await batch([
        ...fixture.set(a, "MeshInstance", { source: baked[index]! }),
        ...fixture.set(a, "MeshPose", { source: "" }),
      ]);
      await capture(`baked-${weight}`);
      await same(`pose-${weight}`, `baked-${weight}`);
    }
    check(
      compareImages(frames.get("pose-0")!, frames.get("pose-1")!)
        .changedFraction > 0.01,
      "endpoint poses must visibly differ",
    );
    const first = new Uint8Array(frames.get("pose-0")!.pixels);
    const last = new Uint8Array(frames.get("pose-1")!.pixels);
    // Compare the right instance, excluding the receiver's changing shadow.
    if (!lit)
      for (let y = 0; y < VIEW.height; y++)
        for (
          let x = Math.ceil(VIEW.width * RIGHT_INSTANCE_START);
          x < VIEW.width;
          x++
        ) {
          const offset = (y * VIEW.width + x) * 4;
          check(
            first[offset] === last[offset] &&
              first[offset + 1] === last[offset + 1] &&
              first[offset + 2] === last[offset + 2],
            "one instance changed its neighbour's blend",
          );
        }
    // Bake the final object transform into the independent reference, so this
    // comparison detects applying a model transform at the wrong deformation stage.
    const affineHalf = await upload(114n, poseMesh(0.5, { affine: true }));
    await batch([
      ...fixture.set(b, "Transform", { ...affinePoseTransform, x: 0 }),
      ...fixture.set(b, "MeshPose", { weight: 0.5 }),
    ]);
    await capture("affine-pose");
    await batch([
      ...fixture.set(b, "Transform", {
        y: 0,
        qy: 0,
        qw: 1,
        sx: 1,
        sy: 1,
        sz: 1,
      }),
      ...fixture.set(b, "MeshInstance", { source: affineHalf }),
      ...fixture.set(b, "MeshPose", { source: "" }),
    ]);
    await capture("affine-baked");
    await same("affine-pose", "affine-baked");
    check(
      compareImages(frames.get("affine-pose")!, frames.get("baked-1")!)
        .changedFraction > 0.005,
      "affine reference does not visibly change the transformed instance",
    );
    const insert = (
      id: bigint,
      name: string,
      values: Record<string, number | bigint | string | boolean>,
    ): Command => ({
      kind: "insertComponent",
      entity: { kind: "handle", id },
      component: client.components[name]!.id,
      fields: fixture.fields(name, values),
    });
    const remove = (id: bigint, name: string): Command => ({
      kind: "removeComponent",
      entity: { kind: "handle", id },
      component: client.components[name]!.id,
    });
    const place = (id: bigint, parent: bigint | null): Command => ({
      kind: "placeEntity",
      entity: { kind: "handle", id },
      placement: {
        parent: parent === null ? null : { kind: "handle", id: parent },
        before: null,
      },
    });
    const parent = await fixture.create("pose-affine-parent", {
      Transform: affinePoseTransform,
    });
    const [x, y, z] = affinePosePosition([1, 0, -1]);
    const aimTarget = await fixture.create("pose-aim-target", {
      Transform: { x: x!, y: y!, z: z! },
    });
    await batch([
      place(b, parent),
      ...fixture.set(b, "MeshInstance", { source: base }),
      ...fixture.set(b, "MeshPose", { source: target, weight: 0.5 }),
    ]);
    await capture("hierarchy-pose");
    await same("hierarchy-pose", "affine-baked");
    await batch([insert(b, "LookAt", { target: aimTarget })]);
    await capture("aimed-pose");
    const aimedHalf = await upload(
      115n,
      poseMesh(0.5, { aim: true, affine: true }),
    );
    await batch([
      place(b, null),
      remove(b, "LookAt"),
      ...fixture.set(b, "MeshInstance", { source: aimedHalf }),
      ...fixture.set(b, "MeshPose", { source: "" }),
    ]);
    await capture("aimed-baked");
    await same("aimed-pose", "aimed-baked");
    if (skin) {
      // The rig and mesh share final object placement; joint motion stays mesh-local.
      // Bake joint deformation, aim, and affine placement independently for comparison.
      const skinnedHalf = await upload(
        116n,
        poseMesh(0.5, { joint: true, aim: true, affine: true }),
      );
      await batch([
        remove(a, "Skin"),
        place(b, parent),
        insert(b, "LookAt", { target: aimTarget }),
        ...fixture.set(skeleton, "Transform", { x: 0 }),
        place(skeleton, b),
        insert(b, "Skin", { skeleton, source: skinSource }),
        ...fixture.set(b, "MeshInstance", { source: base }),
        ...fixture.set(b, "MeshPose", { source: target, weight: 0.5 }),
      ]);
      await capture("skinned-aimed-pose");
      await batch([
        remove(b, "Skin"),
        place(b, null),
        remove(b, "LookAt"),
        ...fixture.set(b, "MeshInstance", { source: skinnedHalf }),
        ...fixture.set(b, "MeshPose", { source: "" }),
      ]);
      await capture("skinned-aimed-baked");
      await same("skinned-aimed-pose", "skinned-aimed-baked");
      await batch([
        place(skeleton, null),
        ...fixture.set(skeleton, "Transform", { x: -1.25 }),
        insert(a, "Skin", { skeleton, source: skinSource }),
      ]);
    }
    await batch([
      ...fixture.set(b, "Transform", { x: 1.25 }),
      ...fixture.set(b, "MeshInstance", { source: base }),
      ...fixture.set(b, "MeshPose", { source: target, weight: 0.25 }),
    ]);
    await batch([
      ...fixture.set(a, "MeshInstance", { source: base }),
      ...fixture.set(a, "MeshPose", { source: target, weight: 0 }),
    ]);
    const descriptor = client.components.MeshPose!;
    const clip = await fixture.upload({
      duration: 1,
      tracks: [
        {
          property: {
            component: descriptor.id,
            offsets: [descriptor.fields.weight!.offset],
          },
          keys: [
            { time: 0, value: { kind: "f32", value: 0 } },
            { time: 1, value: { kind: "f32", value: 1 } },
          ],
        },
      ],
    });
    const controller = await fixture.controller([
      fixture.driver(a, clip, 0, "MeshPose", ["weight"]),
    ]);
    await fixture.seekPaused(controller, 0.5);
    await capture("animated-half");
    await same("animated-half", "pose-0.5");
    const inspection = await fixture.inspect();
    check(
      fixture.value(inspection, a, "MeshPose", "weight") === 0.5,
      "the weight field does not hold the sampled value",
    );
    client.playback(controller, { action: "stop" });
    await capture("stopped");
    await same("stopped", "pose-0");
    // Stop subtracts the controller's contribution, leaving the authored weight.
    check(
      fixture.value(await fixture.inspect(), a, "MeshPose", "weight") === 0,
      "stopping did not subtract the contribution",
    );
    for (const weight of [-0.1, 1.1]) {
      const outcome = await client.batch([
        ...fixture.set(a, "Transform", { x: 99 }),
        ...fixture.set(a, "MeshPose", { weight }),
        ...fixture.set(b, "MeshPose", { weight: 1 }),
      ]);
      check(!outcome.ok, "invalid pose weight was accepted");
      await record("rejection", outcome);
      const partial = await fixture.inspect();
      check(
        fixture.value(partial, a, "Transform", "x") === 99,
        "failure rolled back prior transform",
      );
      check(
        fixture.value(partial, b, "MeshPose", "weight") === 0.25,
        "failure did not stop the batch",
      );
      await batch([
        ...fixture.set(a, "Transform", { x: -1.25 }),
        {
          kind: "insertComponent",
          entity: { kind: "handle", id: a },
          component: descriptor.id,
          fields: fixture.fields("MeshPose", { source: target, weight: 0 }),
        },
      ]);
    }
    await capture("repaired");
    await same("repaired", "stopped");
    // Loaded topology is validated after applying the reference change. The
    // shared asset remains available while this incompatible instance is suppressed.
    const incompatible = await client.batch(
      fixture.set(a, "MeshPose", { source: reversed }),
    );
    await record("rejection", incompatible);
    check(
      !incompatible.ok && incompatible.error?.reason === "InvalidAsset",
      "loaded incompatible topology was accepted",
    );
    await capture("incompatible", lit ? 2 : 1);
    await batch(fixture.set(a, "MeshPose", { source: target }));
    await capture("topology-repaired");
    await same("topology-repaired", "stopped");
    await batch(fixture.set(a, "MeshPose", { source: flat, weight: 0.5 }));
    await capture("flat-pose");
    await batch([
      ...fixture.set(a, "MeshInstance", { source: flatBaked }),
      ...fixture.set(a, "MeshPose", { source: "" }),
    ]);
    await capture("flat-baked");
    await same("flat-pose", "flat-baked");
    await batch([
      ...fixture.set(a, "MeshInstance", { source: base }),
      ...fixture.set(a, "MeshPose", { source: target, weight: 0.5 }),
    ]);
    const before = await capture("before-recovery");
    presentationTesting(presentation.diagnostics).loseContext();
    presentationTesting(presentation.diagnostics).restoreContext();
    await recoverRestoredContext(presentation);
    const recovered = await capture("recovered");
    check(
      recovered.view.surface.context > before.view.surface.context,
      "recovered capture did not use the restored context",
    );
    await same("recovered", "before-recovery");
    // Late input enters the normal owned data plane; pending use stays absent.
    await batch(
      fixture.set(a, "MeshPose", {
        source: clientAssetSource(client.session, 1, 110n).source,
        weight: 1,
      }),
    );
    await capture("pending", lit ? 2 : 1);
    await upload(110n, poseMesh(1));
    await capture("ready");
    await same("ready", "pose-1");
    // Same-source interpolation also exercises aliasing of the shared GPU record.
    await batch(fixture.set(a, "MeshPose", { source: base, weight: 0.5 }));
    await capture("same-source");
    await same("same-source", "pose-0");
    if (lit) {
      await batch(fixture.set(a, "MeshPose", { source: target, weight: 0.5 }));
      await capture("shadowed-pose");
      await batch(fixture.set(a, "PbrMaterial", { cast_shadows: false }));
      await capture("without-pose-shadow");
      const difference = compareImages(
        frames.get("shadowed-pose")!,
        frames.get("without-pose-shadow")!,
      );
      await record("shadow-difference", difference);
      check(
        difference.changedFraction > 0.0003,
        "pose fixture has no measurable receiver shadow",
      );
      await batch(fixture.set(a, "PbrMaterial", { cast_shadows: true }));
    } else {
      const outside = await upload(111n, poseMesh(0, { x: 8 }));
      await batch([
        ...fixture.set(a, "MeshInstance", { source: outside }),
        ...fixture.set(a, "MeshPose", { source: base, weight: 1 }),
      ]);
      await capture("posed-into-frustum");
      await same("posed-into-frustum", "pose-0");
      await batch([
        ...fixture.set(a, "MeshInstance", { source: base }),
        ...fixture.set(a, "MeshPose", { source: target, weight: 0 }),
      ]);
    }
    if (!lit) {
      const texturedBase = await upload(112n, poseMesh(0, { uvs: true }));
      const texturedHalf = await upload(113n, poseMesh(0.5, { uvs: true }));
      const pixels = new Uint8Array(32);
      pixels.set([0x49, 0x50, 0x50, 0x54]);
      const header = new DataView(pixels.buffer);
      [3, 2, 2].forEach((value, index) =>
        header.setUint32(4 + index * 4, value, true),
      );
      pixels.set(
        [255, 255, 255, 255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255],
        16,
      );
      const texture = await upload(301n, pixels, contract.WIRE.ASSET_TEXTURE);
      await batch([
        ...fixture.set(a, "MeshInstance", { source: texturedBase }),
        ...fixture.set(a, "MeshPose", { source: target, weight: 0.5 }),
        {
          kind: "insertComponent",
          entity: { kind: "handle", id: a },
          component: client.components.UnlitTexture!.id,
          fields: fixture.fields("UnlitTexture", { source: texture }),
        },
      ]);
      await capture("textured-pose");
      await batch([
        ...fixture.set(a, "MeshInstance", { source: texturedHalf }),
        ...fixture.set(a, "MeshPose", { source: "" }),
      ]);
      await capture("textured-baked");
      await same("textured-pose", "textured-baked");
      check(
        compareImages(frames.get("textured-pose")!, frames.get("pose-0.5")!)
          .changedFraction > 0.01,
        "texture fixture does not affect the pose surface",
      );
      await batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: a },
          component: client.components.UnlitTexture!.id,
        },
        ...fixture.set(a, "MeshInstance", { source: base }),
        ...fixture.set(a, "MeshPose", { source: target }),
      ]);
    }
    await fixture.seekPaused(controller, 0.5);
    await batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: a },
        component: descriptor.id,
      },
      {
        kind: "insertComponent",
        entity: { kind: "handle", id: a },
        component: descriptor.id,
        fields: fixture.fields("MeshPose", { source: target, weight: 0 }),
      },
    ]);
    const replaced = await fixture.inspect();
    check(
      fixture.state(replaced, controller).state === "stopped",
      "replacement inherited an old pose controller binding",
    );
    await capture("replacement");
    await same("replacement", "pose-0");
    await fixture.seekPaused(controller, 0.5);
    await capture("rebound");
    await same("rebound", "pose-0.5");
    // Remove pose and mesh instances together; no stale draw or playback survives.
    await batch(
      [a, b, ...(receiver ? [receiver] : [])].map((id) => ({
        kind: "delete",
        entity: { kind: "handle", id },
      })),
    );
    await capture("deleted", 0);
    check(
      summarizeImage(frames.get("deleted")!).foregroundPixels === 0,
      "deleted instances left geometry",
    );
    return {
      comparisons,
      skin,
      lit,
      independentInstances: 2,
      contextRestored: true,
    };
  } finally {
    await client.close();
  }
}
