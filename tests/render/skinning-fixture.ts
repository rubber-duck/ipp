import { settledAsset } from "../integration/asset-fixtures.js";
import { clientAssetSource } from "../../packages/ipp-client/src/asset-sources.js";
import type {
  AnimationWorldClient,
  CameraWorldClient,
  FrameCapture,
} from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import { summarizeImage, compareImages } from "./image-assertions.js";

let client: CameraWorldClient | undefined;
let contract: Record<string, any>;
let rigs: bigint[] = [];
let receiver: bigint | undefined;
let animation: AnimationFixture | undefined;
let controllers: bigint[] = [];
let animationSources: { walk: string; wave: string } | undefined;
const captures = new Map<string, FrameCapture>();

export async function initialize(configuration: {
  generatedModuleUrl: string;
  workerScriptUrl: string;
  wasmUrl: string;
}) {
  await close();
  const canvas = document.createElement("canvas");
  canvas.id = "skinning-canvas";
  canvas.width = 400;
  canvas.height = 300;
  document.body.replaceChildren(canvas);
  contract = await import(configuration.generatedModuleUrl);
  client = await contract.IppClient.connectWorker(
    configuration.workerScriptUrl,
    configuration.wasmUrl,
    { canvas: canvas.transferControlToOffscreen(), timeoutMs: 10_000 },
  );
  const c = current();
  const {
    Entity,
    Transform,
    Camera,
    Skeleton,
    Skin,
    MeshInstance,
    UnlitMaterial,
  } = contract;
  const operations = [
    Entity.create(0),
    Transform.insert(Entity.alias(0), { x: 0, y: 1, z: 5 }),
    Camera.insert(Entity.alias(0), { projection: 1, ortho_height: 3 }),
  ];
  for (const [alias, x] of [
    [1, -0.8],
    [2, 0.8],
  ] as const) {
    const e = Entity.alias(alias);
    operations.push(
      Entity.create(alias),
      Transform.insert(e, { x }),
      Skeleton.insert(e, { source: "ipp://skeleton/rig-strip" }),
      Skin.insert(e, { skeleton: e, source: "ipp://skin/rig-strip" }),
      MeshInstance.insert(e, { source: "ipp://mesh/rig-strip" }),
      UnlitMaterial.insert(e),
    );
  }
  const outcome = await c.batch(operations);
  if (!outcome.ok) throw new Error(JSON.stringify(outcome));
  rigs = outcome.aliases.slice(1).map((entry) => entry.id);
  c.sendCommand({
    type: "CameraActivateCommand",
    entity: outcome.aliases[0]!.id,
  });
  return {
    session: c.session,
    schemaHash: c.schemaHash,
    capabilities: contract.CAPABILITIES,
  };
}

function current(): CameraWorldClient {
  if (!client) throw new Error("fixture inactive");
  return client;
}

export async function pose(index: number, mode: "rest" | "bent" | "override") {
  const { Skeleton, Entity, encodeJointOverrides } = contract;
  const entity = Entity.handle(rigs[index]);
  const joints =
    mode === "override"
      ? encodeJointOverrides([
          {
            joint: 1,
            translation: [0, 1, 0],
            rotation: [0, 0, Math.SQRT1_2, Math.SQRT1_2],
          },
        ])
      : new Uint8Array();
  const result = await current().batch([
    Skeleton.setPose_source(
      entity,
      mode === "bent" ? "ipp://pose/rig-strip-bent" : "",
    ),
    Skeleton.setJoints(entity, joints),
  ]);
  if (!result.ok) throw new Error("pose change rejected");
  return result;
}

export async function capture(label: string, draws = 2) {
  const c = current();
  const after = (await c.waitForFrame()).tick;
  const deadline = performance.now() + 10_000;
  while (performance.now() < deadline) {
    const frame = await c.presentation!.capture();
    if (frame.tick > after && frame.drawCalls === draws) {
      captures.set(label, frame);
      return {
        tick: frame.tick,
        drawCalls: frame.drawCalls,
        triangles: frame.triangles,
        backend: frame.backend,
        summary: summarizeImage(frame),
        resources: (await c.inspect()).resources,
      };
    }
    await c.waitForFrame(frame.tick);
  }
  throw new Error("skinning frame readiness timed out");
}

export function captureMetadata(label: string) {
  const { pixels: _, ...metadata } = frame(label);
  return metadata;
}
function frame(label: string) {
  const value = captures.get(label);
  if (!value) throw new Error("missing capture");
  return value;
}
export function captureDataUrl(label: string) {
  const f = frame(label);
  const canvas = document.createElement("canvas");
  canvas.width = f.width;
  canvas.height = f.height;
  const context = canvas.getContext("2d")!;
  context.putImageData(
    new ImageData(new Uint8ClampedArray(f.pixels), f.width, f.height),
    0,
    0,
  );
  return canvas.toDataURL("image/png");
}
export function difference(a: string, b: string) {
  return compareImages(frame(a), frame(b));
}
export function regionDifference(
  a: string,
  b: string,
  left: number,
  right: number,
) {
  const first = frame(a),
    second = frame(b);
  const ap = new Uint8Array(first.pixels),
    bp = new Uint8Array(second.pixels);
  let changed = 0;
  for (let y = 0; y < first.height; y++)
    for (let x = left; x < right; x++) {
      const i = (y * first.width + x) * 4;
      if ([0, 1, 2].some((c) => Math.abs(ap[i + c]! - bp[i + c]!) > 5))
        changed++;
    }
  return changed;
}
export function pixel(label: string, x: number, y: number) {
  const f = frame(label);
  const i = (y * f.width + x) * 4;
  return Array.from(new Uint8Array(f.pixels).slice(i, i + 3));
}
export async function applyInvalidPose() {
  const { Skeleton, Scalar, Entity, encodeJointOverrides } = contract;
  const entity = Entity.handle(rigs[0]);
  const result = await current().batch([
    Scalar.insert(entity, { value: 123 }),
    Skeleton.setJoints(entity, encodeJointOverrides([{ joint: 31 }])),
  ]);
  const inspect = await current().inspect();
  const scalar = inspect.entities
    .find((value) => value.id === rigs[0])!
    .base.find((value) => value.component === Scalar.id)!.fields.value;
  return { result, scalar };
}
export async function recover(draws = 2) {
  const presentation = current().presentation!;
  const previous = await presentation.capture();
  presentation.loseContext();
  presentation.restoreContext();
  const deadline = performance.now() + 10_000;
  while (performance.now() < deadline) {
    const next = await presentation.capture();
    if (
      next.contextGeneration > previous.contextGeneration &&
      next.drawCalls === draws
    )
      return;
  }
  throw new Error("context recovery timed out");
}
export async function close() {
  if (client) await client.close();
  client = undefined;
  rigs = [];
  receiver = undefined;
  animation = undefined;
  controllers = [];
  animationSources = undefined;
  captures.clear();
}

/** Four distinct joints from generated encoders and the real upload data plane. */
export async function uploadedFourJointScene() {
  const {
    Entity,
    Transform,
    Skeleton,
    Skin,
    MeshInstance,
    UnlitMaterial,
    WIRE,
    encodeSkeletonAsset,
    encodePoseAsset,
    encodeSkinAsset,
    encodeSkinnedMesh,
  } = contract;
  const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  const skeleton = encodeSkeletonAsset(
    Array.from({ length: 4 }, () => ({ parent: null })),
  );
  const pose = encodePoseAsset([
    { translation: [-0.8, 0, 0] },
    { translation: [0.8, 0, 0] },
    { translation: [0, 1, 0] },
    { translation: [0, 3, 0] },
  ]);
  const binding = encodeSkinAsset(
    Array.from({ length: 4 }, (_, joint) => ({ joint, inverseBind: identity })),
  );
  const mesh = encodeSkinnedMesh({
    positions: [
      [-0.25, -0.25, 0],
      [0.25, -0.25, 0],
      [-0.25, 0.25, 0],
      [0.25, 0.25, 0],
    ],
    joints: Array.from({ length: 4 }, () => [0, 1, 2, 3]),
    weights: Array.from({ length: 4 }, () => [0.0625, 0.125, 0.1875, 0.125]),
    indices: [0, 1, 2, 1, 3, 2],
  });
  for (const [kind, asset, bytes] of [
    [WIRE.ASSET_SKELETON, 101n, skeleton],
    [WIRE.ASSET_POSE, 102n, pose],
    [WIRE.ASSET_SKIN, 103n, binding],
    [WIRE.ASSET_MESH, 104n, mesh],
  ] as const) {
    const receipt = clientAssetSource(current().session, kind, asset);
    await current().registerAsset(receipt, bytes.buffer);
  }
  const invalid = skeleton.slice();
  new DataView(invalid.buffer).setUint32(12, 0, true); // self-parent
  const rejected = clientAssetSource(
    current().session,
    WIRE.ASSET_SKELETON,
    105n,
  );
  await current().registerAsset(rejected, invalid.buffer);
  const e = Entity.alias(0);
  const outcome = await current().batch([
    ...rigs.map((id) => Entity.delete(Entity.handle(id))),
    Entity.create(0),
    Transform.insert(e),
    Skeleton.insert(e, {
      source: clientAssetSource(current().session, WIRE.ASSET_SKELETON, 101n)
        .source,
      pose_source: clientAssetSource(current().session, WIRE.ASSET_POSE, 102n)
        .source,
    }),
    Skin.insert(e, {
      skeleton: e,
      source: clientAssetSource(current().session, WIRE.ASSET_SKIN, 103n)
        .source,
    }),
    MeshInstance.insert(e, {
      source: clientAssetSource(current().session, WIRE.ASSET_MESH, 104n)
        .source,
    }),
    UnlitMaterial.insert(e, { r: 0, g: 1, b: 0 }),
  ]);
  if (!outcome.ok) throw new Error("four-joint scene rejected");
  rigs = outcome.aliases.map((entry) => entry.id);
  return { rejected: await settledAsset(current(), rejected) };
}

/** Lit geometry and its depth silhouette must agree with an analytic rigid bake. */
export async function lightingScene() {
  const { Entity, Transform, MeshInstance, PbrMaterial, Light } = contract;
  const yaw = Math.atan(0.5) / 2;
  const result = await current().batch([
    ...rigs.map((id) =>
      PbrMaterial.insert(Entity.handle(id), { roughness: 0.8 }),
    ),
    Entity.create(10),
    Transform.insert(Entity.alias(10), { y: 1, z: -1 }),
    MeshInstance.insert(Entity.alias(10), {
      source: "ipp://mesh/cube?width=5&height=3&length=0.05",
    }),
    PbrMaterial.insert(Entity.alias(10), {
      r: 0.6,
      g: 0.6,
      b: 0.6,
      roughness: 0.9,
      cast_shadows: false,
    }),
    Entity.create(11),
    Transform.insert(Entity.alias(11), {
      x: 2,
      y: 1,
      z: 4,
      qy: Math.sin(yaw),
      qw: Math.cos(yaw),
    }),
    Light.insert(Entity.alias(11), {
      kind: 2,
      intensity: 60,
      inner_cone: 0.5,
      outer_cone: 0.9,
      cast_shadows: true,
    }),
  ]);
  if (!result.ok) throw new Error("lit rig scene rejected");
  receiver = result.aliases.find((entry) => entry.alias === 10)!.id;
}

export async function receiveShadows(enabled: boolean) {
  const { Entity, PbrMaterial } = contract;
  if (receiver === undefined) throw new Error("missing receiver");
  const result = await current().batch([
    PbrMaterial.setReceive_shadows(Entity.handle(receiver), enabled),
  ]);
  if (!result.ok) throw new Error("receiver edit rejected");
}

export async function bakeBentRig() {
  const { Entity, components, MeshInstance, WIRE, encodeSkinnedMesh } =
    contract;
  const positions = [],
    colors = [],
    normals = [],
    indices = [];
  for (let row = 0; row < 9; row++) {
    const y = row / 4,
      weight = Math.max(0, Math.min(1, y - 0.5));
    for (const x of [-0.25, 0.25]) {
      positions.push([
        x * (1 - weight) + (1 - y) * weight,
        y * (1 - weight) + (1 + x) * weight,
        0,
      ]);
      colors.push([1 - row / 8, 0.15, row / 8]);
      // Independently invert the blended XY matrix for the normal transform.
      const a = 1 - weight,
        b = weight,
        determinant = a * a + b * b;
      normals.push([(0.6 * a) / determinant, (0.6 * b) / determinant, 0.8]);
    }
    if (row < 8) {
      const a = row * 2;
      indices.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
    }
  }
  const bytes = encodeSkinnedMesh({
    positions,
    colors,
    normals,
    indices,
    joints: Array.from({ length: 18 }, () => [0, 0, 0, 0]),
    weights: Array.from({ length: 18 }, () => [1, 0, 0, 0]),
  });
  const upload = clientAssetSource(current().session, WIRE.ASSET_MESH, 110n);
  await current().registerAsset(upload, bytes.buffer);
  const result = await current().batch([
    {
      kind: "removeComponent",
      entity: Entity.handle(rigs[0]),
      component: components.Skin.id,
    },
    MeshInstance.setSource(
      Entity.handle(rigs[0]),
      clientAssetSource(current().session, WIRE.ASSET_MESH, 110n).source,
    ),
  ]);
  if (!result.ok) throw new Error("rig bake rejected");
}

/** Smooth authored normals exercise skinning composed with main's normal stream. */
export async function authoredNormals() {
  const { Entity, MeshInstance, WIRE, encodeSkinnedMesh } = contract;
  const positions = [],
    colors = [],
    joints = [],
    weights = [],
    indices = [];
  for (let row = 0; row < 9; row++) {
    const y = row / 4,
      weight = Math.max(0, Math.min(1, y - 0.5));
    for (const x of [-0.25, 0.25]) {
      positions.push([x, y, 0]);
      colors.push([1 - row / 8, 0.15, row / 8]);
      joints.push([1, 0, 0, 0]); // built-in binding uses reversed joint order
      weights.push([1 - weight, weight, 0, 0]);
    }
    if (row < 8) {
      const a = row * 2;
      indices.push(a, a + 1, a + 2, a + 1, a + 3, a + 2);
    }
  }
  const bytes = encodeSkinnedMesh({
    positions,
    colors,
    joints,
    weights,
    indices,
    normals: positions.map(() => [0.6, 0, 0.8]),
  });
  const upload = clientAssetSource(current().session, WIRE.ASSET_MESH, 111n);
  await current().registerAsset(upload, bytes.buffer);
  const result = await current().batch(
    rigs.map((id) =>
      MeshInstance.setSource(
        Entity.handle(id),
        clientAssetSource(current().session, WIRE.ASSET_MESH, 111n).source,
      ),
    ),
  );
  if (!result.ok) throw new Error("smooth rig rejected");
}

/** Uses the same production controls/observations as native property scenarios. */
export async function animationScene() {
  if (animation)
    await Promise.all(
      controllers.map((id) => animation!.client.deleteAnimationController(id)),
    );
  controllers = [];
  await pose(0, "rest");
  await pose(1, "rest");
  animation = new AnimationFixture(
    current() as CameraWorldClient & AnimationWorldClient,
    { encodeAnimationClip: contract.encodeAnimationClip },
    async () => {},
  );
  let walk = animationSources?.walk;
  let wave = animationSources?.wave;
  if (!walk || !wave) {
    walk = await animation.upload(
      {
        duration: 4,
        tracks: [
          {
            joints: [0, 1],
            keys: [
              {
                time: 0,
                value: {
                  kind: "pose",
                  value: [
                    { translation: [0, 0, 0] },
                    { translation: [0, 1, 0] },
                  ],
                },
              },
              {
                time: 2,
                value: {
                  kind: "pose",
                  value: [
                    { translation: [0, 0, 0] },
                    {
                      translation: [0, 1, 0],
                      rotation: [0, 0, Math.SQRT1_2, Math.SQRT1_2],
                    },
                  ],
                },
              },
            ],
          },
        ],
      },
      501n,
    );
    wave = await animation.upload(
      {
        duration: 2,
        tracks: [
          {
            joints: [1],
            keys: [
              {
                time: 0,
                value: { kind: "pose", value: [{ translation: [0, 1, 0] }] },
              },
              {
                time: 2,
                value: {
                  kind: "pose",
                  value: [
                    {
                      translation: [0, 1, 0],
                      rotation: [0, 0, Math.SQRT1_2, Math.SQRT1_2],
                    },
                  ],
                },
              },
            ],
          },
        ],
      },
      503n,
    );
    animationSources = { walk, wave };
  }
  controllers = [
    await animation.controller([
      {
        source: walk,
        track: 0,
        target: rigs[0]!,
        property: { joints: [0, 1] },
      },
    ]),
    await animation.controller([
      { source: wave, track: 0, target: rigs[1]!, property: { joints: [1] } },
    ]),
  ];
  await animation.seekPaused(controllers[0]!, 1);
  await animation.seekPaused(controllers[1]!, 2);
  return animationSnapshot();
}

export async function animationSnapshot() {
  check(animation, "animation fixture missing");
  const inspection = await animation.inspect();
  return {
    controllers: controllers.map((id) => animation!.state(inspection, id)),
    events: animation.events,
  };
}

async function waitForTransition(
  id: bigint,
  predicate: (elapsed: number) => boolean,
) {
  check(animation, "animation fixture missing");
  const deadline = performance.now() + 10_000;
  for (;;) {
    await animation.client.waitForFrame();
    const snapshot = animation.state(await animation.inspect(), id);
    if (snapshot.transition && predicate(snapshot.transition.elapsed))
      return snapshot;
    check(
      snapshot.transition !== undefined,
      "animation transition completed before the requested sample",
    );
    check(performance.now() < deadline, "animation transition timed out");
  }
}

/** Real pose clips exercise full and partial joint coverage through WebGL. */
export async function animationTransitions() {
  check(animation, "animation fixture missing");
  const run = await animation.upload(
    {
      duration: 2,
      tracks: [
        {
          joints: [0, 1],
          keys: [
            {
              time: 0,
              value: {
                kind: "pose",
                value: [
                  { translation: [0.3, 0, 0] },
                  {
                    translation: [0, 1, 0],
                    rotation: [0, 0, -Math.SQRT1_2, Math.SQRT1_2],
                  },
                ],
              },
            },
            {
              time: 2,
              value: {
                kind: "pose",
                value: [
                  { translation: [0.3, 0, 0] },
                  {
                    translation: [0, 1, 0],
                    rotation: [0, 0, -Math.SQRT1_2, Math.SQRT1_2],
                  },
                ],
              },
            },
          ],
        },
      ],
    },
    502n,
  );
  const walk = controllers[0]!;
  const partialWave = controllers[1]!;
  const pausedWalk = animation.state(await animation.inspect(), walk);
  await animation.client.updateAnimationController(walk, {
    ...pausedWalk.description,
    speed: 1,
  });
  await animation.client.transitionAnimationController(walk, {
    description: {
      drivers: [
        {
          source: run,
          track: 0,
          target: rigs[0]!,
          property: { joints: [0, 1] },
        },
      ],
      speed: 0,
    },
    duration: 2,
    easing: "smoothstep",
    startTime: { policy: "matchPhase" },
  });
  await animation.client.controlAnimationController(walk, { action: "play" });
  await waitForTransition(walk, (elapsed) => elapsed > 0.15);
  await animation.client.controlAnimationController(walk, { action: "pause" });
  const walkRun = animation.state(await animation.inspect(), walk);

  await animation.client.transitionAnimationController(partialWave, {
    description: {
      drivers: [
        {
          source: run,
          track: 0,
          target: rigs[1]!,
          property: { joints: [0, 1] },
        },
      ],
      speed: 0,
    },
    duration: 2,
    easing: "linear",
    startTime: { policy: "preserve" },
  });
  await animation.client.controlAnimationController(partialWave, {
    action: "play",
  });
  await waitForTransition(partialWave, (elapsed) => elapsed > 0.15);
  await animation.client.controlAnimationController(partialWave, {
    action: "pause",
  });
  const waveRun = animation.state(await animation.inspect(), partialWave);
  return { walkRun, waveRun };
}

export async function interruptAnimationTransition() {
  check(animation, "animation fixture missing");
  const id = controllers[0]!;
  const currentDescription = animation.state(
    await animation.inspect(),
    id,
  ).description;
  await animation.client.transitionAnimationController(id, {
    description: currentDescription,
    duration: 0.4,
    easing: "smoothstep",
    startTime: { policy: "restart" },
  });
  return animation.state(await animation.inspect(), id);
}

export async function completeAnimationTransitions() {
  check(animation, "animation fixture missing");
  for (const id of controllers)
    await animation.client.controlAnimationController(id, { action: "play" });
  const deadline = performance.now() + 10_000;
  for (;;) {
    await animation.client.waitForFrame();
    const inspection = await animation.inspect();
    if (
      controllers.every(
        (id) => animation!.state(inspection, id).transition === undefined,
      )
    )
      return animationSnapshot();
    check(
      performance.now() < deadline,
      "animation transitions did not complete",
    );
  }
}

export async function explicitTransitionPoses(
  firstRootX: number,
  firstAngle: number,
  secondRootX: number,
  secondAngle: number,
) {
  check(animation, "animation fixture missing");
  for (const id of controllers)
    await animation.client.controlAnimationController(id, { action: "stop" });
  const { Skeleton, Entity, encodeJointOverrides } = contract;
  const poses: [bigint, number, number][] = [
    [rigs[0]!, firstRootX, firstAngle],
    [rigs[1]!, secondRootX, secondAngle],
  ];
  const commands = poses.map(([id, rootX, angle]) =>
    Skeleton.setJoints(
      Entity.handle(id),
      encodeJointOverrides([
        { joint: 0, translation: [rootX, 0, 0] },
        {
          joint: 1,
          translation: [0, 1, 0],
          rotation: [0, 0, Math.sin(angle / 2), Math.cos(angle / 2)],
        },
      ]),
    ),
  );
  check(
    (await current().batch(commands)).ok,
    "explicit transition poses rejected",
  );
}

export async function animationWeight(weight: number, additive = false) {
  check(animation, "animation fixture missing");
  const id = controllers[1]!;
  const inspection = await animation.inspect();
  const description = animation.state(inspection, id).description;
  await animation.client.updateAnimationController(id, {
    ...description,
    drivers: description.drivers.map((driver) => ({
      ...driver,
      weight,
      additive,
      referenceTime: 0,
    })),
  });
}

export async function animationStop(index: number) {
  check(animation, "animation fixture missing");
  animation.client.playback(controllers[index]!, { action: "stop" });
  return animationSnapshot();
}

export async function explicitAngle(index: number, angle: number) {
  const { Skeleton, Entity, encodeJointOverrides } = contract;
  const result = await current().batch([
    Skeleton.setJoints(
      Entity.handle(rigs[index]),
      encodeJointOverrides([
        {
          joint: 1,
          translation: [0, 1, 0],
          rotation: [0, 0, Math.sin(angle / 2), Math.cos(angle / 2)],
        },
      ]),
    ),
  ]);
  check(result.ok, "explicit angle rejected");
}

export async function replaceAnimatedSkeleton() {
  check(animation, "animation fixture missing");
  const { Skeleton, Entity } = contract;
  const result = await current().batch([
    Skeleton.insert(Entity.handle(rigs[0]), {
      source: "ipp://skeleton/rig-strip",
    }),
  ]);
  check(result.ok, "skeleton replacement rejected");
  const observation = await animationSnapshot();
  check(
    observation.controllers[0]!.state === "stopped",
    "old animation reached replacement skeleton",
  );
  check(
    observation.events.some(
      (e) => e.controller.id === controllers[0] && e.kind === "invalidated",
    ),
    "invalidation event missing",
  );
  return observation;
}

/** Shared geometry definitions follow joint pairs while mesh bounds remain conservative. */
export async function geometryScene() {
  const { Entity, BoundingGeometry, PickingGeometry, encodeBoundingShape } =
    contract;
  const geometry = encodeBoundingShape({
    type: "pill",
    joints: [0, 1],
    radius: 0.12,
  });
  const receipt = clientAssetSource(current().session, 6, 601n);
  await current().registerAsset(receipt, geometry.buffer);
  const commands = [];
  for (const id of rigs) {
    const entity = Entity.handle(id);
    commands.push(
      BoundingGeometry.insert(entity),
      PickingGeometry.insert(entity, {
        source: clientAssetSource(current().session, 6, 601n).source,
        is_rendered: true,
        has_color_override: true,
        r: 0,
        g: 1,
        b: 0,
      }),
    );
  }
  check((await current().batch(commands)).ok, "geometry scene rejected");
  return rigs;
}

export async function geometryPose() {
  const { Entity, Transform, Skeleton, encodeJointOverrides } = contract;
  const entity = Entity.handle(rigs[0]);
  check(
    (
      await current().batch([
        Transform.insert(entity, { x: -0.6, z: 0.2, sx: 1.5, sy: 0.5, sz: 1 }),
        Skeleton.setJoints(
          entity,
          encodeJointOverrides([
            { joint: 0, rotation: [0, 0, Math.SQRT1_2, Math.SQRT1_2] },
          ]),
        ),
      ])
    ).ok,
    "geometry pose rejected",
  );
}

export async function geometryPick(x: number, y: number) {
  const result = await (
    current() as import("@ipp/client").PickingWorldClient
  ).query({
    type: "GeometryPickQuery",
    x: 0.5 + x / 4,
    y: 0.5 - (y - 1) / 3,
    width: 400,
    height: 300,
  });
  return result;
}

export async function geometryCull(withBounds: boolean) {
  const {
    Entity,
    Transform,
    PickingGeometry,
    BoundingGeometry,
    encodeBoundingShape,
  } = contract;
  check(
    (
      await current().batch([
        Transform.setX(Entity.handle(rigs[1]), 10),
        PickingGeometry.setIs_rendered(Entity.handle(rigs[1]), false),
        ...(withBounds
          ? []
          : [
              BoundingGeometry.insert(Entity.handle(rigs[1]), {
                // Unproven authored bounds keep the object eligible. Removing
                // authored bounds now restores the required generated fallback.
                geometry: encodeBoundingShape({
                  type: "sphere",
                  radius: 0.001,
                }),
              }),
            ]),
      ])
    ).ok,
    "culling edit rejected",
  );
}

export async function geometryReplaceSkeleton() {
  const { Entity, Skeleton } = contract;
  check(
    (
      await current().batch([
        Skeleton.insert(Entity.handle(rigs[0]), {
          source: "ipp://skeleton/rig-strip",
        }),
      ])
    ).ok,
    "skeleton replacement rejected",
  );
}
