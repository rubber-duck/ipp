import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { encodePng } from "../../../../tools/shared-host/png.js";
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
import { chartSheet, chartWorld, VALUES, EXTENT } from "./scene.js";
import { checkMaterialMatrix } from "./chart-material-oracle.js";
import { SHEET } from "./mapping.js";
interface Scene {
  world: Awaited<ReturnType<ClientContext["host"]["createWorld"]>>;
  client: Awaited<ReturnType<ClientContext["host"]["openWorld"]>>;
  root: ReactWorldRoot;
}
type State = {
  sheets: Scene[];
  bindings: RootBinding[];
  spatial: Scene;
  camera: CameraOutputReference;
  font: Uint8Array<ArrayBuffer>;
};
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
async function openWorld(
  context: ClientContext,
  name: string,
  spatial = false,
): Promise<Scene> {
  const world = await context.host.createWorld({
    symbolicId: `glass-chart/${context.name}/${name}`,
    temporary: true,
    canvas: { extent: [SHEET.width, SHEET.height], unitsPerMetre: 96 },
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
      ...(spatial ? [] : ["ipp.world-attachment", "ipp.surface"]),
    ],
  });
  const client = await context.host.openWorld(world.reference),
    root = createRoot(client, { host: context.host });
  return { world, client, root };
}
export default defineClient<State>({
  async open(context) {
    const font = await context.font(),
      spatial = await openWorld(context, "spatial", true);
    await spatial.root.render(
      chartWorld({
        dimension: 3,
        right: false,
        contrast: false,
        violet: false,
      }),
    );
    const camera = (await spatial.root.bindOutput(
      "camera",
      "camera",
    )) as CameraOutputReference;
    const sheets: Scene[] = [],
      bindings: RootBinding[] = [];
    for (const dimension of [2, 3] as const) {
      const scene = await openWorld(context, `sheet-${dimension}`);
      await scene.root.render(
        chartSheet(
          font,
          { dimension, right: false, contrast: false, violet: false },
          dimension === 3
            ? { world: spatial.world.reference, output: camera }
            : undefined,
        ),
      );
      sheets.push(scene);
      bindings.push(
        await context.host.setRootOutput(
          canvasOutput(scene.world.reference),
          EXTENT,
        ),
      );
    }
    return { font, spatial, camera, sheets, bindings };
  },
  async capture(state, context, args) {
    const options = {
      right: args.includes("right"),
      contrast: args.includes("contrast"),
      violet: args.includes("violet"),
    };
    const variants = args.includes("verify")
      ? [
          { right: false, contrast: false, violet: false },
          { right: true, contrast: false, violet: false },
          { right: false, contrast: true, violet: false },
          { right: false, contrast: false, violet: true },
        ]
      : [options];
    const images: Record<
      string,
      Awaited<ReturnType<ClientContext["capture"]>>
    > = {};
    for (const variant of variants) {
      await state.spatial.root.render(chartWorld({ dimension: 3, ...variant }));
      for (const [i, scene] of state.sheets.entries()) {
        const dimension = i === 0 ? 2 : 3;
        await scene.root.render(
          chartSheet(
            state.font,
            { dimension, ...variant },
            dimension === 3
              ? { world: state.spatial.world.reference, output: state.camera }
              : undefined,
          ),
        );
        await context.capture(state.bindings[i]!);
        await ready(scene.root, ["font", "bar-paint", "panel", "line"]);
        if (dimension === 3)
          await ready(state.spatial.root, [
            "bar-mesh",
            "halo",
            "backdrop-paint",
          ]);
        images[
          `bars-${dimension}d${variant.right ? "-right" : ""}${variant.violet ? "-violet" : ""}${variant.contrast ? "-contrast" : ""}`
        ] = await context.capture(state.bindings[i]!);
      }
    }
    let checks;
    if (args.includes("verify")) {
      try {
        checks = checkMaterialMatrix(images);
      } catch (error) {
        const directory = join(
          context.workspace,
          "target/shared-host-captures",
          context.name,
          "failure",
        );
        await mkdir(directory, { recursive: true });
        await Promise.all(
          Object.entries(images).map(([name, image]) =>
            writeFile(join(directory, `${name}.png`), encodePng(image)),
          ),
        );
        await writeFile(join(directory, "error.txt"), String(error));
        throw error;
      }
    }
    return {
      images,
      report: {
        options,
        values: VALUES,
        checks,
        scope:
          "Native Canvas and Camera material fixture with shared geometry mapping. Production Plot theme integration remains separate.",
      },
    };
  },
  async close(state, context) {
    for (const scene of [...state.sheets, state.spatial]) {
      await scene.root.unmount();
      await scene.client.close();
      await context.host.destroyWorld(scene.world.reference);
    }
  },
});
