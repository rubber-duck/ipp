import { createRoot } from "@ipp/react";
import { canvasOutput, type Client, type HostClientBase } from "@ipp/client";
import { check } from "../../../harness/page/checks.js";
import type { Present } from "../../skin-lab/specimen-session.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import {
  openOpticalContext,
  captureOpticalContext,
  closeOpticalContext,
} from "../optical-material/context-session.js";
import { image, settled } from "../../../../tools/shared-host/presentation.js";
import {
  contextScene,
  contextSamples,
} from "../optical-material/context-scene.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";

function base64(bytes: Uint8Array) {
  let value = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    value += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(value);
}

function encoded(value: number) {
  return Math.round(
    255 *
      (value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055),
  );
}
function directionProbe(
  frame: RgbaImage,
  x: number,
  y: number,
  direction: readonly number[],
  label: string,
) {
  const index = (y * frame.width + x) * 4;
  const expected = direction.map((v) => encoded(v * 0.5 + 0.5));
  const actual = Array.from(frame.pixels.slice(index, index + 3));
  check(
    expected.every((value, i) => Math.abs(value - actual[i]!) < 7),
    `${label}: rendered direction ${actual}, expected ${expected}`,
  );
  return { label, actual, expected };
}

export async function exerciseOpticalContext(
  host: HostClientBase<Client>,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const session = await openOpticalContext(
    host,
    font,
    "glass/presentation-context",
  );
  const evidence: object[] = [];
  const save = async (name: string, frame: RgbaImage) =>
    environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: base64(frame.pixels),
    });
  try {
    await present(session.binding, async (view) => {
      const continuous: Present = async (_binding, action) => action(view);
      for (const shape of ["flat", "cylinder", "sphere"] as const) {
        const front = await captureOpticalContext(session, continuous, {
          shape,
          probe: 1,
          cache: true,
        });
        await save(`${shape}-view-front`, front);
        evidence.push(
          directionProbe(front, 550, 350, [0, 0, 1], `${shape} frontal centre`),
        );
        const cache = session.statistics!.surfaces.surfaceCaches;
        check(
          cache.some(
            (item) =>
              item.width > 0 &&
              item.height > 0 &&
              ["reused", "repainted", "animated"].includes(item.mode),
          ),
          `${shape}: test must exercise an actual image cache`,
        );
        if (shape !== "flat") {
          // On a cylinder/sphere through its central horizontal section, the
          // front normal's x component is curvature * world x. Projection is
          // orthographic: world x = (pixelX-centre)*orthoHeight/viewportHeight.
          const nx = 0.35 * (((760 - 550) * 4) / 700);
          evidence.push(
            directionProbe(
              front,
              760,
              350,
              [-nx, 0, Math.sqrt(1 - nx * nx)],
              `${shape} curved normal`,
            ),
          );
        }
        const angle = 0.65;
        const turned = await captureOpticalContext(session, continuous, {
          shape,
          probe: 1,
          cache: true,
          angle,
        });
        await save(`${shape}-view-turned`, turned);
        evidence.push(
          directionProbe(
            turned,
            550,
            350,
            [Math.sin(angle), 0, Math.cos(angle)],
            `${shape} camera-only refresh`,
          ),
        );
        const left = await captureOpticalContext(session, continuous, {
          shape,
          probe: 2,
          cache: true,
          light: -3,
        });
        const rebuilds = session.statistics!.gui.totalGuiRebuilds;
        const right = await captureOpticalContext(session, continuous, {
          shape,
          probe: 2,
          cache: true,
          light: 3,
        });
        await save(`${shape}-light-right`, right);
        check(
          session.statistics!.gui.totalGuiRebuilds === rebuilds,
          `${shape}: light changes must reuse local GUI geometry (${rebuilds} -> ${session.statistics!.gui.totalGuiRebuilds}); ${JSON.stringify(session.statistics!.surfaces.surfaceCaches, (_, v) => (typeof v === "bigint" ? String(v) : v))}`,
        );
        const distance = Math.sqrt(43);
        evidence.push(
          directionProbe(
            left,
            550,
            350,
            [-3 / distance, -3 / distance, 5 / distance],
            `${shape} scene light left`,
          ),
        );
        evidence.push(
          directionProbe(
            right,
            550,
            350,
            [3 / distance, -3 / distance, 5 / distance],
            `${shape} light-only refresh`,
          ),
        );
      }
      const direct = await captureOpticalContext(session, continuous, {
        probe: 1,
        angle: 0.65,
      });
      evidence.push(
        directionProbe(
          direct,
          550,
          350,
          [Math.sin(0.65), 0, Math.cos(0.65)],
          "direct flat camera",
        ),
      );
      const rear = await captureOpticalContext(session, continuous, {
        probe: 1,
        angle: Math.PI,
      });
      evidence.push(
        directionProbe(
          rear,
          550,
          350,
          [0, 0, -1],
          "rear view retains signed front normal",
        ),
      );
      for (const shape of ["flat", "cylinder", "sphere"] as const) {
        const layered = await captureOpticalContext(session, continuous, {
          shape,
          probe: 2,
          layer: 2,
          spacing: 0.2,
        });
        const d = Math.hypot(3, 3, 4.6);
        evidence.push(
          directionProbe(
            layered,
            550,
            350,
            [-3 / d, -3 / d, 4.6 / d],
            `${shape} separated layer light position`,
          ),
        );
      }
      const radiance = await captureOpticalContext(session, continuous, {
        probe: 3,
        intensity: 10,
      });
      const linear = ((10 * (1 - (Math.sqrt(43) / 30) ** 4)) / 43) * 0.1;
      evidence.push(
        directionProbe(
          radiance,
          550,
          350,
          [linear * 2 - 1, linear * 2 - 1, linear * 2 - 1],
          "point light attenuation",
        ),
      );

      const transform = session.sceneClient.components.Transform!;
      await session.canvasRoot.render(contextSamples(font, { probe: 2 }));
      session.samples = "[2,0]";
      await session.sceneRoot.render(
        contextScene(session.canvas.reference, {
          motion: {
            component: transform.id,
            offset: transform.fields.x!.offset,
          },
        }),
      );
      const deadline = Date.now() + 30000;
      while (session.sceneRoot.getAsset("light-motion")?.status !== "loaded") {
        check(Date.now() < deadline, "native light animation asset ready");
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
      const first = await host.presentation.capture(view, {
        afterOutputs: [
          session.binding.output,
          canvasOutput(session.canvas.reference),
        ],
      });
      let changed = false;
      const index = (350 * 1100 + 550) * 4;
      for (let i = 0; i < 8 && !changed; i++) {
        await new Promise((resolve) => setTimeout(resolve, 150));
        const next = await host.presentation.capture(view, {
          afterSequence: first.sequence,
        });
        changed =
          Math.abs(
            new Uint8Array(next.pixels)[index]! -
              new Uint8Array(first.pixels)[index]!,
          ) > 3;
      }
      check(
        changed,
        "native scene-light animation changes the rendered light direction",
      );
      evidence.push({ nativeLightAnimation: true });
    });
    const rootWorld = await host.createWorld({
      symbolicId: "optical-root",
      temporary: true,
      selectedSystems: ["ipp.canvas", "ipp.asset-dependencies"],
      canvas: { extent: [1100, 480], unitsPerMetre: 200 },
    });
    const rootClient = await host.openWorld(rootWorld.reference);
    const rootTree = createRoot(rootClient, { host });
    try {
      await rootTree.render(contextSamples(font, { probe: 1 }));
      const root = await host.setRootOutput(canvasOutput(rootWorld.reference), {
        width: 1100,
        height: 480,
        devicePixelRatio: 1,
      });
      const rootFrame = await present(root, async (view) =>
        image(await settled(host, view, [root.output])),
      );
      evidence.push(
        directionProbe(
          rootFrame,
          550,
          240,
          [0, 0, 1],
          "root canvas frontal default",
        ),
      );
      await rootTree.render(contextSamples(font, { probe: 3 }));
      const unlit = await present(root, async (view) =>
        image(await settled(host, view, [root.output])),
      );
      evidence.push(
        directionProbe(
          unlit,
          550,
          240,
          [-1, -1, -1],
          "root canvas has no inherited scene lights",
        ),
      );
    } finally {
      await rootTree.unmount();
      await rootClient.close();
      await host.destroyWorld(rootWorld.reference);
    }
    await save(
      "optical-context-flat",
      await captureOpticalContext(session, present, {}),
    );
    await save(
      "optical-context-turned",
      await captureOpticalContext(session, present, {
        angle: 0.45,
        light: 1.5,
      }),
    );
    environment.record({ opticalContext: evidence, sceneReflectionPasses: 0 });
  } finally {
    await closeOpticalContext(session);
  }
}
