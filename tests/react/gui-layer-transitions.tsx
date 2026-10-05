/** Moving layer planes through real React, generated transport and Surface presentation. */
import { createRef } from "react";
import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type PresentedCapture,
  type CanvasOutputReference,
  type WorldReference,
  type GuiPhysicalContext,
  type GuiWorldClient,
  type GuiEffectSubscription,
} from "@ipp/client";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import {
  Animation,
  AnimationAsset,
  Camera,
  CanvasWorld,
  Children,
  CylinderSurface,
  Entity,
  FlatSurface,
  SphereSurface,
  SurfaceCache,
  Transform,
  assetRef,
  type AnimationHandle,
} from "@ipp/react";
import {
  Box,
  Button,
  LayerTransition,
  Layout,
  Overlay,
  Skin,
  Style,
} from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, deferred, entity } from "./gui-authoring.js";
import {
  pixelFor,
  projectedSurfacePoint,
  projectedSurfaceRay,
  type Pose,
} from "./gui-projected-surfaces.js";

type Contract = Pick<
  typeof import("@ipp/gui-authoring-contract"),
  "GuiSkin" | "guiPaintPartIndex"
>;
type Point = readonly [number, number];
type Rgb = readonly [number, number, number];
type SkinRow = import("@ipp/gui-authoring-contract").GuiSkinPartsRow;
const VIEW = [400, 300] as const;
const CANVAS = [160, 120] as const;
const SYSTEMS = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.camera",
  "ipp.surface",
  "ipp.world-attachment",
  "ipp.lifecycle-publisher",
];
const CANVAS_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
];
interface Image {
  label: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
}
interface Configuration {
  pose: Pose;
  progress: number;
  nested?: number;
  overlay?: boolean;
  previous?: boolean;
  animate?: boolean;
  zero?: boolean;
  regroup?: boolean;
}

/** Interior, opaque regions avoid skin, blending and silhouette tolerances. */
function colour(frame: PresentedCapture, point: Point, rgb: Rgb) {
  const width = frame.view.binding.viewport.width;
  const pixels = new Uint8Array(frame.pixels);
  const x = Math.round(point[0]),
    y = Math.round(point[1]);
  for (let dy = -2; dy <= 2; dy++)
    for (let dx = -2; dx <= 2; dx++) {
      const offset = ((y + dy) * width + x + dx) * 4;
      if (
        !rgb.every(
          (value, channel) => Math.abs(pixels[offset + channel]! - value) < 12,
        )
      )
        return false;
    }
  return true;
}

/** A full colour footprint proves position, including parallax smaller than a marker. */
function greenFootprint(
  pose: Pose,
  coordinate: number,
  frame?: PresentedCapture,
) {
  const pixels = frame && new Uint8Array(frame.pixels);
  let count = 0,
    xSum = 0,
    ySum = 0;
  for (let y = 0; y < VIEW[1]; y++)
    for (let x = 0; x < VIEW[0]; x++) {
      let green: boolean;
      if (pixels) {
        const i = (y * VIEW[0] + x) * 4;
        green = pixels[i + 1]! > 240 && pixels[i]! < 20 && pixels[i + 2]! < 20;
      } else {
        const p = projectedSurfaceRay(pose, [x, y], coordinate);
        green =
          p !== null && p[0] >= 102 && p[0] < 134 && p[1] >= 38 && p[1] < 62;
      }
      if (green) {
        count++;
        xSum += x;
        ySum += y;
      }
    }
  return { count, centre: [xSum / count, ySum / count] };
}

export async function guiLayerTransitions(
  host: HostClientBase<Client>,
  contract: Contract,
) {
  const images: Image[] = [];
  const observations: unknown[] = [];
  const errors: Error[] = [];
  const world = (await host.createWorld({ selectedSystems: SYSTEMS }))
    .reference;
  const client = await host.openWorld(world);
  const session = new CanvasWorldSession({
    host,
    client,
    onError: (error) => errors.push(error),
  });
  const root = session.createRoot();
  let flatWorld: WorldReference | undefined;
  let flatClient: Client | undefined;
  let flatSession: CanvasWorldSession | undefined;
  let child: WorldReference | undefined;
  let childOutput: CanvasOutputReference | undefined;
  let observer: Client | undefined;
  let input: GuiPhysicalContext | undefined;
  let last: PresentedCapture | undefined;
  let effects: GuiEffectSubscription | undefined;
  const observedEffects: unknown[] = [];
  let presses = 0;
  let pressed = deferred<void>();
  const playback = createRef<AnimationHandle>();
  const transition = client.components.CanvasLayerTransition!;
  const property = {
    component: transition.id,
    offsets: [transition.fields.progress!.offset],
  };
  const solid = (rgb: Rgb) =>
    contract.GuiSkin.encodeParts({
      nextSlot: 2,
      rows: new Map<number, SkinRow>([
        [
          0,
          {
            part: contract.guiPaintPartIndex({ part: "background" }),
            color: [rgb[0] / 255, rgb[1] / 255, rgb[2] / 255, 1],
            corner_radius: [0, 0],
            corner_cut: [0, 0, 0, 0],
            border_width: 0,
            scale: [1, 1],
            opacity: 1,
            glow_intensity: 0,
          },
        ],
        [
          1,
          {
            part: contract.guiPaintPartIndex({ part: "focusRing" }),
            opacity: 0,
          },
        ],
      ]),
    });
  const red = solid([255, 0, 0]),
    green = solid([0, 255, 0]);
  const panel = (pose: Pose, delayed: boolean) => {
    const fields = { width: 1.6, height: 1.2, layer_spacing: pose.spacing };
    const yaw = pose.placement?.yaw ?? 0;
    return (
      <Entity id="transition-panel">
        <Transform qy={Math.sin(yaw / 2)} qw={Math.cos(yaw / 2)} />
        {pose.shape === "flat" ? (
          <FlatSurface {...fields} />
        ) : pose.shape === "cylinder" ? (
          <CylinderSurface {...fields} curvature={pose.curvature} />
        ) : (
          <SphereSurface {...fields} curvature={pose.curvature} />
        )}
        {delayed && (
          <SurfaceCache
            direct_distance={0}
            resolution_scale={1}
            max_refresh_hz={0.001}
          />
        )}
      </Entity>
    );
  };
  const rectangle = (
    id: string,
    layer: number,
    rgb: Rgb,
    x: number,
    y: number,
    width: number,
    height: number,
  ) => (
    <Entity id={id}>
      <Style
        x={x}
        y={y}
        layer={layer}
        red={rgb[0] / 255}
        green={rgb[1] / 255}
        blue={rgb[2] / 255}
      />
      <Box width={width} height={height} />
    </Entity>
  );
  const zeroLayers = () => (
    <>
      <Entity id="zero-first">
        <Style x={40} y={40} layer={100} red={1} green={0} blue={0} />
        <LayerTransition previous_layer={300} progress={0} />
        <Box width={60} height={40} />
      </Entity>
      {rectangle("zero-middle", 200, [0, 255, 0], 40, 40, 60, 40)}
      {rectangle("zero-last", 300, [0, 0, 255], 40, 40, 60, 40)}
    </>
  );
  const scene = (config: Configuration) => (
    <>
      <Entity id="transition-camera">
        <Transform z={2.5} />
        <Camera projection={0} fov_y={Math.PI / 4} near={0.05} far={20} />
      </Entity>
      {panel(config.pose, !!config.regroup)}
      <CanvasWorld
        presentation={{ anchor: "transition-panel" }}
        create={{ selectedSystems: CANVAS_SYSTEMS }}
        extent={CANVAS}
        unitsPerMetre={100}
        onReady={(handle) => {
          child = handle.world;
          childOutput = handle.output;
        }}
      >
        {config.animate && (
          <AnimationAsset
            id="transition-clip"
            clip={{
              duration: 2,
              tracks: [
                {
                  property,
                  keys: [
                    { time: 0, value: { kind: "f32", value: 0 } },
                    { time: 2, value: { kind: "f32", value: 1 } },
                  ],
                },
              ],
            }}
          />
        )}
        <Entity id="transition-root">
          <Layout kind={3} width={160} height={120} align_x={-1} align_y={-1} />
          <Children>
            {config.zero ? (
              zeroLayers()
            ) : config.regroup ? (
              <>
                {rectangle("group-base", 0, [255, 0, 0], 15, 15, 34, 24)}
                {rectangle("group-upper", 100, [0, 0, 255], 15, 65, 34, 24)}
                <Entity id="group-moving">
                  <Style
                    x={108}
                    y={42}
                    layer={config.previous === false ? 0 : 100}
                    red={0}
                    green={1}
                    blue={0}
                  />
                  {config.previous !== false && (
                    <LayerTransition
                      previous_layer={0}
                      progress={config.progress}
                    />
                  )}
                  <Box width={32} height={24} />
                </Entity>
              </>
            ) : (
              <>
                <Entity id="transition-fixed">
                  <Style x={14} y={20} layer={200} />
                  <Layout width={24} height={24} />
                  <Skin parts={red} />
                  <Button label="" />
                </Entity>
                <Entity id="transition-moving">
                  <Style x={102} y={38} layer={100} />
                  {config.previous !== false && (
                    <LayerTransition
                      previous_layer={300}
                      progress={config.progress}
                    />
                  )}
                  <Layout
                    kind={3}
                    width={32}
                    height={24}
                    align_x={-1}
                    align_y={-1}
                  />
                  <Skin parts={green} />
                  <Button
                    label=""
                    onPress={() => {
                      presses++;
                      pressed.resolve();
                    }}
                  />
                  {config.animate && (
                    <Animation
                      source={assetRef("transition-clip")}
                      bindings={[{ track: 0, property }]}
                      ref={playback}
                      speed={0}
                    />
                  )}
                  <Children>
                    {rectangle(
                      "transition-decoration",
                      0,
                      [0, 0, 255],
                      24,
                      -10,
                      10,
                      8,
                    )}
                    {config.nested !== undefined && (
                      <Entity id="transition-nested">
                        <Style
                          x={-60}
                          y={38}
                          layer={100}
                          red={1}
                          green={0}
                          blue={1}
                        />
                        <LayerTransition
                          previous_layer={300}
                          progress={config.nested}
                        />
                        <Box width={20} height={18} />
                      </Entity>
                    )}
                  </Children>
                </Entity>
                {config.overlay && (
                  <Entity id="transition-overlay">
                    <Overlay side={1} align={0} />
                    <Style x={15} y={96} red={1} green={1} blue={0} />
                    <Box width={22} height={16} />
                  </Entity>
                )}
              </>
            )}
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );
  const remember = (label: string, frame: PresentedCapture) => {
    last = frame;
    images.push({
      label,
      width: frame.view.binding.viewport.width,
      height: frame.view.binding.viewport.height,
      pixels: [...new Uint8Array(frame.pixels)],
      sequence: frame.sequence,
    });
  };
  const cameraOutput = async () =>
    host.bindOutput(
      world,
      (await entity(client, "transition-camera")).id,
      "camera",
    );
  const selectCamera = async () =>
    session.selectOutput(await cameraOutput(), {
      width: VIEW[0],
      height: VIEW[1],
      devicePixelRatio: 1,
    });
  async function capture(
    label: string,
    probes: { point: Point; rgb: Rgb }[],
    accept: (frame: PresentedCapture) => boolean = () => true,
    source = { session, output: childOutput },
  ) {
    check(source.output, "Transition output missing");
    let frame: PresentedCapture | undefined;
    const deadline = performance.now() + 12000;
    do {
      frame = await source.session.capture({
        afterOutputs: [source.output],
        ...(frame ? { afterSequence: frame.sequence } : {}),
      });
      last = frame;
      if (
        probes.every((probe) => colour(frame!, probe.point, probe.rgb)) &&
        accept(frame)
      ) {
        remember(label, frame);
        observations.push({ label, probes, sequence: frame.sequence });
        return frame;
      }
    } while (performance.now() < deadline);
    remember(`failed-${label}`, frame);
    throw new Error(
      `Rendered transition did not match independent probes: ${label}; ${JSON.stringify(probes)}`,
    );
  }
  const point = (pose: Pose, content: Point, offset: number) =>
    pixelFor(projectedSurfacePoint(pose, content, offset));
  async function checkMoving(
    label: string,
    config: Configuration,
    offset: number,
  ) {
    const probes: { point: Point; rgb: Rgb }[] = [
      { point: point(config.pose, [118, 50], offset), rgb: [0, 255, 0] },
      { point: point(config.pose, [131, 32], offset), rgb: [0, 0, 255] },
    ];
    if (config.nested !== undefined)
      probes.push({
        point: point(config.pose, [52, 85], offset + 3 - 2 * config.nested),
        rgb: [255, 0, 255],
      });
    if (config.overlay)
      probes.push({
        point: point(config.pose, [26, 88], 7),
        rgb: [255, 255, 0],
      });
    const expected = greenFootprint(config.pose, offset);
    let actual: ReturnType<typeof greenFootprint> | undefined;
    try {
      return await capture(label, probes, (frame) => {
        actual = greenFootprint(config.pose, offset, frame);
        return (
          Math.abs(actual.count - expected.count) < expected.count * 0.12 &&
          actual.centre.every(
            (value, axis) => Math.abs(value - expected.centre[axis]!) < 0.8,
          )
        );
      });
    } finally {
      observations.push({ label: `${label}-footprint`, expected, actual });
    }
  }
  async function clickMoving(config: Configuration, offset: number) {
    check(input, "Transition input missing");
    const p = point(config.pose, [118, 50], offset);
    const before = presses;
    pressed = deferred<void>();
    const rear = Math.abs(config.pose.placement?.yaw ?? 0) > 2;
    for (const kind of ["pointerDown", "pointerUp"] as const) {
      const receipt = await input.send({
        kind,
        pointer: 1n,
        point: [p[0] / VIEW[0], p[1] / VIEW[1]],
      });
      check(
        rear
          ? receipt.disposition !== "routed"
          : receipt.disposition === "routed" && receipt.rejected === 0,
        `Front-normal input policy: ${JSON.stringify(receipt)}`,
      );
      observations.push({
        label: "moving-input",
        kind,
        pose: config.pose,
        coordinate: offset,
        projectedPoint: p,
        receipt,
        state: await observer!.inspectPage({ collection: "guiPointers" }),
        target: await entity(observer!, "transition-moving"),
      });
    }
    if (rear) {
      check(presses === before, "Rear-facing Surface activated a control");
      return;
    }
    await waitPressed(before, offset);
    observations.push({
      label: "moving-effects",
      coordinate: offset,
      observedEffects: [...observedEffects],
    });
  }

  async function waitPressed(before: number, offset: number) {
    // Match the callback suite's bounded signal wait: routing settlement and
    // effects arrive on independent asynchronous observer connections.
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        pressed.promise,
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () =>
              reject(
                new Error(
                  `Moving button callback did not arrive at offset ${offset}`,
                ),
              ),
            8000,
          );
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
    check(
      presses === before + 1,
      `Moving button callback count at offset ${offset}`,
    );
  }
  const statistics = async () => {
    const diagnostics = renderDiagnostics(host);
    check(diagnostics, "Transition renderer diagnostics missing");
    const records = (await diagnostics.statistics()).surfaces.surfaceCaches;
    check(records.length === 1, "Expected exactly one projected Surface cache");
    return records[0]!;
  };
  let failure: string | null = null;
  try {
    const initial: Configuration = {
      pose: { shape: "cylinder", curvature: 0.8, spacing: 0.12 },
      progress: 0.125,
    };
    await root.render(scene(initial));
    await selectCamera();
    check(child, "Transition Canvas World was not acknowledged");
    observer = await host.openWorld(child);
    effects = await (observer as GuiWorldClient).subscribeGuiEffects(
      (effect) => observedEffects.push(effect),
      { classes: "all" },
    );
    input = await host.input.open(session.view!);
    await checkMoving("cache-motion-before", initial, 2.75);
    const before = await statistics();
    const moved = { ...initial, progress: 0.25 };
    await root.render(scene(moved));
    await checkMoving("cache-motion-after", moved, 2.5);
    const after = await statistics();
    check(
      after.repaints === before.repaints,
      "Pure normal motion rerasterized unchanged image membership",
    );
    observations.push({ label: "cache-motion-reuse", before, after });
    for (const shape of ["flat", "cylinder", "sphere"] as const)
      for (const rear of [false, true])
        for (const progress of [0, 0.5, 1]) {
          const config: Configuration = {
            pose: {
              shape,
              curvature: 0.8,
              spacing: 0.12,
              placement: { yaw: rear ? Math.PI : 0, scale: [1, 1, 1] },
            },
            progress,
          };
          await root.render(scene(config));
          await checkMoving(
            `${shape}-${rear ? "rear" : "front"}-${progress}`,
            config,
            3 - 2 * progress,
          );
          await clickMoving(config, 3 - 2 * progress);
        }
    // Hold the same target while its physical plane ID changes from2 to1.
    const held: Configuration = { pose: initial.pose, progress: 0 };
    await root.render(scene(held));
    await checkMoving("captured-source-plane", held, 3);
    const sourcePoint = point(held.pose, [118, 50], 3);
    const beforePress = presses;
    pressed = deferred<void>();
    const down = await input.send({
      kind: "pointerDown",
      pointer: 2n,
      point: [sourcePoint[0] / VIEW[0], sourcePoint[1] / VIEW[1]],
    });
    check(down.disposition === "routed", "Moving target did not start capture");
    const pointer = async () =>
      (
        await observer!.inspectPage({ collection: "guiPointers" })
      ).guiPointers?.find((p) => p.pointer === 2n);
    const sourceTarget = await pointer();
    check(sourceTarget?.state.captured, "Source plane pointer did not capture");
    const destination = { ...held, progress: 1 };
    await root.render(scene(destination));
    await checkMoving("captured-destination-plane", destination, 1);
    const destinationPoint = point(held.pose, [118, 50], 1);
    const move = await input.send({
      kind: "pointerMove",
      pointer: 2n,
      point: [destinationPoint[0] / VIEW[0], destinationPoint[1] / VIEW[1]],
    });
    const destinationTarget = await pointer();
    check(
      move.disposition === "routed" &&
        destinationTarget?.state.captured &&
        destinationTarget.state.hovered &&
        destinationTarget.target.entity === sourceTarget.target.entity &&
        destinationTarget.target.incarnation ===
          sourceTarget.target.incarnation,
      "Moving capture lost its exact target or retained the old projection",
    );
    const up = await input.send({
      kind: "pointerUp",
      pointer: 2n,
      point: [destinationPoint[0] / VIEW[0], destinationPoint[1] / VIEW[1]],
    });
    check(up.disposition === "routed", "Moving capture release did not route");
    await waitPressed(beforePress, 1);
    check(!(await pointer())?.state.captured, "Moving capture did not release");
    observations.push({
      label: "held-pointer-current-plane",
      down,
      move,
      up,
      sourceTarget,
      destinationTarget,
    });
    const nested: Configuration = {
      pose: initial.pose,
      progress: 0.25,
      nested: 0.25,
      overlay: true,
    };
    await root.render(scene(nested));
    await checkMoving("nested-inherited-scope", nested, 2.5);
    const removed: Configuration = {
      pose: nested.pose,
      progress: nested.progress,
      previous: false,
    };
    await root.render(scene(removed));
    await checkMoving("previous-removal-snap", removed, 1);
    const zero: Configuration = {
      pose: { shape: "cylinder", curvature: 0.8, spacing: 0 },
      progress: 0,
      zero: true,
    };
    await root.render(scene(zero));
    await capture("zero-spacing-cached-order", [
      { point: point(zero.pose, [70, 60], 0), rgb: [0, 0, 255] },
    ]);
    await input.close();
    input = undefined;
    // An attached child cannot simultaneously become a root presentation.
    // Use an independently owned Canvas World for the root-order oracle.
    flatWorld = (await host.createWorld({ selectedSystems: CANVAS_SYSTEMS }))
      .reference;
    flatClient = await host.openWorld(flatWorld);
    flatSession = new CanvasWorldSession({ host, client: flatClient });
    await flatSession.createRoot().render(
      <Entity id="root-order">
        <Layout kind={3} width={160} height={120} align_x={-1} align_y={-1} />
        <Children>{zeroLayers()}</Children>
      </Entity>,
    );
    await flatSession.selectOutput(canvasOutput(flatWorld), {
      width: 160,
      height: 120,
      devicePixelRatio: 1,
    });
    await capture(
      "root-logical-interleaving",
      [{ point: [70, 60], rgb: [0, 0, 255] }],
      undefined,
      { session: flatSession, output: canvasOutput(flatWorld) },
    );
    // Explicitly retire the previous presentation selection before rebinding
    // after another owner has presented its independently owned root.
    await session.selectOutput(null, {
      width: VIEW[0],
      height: VIEW[1],
      devicePixelRatio: 1,
    });
    await selectCamera();
    const regroup: Configuration = {
      pose: initial.pose,
      progress: 0,
      regroup: true,
    };
    await root.render(scene(regroup));
    await capture("group-before", [
      { point: point(regroup.pose, [124, 54], 0), rgb: [0, 255, 0] },
    ]);
    const oldGrouping = await statistics();
    await root.render(scene({ ...regroup, progress: 1 }));
    await capture("group-after", [
      { point: point(regroup.pose, [124, 54], 1), rgb: [0, 255, 0] },
    ]);
    const newGrouping = await statistics();
    check(
      newGrouping.repaints > oldGrouping.repaints,
      "Same plane-ID set did not invalidate changed image membership under delayed refresh",
    );
    await root.render(scene({ ...regroup, previous: false }));
    await capture("group-removal-snap", [
      { point: point(regroup.pose, [124, 54], 0), rgb: [0, 255, 0] },
    ]);
    observations.push({
      label: "delayed-group-membership",
      oldGrouping,
      newGrouping,
    });
    const animated: Configuration = {
      pose: initial.pose,
      progress: 0,
      animate: true,
    };
    await root.render(scene(animated));
    check(playback.current, "Transition playback ref missing");
    await playback.current.play();
    await playback.current.pause();
    await playback.current.seek(1);
    for (let i = 0; i < 100; i++) {
      await observer.waitForFrame();
      await root.flush();
      const target = await entity(observer, "transition-moving");
      const progress = Number(
        target.components.find((c) => c.component === transition.id)?.fields
          .progress,
      );
      if (Math.abs(progress - 0.5) < 1e-6) break;
      check(
        i < 99,
        "Ordinary AnimationController did not sample transition progress",
      );
    }
    await checkMoving(
      "host-driver-midpoint",
      { ...animated, progress: 0.5 },
      2,
    );
    await playback.current.seek(0);
    await playback.current.playAtSpeed(1);
    let sampled = 0;
    for (let i = 0; i < 100; i++) {
      await observer.waitForFrame();
      sampled = Number(
        (await entity(observer, "transition-moving")).components.find(
          (c) => c.component === transition.id,
        )?.fields.progress,
      );
      if (sampled > 0.15 && sampled < 0.8) break;
      check(i < 99, "Host clock did not advance layer animation");
    }
    await playback.current.pause();
    await observer.waitForFrame();
    sampled = Number(
      (await entity(observer, "transition-moving")).components.find(
        (c) => c.component === transition.id,
      )?.fields.progress,
    );
    check(
      sampled > 0.15 && sampled < 0.9,
      `Host-driver sample out of range: ${sampled}`,
    );
    await checkMoving("host-driver-moving-sample", animated, 3 - 2 * sampled);
    observations.push({ label: "Host-clock-progress", sampled, presses });
    check(errors.length === 0, errors.map((error) => error.message).join("; "));
  } catch (error) {
    failure = error instanceof Error ? error.message : String(error);
    if (last) remember("transition-failure", last);
    observations.push({
      failure,
      observedEffects,
      inspection: await observer?.inspect().catch(() => null),
    });
  } finally {
    for (const [label, cleanup] of [
      ["flat-session", () => flatSession?.close()],
      ["flat-client", () => flatClient?.close()],
      ["flat-world", () => flatWorld && host.destroyWorld(flatWorld)],
      ["input", () => input?.close()],
      ["effects", () => effects?.unsubscribe()],
      ["observer", () => observer?.close()],
      ["session", () => session.close()],
      ["client", () => client.close()],
      ["world", () => host.destroyWorld(world)],
    ] as const) {
      try {
        await cleanup();
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        failure ??= `Cleanup failed for ${label}: ${message}`;
        observations.push({ label: `cleanup-${label}`, error: message });
      }
    }
  }
  // Inspection may contain generated table proxies; export durable JSON data
  // rather than asking the browser driver to traverse those live wrappers.
  return {
    failure,
    images,
    observations: JSON.parse(
      JSON.stringify(observations, (_, value) =>
        typeof value === "bigint" ? value.toString() : value,
      ),
    ) as unknown[],
  };
}
