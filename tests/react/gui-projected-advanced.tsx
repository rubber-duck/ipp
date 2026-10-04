/** Additional geometric, image and lifetime assertions using the maintained independent oracle. */
import { clientAssetSource } from "@ipp/client";
import type {
  AssetWorldClient,
  ClientAssetSource,
  CanvasOutputReference,
  Client,
  GuiPhysicalContext,
  HostClientBase,
  PresentedCapture,
  WorldReference,
} from "@ipp/client";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { presentationTesting } from "@ipp/client/testing";
import {
  AttachedWorld,
  BoundingGeometry,
  Camera,
  CanvasWorld,
  Children,
  CylinderSurface,
  Entity,
  SphereSurface,
  Transform,
} from "@ipp/react";
import { Box, Image, Layout, Slider, Style, Text } from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import type { GuiPaintAssets } from "./gui-paint.js";
import { check, entity } from "./gui-authoring.js";
import {
  pixelFor,
  projectedSurfacePoint,
  projectedSurfaceRay,
  type Pose,
} from "./gui-projected-surfaces.js";

type Contract = Pick<
  typeof import("@ipp/gui-authoring-contract"),
  "encodeBoundingShape"
>;
type Point = readonly [number, number];
const VIEW = [400, 300] as const;
const SYSTEMS = [
  "ipp.animation",
  "ipp.asset-dependencies",
  "ipp.hierarchy",
  "ipp.look-at",
  "ipp.final-propagation",
  "ipp.geometry",
  "ipp.render",
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
/** Fixed scene backdrop in sRGB; decode independently before source-over. */
const BACKDROP = [0.04, 0.055, 0.08] as const;
const LOWER = [0.02, 0.2, 0.04, 0.7] as const;
const UPPER = [0.8, 0.1, 0.4, 0.5] as const;
const lowerRect = [8, 8, 144, 104] as const;
const upperRect = [96, 26, 48, 66] as const;
const sliderRect = [35, 96, 90, 12] as const;

export const RECOVERY_POSE: Pose = {
  shape: "cylinder",
  curvature: 0.8,
  spacing: 0.28,
};
export const ADVANCED_POSES: Pose[] = [
  {
    shape: "cylinder",
    curvature: 0.8,
    spacing: 0.28,
    placement: { yaw: 0.5, scale: [1.1, 0.85, 1.15] },
  },
  {
    shape: "sphere",
    curvature: -0.8,
    spacing: 0.28,
    placement: { yaw: -0.45, scale: [0.95, 1.1, 0.85] },
  },
  {
    shape: "cylinder",
    curvature: 0.8,
    spacing: 0.28,
    placement: { yaw: Math.PI, scale: [1, 1, 1] },
  },
  {
    shape: "sphere",
    curvature: -0.8,
    spacing: 0.28,
    placement: { yaw: Math.PI, scale: [1, 1, 1] },
  },
];

function inside(point: Point | null, rect: readonly number[], inset = 0) {
  return (
    point !== null &&
    point[0] >= rect[0]! + inset &&
    point[0] <= rect[0]! + rect[2]! - inset &&
    point[1] >= rect[1]! + inset &&
    point[1] <= rect[1]! + rect[3]! - inset
  );
}

/** Straight linear-light source-over, then the capture's sRGB encoding. */
function composite(front: readonly number[], back: readonly number[]) {
  return [0, 1, 2].map((channel) => {
    const encodedBackground = BACKDROP[channel]!;
    const background =
      encodedBackground <= 0.04045
        ? encodedBackground / 12.92
        : ((encodedBackground + 0.055) / 1.055) ** 2.4;
    const linear =
      front[channel]! * front[3]! +
      back[channel]! * back[3]! * (1 - front[3]!) +
      background * (1 - back[3]!) * (1 - front[3]!);
    return Math.round(
      255 *
        (linear <= 0.0031308
          ? 12.92 * linear
          : 1.055 * linear ** (1 / 2.4) - 0.055),
    );
  });
}

function pixel(frame: PresentedCapture, point: Point) {
  const bytes = new Uint8Array(frame.pixels);
  const offset = (Math.floor(point[1]) * VIEW[0] + Math.floor(point[0])) * 4;
  return [bytes[offset]!, bytes[offset + 1]!, bytes[offset + 2]!];
}

function colour(actual: readonly number[], expected: readonly number[]) {
  return actual.every(
    (value, channel) => Math.abs(value - expected[channel]!) <= 9,
  );
}

function surface(pose: Pose) {
  const fields = {
    width: 1.6,
    height: 1.2,
    curvature: pose.curvature,
    layer_spacing: pose.spacing,
  };
  return pose.shape === "cylinder" ? (
    <CylinderSurface {...fields} />
  ) : (
    <SphereSurface {...fields} />
  );
}

function placement(pose: Pose) {
  const p = pose.placement ?? { yaw: 0, scale: [1, 1, 1] };
  return (
    <Transform
      qy={Math.sin(p.yaw / 2)}
      qw={Math.cos(p.yaw / 2)}
      sx={p.scale[0]}
      sy={p.scale[1]}
      sz={p.scale[2]}
    />
  );
}

/** Independent scan selects interior overlapping pixels and pixels exposed by physical parallax. */
export function imageExpectations(pose: Pose, rear: boolean) {
  const overlap: Point[] = [],
    revealed: Point[] = [];
  for (let y = 20; y < VIEW[1] - 20; y += 2)
    for (let x = 20; x < VIEW[0] - 20; x += 2) {
      const lower = projectedSurfaceRay(pose, [x, y], 0);
      const upper = projectedSurfaceRay(pose, [x, y], 1);
      if (!inside(lower, lowerRect, 4)) continue;
      if (inside(upper, upperRect, 4)) overlap.push([x, y]);
      // Base chart covered by the upper rectangle, while its separated shell misses it.
      // A single flattened image/topmost mask would incorrectly suppress these base pixels.
      if (inside(lower, upperRect, 1) && !inside(upper, upperRect, -2))
        revealed.push([x, y]);
    }
  check(
    overlap.length > 30,
    `No independently selected overlap pixels: ${JSON.stringify(pose)}`,
  );
  check(
    revealed.length > 3,
    `No independently selected parallax stripe: ${JSON.stringify(pose)}`,
  );
  return {
    overlap,
    revealed,
    mixed: rear ? composite(LOWER, UPPER) : composite(UPPER, LOWER),
    base: composite(LOWER, [0, 0, 0, 0]),
  };
}

export async function guiProjectedAdvanced(
  host: HostClientBase<Client>,
  contract: Contract,
  assets: GuiPaintAssets,
  recovery = false,
) {
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const comparisons: typeof images = [];
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
  let child: WorldReference | undefined;
  let childOutput: CanvasOutputReference | undefined;
  let observer: Client | undefined;
  let input: GuiPhysicalContext | undefined;
  let last: PresentedCapture | undefined;
  const remember = (label: string, frame: PresentedCapture) => {
    last = frame;
    images.push({
      label,
      width: VIEW[0],
      height: VIEW[1],
      pixels: [...new Uint8Array(frame.pixels)],
      sequence: frame.sequence,
    });
  };
  const camera = (
    <Entity id="advanced-camera">
      <Transform z={2.5} />
      <Camera projection={0} fov_y={Math.PI / 4} near={0.05} far={20} />
    </Entity>
  );
  const scene = (
    pose: Pose,
    delayed?: { bitmap: ClientAssetSource; font: ClientAssetSource },
    empty = false,
  ) => (
    <>
      {camera}
      <Entity id="advanced-panel">
        {placement(pose)}
        {surface(pose)}
      </Entity>
      <CanvasWorld
        presentation={{ anchor: "advanced-panel" }}
        create={{ selectedSystems: CANVAS_SYSTEMS }}
        extent={[160, 120]}
        unitsPerMetre={100}
        onReady={(handle) => {
          child = handle.world;
          childOutput = handle.output;
        }}
      >
        {empty ? (
          <Entity id="empty-layer">
            <Style layer={900000} />
          </Entity>
        ) : (
          <Entity id="advanced-content">
            <Layout
              kind={3}
              width={160}
              height={120}
              align_x={-1}
              align_y={-1}
            />
            <Children>
              <Entity id="advanced-lower">
                <Style
                  x={lowerRect[0]}
                  y={lowerRect[1]}
                  red={LOWER[0]}
                  green={LOWER[1]}
                  blue={LOWER[2]}
                  alpha={LOWER[3]}
                />
                <Box width={lowerRect[2]} height={lowerRect[3]} />
              </Entity>
              <Entity id="advanced-upper">
                <Style
                  x={upperRect[0]}
                  y={upperRect[1]}
                  layer={900000}
                  red={UPPER[0]}
                  green={UPPER[1]}
                  blue={UPPER[2]}
                  alpha={UPPER[3]}
                />
                <Box width={upperRect[2]} height={upperRect[3]} />
              </Entity>
              {delayed && (
                <>
                  <Entity id="delayed-bitmap">
                    <Style x={20} y={42} />
                    <Image
                      source={delayed.bitmap.source}
                      width={20}
                      height={20}
                    />
                  </Entity>
                  <Entity id="delayed-font">
                    <Style x={10} y={14} />
                    <Text
                      source={delayed.font.source}
                      text="WAIT7"
                      font_size={12}
                    />
                  </Entity>
                </>
              )}
              <Entity id="advanced-slider">
                <Style x={sliderRect[0]} y={sliderRect[1]} layer={900000} />
                <Layout width={sliderRect[2]} height={sliderRect[3]} />
                <Slider min={0} max={1} value={0.35} />
              </Entity>
            </Children>
          </Entity>
        )}
      </CanvasWorld>
    </>
  );
  function comparison(
    label: string,
    frame: PresentedCapture,
    probes: readonly { point: Point; expected: readonly number[] }[],
  ) {
    // Transparent pixels are unconstrained: this reference covers only the independently selected probes.
    const expected = new Uint8Array(VIEW[0] * VIEW[1] * 4),
      diff = new Uint8Array(expected.length);
    for (const probe of probes) {
      const offset =
        (Math.floor(probe.point[1]) * VIEW[0] + Math.floor(probe.point[0])) * 4;
      const actual = pixel(frame, probe.point);
      for (let channel = 0; channel < 3; channel++) {
        expected[offset + channel] = probe.expected[channel]!;
        diff[offset + channel] = Math.min(
          255,
          4 * Math.abs(actual[channel]! - probe.expected[channel]!),
        );
      }
      expected[offset + 3] = 255;
      diff[offset + 3] = 255;
    }
    for (const [prefix, bytes] of [
      ["expected", expected],
      ["diff", diff],
    ] as const)
      comparisons.push({
        label: `${prefix}-${label}`,
        width: VIEW[0],
        height: VIEW[1],
        pixels: [...bytes],
        sequence: frame.sequence,
      });
  }
  async function captureMatched(
    label: string,
    accept: (frame: PresentedCapture) => boolean,
    probes: readonly { point: Point; expected: readonly number[] }[] = [],
  ) {
    let frame: PresentedCapture | undefined;
    const deadline = performance.now() + 15000;
    do {
      frame = await session.capture(
        frame ? { afterSequence: frame.sequence } : {},
      );
      last = frame;
      if (accept(frame)) {
        remember(label, frame);
        if (probes.length) comparison(label, frame, probes);
        return frame;
      }
    } while (performance.now() < deadline);
    remember(`failed-${label}`, frame);
    if (probes.length) comparison(label, frame, probes);
    throw new Error(
      `Captured pixels did not match ${label}; observations=${JSON.stringify(observations)}`,
    );
  }
  async function pointInput(
    kind: "pointerDown" | "pointerMove" | "pointerUp",
    pose: Pose,
    content: Point,
    pointer = 1n,
  ) {
    check(input, "No physical input context");
    const p = pixelFor(projectedSurfacePoint(pose, content, 1));
    return input.send({
      kind,
      pointer,
      point: [p[0] / VIEW[0], p[1] / VIEW[1]],
    });
  }
  async function state() {
    check(observer, "No child observer");
    const slider = await entity(observer, "advanced-slider");
    const fields = slider.components.find(
      (item) => item.component === observer!.components.GuiSlider!.id,
    )?.fields;
    check(fields, "Slider fields disappeared");
    const pointers =
      (await observer.inspectPage({ collection: "guiPointers" })).guiPointers ??
      [];
    return {
      value: Number(fields.value),
      captured: pointers.some((p) => p.pointer === 1n && p.state.captured),
    };
  }
  let failure: string | null = null;
  try {
    const poses = recovery ? [RECOVERY_POSE] : ADVANCED_POSES;
    for (const [index, pose] of poses.entries()) {
      await input?.close();
      input = undefined;
      await observer?.close();
      observer = undefined;
      await root.render(scene(pose, undefined, index === 0));
      if (index === 0)
        await session.selectOutput(
          await host.bindOutput(
            world,
            (await entity(client, "advanced-camera")).id,
            "camera",
          ),
          { width: VIEW[0], height: VIEW[1], devicePixelRatio: 1 },
        );
      if (index === 0) {
        check(childOutput, "Empty curved Canvas output missing");
        const blank = await session.capture({ afterOutputs: [childOutput] });
        remember("advanced-empty-curved", blank);
        check(
          pixel(blank, [200, 150]).every(
            (channel, index) =>
              Math.abs(channel - pixel(blank, [2, 2])[index]!) <= 1,
          ),
          "Empty curved output painted nontransparent geometry",
        );
        check(
          blank.sources.some((source) => source.output.world.id === child!.id),
          "Empty curved output omitted current inclusion witness",
        );
        await root.render(scene(pose));
      }
      check(child, "Advanced Canvas World was not acknowledged");
      observer = await host.openWorld(child);
      const rear = Math.abs(pose.placement?.yaw ?? 0) > 2;
      const expected = imageExpectations(pose, rear);
      await captureMatched(
        `advanced-shell-${index}`,
        (frame) => {
          const mixed = expected.overlap.filter((p) =>
            colour(pixel(frame, p), expected.mixed),
          ).length;
          const base = expected.revealed.filter((p) =>
            colour(pixel(frame, p), expected.base),
          ).length;
          observations[index] = {
            pose,
            mixed,
            overlap: expected.overlap.length,
            base,
            revealed: expected.revealed.length,
            expectedMixed: expected.mixed,
            expectedBase: expected.base,
            actualMixed: pixel(frame, expected.overlap[0]!),
            actualBase: pixel(frame, expected.revealed[0]!),
          };
          return (
            mixed >= expected.overlap.length * 0.9 &&
            base >= expected.revealed.length * 0.8
          );
        },
        [
          ...expected.overlap.map((point) => ({
            point,
            expected: expected.mixed,
          })),
          ...expected.revealed.map((point) => ({
            point,
            expected: expected.base,
          })),
        ],
      );
      check(session.view, "Advanced view missing");
      input = await host.input.open(session.view);
      const routed = await pointInput("pointerDown", pose, [65, 102]);
      check(
        rear
          ? routed.disposition !== "routed"
          : routed.disposition === "routed",
        `Front-normal input policy failed: ${JSON.stringify(pose)}`,
      );
      if (!rear) {
        check((await state()).captured, "Projected slider did not capture");
        await pointInput("pointerMove", pose, [85, 102]);
        check(
          (await state()).value > 0.4,
          "Transformed projected drag did not change value",
        );
        await pointInput("pointerUp", pose, [85, 102]);
      }
    }
    // Stable identity through geometry writes, continuation outside the rectangle,
    // full shell misses, then provider lifetime replacement under an active capture.
    const pose: Pose = { shape: "cylinder", curvature: 0.8, spacing: 0.28 };
    await input?.close();
    input = undefined;
    await observer?.close();
    observer = undefined;
    await root.render(scene(pose));
    check(child && session.view, "Lifecycle scene missing");
    observer = await host.openWorld(child);
    await session.capture();
    input = await host.input.open(session.view);
    const cancellations: bigint[] = [];
    input.onCancel((event) => cancellations.push(...event.pointers));
    await pointInput("pointerDown", pose, [65, 102]);
    await pointInput("pointerMove", pose, [85, 102]);
    const beforeMiss = await state();
    check(beforeMiss.captured, "Lifecycle capture missing");
    await input.send({ kind: "pointerMove", pointer: 1n, point: [0, 0] });
    const afterMiss = await state();
    check(
      afterMiss.captured && Math.abs(afterMiss.value - beforeMiss.value) < 1e-6,
      "Full shell miss invented movement or dropped capture",
    );
    const edited = { ...pose, curvature: 0.9 };
    await root.render(scene(edited));
    await session.capture();
    await pointInput("pointerMove", edited, [100, 102]);
    const afterEdit = await state();
    check(
      afterEdit.captured && afterEdit.value > beforeMiss.value,
      "Geometry edit fenced a live provider identity",
    );
    await pointInput("pointerMove", edited, [172, 102]);
    check(
      (await state()).captured && (await state()).value > 0.99,
      "Captured principal-chart continuation stopped at the content rectangle",
    );
    await root.render(scene({ ...edited, shape: "sphere" }));
    await session.capture();
    await input.send({ kind: "pointerMove", pointer: 1n, point: [0.5, 0.5] });
    check(
      !(await state()).captured,
      "Provider-kind replacement retained old capture",
    );
    const cancelDeadline = performance.now() + 5000;
    while (!cancellations.includes(1n) && performance.now() < cancelDeadline)
      await new Promise((resolve) => setTimeout(resolve, 16));
    check(
      cancellations.includes(1n),
      "Provider-kind replacement omitted physical cancellation",
    );
    await input.close();
    input = undefined;
    await root.render(scene(pose));
    await session.capture();
    input = await host.input.open(session.view!);
    await pointInput("pointerDown", pose, [65, 102]);
    const provider = client.components.CylinderSurface!;
    check(
      (
        await client.batch([
          {
            kind: "removeComponent",
            entity: { kind: "symbol", symbol: "advanced-panel" },
            component: provider.id,
          },
        ])
      ).ok,
      "Provider removal failed",
    );
    await input.send({ kind: "pointerMove", pointer: 1n, point: [0.5, 0.5] });
    check(!(await state()).captured, "Removed provider retained capture");
    check(
      (
        await client.batch([
          {
            kind: "insertComponent",
            entity: { kind: "symbol", symbol: "advanced-panel" },
            component: provider.id,
            fields: Object.entries({
              width: 1.6,
              height: 1.2,
              curvature: 0.8,
              layer_spacing: 0.28,
            }).map(([name, value]) => ({
              offset: provider.fields[name]!.offset,
              value: { kind: "f32" as const, value },
            })),
          },
        ])
      ).ok,
      "Provider reinsertion failed",
    );
    await session.capture();
    await input.send({ kind: "pointerMove", pointer: 1n, point: [0.5, 0.5] });
    check(
      !(await state()).captured,
      "Reinserted provider resurrected old capture",
    );
    observations.push({
      beforeMiss,
      afterMiss,
      afterEdit,
      providerReplaced: true,
    });

    if (recovery) {
      const testing = presentationTesting(host);
      const diagnostics = renderDiagnostics(host);
      check(diagnostics, "Recovery renderer diagnostics missing");
      async function currentChild() {
        check(childOutput, "Restored child output missing");
        const completed = await session.capture({
          afterOutputs: [childOutput],
        });
        check(
          completed.failedDrawCalls === 0 &&
            completed.sources.some(
              (source) =>
                source.output.world.id === child!.id &&
                source.tick >= source.minimumTick,
            ),
          "Restored curved output did not acknowledge current complete content",
        );
      }
      const baseline = await session.capture();
      remember("recovery-before-budget", baseline);
      testing.setSurfaceCacheBudget(0);
      const expected = imageExpectations(pose, false);
      const unavailable = await captureMatched(
        "recovery-budget-unavailable",
        (frame) =>
          frame.failedDrawCalls > 0 &&
          expected.overlap.every((point) =>
            pixel(frame, point).every(
              (channel, index) =>
                Math.abs(channel - pixel(frame, [2, 2])[index]!) <= 1,
            ),
          ),
      );
      check(
        (await observer.inspect()).entities.some(
          (e) => e.metadata.symbolicId === "advanced-slider",
        ),
        "Budget failure lost CPU World state",
      );
      const unavailableStatistics = await diagnostics.statistics();
      check(
        unavailableStatistics.surfaces.surfaceCacheResidentBytes === 0,
        "Zero image budget retained stale curved GPU images",
      );
      testing.setSurfaceCacheBudget(64 * 1024 * 1024);
      await captureMatched("recovery-budget-restored", (frame) =>
        colour(pixel(frame, expected.overlap[0]!), expected.mixed),
      );
      await currentChild();
      const restoredStatistics = await diagnostics.statistics();
      check(
        restoredStatistics.surfaces.surfaceCacheResidentBytes > 0,
        "Restored image budget did not rebuild curved GPU images",
      );
      const oldContext = (await host.presentation.surface()).context;
      const loss = session
        .capture({ afterSequence: 0xffff_ffff_ffff_ffffn })
        .then(
          () => {
            throw new Error("Context-loss capture unexpectedly completed");
          },
          (error: unknown) => error,
        );
      testing.loseContext();
      check(
        (await loss) instanceof Error,
        "Context loss did not reject pending capture",
      );
      check(
        (
          await observer.batch([
            {
              kind: "create",
              alias: 99,
              metadata: {
                symbolicId: "alive-during-context-loss",
                classes: [],
              },
            },
          ])
        ).ok,
        "CPU commands failed during graphics context loss",
      );
      check(
        (await observer.inspect()).entities.some(
          (e) => e.metadata.symbolicId === "alive-during-context-loss",
        ),
        "CPU mutation missing during context loss",
      );
      testing.restoreContext();
      const deadline = performance.now() + 10000;
      let changed = false;
      do {
        try {
          changed = (await host.presentation.surface()).context !== oldContext;
        } catch {
          /* Real context restoration is asynchronous. */
        }
        if (!changed) await new Promise((resolve) => setTimeout(resolve, 16));
      } while (!changed && performance.now() < deadline);
      check(changed, "Context restoration reused old context stamp");
      await session.recoverPresentation();
      await captureMatched("recovery-context-restored", (frame) =>
        colour(pixel(frame, expected.overlap[0]!), expected.mixed),
      );
      await currentChild();
      await input.close();
      input = await host.input.open(session.view!);
      check(
        (await pointInput("pointerDown", pose, [65, 102], 2n)).disposition ===
          "routed",
        "Fresh input context did not route after recovery",
      );
      await pointInput("pointerUp", pose, [65, 102], 2n);
      observations.push({
        budgetFailureDraws: unavailable.failedDrawCalls,
        unavailableResidentBytes:
          unavailableStatistics.surfaces.surfaceCacheResidentBytes,
        restoredResidentBytes:
          restoredStatistics.surfaces.surfaceCacheResidentBytes,
        oldContext,
        newContext: (await host.presentation.surface()).context,
        cpuDuringLoss: true,
      });
    } else {
      await input.close();
      input = undefined;
      const sourceClient = observer as AssetWorldClient;
      check(
        typeof sourceClient.registerAsset === "function",
        "Missing public asset data-plane registration",
      );
      const delayed = {
        bitmap: clientAssetSource(
          sourceClient.session,
          2,
          "projected-delayed-bitmap",
        ),
        font: clientAssetSource(
          sourceClient.session,
          17,
          "projected-delayed-font",
        ),
      };
      await root.render(scene(pose, delayed));
      const sibling = pixelFor(projectedSurfacePoint(pose, [65, 42]));
      const bitmap = pixelFor(projectedSurfacePoint(pose, [30, 52]));
      const base = composite(LOWER, [0, 0, 0, 0]);
      function fontInk(frame: PresentedCapture) {
        let ink = 0;
        for (let y = 35; y < 130; y++)
          for (let x = 50; x < 220; x++) {
            const content = projectedSurfaceRay(pose, [x, y]);
            if (!inside(content, [10, 14, 50, 15])) continue;
            if (pixel(frame, [x, y]).every((channel) => channel > 180)) ink++;
          }
        return ink;
      }
      await captureMatched(
        "advanced-assets-pending",
        (frame) =>
          colour(pixel(frame, sibling), base) &&
          colour(pixel(frame, bitmap), base) &&
          fontInk(frame) === 0,
      );
      const bytes = new Uint8Array(16 + 4 * 4 * 4);
      bytes.set([73, 80, 80, 84]);
      const header = new DataView(bytes.buffer);
      header.setUint32(4, 3, true);
      header.setUint32(8, 4, true);
      header.setUint32(12, 4, true);
      for (let offset = 16; offset < bytes.length; offset += 4)
        bytes.set([255, 128, 0, 255], offset);
      await sourceClient.registerAsset(delayed.bitmap, bytes.buffer);
      await captureMatched(
        "advanced-bitmap-ready",
        (frame) =>
          colour(pixel(frame, sibling), base) &&
          colour(pixel(frame, bitmap), [255, 128, 0]) &&
          fontInk(frame) === 0,
      );
      await sourceClient.registerAsset(
        delayed.font,
        assets.font.slice().buffer,
      );
      await captureMatched(
        "advanced-font-ready",
        (frame) =>
          colour(pixel(frame, sibling), base) &&
          colour(pixel(frame, bitmap), [255, 128, 0]) &&
          fontInk(frame) > 10,
      );
      observations.push({ controlledAssetReadiness: true, sibling, bitmap });
      await observer.close();
      observer = undefined;
      const geometry = contract.encodeBoundingShape({
        type: "box",
        min: [-0.12, -0.1, -0.04],
        max: [0.12, 0.1, 0.04],
      });
      for (const [index, yaw] of [0.45, Math.PI].entries()) {
        const cameraPose: Pose = {
          shape: "sphere",
          curvature: 0.8,
          spacing: 0,
          placement: { yaw, scale: [1.05, 0.9, 1] },
        };
        await root.render(
          <>
            {camera}
            <Entity id="advanced-camera-panel">
              {placement(cameraPose)}
              {surface(cameraPose)}
            </Entity>
            <AttachedWorld
              anchor="advanced-camera-panel"
              child={{ create: { selectedSystems: SYSTEMS } }}
              attachment={{
                mode: "surface-camera",
                output: { entity: "asymmetric-camera" },
              }}
            >
              <Entity id="asymmetric-camera">
                <Transform z={2} />
                <Camera
                  projection={0}
                  fov_y={Math.PI / 4}
                  near={0.05}
                  far={20}
                />
              </Entity>
              <Entity id="camera-red">
                <Transform x={-0.35} y={0.18} />
                <BoundingGeometry
                  geometry={geometry}
                  is_rendered
                  color={[1, 0, 0]}
                />
              </Entity>
              <Entity id="camera-blue">
                <Transform x={0.3} y={-0.17} />
                <BoundingGeometry
                  geometry={geometry}
                  is_rendered
                  color={[0, 0, 1]}
                />
              </Entity>
            </AttachedWorld>
          </>,
        );
        const sample = (x: number, y: number): Point =>
          pixelFor(
            projectedSurfacePoint(cameraPose, [
              (x / ((2 * Math.tan(Math.PI / 8) * 4) / 3) + 1) * 80,
              (1 - y / (2 * Math.tan(Math.PI / 8))) * 60,
            ]),
          );
        const red = sample(-0.35, 0.18),
          blue = sample(0.3, -0.17);
        await captureMatched(`advanced-camera-asymmetric-${index}`, (frame) => {
          const r = pixel(frame, red),
            b = pixel(frame, blue);
          return (
            r[0]! > 170 &&
            r[1]! < 70 &&
            r[2]! < 70 &&
            b[2]! > 170 &&
            b[0]! < 70 &&
            b[1]! < 70
          );
        });
        observations.push({ cameraPose, red, blue });
      }
    }
    check(
      errors.length === 0,
      `Advanced React errors: ${errors.map(String).join("; ")}`,
    );
  } catch (error) {
    if (last) remember("advanced-failure", last);
    failure = error instanceof Error ? error.message : String(error);
  } finally {
    await input?.close();
    await observer?.close();
    await session.close();
    await client.close();
    await host.destroyWorld(world);
  }
  return {
    failure,
    images,
    observations,
    comparisons,
    assertions: [
      "independent linear-light transparent shell composition and parallax",
      "rear/oblique/nonuniform placement and local-normal input",
      "capture miss/continuation and provider lifetime fences",
      recovery
        ? "budget/context recovery preserves CPU state and rebuilds presentation/fresh input"
        : "asymmetric Camera output UV orientation including rear",
    ],
  };
}
