import assert from "node:assert/strict";
import { defineClient } from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import { capturedImage } from "../../harness/page/presentation.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import {
  exerciseChartInput,
  type ChartInputDriver,
} from "../scenarios/chart-input.js";
import type { GalleryChartsState } from "../scenarios/gallery-charts.js";
import { exerciseControlsDropdown } from "../scenarios/controls-dropdown.js";
import { exerciseParticleLifetime } from "../scenarios/particle-lifetime.js";

export default defineClient<{ gallery: NativeGallerySession; kind: string }>({
  async open(context, args) {
    const kind = args[0] ?? "chart-input";
    const scene = kind === "chart-input" ? "charts" : "particles";
    const gallery = new NativeGallerySession(
      context,
      scene,
      "examples/demos/scene-registry.ts",
      args[1] ?? context.assetRoot,
      { width: 720, height: 480, devicePixelRatio: 1 },
      {
        skin: "cybertron",
        ...(scene === "particles" ? { emitting: false } : {}),
      },
    );
    try {
      await gallery.open();
      return { gallery, kind };
    } catch (error) {
      await gallery.close();
      throw error;
    }
  },
  async capture({ gallery, kind }, context) {
    const app = gallery.application;
    const images: Record<string, RgbaImage> = {};
    const evidence: Record<string, unknown> = {};
    const capture = async (name: string) => {
      const frame = await app.sceneCanvas!.capture();
      const image = capturedImage(frame);
      const rgba = { ...image, pixels: new Uint8Array(image.pixels) };
      images[name] = rgba;
      return { image: rgba, draws: frame.drawCalls };
    };
    let failure: string | undefined;
    try {
      if (kind === "controls-dropdown") {
        const root = await context.host.openWorld(app.output.world);
        try {
          await capture("controls-initial");
          await context.present(
            app.sceneCanvas!.view!.binding,
            async (view) => {
              // Keep the input context alive for the entire overlay interaction.
              // Closing it releases its focus and dismisses a light overlay.
              const input = await context.host.input.open(view);
              try {
                await exerciseControlsDropdown({
                  async press(id) {
                    const state = await root.inspect();
                    const entity = state.entities.find(
                      (entity) => entity.metadata.symbolicId === id,
                    );
                    assert.ok(entity, id);
                    const rect = entity.components.find(
                      (component) =>
                        component.component ===
                        root.components.CanvasBounds!.id,
                    )?.fields;
                    assert.ok(rect);
                    const point = [
                      (Number(rect.x) + Number(rect.width) / 2) / 720,
                      (Number(rect.y) + Number(rect.height) / 2) / 480,
                    ] as const;
                    await input.send({
                      kind: "pointerMove",
                      pointer: 1n,
                      point,
                    });
                    const feedback = await root.inspectPage({
                      collection: "guiPointers",
                    });
                    assert.ok(
                      feedback.guiPointers?.some(
                        (record) =>
                          record.target.entity === entity.id &&
                          record.state.hovered,
                      ),
                      `${id} receives real hover at its CanvasBounds center`,
                    );
                    const down = await input.send({
                      kind: "pointerDown",
                      pointer: 1n,
                      point,
                    });
                    const up = await input.send({
                      kind: "pointerUp",
                      pointer: 1n,
                      point,
                    });
                    assert.equal(down.disposition, "routed");
                    assert.equal(up.rejected, 0);
                    evidence[id] = { rect, point, feedback, down, up };
                    await app.flush();
                    await context.host.presentation.frame(view, {
                      afterOutputs: app.frameOutputs,
                    });
                  },
                  async state() {
                    const snapshot = await root.inspect();
                    const overlay = snapshot.entities.find(
                      (entity) =>
                        entity.metadata.symbolicId ===
                        "particles-presentation/list",
                    )!;
                    const behavior = overlay.components.find(
                      (component) =>
                        component.component === root.components.GuiBehavior!.id,
                    )!.fields;
                    return {
                      presentation: app.options.presentation,
                      open: behavior.visible === true,
                    };
                  },
                  async capture(name) {
                    const frame = await context.host.presentation.capture(
                      view,
                      { afterOutputs: app.frameOutputs },
                    );
                    const image = capturedImage(frame);
                    const rgba = {
                      ...image,
                      pixels: new Uint8Array(image.pixels),
                    };
                    images[name] = rgba;
                    return rgba;
                  },
                });
              } finally {
                await input.close();
              }
            },
          );
        } finally {
          await root.close();
        }
      } else if (kind === "particles") {
        await exerciseParticleLifetime({
          async state() {
            return {
              options: app.options,
              world: app.sceneClient!.worldReference!,
              scene: await app.sceneClient!.inspect(),
              worlds: (await app.sceneCanvas!.host.listWorlds()).map(
                (world) => world.id,
              ),
            };
          },
          action: (name, args) => app.action(name, args),
          update: (options) => app.update(options),
          capture,
          async advance(seconds) {
            const initial = await app.sceneClient!.inspect();
            for (;;) {
              const frame = await app.sceneClient!.waitForFrame();
              if (frame.time >= initial.time + seconds) return;
            }
          },
        });
      } else {
        const driver: ChartInputDriver = {
          inspect: async () =>
            (await app.scene!.inspect()) as GalleryChartsState,
          action: (name, args) => app.scene!.action(name, args),
          capture: async (name) => (await capture(name)).image,
          record: async (name, value) => {
            evidence[name] = value;
          },
          async burst(actions) {
            let inspections = 0,
              notifications = 0;
            const client = app.sceneClient!;
            const original = client.inspect;
            client.inspect = (...args) => {
              inspections++;
              return original.apply(client, args);
            };
            const unsubscribe = app.scene!.subscribe?.(() => {
              notifications++;
            });
            try {
              await Promise.all(
                actions.map(({ name, args }) => app.scene!.action(name, args)),
              );
            } finally {
              client.inspect = original;
              unsubscribe?.();
            }
            return { inspections, notifications };
          },
        };
        await exerciseChartInput(driver, 720 / 480);
      }
    } catch (error) {
      failure = error instanceof Error ? error.stack : String(error);
      await capture("failure").catch(() => {});
    }
    return {
      images,
      report: { failure, frames: Object.keys(images), evidence },
    };
  },
  async close({ gallery }, context) {
    const root = gallery.application.output.world;
    await gallery.close();
    assert.ok(
      (await context.host.listWorlds()).every(
        (world) =>
          world.id !== root.id &&
          !world.symbolicId.startsWith(`demo/${root.incarnation}/${root.id}/`),
      ),
      "behavior scenarios release all their owned Worlds",
    );
  },
});
