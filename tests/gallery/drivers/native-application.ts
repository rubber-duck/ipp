/** Application lifecycle through the maintained native application adapter. */
import assert from "node:assert/strict";
import {
  assertFrameRateData,
  assertFrameRatePixels,
} from "../scenarios/frame-rate.js";
import { setTimeout } from "node:timers/promises";
import type {
  PresentationView,
  GuiPhysicalContext,
  PresentationViewport,
} from "@ipp/client";
import { image } from "../../../tools/shared-host/presentation.js";
import {
  exerciseApplicationLayout,
  type ApplicationLayoutDriver,
} from "../scenarios/application-layout.js";
import { defineClient } from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import { crop, type RgbaImage } from "../../../tools/shared-host/images.js";
import {
  exerciseApplication,
  type ApplicationDriver,
} from "../scenarios/application.js";

export default defineClient<NativeGallerySession>({
  async open(context, args) {
    const session = new NativeGallerySession(
      context,
      "shapes",
      "tests/gallery/fixtures/application-scenes.ts",
      args.find((argument) => argument.startsWith("/")) ?? context.assetRoot,
      {
        width: args[0] === "layout" ? Number(args[1]) : 960,
        height: args[0] === "layout" ? Number(args[2]) : 640,
        devicePixelRatio: 1,
      },
      {},
    );
    try {
      await session.open();
      return session;
    } catch (error) {
      await session.close();
      throw error;
    }
  },
  async capture(session, context, args) {
    const app = session.application;
    const retired = app.sceneCanvas!;
    const images: Record<string, RgbaImage> = {};
    const layoutEvidence: unknown[] = [];
    const bounds = async (id: string) => {
      const state = await app.inspectApplication();
      const root = await context.host.openWorld(state.cameraPath[0]!.parent);
      try {
        const entity = state.shell.entities.find(
          (entity) => entity.metadata.symbolicId === id,
        );
        const fields = entity?.components.find(
          (component) =>
            component.component === root.components.CanvasBounds?.id,
        )?.fields;
        assert.ok(fields, `${id} has evaluated Canvas bounds`);
        return ["x", "y", "width", "height"].map((name) =>
          Number(fields[name]),
        );
      } finally {
        await root.close();
      }
    };
    let held: { view: PresentationView; input: GuiPhysicalContext } | undefined;
    const interact = async (section: () => Promise<void>) => {
      await session.capture();
      await session.present(async (view) => {
        const input = await context.host.input.open(view);
        held = { view, input };
        try {
          await section();
        } finally {
          held = undefined;
          await input.close();
        }
      });
    };
    const driver: ApplicationDriver & ApplicationLayoutDriver = {
      interact,
      async layout(ids) {
        await app.flush();
        const root = await context.host.openWorld(app.output.world);
        try {
          const before = await root.inspect();
          const boundsId = root.components.CanvasBounds!.id;
          const inserted = await root.batch(
            ids
              .filter(
                (name) =>
                  !before.entities
                    .find((entity) => entity.metadata.symbolicId === name)
                    ?.components.some(
                      (component) => component.component === boundsId,
                    ),
              )
              .map((symbol) => ({
                kind: "insertComponent",
                entity: { kind: "symbol", symbol },
                component: boundsId,
                fields: [],
              })),
          );
          assert.ok(
            inserted.ok,
            "ordinary observation components expose evaluated container bounds",
          );
          await root.waitForFrame(before.tick);
          const snapshot = await root.inspect();
          const rects: Record<
            string,
            { x: number; y: number; width: number; height: number }
          > = {};
          const layouts: Record<string, Record<string, unknown>> = {};
          const scrollbars: Record<string, Record<string, unknown>> = {};
          const skins: Record<string, string> = {};
          for (const id of ids) {
            const entity = snapshot.entities.find(
              (entity) => entity.metadata.symbolicId === id,
            );
            assert.ok(entity, id);
            const bounds = entity.components.find(
              (component) => component.component === boundsId,
            )?.fields;
            assert.ok(bounds, id);
            rects[id] = {
              x: Number(bounds.x),
              y: Number(bounds.y),
              width: Number(bounds.width),
              height: Number(bounds.height),
            };
            scrollbars[id] =
              entity.components.find(
                (component) =>
                  component.component === root.components.GuiScrollView!.id,
              )?.fields ?? {};
            const theme = entity.components.find(
              (component) =>
                component.component === root.components.GuiSkin!.id,
            )?.fields.theme;
            skins[id] =
              snapshot.entities.find((candidate) => candidate.id === theme)
                ?.metadata.symbolicId ?? "";
            layouts[id] =
              entity.components.find(
                (component) =>
                  component.component === root.components.GuiLayout!.id,
              )?.fields ?? {};
          }
          const viewport = app.sceneCanvas!.viewport!;
          layoutEvidence.push({
            skin: app.options.skin,
            ids,
            rects,
            layouts,
            scrollbars,
            skins,
          });
          return {
            viewport: [
              viewport.width / viewport.devicePixelRatio,
              viewport.height / viewport.devicePixelRatio,
            ] as const,
            rects,
            layouts,
            scrollbars,
            skins,
          };
        } finally {
          await root.close();
        }
      },
      async state() {
        const state = await app.inspectApplication();
        return {
          ...state,
          root: state.cameraPath[0]!.parent,
          worlds: (await context.host.listWorlds())
            .filter(
              (world) =>
                world.id === state.cameraPath[0]!.parent.id ||
                world.symbolicId.startsWith(
                  `demo/${state.cameraPath[0]!.parent.incarnation}/${state.cameraPath[0]!.parent.id}/`,
                ),
            )
            .map((world) => world.id),
          toolbar: await bounds("demo-scene-picker"),
        };
      },
      async press(...ids) {
        const press = async () => {
          assert.ok(held);
          for (const id of ids) {
            const rect = await bounds(id);
            const viewport: PresentationViewport = held.view.binding.viewport;
            const point: readonly [number, number] = [
              ((rect[0]! + rect[2]! / 2) * viewport.devicePixelRatio) /
                viewport.width,
              ((rect[1]! + rect[3]! / 2) * viewport.devicePixelRatio) /
                viewport.height,
            ] as const;
            await held.input.send({ kind: "pointerMove", pointer: 1n, point });
            const down = await held.input.send({
              kind: "pointerDown",
              pointer: 1n,
              point,
            });
            const up = await held.input.send({
              kind: "pointerUp",
              pointer: 1n,
              point,
            });
            assert.equal(
              down.disposition,
              "routed",
              `${id} receives physical pointer input at ${point}`,
            );
            assert.equal(up.rejected, 0);
            await context.host.presentation.frame(held.view, {
              afterOutputs: app.frameOutputs,
            });
          }
        };
        if (held) await press();
        else {
          await interact(press);
          await session.capture();
        }
      },
      async scroll(id, delta) {
        await interact(async () => {
          assert.ok(held);
          const before = await this.layout([id]);
          const rect = before.rects[id]!;
          const scroll = before.scrollbars[id]!;
          const thickness = Number(scroll.bar_thickness);
          assert.ok(thickness > 0, `${id} exposes an actual scrollbar gutter`);
          const viewport = held.view.binding.viewport;
          // Value controls own wheel input even at their bounds. Use the
          // evaluated scrollbar gutter so scrolling never edits a form field.
          const point = [
            ((rect.x + rect.width - Number(scroll.bar_inset) - thickness / 2) *
              viewport.devicePixelRatio) /
              viewport.width,
            ((rect.y + rect.height / 2) * viewport.devicePixelRatio) /
              viewport.height,
          ] as const;
          const outcome = await held.input.send({
            kind: "wheel",
            point,
            delta: [0, delta],
          });
          assert.equal(
            outcome.disposition,
            "routed",
            "controls content receives real wheel input",
          );
          const shell = (await app.inspectApplication()).shell;
          const target = shell.entities.find(
            (entity) => entity.metadata.symbolicId === id,
          );
          assert.ok(target, `${id} exists in the application Canvas`);
          assert.equal(
            outcome.hit?.target.entity,
            target.id,
            `${id} owns wheel input in its scrollbar gutter`,
          );
          assert.equal(outcome.rejected, 0);
          await context.host.presentation.frame(held.view, {
            afterOutputs: app.frameOutputs,
          });
          const after = await this.layout([id]);
          const expected = Math.max(
            0,
            Math.min(
              Number(scroll.capacity_y),
              Number(scroll.offset_y) + delta,
            ),
          );
          assert.ok(
            Math.abs(Number(after.scrollbars[id]!.offset_y) - expected) < 0.1,
            `${id} reaches its requested scroll offset ${expected}`,
          );
        });
      },
      action: (name, value) => app.action(name, value),
      async capture(name) {
        const frame = held
          ? image(
              await context.host.presentation.capture(held.view, {
                afterOutputs: app.frameOutputs,
              }),
            )
          : (await session.capture()).images!.shapes!;
        images[name] = frame;
        if (args[0] === "layout" && name.endsWith("-toolbar")) {
          const rect = await bounds("demo-toolbar-panel");
          images[`${name}-crop`] = crop(frame, [
            Math.floor(rect[0]!),
            Math.floor(rect[1]!),
            Math.ceil(rect[2]!),
            Math.ceil(rect[3]!),
          ]);
        }
        return frame;
      },
      async waitForOption(name, value) {
        await app.flush();
        for (let count = 0; count < 300; count++) {
          if (
            app.options[name] === value &&
            !(await app.inspectApplication()).preparing
          ) {
            // Scene options precede their acknowledged declarations and shell
            // notification. Observe that queue before measuring the new form.
            await app.inspect();
            await app.flush();
            return;
          }
          await setTimeout(10);
        }
        assert.equal(app.options[name], value);
      },
      wait: (milliseconds) => setTimeout(milliseconds),
      retiredCapture: () => retired.capture(),
    };
    let failure: string | undefined;
    try {
      if (args[0] === "layout") await exerciseApplicationLayout(driver);
      else {
        await interact(async () => {
          await app.action("performance", true);
          const deadline = performance.now() + 15000;
          let history = (await app.inspectApplication()).frameRate;
          while (
            (history?.rows.length ?? 0) < 3 &&
            performance.now() < deadline
          ) {
            await setTimeout(100);
            history = (await app.inspectApplication()).frameRate;
          }
          assertFrameRateData(history);
          assertFrameRatePixels(
            await driver.capture("fps-history"),
            await bounds("demo-frame-rate-chart"),
          );
          await app.action("performance", false);
          assert.equal((await app.inspectApplication()).frameRate, undefined);
          await assert.rejects(context.host.datasets.read(history!.name));
        });
        await exerciseApplication(driver);
      }
    } catch (error) {
      failure = error instanceof Error ? error.stack : String(error);
      await driver.capture("failure").catch(() => {});
    }
    return {
      images,
      report: {
        failure,
        frames: Object.keys(images),
        layoutEvidence,
        state: await driver.state(),
      },
    };
  },
  close: (session) => session.close(),
});
