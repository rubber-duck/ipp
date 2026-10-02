/**
 * Canvas layers through the generated client, React declarations and real
 * presentation: a flat root canvas whose raised content overlaps against tree
 * order and escapes a lower clip, and an exploded Surface whose layer planes
 * separate along its normal in front, oblique and rear views and take
 * pointer presses on the plane nearest along the ray.
 *
 * Layers are plane ids: the green button is on plane 1 and its badge and
 * ring arc, declared inside it, on plane 3, so plane 2 is empty and the
 * badge sits three spacings out, leaving a gap. Exploded captures are judged
 * against an independent geometric oracle: the ray through each sampled pixel
 * meets each plane at its id times the spacing, and the authored rectangles
 * and the ring arc on those planes decide the colour the view-depth order
 * must show.
 */
import {
  canvasOutput,
  type Client,
  type GuiPhysicalContext,
  type HostClientBase,
  type PresentedCapture,
} from "@ipp/client";
import {
  Camera,
  CanvasWorld,
  Children,
  Entity,
  Surface,
  Transform,
} from "@ipp/react";
import { Box, Button, Layout, Skin, Style } from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, entity, type GuiContract } from "./gui-authoring.js";

type Rgb = readonly [number, number, number];
type Rect = readonly [number, number, number, number];

interface LayerImage {
  label: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
}

/** Systems of the layered panel's Canvas World. */
const PANEL_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

/** Systems of the scene presenting the panel on a Surface to a camera. */
const SCENE_SYSTEMS = [
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
] as const;

/** Physical panel size in metres and its Canvas extent at 100 units per metre. */
const PANEL = { size: [1.6, 1.2], extent: [160, 120] } as const;

/** Base red button, plane-1 green button and its plane-3 blue badge, in content units. */
const RED: Rect = [16, 50, 72, 90];
const GREEN: Rect = [45, 34, 125, 82];
const BLUE: Rect = [101, 14, 137, 38];
const CANVAS: Rect = [0, 0, PANEL.extent[0], PANEL.extent[1]];

/** Plane ids of the raised green button and of its badge and ring arc. */
const PLANES = { raised: 1, badge: 3 } as const;

/**
 * A yellow plane-3 ring arc over the green button: 20 units out and 8 thick
 * around (100, 62), from three o'clock clockwise to twelve, so the quarter
 * between twelve and three o'clock and the hollow show the planes beneath.
 */
const RING = { center: [100, 62], outer: 20, inner: 12 } as const;

/** Whether a content point lies in the ring arc, at least `margin` inside it. */
function inRing(point: readonly [number, number] | null, margin: number) {
  if (point === null) return false;
  const [dx, dy] = [point[0] - RING.center[0], point[1] - RING.center[1]];
  const radius = Math.hypot(dx, dy);
  return (
    radius >= RING.inner + margin &&
    radius <= RING.outer - margin &&
    (dx <= -margin || dy >= margin)
  );
}

/** Whether a content point lies clearly outside the ring arc, beyond `margin`. */
function outsideRing(point: readonly [number, number] | null, margin: number) {
  if (point === null) return true;
  const [dx, dy] = [point[0] - RING.center[0], point[1] - RING.center[1]];
  const radius = Math.hypot(dx, dy);
  return (
    radius <= RING.inner - margin ||
    radius >= RING.outer + margin ||
    (dx >= margin && dy <= -margin)
  );
}

/** Straight linear RGBA of the translucent base panel behind the buttons. */
const PANEL_TINT = [0.02, 0.03, 0.05, 0.5] as const;

/** Camera distance, vertical field of view and viewport of the exploded views. */
const CAMERA_Z = 2.5;
const FOV_Y = Math.PI / 4;
const VIEW = [320, 240] as const;

const red = ([r, g, b]: Rgb) => r > 150 && g < 90 && b < 90;
const green = ([r, g, b]: Rgb) => g > 150 && r < 90 && b < 90;
const blue = ([r, g, b]: Rgb) => b > 150 && r < 90 && g < 90;
const black = ([r, g, b]: Rgb) => r < 24 && g < 24 && b < 24;
const yellow = ([r, g, b]: Rgb) => r > 200 && g > 200 && b < 60;
/** Seen through the translucent panel: dominant, but dimmed. */
const greenish = ([r, g, b]: Rgb) => g > r + 60 && g > b + 60;
const bluish = ([r, g, b]: Rgb) => b > r + 40 && b > g + 30;
const yellowish = ([r, g, b]: Rgb) => r > b + 60 && g > b + 60;

function inside(
  point: readonly [number, number] | null,
  rect: Rect,
  margin = 0,
) {
  return (
    point !== null &&
    point[0] >= rect[0] + margin &&
    point[1] >= rect[1] + margin &&
    point[0] < rect[2] - margin &&
    point[1] < rect[3] - margin
  );
}

/** Whether the point is clearly outside the rectangle, beyond `margin`. */
function outside(
  point: readonly [number, number] | null,
  rect: Rect,
  margin: number,
) {
  return (
    point === null ||
    point[0] < rect[0] - margin ||
    point[1] < rect[1] - margin ||
    point[0] >= rect[2] + margin ||
    point[1] >= rect[3] + margin
  );
}

function sample(frame: PresentedCapture, x: number, y: number): Rgb {
  const width = frame.view.binding.viewport.width;
  const offset = (y * width + x) * 4;
  const pixels = new Uint8Array(frame.pixels);
  return [pixels[offset]!, pixels[offset + 1]!, pixels[offset + 2]!];
}

/** Pose of the exploded panel: turned `angle` radians about +Y, plane ids `spacing` m apart. */
interface PanelPose {
  angle: number;
  spacing: number;
}

/**
 * Canvas content point where the ray through viewport pixel `(x, y)` meets
 * layer `layer`'s plane, `layer` spacings out, or null when the ray meets it
 * behind the camera.
 */
function contentAt(
  pose: PanelPose,
  [x, y]: readonly [number, number],
  layer: number,
): [number, number] | null {
  const tan = Math.tan(FOV_Y / 2);
  const ndc = [((x + 0.5) / VIEW[0]) * 2 - 1, 1 - ((y + 0.5) / VIEW[1]) * 2];
  const direction = [ndc[0]! * tan * (VIEW[0] / VIEW[1]), ndc[1]! * tan, -1];
  const [c, s] = [Math.cos(pose.angle), Math.sin(pose.angle)];
  // Panel-local coordinates: the inverse of the rotation about +Y.
  const local = (v: readonly number[]) => [
    v[0]! * c - v[2]! * s,
    v[1]!,
    v[0]! * s + v[2]! * c,
  ];
  const origin = local([0, 0, CAMERA_Z]);
  const ray = local(direction);
  const t = (layer * pose.spacing - origin[2]!) / ray[2]!;
  if (!(t > 0)) return null;
  const point = [origin[0]! + t * ray[0]!, origin[1]! + t * ray[1]!];
  return [
    (point[0]! / PANEL.size[0] + 0.5) * PANEL.extent[0],
    (0.5 - point[1]! / PANEL.size[1]) * PANEL.extent[1],
  ];
}

/** Layer-plane content points of one pixel: base, raised and badge planes. */
function planes(pose: PanelPose, pixel: readonly [number, number]) {
  return [
    contentAt(pose, pixel, 0),
    contentAt(pose, pixel, PLANES.raised),
    contentAt(pose, pixel, PLANES.badge),
  ] as const;
}

/**
 * The colour test the oracle expects at a pixel, or undefined where an edge
 * of any rectangle is too close to judge. `front` draws the base plane first.
 */
function expected(
  pose: PanelPose,
  pixel: readonly [number, number],
  front: boolean,
): [string, (rgb: Rgb) => boolean] | undefined {
  const margin = 2;
  const [base, raised, badge] = planes(pose, pixel);
  const hits = {
    red: inside(base, RED, margin),
    green: inside(raised, GREEN, margin),
    blue: inside(badge, BLUE, margin),
    ring: inRing(badge, margin),
    panel: inside(base, CANVAS, margin),
  };
  const clear =
    (hits.red || outside(base, RED, margin)) &&
    (hits.green || outside(raised, GREEN, margin)) &&
    (hits.blue || outside(badge, BLUE, margin)) &&
    (hits.ring || outsideRing(badge, margin)) &&
    (hits.panel || outside(base, CANVAS, margin));
  if (!clear) return undefined;
  if (front) {
    if (hits.blue) return ["blue", blue];
    if (hits.ring) return ["yellow", yellow];
    if (hits.green) return ["green", green];
    if (hits.red) return ["red", red];
    return undefined;
  }
  // From behind the base plane is nearest: red covers, the panel tints.
  if (hits.red) return ["red", red];
  if (!hits.panel) {
    if (hits.green) return ["green", green];
    if (hits.blue) return ["blue", blue];
    if (hits.ring) return ["yellow", yellow];
    return undefined;
  }
  if (hits.green) return ["green through the panel", greenish];
  if (hits.blue) return ["blue through the panel", bluish];
  if (hits.ring) return ["yellow through the panel", yellowish];
  return undefined;
}

/** Sampled agreement of one capture with the oracle, per expected colour. */
function judge(frame: PresentedCapture, pose: PanelPose, front: boolean) {
  const counts = new Map<string, { matched: number; missed: number }>();
  for (let y = 1; y < VIEW[1]; y += 3)
    for (let x = 1; x < VIEW[0]; x += 3) {
      const verdict = expected(pose, [x, y], front);
      if (!verdict) continue;
      const count = counts.get(verdict[0]) ?? { matched: 0, missed: 0 };
      if (verdict[1](sample(frame, x, y))) count.matched++;
      else count.missed++;
      counts.set(verdict[0], count);
    }
  return counts;
}

/**
 * Whether every expected colour appears on enough samples and at most 2% of
 * each colour's samples disagree, which leaves room for rasterization at
 * rectangle corners but not for a misplaced or misordered plane.
 */
function agrees(
  counts: Map<string, { matched: number; missed: number }>,
  colours: readonly string[],
) {
  return colours.every((colour) => {
    const count = counts.get(colour);
    return (
      count !== undefined &&
      count.matched >= 20 &&
      count.missed <= count.matched / 50
    );
  });
}

/** Viewport pixels whose layer planes satisfy `test`, for presses and checks. */
function find(
  pose: PanelPose,
  test: (
    base: [number, number] | null,
    raised: [number, number] | null,
    badge: [number, number] | null,
  ) => boolean,
): [number, number] {
  for (let y = 2; y < VIEW[1]; y += 2)
    for (let x = 2; x < VIEW[0]; x += 2) {
      const [base, raised, badge] = planes(pose, [x, y]);
      if (test(base, raised, badge)) return [x, y];
    }
  throw new Error("No pixel satisfies the layered press geometry");
}

export async function guiLayers(
  host: HostClientBase<Client>,
  contract: GuiContract,
) {
  const images: LayerImage[] = [];
  const part = contract.guiPaintPartIndex({ part: "background" });
  // Plain rectangles, as the oracle measures them: the rows state away the
  // default button look's corner cuts, which they would otherwise sit on.
  const solid = (color: readonly [number, number, number, number]) =>
    contract.GuiSkin.encodeParts({
      nextSlot: 1,
      rows: new Map([
        [
          0,
          {
            part,
            color,
            corner_radius: [0, 0],
            border_width: 0,
            corner_cut: [0, 0, 0, 0],
          },
        ],
      ]),
    });
  const ringArc = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          part,
          color: [1, 1, 0, 1],
          border_width: RING.outer - RING.inner,
          shape: 2,
          arc_start: 0.25,
          arc_sweep: 0.75,
        },
      ],
    ]),
  });
  const keep = (label: string, frame: PresentedCapture) =>
    images.push({
      label,
      width: frame.view.binding.viewport.width,
      height: frame.view.binding.viewport.height,
      pixels: [...new Uint8Array(frame.pixels)],
      sequence: frame.sequence,
    });

  /** Capture until `accept` passes, keeping the last frame either way. */
  async function captureUntil(
    session: CanvasWorldSession,
    label: string,
    accept: (frame: PresentedCapture) => string | null,
  ) {
    const deadline = performance.now() + 20_000;
    let sequence: bigint | undefined;
    for (;;) {
      const frame = await session.capture(
        sequence === undefined ? {} : { afterSequence: sequence },
      );
      const failure = accept(frame);
      if (failure === null || performance.now() >= deadline) {
        keep(failure === null ? label : `failed-${label}`, frame);
        check(failure === null, `${label}: ${failure}`);
        return frame;
      }
      sequence = frame.sequence;
    }
  }

  const box = (
    id: string,
    at: readonly [number, number],
    size: readonly [number, number],
    tint: readonly [number, number, number],
    layer = 0,
  ) => (
    <Entity id={id}>
      <Style
        x={at[0]}
        y={at[1]}
        red={tint[0]}
        green={tint[1]}
        blue={tint[2]}
        layer={layer}
      />
      <Box width={size[0]} height={size[1]} />
    </Entity>
  );
  const cleanup: (() => Promise<unknown>)[] = [];

  /** The flat root canvas: tree order contradicted, and a clip escaped. */
  async function flatOverlap() {
    // A flat root canvas: the raised green box precedes the red one in tree
    // order yet paints over it, and a raised blue child of a 16-unit clip paints
    // outside it while its yellow sibling stays clipped.
    const flatWorld = (
      await host.createWorld({ selectedSystems: [...PANEL_SYSTEMS] })
    ).reference;
    const flatClient = await host.openWorld(flatWorld);
    const flatSession = new CanvasWorldSession({ host, client: flatClient });
    const flatRoot = flatSession.createRoot();
    cleanup.push(
      () => flatSession.close(),
      () => flatClient.close(),
      () => host.destroyWorld(flatWorld),
    );
    await flatRoot.render(
      <Entity id="flat-root">
        <Layout kind={3} width={96} height={64} align_x={-1} align_y={-1} />
        <Children>
          {box("flat-page", [0, 0], [96, 64], [0, 0, 0])}
          {box("flat-raised", [28, 12], [40, 40], [0, 1, 0], 1)}
          {box("flat-later", [8, 4], [40, 40], [1, 0, 0])}
          <Entity id="flat-clip">
            <Layout width={16} height={16} align_x={-1} align_y={-1} />
            <Style x={72} y={8} clipped clip_max_x={16} clip_max_y={16} />
            <Children>
              {box("flat-clipped", [0, 0], [32, 32], [1, 1, 0])}
              {box("flat-escaped", [8, 24], [24, 24], [0, 0, 1], 1)}
            </Children>
          </Entity>
        </Children>
      </Entity>,
    );
    await flatSession.selectOutput(canvasOutput(flatWorld), {
      width: 96,
      height: 64,
      devicePixelRatio: 1,
    });
    const probes: [string, number, number, (rgb: Rgb) => boolean][] = [
      ["raised green over the later red", 36, 30, green],
      ["red outside the raised box", 14, 20, red],
      ["yellow inside its parent's clip", 80, 14, yellow],
      ["yellow clipped beyond its parent", 92, 14, black],
      ["raised blue outside its parent's clip", 86, 44, blue],
      ["page", 60, 58, black],
    ];
    await captureUntil(flatSession, "layers-flat-overlap", (frame) => {
      if (frame.view.binding.viewport.width !== 96) return "unsized";
      const failed = probes.filter(
        ([, x, y, test]) => !test(sample(frame, x, y)),
      );
      return failed.length === 0
        ? null
        : failed
            .map(([label, x, y]) => `${label} ${sample(frame, x, y).join(",")}`)
            .join("; ");
    });
  }

  // An exploded Surface in a 3D scene, presented to a perspective camera.
  const presses: string[] = [];
  let input: GuiPhysicalContext | undefined;
  const scene = (pose: PanelPose) => (
    <>
      <Entity id="layers-camera">
        <Transform z={CAMERA_Z} />
        <Camera projection={0} fov_y={FOV_Y} near={0.05} far={50} />
      </Entity>
      <Entity id="layers-panel">
        <Transform ry={pose.angle} />
        <Surface
          width={PANEL.size[0]}
          height={PANEL.size[1]}
          layer_spacing={pose.spacing}
        />
      </Entity>
      <CanvasWorld
        presentation={{ anchor: "layers-panel" }}
        create={{ selectedSystems: PANEL_SYSTEMS }}
        extent={[PANEL.extent[0], PANEL.extent[1]]}
        unitsPerMetre={100}
      >
        <Entity id="layers-content">
          <Layout
            kind={3}
            width={PANEL.extent[0]}
            height={PANEL.extent[1]}
            align_x={-1}
            align_y={-1}
          />
          <Children>
            <Entity id="layers-base">
              <Layout
                width={PANEL.extent[0]}
                height={PANEL.extent[1]}
                align_x={-1}
                align_y={-1}
              />
              <Style
                red={PANEL_TINT[0]}
                green={PANEL_TINT[1]}
                blue={PANEL_TINT[2]}
                alpha={PANEL_TINT[3]}
              />
              <Box width={PANEL.extent[0]} height={PANEL.extent[1]} />
            </Entity>
            <Entity id="layers-red">
              <Layout
                width={RED[2] - RED[0]}
                height={RED[3] - RED[1]}
                align_x={-1}
                align_y={-1}
              />
              <Style x={RED[0]} y={RED[1]} />
              <Skin parts={solid([1, 0, 0, 1])} />
              <Button label="" onPress={() => presses.push("red")} />
            </Entity>
            <Entity id="layers-green">
              <Layout
                width={GREEN[2] - GREEN[0]}
                height={GREEN[3] - GREEN[1]}
                align_x={-1}
                align_y={-1}
              />
              <Style x={GREEN[0]} y={GREEN[1]} layer={PLANES.raised} />
              <Skin parts={solid([0, 1, 0, 1])} />
              <Button label="" onPress={() => presses.push("green")} />
              <Children>
                {box(
                  "layers-blue",
                  [BLUE[0] - GREEN[0], BLUE[1] - GREEN[1]],
                  [BLUE[2] - BLUE[0], BLUE[3] - BLUE[1]],
                  [0, 0, 1],
                  PLANES.badge,
                )}
                <Entity id="layers-ring">
                  <Style
                    x={RING.center[0] - RING.outer - GREEN[0]}
                    y={RING.center[1] - RING.outer - GREEN[1]}
                    layer={PLANES.badge}
                  />
                  <Box width={2 * RING.outer} height={2 * RING.outer} />
                  <Skin parts={ringArc} />
                </Entity>
              </Children>
            </Entity>
          </Children>
        </Entity>
      </CanvasWorld>
    </>
  );

  /** Press and release at a viewport pixel and return the control pressed. */
  async function press(
    session: CanvasWorldSession,
    pixel: readonly [number, number],
  ) {
    const view = session.view;
    check(view, "The exploded scene has no presented view");
    if (!input) input = await host.input.open(view);
    presses.length = 0;
    const point = [
      (pixel[0] + 0.5) / VIEW[0],
      (pixel[1] + 0.5) / VIEW[1],
    ] as const;
    for (const kind of ["pointerDown", "pointerUp"] as const) {
      const outcome = await input.send({ kind, pointer: 1n, point });
      check(
        outcome.disposition === "routed",
        `${kind} at ${pixel} was not routed: ${JSON.stringify(outcome)}`,
      );
    }
    const deadline = performance.now() + 10_000;
    while (presses.length === 0 && performance.now() < deadline)
      await new Promise<void>((resolve) => setTimeout(resolve, 16));
    return presses.join(",");
  }

  try {
    await flatOverlap();
    const sceneWorld = (
      await host.createWorld({ selectedSystems: [...SCENE_SYSTEMS] })
    ).reference;
    cleanup.push(() => host.destroyWorld(sceneWorld));
    const sceneClient = await host.openWorld(sceneWorld);
    cleanup.push(() => sceneClient.close());
    const session = new CanvasWorldSession({ host, client: sceneClient });
    cleanup.push(
      () => session.close(),
      async () => input?.close(),
    );
    const root = session.createRoot();
    const front: PanelPose = { angle: 0, spacing: 0.6 };
    await root.render(scene(front));
    const camera = await entity(sceneClient, "layers-camera");
    await session.selectOutput(
      await host.bindOutput(sceneWorld, camera.id, "camera"),
      { width: VIEW[0], height: VIEW[1], devicePixelRatio: 1 },
    );
    const exploded = async (
      label: string,
      pose: PanelPose,
      viewFront: boolean,
      colours: readonly string[],
    ) => {
      await root.render(scene(pose));
      await captureUntil(session, label, (frame) => {
        if (frame.view.binding.viewport.width !== VIEW[0]) return "unsized";
        const counts = judge(frame, pose, viewFront);
        return agrees(counts, colours) ? null : JSON.stringify([...counts]);
      });
    };
    await exploded("layers-exploded-front", front, true, [
      "red",
      "green",
      "blue",
      "yellow",
    ]);
    // Turned 40 degrees, the planes visibly separate: the capture matches the
    // exploded oracle, and the pose leaves enough samples whose exploded
    // colour differs from one flat plane for that match to tell them apart.
    const oblique: PanelPose = { angle: (40 * Math.PI) / 180, spacing: 0.3 };
    await exploded("layers-exploded-oblique", oblique, true, [
      "red",
      "green",
      "blue",
      "yellow",
    ]);
    let separated = 0;
    for (let y = 1; y < VIEW[1]; y += 3)
      for (let x = 1; x < VIEW[0]; x += 3) {
        const layered = expected(oblique, [x, y], true)?.[0];
        const flat = expected({ ...oblique, spacing: 0 }, [x, y], true)?.[0];
        if (layered && flat && layered !== flat) separated++;
      }
    check(separated >= 50, `Oblique planes barely separate: ${separated}`);
    // From behind, the base plane is nearest and its red button covers.
    await exploded(
      "layers-exploded-behind",
      { angle: Math.PI - (40 * Math.PI) / 180, spacing: 0.2 },
      false,
      ["red", "green through the panel", "blue through the panel"],
    );

    // Presses in the front view: on the raised plane's control where the
    // base plane shows only the red button, and beside the raised control,
    // where the ray falls through to the red one.
    await root.render(scene(front));
    await captureUntil(session, "layers-exploded-input", (frame) =>
      agrees(judge(frame, front, true), ["red", "green"]) ? null : "settling",
    );
    const overRaised = find(
      front,
      (base, raised, badge) =>
        inside(raised, GREEN, 4) &&
        inside(base, RED, 4) &&
        outside(base, GREEN, 4) &&
        outside(badge, BLUE, 4),
    );
    const beside = find(
      front,
      (base, raised, badge) =>
        inside(base, RED, 4) &&
        outside(raised, GREEN, 4) &&
        outside(badge, BLUE, 4),
    );
    check(
      (await press(session, overRaised)) === "green",
      `Press over the raised plane at ${overRaised}: ${presses}`,
    );
    check(
      (await press(session, beside)) === "red",
      `Press beside the raised control at ${beside}: ${presses}`,
    );
    // On one plane the same pixel reaches the red button the base shows there.
    await root.render(scene({ angle: 0, spacing: 0 }));
    await captureUntil(session, "layers-flat-input", (frame) =>
      red(sample(frame, overRaised[0], overRaised[1])) ? null : "settling",
    );
    check(
      (await press(session, overRaised)) === "red",
      `Press at ${overRaised} without spacing: ${presses}`,
    );
    return {
      images,
      assertions: [
        "raised paint over a later sibling against tree order",
        "raised child outside its lower-layer parent's clip, sibling clipped",
        "exploded front, oblique and rear captures match the layer-plane oracle",
        "the badge on plane 3 three spacings out, the empty plane 2 leaving a gap",
        "a raised ring arc on its layer plane, its hollow and open quarter showing the plane beneath",
        "oblique layer planes separate from the flat layout",
        "rear view draws the nearest base plane last",
        "press on the raised plane's control, fall-through beside it",
        "the same pixel without spacing reaches the base control",
      ],
      failure: null,
    };
  } catch (error) {
    return {
      images,
      assertions: [],
      failure: error instanceof Error ? error.message : String(error),
    };
  } finally {
    for (const release of cleanup.reverse()) await release().catch(() => {});
  }
}
