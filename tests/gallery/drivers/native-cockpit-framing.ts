import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import type { CockpitRuntime } from "../../../examples/demos/scenes/cockpit/runtime.js";
import { assertCockpitLightTextures } from "../scenarios/cockpit-light-textures.js";
import type { ClientContext } from "../../../tools/shared-host/client.js";
import type { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import { crop, type RgbaImage } from "../../../tools/shared-host/images.js";
import { image } from "../../../tools/shared-host/presentation.js";
import { insertComponent, successfulBatch } from "../../fixtures/commands.js";
import { cockpitFields } from "../cockpit-oracle.js";
import { exerciseCockpitFraming } from "../scenarios/cockpit-framing.js";

/** Real camera captures isolate bridge framing from animated GUI and shell chrome. */
export async function captureCockpitFraming(
  gallery: NativeGallerySession,
  context: ClientContext,
) {
  const app = gallery.application;
  await app.update({
    lightsPlaying: false,
    atmospherePlaying: false,
    scanning: false,
  });
  await app.flush();
  const scene = app.sceneClient!;
  const snapshot = await scene.inspect();
  const carousel = snapshot.entities.find(
    (entity) => entity.metadata.symbolicId === "cockpit-carousel",
  )!;
  assert.ok(
    !carousel.components.some(
      (component) =>
        component.component === scene.components.PresentationGate!.id,
    ),
  );
  successfulBatch(
    await scene.batch([
      insertComponent(
        scene,
        "PresentationGate",
        { kind: "handle", id: carousel.id },
        { open: false },
      ),
    ]),
  );
  const images: Record<string, RgbaImage> = {};
  let evidence: unknown;
  let failure: string | undefined;
  const initialProjection = cockpitFields(
    snapshot,
    "cockpit-camera",
    scene.components.Camera!.id,
  );
  const initialPose = cockpitFields(
    snapshot,
    "cockpit-camera",
    scene.components.Transform!.id,
  );
  const initialShell = await app.inspectApplication();
  const initialAnchor = initialShell.shell.entities.find(
    (entity) => entity.id === initialShell.cameraPath[0]?.anchor,
  )!;
  const chromeHeight = Number(
    initialAnchor.components.find(
      (component) => component.component === scene.components.CanvasStyle!.id,
    )!.fields.y,
  );
  try {
    const assetDirectory = join(
      context.assetRoot,
      "demos-cockpit-native-assets",
    );
    const runtime = JSON.parse(
      await readFile(join(assetDirectory, "manifest.json"), "utf8"),
    ) as CockpitRuntime;
    await assertCockpitLightTextures(runtime, (source) =>
      readFile(join(assetDirectory, source.replace(/^\/assets\//, ""))),
    );
    evidence = await exerciseCockpitFraming({
      async tint(value) {
        successfulBatch(
          await scene.batch(
            snapshot.entities.flatMap((entity) => {
              const material = entity.components.find(
                (component) => component.properties?.cabinTint,
              );
              if (!material) return [];
              return ["cabinTint", "dashboardTint", "buttonsTint", "dialsTint"]
                .filter((name) => material.properties?.[name])
                .map((name) => ({
                  kind: "setDynamicProperty" as const,
                  entity: { kind: "handle" as const, id: entity.id },
                  component: scene.components.CustomMaterial!.id,
                  name,
                  value: { kind: "vec3" as const, value },
                }));
            }),
          ),
        );
      },
      async illumination(value) {
        await app.update({ illumination: value });
        await app.flush();
      },
      async capture(width, height, name) {
        const viewport = {
          width,
          height: height + Math.round(chromeHeight),
          devicePixelRatio: 1,
        };
        await gallery.resize(viewport);
        await app.flush();
        const current = await scene.inspect();
        assert.deepEqual(
          cockpitFields(current, "cockpit-camera", scene.components.Camera!.id),
          initialProjection,
          "landscape resize preserves vertical field of view",
        );
        assert.deepEqual(
          cockpitFields(
            current,
            "cockpit-camera",
            scene.components.Transform!.id,
          ),
          initialPose,
          "resize never moves or stretches the camera",
        );
        const output = app.sceneOutput!;
        return gallery.present(async (view) => {
          await context.host.presentation.frame(view, {
            afterOutputs: [output],
          });
          const capture = await context.host.presentation.capture(view, {
            afterOutputs: [output],
          });
          assert.equal(capture.failedDrawCalls, 0);
          const shell = await app.inspectApplication();
          const anchor = shell.shell.entities.find(
            (entity) => entity.id === shell.cameraPath[0]?.anchor,
          )!;
          const placement = anchor.components.find(
            (component) =>
              component.component === scene.components.CanvasStyle!.id,
          )!.fields;
          const extent = anchor.components.find(
            (component) =>
              component.component === scene.components.FlatSurface!.id,
          )!.fields;
          assert.ok(Math.abs(Number(extent.width) - width) <= 1);
          assert.ok(Math.abs(Number(extent.height) - height) <= 1);
          // The toolbar can occupy fractional pixels. Compare the same whole-
          // pixel interior in both views, excluding its partially covered edge.
          return (images[name] = crop(image(capture), [
            Math.ceil(Number(placement.x)),
            Math.ceil(Number(placement.y)),
            width,
            height,
          ]));
        });
      },
    });
  } catch (error) {
    failure = error instanceof Error ? error.stack : String(error);
  } finally {
    successfulBatch(
      await scene.batch([
        {
          kind: "removeComponent",
          entity: { kind: "handle", id: carousel.id },
          component: scene.components.PresentationGate!.id,
        },
      ]),
    );
  }
  return { images, report: { failure, evidence, frames: Object.keys(images) } };
}
