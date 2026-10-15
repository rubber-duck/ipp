import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import assert from "node:assert/strict";
import {
  canvasOutput,
  type RootBinding,
  type CameraOutputReference,
} from "@ipp/client";
import { createRoot, type ReactWorldRoot } from "@ipp/react";
import {
  defineClient,
  type ClientContext,
} from "../../../../tools/shared-host/client.js";
import { encodePng } from "../../../../tools/shared-host/png.js";
import {
  sideBySide,
  blit,
  type RgbaImage,
} from "../../../../tools/shared-host/images.js";
import {
  BASE,
  EXTENT,
  STUDY,
  reflectionSheet,
  reflectionWorld,
  type Finish,
} from "./reflection-scene.js";
interface Scene {
  world: Awaited<ReturnType<ClientContext["host"]["createWorld"]>>;
  client: Awaited<ReturnType<ClientContext["host"]["openWorld"]>>;
  root: ReactWorldRoot;
}
interface State {
  sheets: Scene[];
  spatial: Scene;
  cameraBinding: RootBinding;
  bindings: RootBinding[];
  font: Uint8Array<ArrayBuffer>;
}
async function ready(root: ReactWorldRoot, names: string[]) {
  const deadline = Date.now() + 30000;
  for (const name of names) {
    while (root.getAsset(name)?.status !== "loaded") {
      if (root.getAsset(name)?.status === "failed" || Date.now() > deadline)
        throw new Error(`${name}: ${root.getAsset(name)?.status}`);
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
}
async function open(
  context: ClientContext,
  name: string,
  _spatial = false,
): Promise<Scene> {
  const world = await context.host.createWorld({
    symbolicId: `glass-reflections/${context.name}/${name}`,
    temporary: true,
    canvas: { extent: [STUDY.width, STUDY.height], unitsPerMetre: 96 },
    selectedSystems: [
      "ipp.hierarchy",
      "ipp.look-at",
      "ipp.final-propagation",
      "ipp.animation",
      "ipp.asset-dependencies",
      "ipp.geometry",
      "ipp.camera",
      "ipp.render",
      "ipp.canvas",
    ],
  });
  const client = await context.host.openWorld(world.reference),
    root = createRoot(client, { host: context.host });
  return { world, client, root };
}
function difference(a: RgbaImage, b: RgbaImage, rect: readonly number[]) {
  let sum = 0;
  const [x, y, w, h] = rect as [number, number, number, number];
  for (let dy = 0; dy < h; dy++)
    for (let dx = 0; dx < w; dx++) {
      const p = ((y + dy) * a.width + x + dx) * 4;
      for (let c = 0; c < 3; c++)
        sum += Math.abs(a.pixels[p + c]! - b.pixels[p + c]!);
    }
  return sum / (w * h * 3);
}
function verify(images: Record<string, RgbaImage>) {
  return [2, 3].map((d) => {
    const get = (name: string) => {
      const i = images[`${d}d-${name}`];
      assert(i, `${d}D ${name}`);
      assert.equal(i.width, 800);
      assert.equal(i.height, 1000);
      for (let p = 0; p < i.pixels.length; p += 4)
        assert(
          !(
            i.pixels[p]! > 220 &&
            i.pixels[p + 1]! < 20 &&
            i.pixels[p + 2]! < 20
          ),
          "No red fallback",
        );
      return i;
    };
    const base = get("current"),
      after = get("glass"),
      right = get("right"),
      contrast = get("contrast"),
      violet = get("violet");
    const face = d === 2 ? [324, 350, 140, 350] : [295, 410, 70, 220];
    const material = difference(base, after, face),
      moving = difference(after, right, face),
      opacity = difference(after, contrast, face),
      palette = difference(after, violet, face),
      backdrop = difference(after, contrast, [80, 250, 40, 60]);
    assert(material > 2, `Visible new finish: ${material}`);
    assert(moving > 1, `Light-responsive sheen: ${moving}`);
    assert(opacity < 0.1, `Opaque face: ${opacity}`);
    assert(palette > 20, `Palette response: ${palette}`);
    assert(backdrop > 35, `Backdrop changed: ${backdrop}`);
    return { dimension: d, material, moving, opacity, palette, backdrop };
  });
}
export default defineClient<State>({
  async open(context) {
    const font = await context.font(),
      spatial = await open(context, "spatial", true);
    await spatial.root.render(reflectionWorld(BASE));
    const camera = (await spatial.root.bindOutput(
      "camera",
      "camera",
    )) as CameraOutputReference;
    const cameraBinding = await context.host.setRootOutput(camera, {
      width: EXTENT.width,
      height: STUDY.viewHeight * 2,
      devicePixelRatio: 2,
    });
    const sheets: Scene[] = [],
      bindings: RootBinding[] = [];
    for (const dimension of [2, 3] as const) {
      const s = await open(context, `sheet-${dimension}`);
      await s.root.render(reflectionSheet(font, dimension, BASE));
      sheets.push(s);
      bindings.push(
        await context.host.setRootOutput(
          canvasOutput(s.world.reference),
          EXTENT,
        ),
      );
    }
    return { font, spatial, cameraBinding, sheets, bindings };
  },
  async capture(state, context, args) {
    const variants: { name: string; finish: Finish }[] = args.includes("motion")
      ? Array.from({ length: 9 }, (_, i) => ({
          name: `motion-${i}`,
          finish: { ...BASE, amount: 1, light: i / 8 },
        }))
      : args.includes("verify")
        ? [
            { name: "current", finish: BASE },
            { name: "glass", finish: { ...BASE, amount: 1 } },
            { name: "right", finish: { ...BASE, amount: 1, light: 1 } },
            {
              name: "contrast",
              finish: { ...BASE, amount: 1, contrast: true },
            },
            { name: "violet", finish: { ...BASE, amount: 1, violet: true } },
          ]
        : [
            { name: "current", finish: BASE },
            { name: "glass", finish: { ...BASE, amount: 1 } },
          ];
    const images: Record<string, RgbaImage> = {};
    for (const { name, finish } of variants) {
      await state.spatial.root.render(reflectionWorld(finish));
      for (const [i, scene] of state.sheets.entries()) {
        const dimension = i === 0 ? 2 : 3;
        await scene.root.render(reflectionSheet(state.font, dimension, finish));
        await context.capture(state.bindings[i]!);
        await ready(scene.root, ["font", "bar-paint", "panel"]);
        const frame = await context.capture(state.bindings[i]!);
        if (dimension === 3) {
          await context.capture(state.cameraBinding);
          await ready(state.spatial.root, [
            "bar-mesh",
            "floor",
            "backdrop-paint",
          ]);
          const cameraFrame = await context.capture(state.cameraBinding);
          blit(
            frame,
            cameraFrame,
            [0, 0, cameraFrame.width, cameraFrame.height],
            0,
            STUDY.viewY * 2,
          );
        }
        images[`${dimension}d-${name}`] = frame;
      }
    }
    let checks;
    try {
      if (args.includes("verify")) checks = verify(images);
    } catch (error) {
      const dir = join(
        context.workspace,
        "target/shared-host-captures",
        context.name,
        "failure",
      );
      await mkdir(dir, { recursive: true });
      await Promise.all(
        Object.entries(images).map(([name, i]) =>
          writeFile(join(dir, `${name}.png`), encodePng(i)),
        ),
      );
      throw error;
    }
    if (images["2d-current"] && images["2d-glass"]) {
      images["2d-comparison"] = sideBySide(
        [images["2d-current"]!, images["2d-glass"]!],
        16,
      );
      images["3d-comparison"] = sideBySide(
        [images["3d-current"]!, images["3d-glass"]!],
        16,
      );
    }
    return {
      images,
      report: {
        checks,
        scope:
          "Stylized procedural light bands and painted floor sheen; no reflected scene render or scene sampling.",
      },
    };
  },
  async close(state, context) {
    for (const s of [...state.sheets, state.spatial]) {
      await s.root.unmount();
      await s.client.close();
      await context.host.destroyWorld(s.world.reference);
    }
  },
});
