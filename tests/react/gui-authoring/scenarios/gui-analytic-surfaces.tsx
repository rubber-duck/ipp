/** Analytic curved coverage against sampled images and independent headless-free rays. */
import type {
  Client,
  HostClientBase,
  CanvasOutputReference,
  PresentedCapture,
} from "@ipp/client";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import {
  Asset,
  BoundingGeometry,
  Camera,
  CanvasWorld,
  CylinderSurface,
  Entity,
  SphereSurface,
  SurfaceCache,
  Transform,
  assetRef,
} from "@ipp/react";
import { Box, Drawing, Image, Style, Text } from "@ipp/react/gui";
import { CanvasWorldSession } from "@ipp/react/web";
import { check } from "../../../harness/page/checks.js";
import { entity } from "../pages/gui-authoring.js";
import type { GuiPaintAssets } from "../pages/gui-paint.js";
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
const poses: Pose[] = [
  { shape: "cylinder", curvature: 0.8, spacing: 0 },
  { shape: "sphere", curvature: -0.8, spacing: 0 },
  {
    shape: "cylinder",
    curvature: -0.8,
    spacing: 0,
    placement: { yaw: 1.15, scale: [1.1, 0.85, 1.15] },
  },
  {
    shape: "sphere",
    curvature: 0.8,
    spacing: 0,
    placement: { yaw: -0.65, scale: [0.95, 1.1, 0.85] },
  },
  {
    shape: "cylinder",
    curvature: 0.8,
    spacing: 0,
    placement: { yaw: Math.PI, scale: [1, 1, 1] },
  },
  {
    shape: "sphere",
    curvature: -0.8,
    spacing: 0,
    placement: { yaw: Math.PI, scale: [1, 1, 1] },
  },
];

export async function guiAnalyticSurfaces(
  host: HostClientBase<Client>,
  contract: Contract,
  assets: GuiPaintAssets,
) {
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
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
  let childOutput: CanvasOutputReference | undefined;
  const encode = (bytes: Uint8Array<ArrayBuffer>) => bytes;
  const blocker = contract.encodeBoundingShape({
    type: "box",
    min: [-0.95, -0.025, -0.14],
    max: [0.95, 0.025, -0.12],
  });
  const scene = (
    pose: Pose,
    reference: boolean,
    depth = false,
    low = false,
    view = { z: 2.5, near: 0.05, projection: 0 },
  ) => {
    const placement = pose.placement ?? { yaw: 0, scale: [1, 1, 1] };
    return (
      <>
        <Entity id="analytic-camera">
          <Transform z={view.z} />
          <Camera
            projection={view.projection}
            ortho_height={2}
            fov_y={Math.PI / 4}
            near={view.near}
            far={20}
          />
        </Entity>
        <Entity id="analytic-panel">
          <Transform
            qy={Math.sin(placement.yaw / 2)}
            qw={Math.cos(placement.yaw / 2)}
            sx={placement.scale[0]}
            sy={placement.scale[1]}
            sz={placement.scale[2]}
          />
          {pose.shape === "cylinder" ? (
            <CylinderSurface
              width={1.6}
              height={1.2}
              curvature={pose.curvature}
              layer_spacing={0}
            />
          ) : (
            <SphereSurface
              width={1.6}
              height={1.2}
              curvature={pose.curvature}
              layer_spacing={0}
            />
          )}
          {reference && (
            <SurfaceCache
              force_cache
              resolution_scale={4}
              max_texture_size={low ? 64 : 2048}
            />
          )}
        </Entity>
        {depth && (
          <Entity id="analytic-blocker">
            <BoundingGeometry
              geometry={blocker}
              is_rendered
              color={[1, 0, 0]}
            />
          </Entity>
        )}
        <CanvasWorld
          presentation={{ anchor: "analytic-panel" }}
          create={{ selectedSystems: CANVAS_SYSTEMS }}
          extent={[160, 120]}
          unitsPerMetre={100}
          onReady={(handle) => {
            childOutput = handle.output;
          }}
        >
          <Asset
            id="analytic-font"
            kind={17}
            data={assets.font}
            encode={encode}
          />
          <Asset
            id="analytic-drawing"
            kind={18}
            data={assets.drawing}
            encode={encode}
          />
          <Asset
            id="analytic-bitmap"
            kind={2}
            data={assets.bitmap}
            encode={encode}
          />
          <Entity id="analytic-background">
            <Style x={4} y={4} red={0.03} green={0.7} blue={0.08} />
            <Box width={152} height={112} />
          </Entity>
          <Entity id="analytic-overlay">
            <Style
              x={15}
              y={12}
              red={0.9}
              green={0.1}
              blue={0.3}
              alpha={0.45}
            />
            <Box width={55} height={24} radius_x={5} radius_y={5} />
          </Entity>
          <Entity id="analytic-text">
            <Style x={12} y={40} />
            <Text
              source={assetRef("analytic-font")}
              text="CURVE 0123"
              font_size={12}
            />
          </Entity>
          <Entity id="analytic-art">
            <Style x={90} y={12} scale_x={0.3} scale_y={0.3} />
            <Drawing source={assetRef("analytic-drawing")} />
          </Entity>
          <Entity id="analytic-wide-line">
            <Style x={10} y={72} red={1} green={1} blue={1} />
            <Box width={140} height={0.4} />
          </Entity>
          <Entity id="analytic-image">
            <Style x={120} y={15} />
            <Image
              source={assetRef("analytic-bitmap")}
              width={20}
              height={24}
            />
          </Entity>
          {Array.from({ length: 40 }, (_, i) => (
            <Entity key={i} id={`analytic-stripe-${i}`}>
              <Style x={10 + i * 3.4} y={80} red={1} green={1} blue={1} />
              <Box width={0.4} height={20} />
            </Entity>
          ))}
        </CanvasWorld>
      </>
    );
  };
  const remember = (label: string, frame: PresentedCapture) => {
    images.push({
      label,
      width: 400,
      height: 300,
      pixels: [...new Uint8Array(frame.pixels)],
      sequence: frame.sequence,
    });
  };
  const pixel = (frame: PresentedCapture, p: readonly number[]) => {
    const bytes = new Uint8Array(frame.pixels),
      x = Math.max(0, Math.min(399, Math.round(p[0]!))),
      y = Math.max(0, Math.min(299, Math.round(p[1]!)));
    return [...bytes.slice((y * 400 + x) * 4, (y * 400 + x) * 4 + 3)];
  };
  async function capture(label: string) {
    check(childOutput, "Analytic Canvas output missing");
    const observer = await host.openWorld(childOutput.world);
    try {
      const deadline = performance.now() + 15000;
      for (;;) {
        const frame = await session.capture({ afterOutputs: [childOutput] });
        const state = await observer.inspect();
        check(
          !state.resources.some((resource) => resource.status === "failed"),
          "Analytic assets failed",
        );
        if (
          state.resources.length >= 3 &&
          state.resources.every((resource) => resource.status === "loaded")
        ) {
          const ready = await session.capture({
            afterOutputs: [childOutput],
            afterSequence: frame.sequence,
          });
          remember(label, ready);
          return ready;
        }
        check(
          performance.now() < deadline,
          "Analytic assets did not become ready",
        );
      }
    } finally {
      await observer.close();
    }
  }
  let failure: string | null = null;
  let twoBranchDrawCalls = 0;
  try {
    for (const [i, pose] of poses.entries()) {
      await root.render(scene(pose, false));
      if (i === 0)
        await session.selectOutput(
          await host.bindOutput(
            world,
            (await entity(client, "analytic-camera")).id,
            "camera",
          ),
          { width: 400, height: 300, devicePixelRatio: 1 },
        );
      const direct = await capture(`analytic-${i}`);
      if (i === 0) twoBranchDrawCalls = direct.drawCalls;
      const probe = pixel(
        direct,
        pixelFor(projectedSurfacePoint(pose, [80, 65])),
      );
      check(
        probe[1]! > 150 && probe[1]! > probe[0]! + 60,
        `Analytic interior ${i}: ${probe}`,
      );
      const corners = [
        [10, 38],
        [100, 38],
        [10, 55],
        [100, 55],
      ].map((p) => pixelFor(projectedSurfacePoint(pose, [p[0]!, p[1]!])));
      let textPixels = 0;
      for (
        let y = Math.floor(Math.min(...corners.map((p) => p[1])));
        y <= Math.ceil(Math.max(...corners.map((p) => p[1])));
        y++
      )
        for (
          let x = Math.floor(Math.min(...corners.map((p) => p[0])));
          x <= Math.ceil(Math.max(...corners.map((p) => p[0])));
          x++
        )
          if (pixel(direct, [x, y]).every((c) => c > 225)) textPixels++;
      check(
        textPixels > 5,
        `Vector glyphs missing in analytic view ${i}: ${textPixels}`,
      );
      const before = await renderDiagnostics(host)?.statistics();
      check(
        before?.surfaces.surfaceCacheRepaints === 0,
        "Direct analytic presentation repainted an intermediate image",
      );
      await root.render(scene(pose, true));
      const reference = await capture(`texture-reference-${i}`);
      const a = new Uint8Array(direct.pixels),
        b = new Uint8Array(reference.pixels);
      let error = 0,
        ink = 0;
      for (let p = 0; p < a.length; p += 4) {
        for (let c = 0; c < 3; c++) error += Math.abs(a[p + c]! - b[p + c]!);
        if (a[p + 1]! > 100) ink++;
      }
      const mean = error / (400 * 300 * 3);
      observations.push({
        pose,
        mean,
        ink,
        textPixels,
        before: before?.surfaces,
      });
      check(ink > 1000, `Analytic surface ${i} missing coverage: ${ink}`);
      check(mean < 3, `Analytic/reference disagreement ${i}: ${mean}`);
    }
    const pose = poses[0]!;
    await root.render(scene(pose, false, true));
    const depth = await capture("analytic-depth");
    const centre = pixel(
      depth,
      pixelFor(projectedSurfacePoint(pose, [80, 60])),
    );
    const edge = pixel(depth, pixelFor(projectedSurfacePoint(pose, [145, 60])));
    check(
      centre[1]! > centre[0]! + 60,
      `Centre should occlude blocker: ${centre}`,
    );
    check(
      edge[0]! > edge[1]! + 60,
      `Blocker should occlude curved edge: ${edge}`,
    );
    await root.render(scene(pose, false));
    const sharp = await capture("analytic-fine-lines");
    await root.render(scene(pose, true, false, true));
    const low = await capture("texture-low-resolution");
    const contrast = (frame: PresentedCapture) => {
      let result = 0;
      for (let y = 190; y < 205; y++)
        for (let x = 110; x < 290; x++)
          result += Math.abs(
            pixel(frame, [x, y])[0]! - pixel(frame, [x - 1, y])[0]!,
          );
      return result;
    };
    const quality = { direct: contrast(sharp), low: contrast(low) };
    observations.push({ quality });
    check(
      quality.direct > quality.low * 1.2,
      `Analytic fine detail did not exceed capped texture: ${JSON.stringify(quality)}`,
    );
    const variants = [
      {
        label: "inside-cylinder",
        pose: { shape: "cylinder", curvature: -0.8, spacing: 0 } as Pose,
        view: { z: 1.25, near: 0.05, projection: 0 },
      },
      {
        label: "inside-sphere",
        pose: { shape: "sphere", curvature: -0.8, spacing: 0 } as Pose,
        view: { z: 1.25, near: 0.05, projection: 0 },
      },
      {
        label: "orthographic",
        pose: poses[1]!,
        view: { z: 2.5, near: 0.05, projection: 1 },
      },
      {
        label: "cylinder-flat-limit",
        pose: { shape: "cylinder", curvature: 1e-32, spacing: 0 } as Pose,
      },
      {
        label: "sphere-flat-limit",
        pose: { shape: "sphere", curvature: -1e-6, spacing: 0 } as Pose,
      },
      {
        label: "wrapped-cylinder",
        pose: {
          shape: "cylinder",
          curvature: 3.5,
          spacing: 0,
          placement: { yaw: 1.5, scale: [1, 1, 1] },
        } as Pose,
      },
      {
        label: "negative-wrapped-cylinder",
        pose: {
          shape: "cylinder",
          curvature: -3.5,
          spacing: 0,
          placement: { yaw: -1.5, scale: [1.1, 0.8, 0.9] },
        } as Pose,
      },
      {
        label: "cylinder-near-plane",
        pose: { shape: "cylinder", curvature: 2.8, spacing: 0 } as Pose,
        view: { z: 0.3, near: 0.35, projection: 0 },
      },
      {
        label: "wrapped-sphere",
        pose: {
          shape: "sphere",
          curvature: 2.8,
          spacing: 0,
          placement: { yaw: 1.2, scale: [1, 1, 1] },
        } as Pose,
      },
      {
        label: "near-plane",
        pose: { shape: "sphere", curvature: 2.8, spacing: 0 } as Pose,
        view: { z: 0.3, near: 0.35, projection: 0 },
      },
    ];
    for (const variant of variants) {
      await root.render(scene(variant.pose, false, false, false, variant.view));
      const direct = await capture(`analytic-${variant.label}`);
      if (variant.label.startsWith("inside-"))
        check(
          direct.drawCalls < twoBranchDrawCalls * 0.75,
          `Inside view still submitted both branches: ${direct.drawCalls} / ${twoBranchDrawCalls}`,
        );
      await root.render(scene(variant.pose, true, false, false, variant.view));
      const reference = await capture(`reference-${variant.label}`);
      const a = new Uint8Array(direct.pixels),
        b = new Uint8Array(reference.pixels);
      let error = 0,
        ink = 0;
      for (let p = 0; p < a.length; p += 4) {
        for (let c = 0; c < 3; c++) error += Math.abs(a[p + c]! - b[p + c]!);
        if (a[p + 1]! > 100) ink++;
      }
      const mean = error / (400 * 300 * 3);
      observations.push({ variant: variant.label, mean, ink });
      check(ink > 100, `Missing analytic coverage for ${variant.label}`);
      check(
        mean < 5,
        `Analytic/reference disagreement for ${variant.label}: ${mean}`,
      );
      if (variant.label === "near-plane") {
        check(
          pixel(direct, [200, 150])[1]! < 50,
          "Near plane did not clip the front centre",
        );
        let checked = 0;
        for (let x = 10; x < 390; x += 5)
          for (let y = 140; y < 160; y += 5) {
            const content = projectedSurfaceRay(
              variant.pose,
              [x, y],
              0,
              variant.view,
            );
            if (
              content &&
              content[0] > 6 &&
              content[0] < 154 &&
              content[1] > 56 &&
              content[1] < 76
            ) {
              const colour = pixel(direct, [x, y]);
              check(
                colour[1]! > colour[0]! + 60,
                `Near-plane ray disagrees at ${x},${y}: ${colour}`,
              );
              checked++;
            }
          }
        observations.push({ nearPlaneProbes: checked });
        check(
          checked > 10,
          "Near-plane oracle did not exercise visible intersections",
        );
      }
    }
    check(errors.length === 0, errors.map(String).join("\n"));
  } catch (error) {
    failure =
      error instanceof Error ? (error.stack ?? error.message) : String(error);
  } finally {
    await session.close();
    await client.close();
    await host.destroyWorld(world);
  }
  return { failure, images, observations };
}
