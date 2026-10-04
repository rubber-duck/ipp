/** Shared projected-Surface assertions for native WebSocket/GLES and worker WASM/WebGL. */
import type {
  Client,
  GuiPhysicalContext,
  HostClientBase,
  PresentedCapture,
  WorldReference,
} from "@ipp/client";
import {
  Asset,
  AttachedWorld,
  BoundingGeometry,
  Camera,
  CanvasWorld,
  Children,
  CylinderSurface,
  Entity,
  FlatSurface,
  SphereSurface,
  Transform,
  assetRef,
} from "@ipp/react";
import {
  Box,
  Button,
  Drawing,
  Image,
  Layout,
  Skin,
  Style,
  Text,
} from "@ipp/react/gui";
import {
  GuiKit,
  Panel,
  PanelFooter,
  PanelHeader,
  SecondaryButton,
  TextLine,
} from "@ipp/react/gui-kit";
import { CanvasWorldSession } from "@ipp/react/web";
import { check, entity, type GuiContract } from "./gui-authoring.js";
import type { GuiPaintAssets } from "./gui-paint.js";

type Shape = "flat" | "cylinder" | "sphere";
type Vec3 = readonly [number, number, number];
type Point = readonly [number, number];
type ProjectedContract = GuiContract &
  Pick<
    typeof import("@ipp/gui-authoring-contract"),
    | "encodeBoundingShape"
    | "GUI_SKIN_LOOKS"
    | "GuiThemeMotion"
    | "GUI_SKIN_TOKENS"
  >;
const SIZE = [1.6, 1.2] as const;
const EXTENT = [160, 120] as const;
const VIEW = [400, 300] as const;
const CAMERA_Z = 2.5;
const FOV = Math.PI / 4;
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
] as const;
const CANVAS_SYSTEMS = [
  "ipp.animation",
  "ipp.gui",
  "ipp.gui-layout",
  "ipp.canvas",
  "ipp.asset-dependencies",
  "ipp.lifecycle-publisher",
] as const;

export interface Pose {
  shape: Shape;
  curvature: number;
  spacing: number;
  /** Local Y rotation and positive nonuniform TRS scale, independently inverted below. */
  placement?: { yaw: number; scale: Vec3 };
}
interface Capture {
  label: string;
  width: number;
  height: number;
  pixels: number[];
  sequence: bigint;
}

/** Independent chart evaluation; tests never call the production authoring/mapping helpers. */
export function projectedSurfacePoint(
  pose: Pose,
  content: Point,
  rank = 0,
): Vec3 {
  const x = content[0] / 100 - SIZE[0] / 2;
  const y = SIZE[1] / 2 - content[1] / 100;
  const k = pose.shape === "flat" ? 0 : pose.curvature;
  const distance = rank * pose.spacing;
  if (k === 0) return placed(pose, [x, y, distance]);
  const length = pose.shape === "sphere" ? Math.hypot(x, y) : Math.abs(x);
  const angle = k * length;
  const sinc = angle === 0 ? 1 : Math.sin(angle) / angle;
  const cosine = Math.cos(angle);
  const factor = 1 + k * distance;
  return placed(pose, [
    x * sinc * factor,
    pose.shape === "sphere" ? y * sinc * factor : y,
    (cosine - 1) / k + distance * cosine,
  ]);
}

/** Independent local TRS composition; no runtime matrix or authoring helper. */
function placed(pose: Pose, point: Vec3): Vec3 {
  const { yaw, scale } = pose.placement ?? { yaw: 0, scale: [1, 1, 1] };
  const c = Math.cos(yaw),
    s = Math.sin(yaw);
  return [
    c * point[0] * scale[0] + s * point[2] * scale[2],
    point[1] * scale[1],
    -s * point[0] * scale[0] + c * point[2] * scale[2],
  ];
}

export function pixelFor(point: Vec3): Point {
  const depth = CAMERA_Z - point[2];
  const scale = Math.tan(FOV / 2);
  return [
    ((point[0] / ((depth * scale * VIEW[0]) / VIEW[1]) + 1) * VIEW[0]) / 2,
    ((1 - point[1] / (depth * scale)) * VIEW[1]) / 2,
  ];
}

/** Solve the analytic shell, then invert its principal chart, independently of runtime queries. */
export function projectedSurfaceRay(
  pose: Pose,
  pixel: Point,
  rank = 0,
): Point | null {
  const scale = Math.tan(FOV / 2);
  const worldRay: Vec3 = [
    (((2 * (pixel[0] + 0.5)) / VIEW[0] - 1) * scale * VIEW[0]) / VIEW[1],
    (1 - (2 * (pixel[1] + 0.5)) / VIEW[1]) * scale,
    -1,
  ];
  const { yaw, scale: axes } = pose.placement ?? { yaw: 0, scale: [1, 1, 1] };
  const cosine = Math.cos(yaw),
    sine = Math.sin(yaw);
  const origin: Vec3 = [
    (-sine * CAMERA_Z) / axes[0],
    0,
    (cosine * CAMERA_Z) / axes[2],
  ];
  const ray: Vec3 = [
    (cosine * worldRay[0] - sine * worldRay[2]) / axes[0],
    worldRay[1] / axes[1],
    (sine * worldRay[0] + cosine * worldRay[2]) / axes[2],
  ];
  const k = pose.shape === "flat" ? 0 : pose.curvature;
  const d = rank * pose.spacing;
  if (k === 0) {
    const t = (d - origin[2]) / ray[2];
    if (t <= 0) return null;
    return [
      100 * (origin[0] + t * ray[0] + SIZE[0] / 2),
      100 * (SIZE[1] / 2 - origin[1] - t * ray[1]),
    ];
  }
  const radius = 1 / k + d;
  const centreZ = -1 / k;
  const oz = origin[2] - centreZ;
  const sphere = pose.shape === "sphere";
  const a = ray[0] ** 2 + ray[2] ** 2 + (sphere ? ray[1] ** 2 : 0);
  const b =
    2 * (origin[0] * ray[0] + oz * ray[2] + (sphere ? origin[1] * ray[1] : 0));
  const c =
    origin[0] ** 2 + oz ** 2 + (sphere ? origin[1] ** 2 : 0) - radius ** 2;
  const discriminant = b * b - 4 * a * c;
  if (discriminant < 0) return null;
  for (const t of [
    (-b - Math.sqrt(discriminant)) / (2 * a),
    (-b + Math.sqrt(discriminant)) / (2 * a),
  ]) {
    if (t <= 0) continue;
    const nx = (origin[0] + t * ray[0]) / radius;
    const ny = sphere ? (origin[1] + t * ray[1]) / radius : 0;
    const nz = (oz + t * ray[2]) / radius;
    let x: number;
    let y: number;
    if (sphere) {
      const sin = Math.hypot(nx, ny);
      const angle = Math.atan2(sin, nz);
      const factor = sin === 0 ? 1 / k : angle / (k * sin);
      x = nx * factor;
      y = ny * factor;
    } else {
      x = Math.atan2(nx, nz) / k;
      y = origin[1] + t * ray[1];
    }
    const content: Point = [100 * (x + SIZE[0] / 2), 100 * (SIZE[1] / 2 - y)];
    if (
      content[0] >= 0 &&
      content[0] <= EXTENT[0] &&
      content[1] >= 0 &&
      content[1] <= EXTENT[1]
    )
      return content;
  }
  return null;
}

/** Complete controls, asymmetric assets and a separately rendered Camera-output panel. */
export async function guiProjectedSurfaces(
  host: HostClientBase<Client>,
  contract: ProjectedContract,
  assets: GuiPaintAssets,
) {
  const images: Capture[] = [];
  const errors: Error[] = [];
  const presses: string[] = [];
  const world = (await host.createWorld({ selectedSystems: [...SYSTEMS] }))
    .reference;
  const client = await host.openWorld(world);
  const session = new CanvasWorldSession({
    host,
    client,
    onError: (error) => errors.push(error),
  });
  const root = session.createRoot();
  let input: GuiPhysicalContext | undefined;
  let panelWorld: WorldReference | undefined;
  const encode = (data: Uint8Array<ArrayBuffer>) => data;
  const geometry = contract.encodeBoundingShape({
    type: "box",
    min: [-0.5, -0.5, -0.05],
    max: [0.5, 0.5, 0.05],
  });
  const solid = contract.GuiSkin.encodeParts({
    nextSlot: 1,
    rows: new Map([
      [
        0,
        {
          part: contract.guiPaintPartIndex({ part: "background" }),
          color: [0, 1, 1, 1],
          border_width: 0,
          corner_radius: [0, 0],
          corner_cut: [0, 0, 0, 0],
        },
      ],
    ]),
  });
  const surface = (pose: Pose, width: number, height: number) =>
    pose.shape === "flat" ? (
      <FlatSurface width={width} height={height} layer_spacing={pose.spacing} />
    ) : pose.shape === "cylinder" ? (
      <CylinderSurface
        width={width}
        height={height}
        curvature={pose.curvature}
        layer_spacing={pose.spacing}
      />
    ) : (
      <SphereSurface
        width={width}
        height={height}
        curvature={pose.curvature}
        layer_spacing={pose.spacing}
      />
    );
  const scene = (pose: Pose) => (
    <>
      <Entity id="projected-camera">
        <Transform z={CAMERA_Z} />
        <Camera projection={0} fov_y={FOV} near={0.05} far={20} />
      </Entity>
      <Entity id="projected-panel">{surface(pose, ...SIZE)}</Entity>
      <CanvasWorld
        presentation={{ anchor: "projected-panel" }}
        create={{ selectedSystems: CANVAS_SYSTEMS }}
        extent={EXTENT}
        unitsPerMetre={100}
        onReady={(handle) => {
          panelWorld = handle.world;
        }}
      >
        <Asset
          id="projected-font"
          kind={17}
          data={assets.font}
          encode={encode}
        />
        <Asset
          id="projected-drawing"
          kind={18}
          data={assets.drawing}
          encode={encode}
        />
        <Asset
          id="projected-bitmap"
          kind={2}
          data={assets.bitmap}
          encode={encode}
        />
        <Entity id="projected-content">
          <Layout kind={3} width={160} height={120} align_x={-1} align_y={-1} />
          <Children>
            <Entity id="projected-background">
              {/* A translucent base keeps negative-spacing controls observable. */}
              <Style red={0.08} green={0.02} blue={0.01} alpha={0.25} />
              <Box width={160} height={120} />
            </Entity>
            <Entity id="projected-raised">
              <Layout
                kind={3}
                width={72}
                height={20}
                align_x={-1}
                align_y={-1}
              />
              <Style x={78} y={14} layer={1000000} />
              <Skin parts={solid} />
              <Button label="" onPress={() => presses.push("raised")} />
            </Entity>
            <Entity id="projected-text">
              <Style x={8} y={11} />
              <Text
                text="LEFT 7"
                source={assetRef("projected-font")}
                font_size={14}
              />
            </Entity>
            <Entity id="projected-drawing-item">
              <Style x={12} y={43} scale_x={0.65} scale_y={0.65} />
              <Drawing source={assetRef("projected-drawing")} />
            </Entity>
            <Entity id="projected-image">
              <Style x={125} y={80} />
              <Image
                source={assetRef("projected-bitmap")}
                width={20}
                height={24}
              />
            </Entity>
            <GuiKit
              contract={contract}
              font={assetRef("projected-font")}
              fontSize={8}
            >
              <Panel
                id="projected-kit-panel"
                layer={1000000}
                layout={{
                  width: 100,
                  height: 64,
                  align_x: -1,
                  align_y: -1,
                  margin_left: 8,
                  margin_top: 55,
                }}
              >
                <PanelHeader id="projected-kit-header" title="WHOLE" />
                <SecondaryButton
                  id="projected-kit-button"
                  label="GO"
                  onPress={() => presses.push("kit")}
                  layout={{ flex: 1 }}
                />
                <PanelFooter id="projected-kit-footer">
                  <TextLine
                    id="projected-kit-footer/text"
                    text="RIGHT"
                    size="small"
                  />
                </PanelFooter>
              </Panel>
            </GuiKit>
          </Children>
        </Entity>
      </CanvasWorld>
      <Entity id="projected-camera-panel">
        <Transform x={1.15} y={0.45} />
        {surface(pose, 0.4, 0.3)}
      </Entity>
      <AttachedWorld
        anchor="projected-camera-panel"
        child={{ create: { selectedSystems: SYSTEMS } }}
        attachment={{
          mode: "surface-camera",
          output: { entity: "nested-camera" },
        }}
      >
        <Entity id="nested-camera">
          <Transform z={2} />
          <Camera projection={0} fov_y={FOV} near={0.05} far={20} />
        </Entity>
        <Entity id="nested-object">
          <BoundingGeometry geometry={geometry} is_rendered color={[1, 0, 1]} />
        </Entity>
      </AttachedWorld>
    </>
  );

  const poses: Pose[] = [
    { shape: "flat", curvature: 0, spacing: 0.12 },
    { shape: "cylinder", curvature: 0.8, spacing: 0.12 },
    { shape: "cylinder", curvature: -0.8, spacing: 0.12 },
    { shape: "sphere", curvature: 0.8, spacing: 0.12 },
    { shape: "sphere", curvature: -0.8, spacing: 0.12 },
    { shape: "cylinder", curvature: 0, spacing: 0.12 },
    { shape: "sphere", curvature: 0, spacing: -0.08 },
  ];
  let failure: string | null = null;
  let diagnosticFrame: PresentedCapture | undefined;
  try {
    for (const [index, pose] of poses.entries()) {
      diagnosticFrame = undefined;
      await input?.close();
      input = undefined;
      await root.render(scene(pose));
      if (index === 0) {
        const camera = await entity(client, "projected-camera");
        await session.selectOutput(
          await host.bindOutput(world, camera.id, "camera"),
          { width: VIEW[0], height: VIEW[1], devicePixelRatio: 1 },
        );
      }
      check(panelWorld, "Projected canvas did not acknowledge its World");
      const observer = await host.openWorld(panelWorld);
      let layout: Readonly<
        Record<string, { x: number; y: number; width: number; height: number }>
      >;
      try {
        const boundsId = observer.components.CanvasBounds?.id;
        check(
          boundsId !== undefined,
          "Projected World does not expose CanvasBounds",
        );
        // Pure layout/Skin roots need no stored bounds. Add ordinary observer
        // components through the raw client; Canvas writes their evaluated values.
        const observedRoots = [
          "projected-kit-panel",
          "projected-kit-header",
          "projected-kit-footer",
          "projected-kit-header/separator",
          "projected-kit-footer/separator",
        ];
        const before = await observer.inspect();
        const inserted = await observer.batch(
          observedRoots
            .filter(
              (name) =>
                !before.entities
                  .find((item) => item.metadata.symbolicId === name)
                  ?.components.some((item) => item.component === boundsId),
            )
            .map((symbol) => ({
              kind: "insertComponent",
              entity: { kind: "symbol", symbol },
              component: boundsId,
              fields: [],
            })),
        );
        check(
          inserted.ok,
          "Raw client failed to add layout observation components",
        );
        diagnosticFrame = await session.capture();
        const snapshot = await observer.inspect();
        layout = Object.fromEntries(
          [
            "projected-kit-panel",
            "projected-kit-header",
            "projected-kit-button",
            "projected-kit-footer",
            "projected-kit-header/separator",
            "projected-kit-footer/separator",
          ].map((name) => {
            const fields = snapshot.entities
              .find((item) => item.metadata.symbolicId === name)
              ?.components.find((item) => item.component === boundsId)?.fields;
            check(fields, `Missing evaluated bounds for ${name}`);
            return [
              name,
              {
                x: Number(fields.x),
                y: Number(fields.y),
                width: Number(fields.width),
                height: Number(fields.height),
              },
            ];
          }),
        );
      } finally {
        await observer.close();
      }
      const panel = layout["projected-kit-panel"]!;
      const header = layout["projected-kit-header"]!;
      const button = layout["projected-kit-button"]!;
      const footer = layout["projected-kit-footer"]!;
      check(
        header.height > 15 && button.height > 10 && footer.height > 15,
        `Grouped panel parts collapsed: ${JSON.stringify(layout)}`,
      );
      check(
        header.y + header.height <= button.y + 0.01 &&
          button.y + button.height <= footer.y + 0.01,
        `Panel parts overlap: ${JSON.stringify(layout)}`,
      );
      check(
        footer.y + footer.height <= panel.y + panel.height + 0.01,
        "Footer escaped its complete panel root",
      );
      const target = pixelFor(projectedSurfacePoint(pose, [110, 24], 1));
      const inverse = projectedSurfaceRay(
        pose,
        [target[0] - 0.5, target[1] - 0.5],
        1,
      );
      check(
        inverse && Math.hypot(inverse[0] - 110, inverse[1] - 24) < 1e-6,
        "Independent chart/ray oracle failed its round trip",
      );
      let frame: PresentedCapture;
      let sequence: bigint | undefined;
      let matched = false;
      const deadline = performance.now() + 15_000;
      do {
        frame = await session.capture(
          sequence === undefined ? {} : { afterSequence: sequence },
        );
        sequence = frame.sequence;
        const bytes = new Uint8Array(frame.pixels);
        const offset =
          (Math.floor(target[1]) * VIEW[0] + Math.floor(target[0])) * 4;
        let magenta = 0;
        for (let y = 50; y < 125; y++)
          for (let x = 320; x < VIEW[0]; x++) {
            const p = (y * VIEW[0] + x) * 4;
            if (bytes[p]! > 160 && bytes[p + 1]! < 80 && bytes[p + 2]! > 160)
              magenta++;
          }
        const visible = { text: 0, drawing: 0, bitmap: 0 };
        for (let y = 35; y < VIEW[1] - 25; y += 2)
          for (let x = 50; x < 310; x += 2) {
            const point = projectedSurfaceRay(pose, [x, y]);
            if (!point) continue;
            const p = (y * VIEW[0] + x) * 4;
            const r = bytes[p]!;
            const g = bytes[p + 1]!;
            const b = bytes[p + 2]!;
            if (
              point[0] >= 8 &&
              point[0] < 65 &&
              point[1] >= 11 &&
              point[1] < 26 &&
              r > 180 &&
              g > 180 &&
              b > 180
            )
              visible.text++;
            if (
              point[0] >= 12 &&
              point[0] < 33 &&
              point[1] >= 43 &&
              point[1] < 59 &&
              b > 90 &&
              b > r + 30
            )
              visible.drawing++;
            if (
              point[0] >= 125 &&
              point[0] < 145 &&
              point[1] >= 80 &&
              point[1] < 104 &&
              r > 140 &&
              g > 75 &&
              b < 100
            )
              visible.bitmap++;
          }
        matched =
          bytes[offset]! < 70 &&
          bytes[offset + 1]! > 170 &&
          bytes[offset + 2]! > 170 &&
          magenta > 30 &&
          visible.text > 10 &&
          visible.drawing > 5 &&
          visible.bitmap > 15;
      } while (!matched && performance.now() < deadline);
      images.push({
        label: `projected-${index}-${pose.shape}-${pose.curvature}`,
        width: VIEW[0],
        height: VIEW[1],
        pixels: [...new Uint8Array(frame.pixels)],
        sequence: frame.sequence,
      });
      check(
        matched,
        `Projected shell/control and nested Camera pixels did not match ${JSON.stringify(pose)}`,
      );
      check(session.view, "Projected scene has no view");
      input = await host.input.open(session.view);
      presses.length = 0;
      const point: Point = [target[0] / VIEW[0], target[1] / VIEW[1]];
      for (const kind of ["pointerDown", "pointerUp"] as const) {
        const routed = await input.send({ kind, pointer: 1n, point });
        check(
          routed.disposition === "routed",
          `${kind} missed independently projected rank-1 control`,
        );
      }
      const pressDeadline = performance.now() + 5000;
      while (presses.length === 0 && performance.now() < pressDeadline)
        await new Promise((resolve) => setTimeout(resolve, 16));
      check(
        presses.join() === "raised",
        `Wrong projected input target: ${presses}`,
      );
      presses.length = 0;
      const go = pixelFor(
        projectedSurfacePoint(
          pose,
          [button.x + button.width / 2, button.y + button.height / 2],
          1,
        ),
      );
      for (const kind of ["pointerDown", "pointerUp"] as const) {
        const routed = await input.send({
          kind,
          pointer: 2n,
          point: [go[0] / VIEW[0], go[1] / VIEW[1]],
        });
        check(
          routed.disposition === "routed",
          `${kind} missed complete kit button`,
        );
      }
      const goDeadline = performance.now() + 5000;
      while (presses.length === 0 && performance.now() < goDeadline)
        await new Promise((resolve) => setTimeout(resolve, 16));
      check(
        presses.join() === "kit",
        `GO callback was not routed through the complete component layer: ${presses}`,
      );
      const pixels = new Uint8Array(frame.pixels);
      for (const name of [
        "projected-kit-header",
        "projected-kit-footer",
        "projected-kit-header/separator",
        "projected-kit-footer/separator",
      ]) {
        const bounds = layout[name]!;
        let ink = 0;
        for (let y = 90; y < VIEW[1] - 15; y++)
          for (let x = 60; x < 260; x++) {
            const point = projectedSurfaceRay(pose, [x, y], 1);
            if (
              !point ||
              point[0] < bounds.x ||
              point[0] >= bounds.x + bounds.width ||
              point[1] < bounds.y ||
              point[1] >= bounds.y + bounds.height
            )
              continue;
            const offset = (y * VIEW[0] + x) * 4;
            if (
              Math.max(
                pixels[offset]!,
                pixels[offset + 1]!,
                pixels[offset + 2]!,
              ) > 100
            )
              ink++;
          }
        check(ink > 2, `${name} has no ink on its inherited physical rank`);
      }
      check(errors.length === 0, errors.map(String).join("; "));
    }
  } catch (error) {
    if (diagnosticFrame)
      images.push({
        label: "failed-projected-layout",
        width: VIEW[0],
        height: VIEW[1],
        pixels: [...new Uint8Array(diagnosticFrame.pixels)],
        sequence: diagnosticFrame.sequence,
      });
    failure = error instanceof Error ? error.message : String(error);
  } finally {
    await input?.close();
    await session.close();
    await client.close();
    await host.destroyWorld(world);
  }
  return {
    failure,
    images,
    assertions: [
      "independent signed cylinder/sphere chart and shell rays",
      "rank-1 routed controls, asymmetric assets and complete kit roots",
      "curved Camera output, both facing directions and zero-curvature limits",
    ],
  };
}
