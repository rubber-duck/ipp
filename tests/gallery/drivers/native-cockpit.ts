/** Cockpit controls through the composed application and real native input. */
import assert from "node:assert/strict";
import { renderDiagnostics } from "@ipp/client/diagnostics";
import { nativePresentationTransport } from "@ipp/client/testing";
import { exerciseCockpitSpatial } from "../scenarios/cockpit-spatial.js";
import { exerciseCockpitQuality } from "../scenarios/cockpit-quality.js";
import { setTimeout } from "node:timers/promises";
import type {
  Client,
  WorldReference,
  PresentationView,
  GuiPhysicalInput,
} from "@ipp/client";
import {
  defineClient,
  type ClientContext,
} from "../../../tools/shared-host/client.js";
import { componentFields, successfulBatch } from "../../fixtures/commands.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import { image } from "../../../tools/shared-host/presentation.js";
import {
  exerciseCockpit,
  exerciseCockpitTracking,
} from "../scenarios/cockpit.js";
import {
  exerciseCockpitWheel,
  type CockpitWheelDriver,
} from "../scenarios/cockpit-wheel.js";
import {
  cockpitFields as fields,
  rotate,
  surfaceSample,
} from "../cockpit-oracle.js";

interface State {
  readonly gallery: NativeGallerySession;
  readonly root: Client;
  readonly panels: readonly Client[];
  readonly viewport: typeof VIEWPORT;
  readonly diagnosticsHost?: import("@ipp/host-contract").IppHostClient;
}

const VIEWPORT = { width: 1600, height: 900, devicePixelRatio: 1 };

function driver(
  state: State,
  context: ClientContext,
  images: Record<string, RgbaImage>,
  view: PresentationView,
  input: Awaited<ReturnType<ClientContext["host"]["input"]["open"]>>,
): CockpitWheelDriver {
  const app = state.gallery.application;
  const viewport = state.viewport;
  const binding = () => view.binding;
  const frame = async () => {
    await app.flush();
    return context.host.presentation.frame(view, {
      afterOutputs: app.frameOutputs,
    });
  };
  const capture = async () => {
    await app.flush();
    const completed = await context.host.presentation.capture(view, {
      afterOutputs: app.frameOutputs,
    });
    assert.equal(
      completed.failedDrawCalls,
      0,
      "cockpit capture has no failed draws",
    );
    return image(completed);
  };
  const send = async (command: GuiPhysicalInput) => {
    const result = await input.send(command);
    await app.handlePhysicalInput(command, result);
    return result;
  };
  const components = app.sceneClient!.components;
  const componentIds = Object.fromEntries(
    Object.entries(components).map(([name, value]) => [name, value!.id]),
  );
  const point = async (
    panel: number | "shell",
    id: string,
    fraction: number,
  ) => {
    const client = panel === "shell" ? state.root : state.panels[panel]!;
    const header = id.startsWith("cockpit-header-");
    const symbol = header ? `cockpit-grip-${panel}` : id;
    // Native pointer-up can finish before its React callback mounts new tab
    // content. Observe the resulting layout before targeting the next control.
    let snapshot = await client.inspect();
    const deadline = performance.now() + 15_000;
    for (;;) {
      const target = snapshot.entities.find(
        (entity) => entity.metadata.symbolicId === symbol,
      );
      const bounds = target?.components.find(
        (component) =>
          component.component === client.components.CanvasBounds!.id,
      )?.fields;
      if (bounds && Number(bounds.width) > 0 && Number(bounds.height) > 0)
        break;
      assert.ok(
        performance.now() < deadline,
        `${symbol} has a completed layout`,
      );
      await client.waitForFrame(snapshot.tick);
      snapshot = await client.inspect();
    }
    const rect = fields(snapshot, symbol, client.components.CanvasBounds!.id);
    const x = Number(rect.x) + Number(rect.width) * fraction;
    const targetHeight = header
      ? fields(snapshot, id, client.components.GuiLayout!.id).height
      : rect.height;
    const y = Number(rect.y) + Number(targetHeight) * 0.5;
    const rootExtent = (await state.root.inspectPage({ collection: "canvas" }))
      .canvas?.state.extent;
    assert.ok(rootExtent, "application Canvas has a logical extent");
    if (panel === "shell")
      return [x / rootExtent[0], y / rootExtent[1]] as const;
    const canvas = (await client.inspectPage({ collection: "canvas" })).canvas
      ?.state;
    assert.ok(canvas);
    const scene = await app.sceneClient!.inspect();
    const world = surfaceSample(
      scene,
      componentIds,
      panel,
      x / canvas.extent[0],
      y / canvas.extent[1],
    ).position;
    const camera = fields(scene, "cockpit-camera", components.Transform!.id);
    const projection = fields(scene, "cockpit-camera", components.Camera!.id);
    const view = rotate(
      world.map(
        (value, index) => value - Number(camera[["x", "y", "z"][index]!]),
      ),
      camera,
      true,
    );
    const height = Math.tan(Number(projection.fov_y) / 2) * -view[2]!;
    assert.ok(height > 0, `${id} lies ahead of the fixed Camera`);
    const shell = await app.inspectApplication();
    const anchor = shell.shell.entities.find(
      (entity) => entity.id === shell.cameraPath[0]?.anchor,
    );
    const placement = anchor?.components.find(
      (component) => component.component === components.CanvasStyle!.id,
    )?.fields;
    const surface = anchor?.components.find(
      (component) => component.component === components.FlatSurface!.id,
    )?.fields;
    assert.ok(placement && surface, "scene Camera Surface exists");
    const surfaceWidth = Number(surface.width);
    const surfaceHeight = Number(surface.height);
    return [
      (Number(placement.x) +
        surfaceWidth *
          (0.5 + view[0]! / ((height * 2 * surfaceWidth) / surfaceHeight))) /
        rootExtent[0],
      (Number(placement.y) + surfaceHeight * (0.5 - view[1]! / (height * 2))) /
        rootExtent[1],
    ] as const;
  };
  const contentLayouts = new Map<number, Record<string, number>>();
  return {
    async cameraViewport() {
      const shell = await app.inspectApplication();
      const anchor = shell.shell.entities.find(
        (entity) => entity.id === shell.cameraPath[0]?.anchor,
      );
      const placement = anchor?.components.find(
        (component) => component.component === components.CanvasStyle!.id,
      )?.fields;
      const surface = anchor?.components.find(
        (component) => component.component === components.FlatSurface!.id,
      )?.fields;
      const extent = (await state.root.inspectPage({ collection: "canvas" }))
        .canvas?.state.extent;
      assert.ok(placement && surface && extent, "scene Camera slot exists");
      return [
        Number(placement.x) / extent[0],
        Number(placement.y) / extent[1],
        Number(surface.width) / extent[0],
        Number(surface.height) / extent[1],
      ] as const;
    },
    async contentHeight(panel, height) {
      const client = state.panels[panel]!;
      const snapshot = await client.inspect();
      const id = `cockpit-content-${panel}`;
      const entity = snapshot.entities.find(
        (value) => value.metadata.symbolicId === id,
      )!;
      const current = fields(snapshot, id, components.GuiLayout!.id);
      if (!contentLayouts.has(panel))
        contentLayouts.set(
          panel,
          Object.fromEntries(
            ["height", "max_height", "flex"].map((name) => [
              name,
              Number(current[name]),
            ]),
          ),
        );
      const values =
        height === null
          ? contentLayouts.get(panel)!
          : { height, max_height: height, flex: 0 };
      successfulBatch(
        await client.batch(
          componentFields(client, "GuiLayout", values).map((field) => ({
            kind: "setField",
            entity: { kind: "handle", id: entity.id },
            component: components.GuiLayout!.id,
            field,
          })),
        ),
      );
      await frame();
    },
    async history() {
      const snapshot = await state.panels[1]!.inspect();
      const chart = snapshot.entities.find(
        (value) => value.metadata.symbolicId === "cockpit-fps-chart",
      )!;
      const source = chart.components
        .flatMap((component) => Object.values(component.fields))
        .find(
          (value) =>
            typeof value === "string" &&
            value.startsWith("datasets://demo-frame-rate/"),
        );
      assert.equal(
        typeof source,
        "string",
        "FPS chart binds the real presentation history",
      );
      const result = await context.host.datasets.read(String(source));
      let page = result;
      while (page.nextOffset !== null) {
        page = await context.host.datasets.read(String(source), {
          incarnation: result.incarnation,
          offset: page.nextOffset,
        });
        result.rows.push(...page.rows);
      }
      return { ...result, nextOffset: null, memory: page.memory };
    },
    async text(panel, id, value, submit = false) {
      await this.press(panel, id);
      const expected = (await state.panels[panel]!.inspect()).entities.find(
        (entity) => entity.metadata.symbolicId === id,
      )!.id;
      const wait = async (text?: string) => {
        const deadline = performance.now() + 10000;
        while (
          input.nativeText?.fence.target.entity !== expected ||
          (text !== undefined && input.nativeText.text !== text)
        ) {
          assert.ok(
            performance.now() < deadline,
            `native text ${id} reaches ${String(text)}`,
          );
          await frame();
        }
        assert.ok(input.nativeText);
        return input.nativeText;
      };
      const focused = await wait();
      await input.editText(focused.fence, { kind: "key", key: "selectAll" });
      assert.ok(input.nativeText);
      await input.editText(input.nativeText.fence, {
        kind: "text",
        text: value,
      });
      await wait(value);
      if (submit) await send({ kind: "key", key: "enter" });
      await frame();
    },
    async wheel(panel, id, delta, fraction = 0.5, count = 1) {
      await frame();
      const target =
        panel === "background"
          ? ([0.98, 0.98] as const)
          : await point(panel, id, fraction);
      assert.ok(
        target.every((value) => value >= 0 && value <= 1),
        `${id || "background"} wheel point lies inside the viewport`,
      );
      const initial = await app.sceneClient!.inspect();
      const rotorId = initial.entities.find(
        (entity) => entity.metadata.symbolicId === "cockpit-carousel",
      )!.id;
      const rotation = initial.controllers!.find((controller) =>
        controller.description.drivers.some((driver) =>
          driver.source.includes("cockpit-carousel-rotation"),
        ),
      )!;
      const screen = initial.entities.find(
        (entity) => entity.metadata.symbolicId === "cockpit-screen-0",
      )!;
      const surface = screen.components.find(
        (component) =>
          component.component === components.CylinderSurface!.id ||
          component.component === components.FlatSurface!.id,
      )!;
      const step =
        (Number(surface.fields.width) + 0.07) / Number(app.options.projection);
      const observe = async () => {
        const [snapshot, controllers] = await Promise.all([
          app.sceneClient!.inspectPage({
            collection: "entities",
            after: rotorId - 1n,
            limit: 1,
          }),
          app.sceneClient!.inspectPage({
            collection: "controllers",
            after: rotation.id - 1n,
            limit: 1,
          }),
        ]);
        const pose = fields(
          snapshot,
          "cockpit-carousel",
          components.Transform!.id,
        );
        return {
          pose: (2 * Math.atan2(Number(pose.qy), Number(pose.qw))) / step,
          target: Number(app.options.carousel),
          elapsed: controllers.controllers?.[0]?.transition?.elapsed,
          tick: snapshot.tick.toString(),
        };
      };
      const expected =
        panel === "background"
          ? undefined
          : (
              await (panel === "shell"
                ? state.root
                : state.panels[panel]!
              ).inspect()
            ).entities.find(
              (entity) =>
                entity.metadata.symbolicId ===
                (id.startsWith("cockpit-header-")
                  ? `cockpit-grip-${panel}`
                  : id),
            );
      assert.ok(
        panel === "background" || expected,
        `${id} native target exists`,
      );
      const admissionMs: number[] = [];
      let completed = false;
      const pendingOutcomes = Promise.all(
        Array.from({ length: count }, async () => {
          const command = { kind: "wheel" as const, point: target, delta };
          const outcome = await input.send(command);
          if (expected)
            assert.equal(
              outcome.hit?.target.entity,
              expected.id,
              `${id} wheel resolves its fresh exact native control`,
            );
          assert.equal(outcome.rejected, 0, "wheel physical input is accepted");
          const began = performance.now();
          const pending = app.handlePhysicalInput(command, outcome);
          admissionMs.push(performance.now() - began);
          await pending;
          return {
            disposition: outcome.disposition,
            applied: outcome.applied,
            rejected: outcome.rejected,
            cancelled: outcome.cancelled,
            ...(outcome.remaining ? { remaining: outcome.remaining } : {}),
            ...(outcome.error ? { error: outcome.error } : {}),
          };
        }),
      ).finally(() => {
        completed = true;
      });
      void pendingOutcomes.catch(() => {});
      const samples = [];
      if (count > 1) {
        const deadline = performance.now() + 5000;
        do {
          const sample = await observe();
          samples.push(sample);
          if (completed && sample.elapsed === undefined) break;
          assert.ok(
            performance.now() < deadline,
            "wheel tracking settles on the Host clock",
          );
        } while (true);
      }
      const outcomes = await pendingOutcomes;
      await setTimeout(300);
      await frame();
      const deadline = performance.now() + 10_000;
      while (
        (
          await app.sceneClient!.inspectPage({ collection: "controllers" })
        ).controllers?.some(
          (controller) =>
            controller.transition &&
            controller.description.drivers.some((driver) =>
              driver.source.includes("cockpit-carousel-rotation"),
            ),
        )
      ) {
        assert.ok(performance.now() < deadline, "native wheel easing settles");
        await context.host.presentation.frame(view);
      }
      const retainedController = (
        await app.sceneClient!.inspectPage({ collection: "controllers" })
      ).controllers!.find((controller) =>
        controller.description.drivers.some((driver) =>
          driver.source.includes("cockpit-carousel-rotation"),
        ),
      )!.id;
      return {
        outcomes,
        admissionMs,
        samples,
        controller: rotation.id.toString(),
        retainedController: retainedController.toString(),
        viewport: [viewport.width, viewport.height],
      };
    },
    async tracking() {
      await frame();
      const start = await point(1, "cockpit-header-1", 0.5);
      const initial = await app.sceneClient!.inspect();
      const rotor = (snapshot: typeof initial) =>
        fields(snapshot, "cockpit-carousel", components.Transform!.id);
      const initialPose = rotor(initial);
      const step =
        2 * Math.atan2(Number(initialPose.qy), Number(initialPose.qw));
      const rotorId = initial.entities.find(
        (entity) => entity.metadata.symbolicId === "cockpit-carousel",
      )!.id;
      const controllerId = initial.controllers!.find((controller) =>
        controller.description.drivers.some((driver) =>
          driver.source.includes("cockpit-carousel-rotation"),
        ),
      )!.id;
      const observe = async () => {
        // Page only the moving entity/controller; full scene inspection can
        // consume the entire short tracking interval on a software renderer.
        const [snapshot, controllers] = await Promise.all([
          app.sceneClient!.inspectPage({
            collection: "entities",
            after: rotorId - 1n,
            limit: 1,
          }),
          app.sceneClient!.inspectPage({
            collection: "controllers",
            after: controllerId - 1n,
            limit: 1,
          }),
        ]);
        const pose = rotor(snapshot);
        const transition = controllers.controllers?.find((controller) =>
          controller.description.drivers.some((driver) =>
            driver.source.includes("cockpit-carousel-rotation"),
          ),
        )?.transition;
        return {
          pose: (2 * Math.atan2(Number(pose.qy), Number(pose.qw))) / step,
          tick: snapshot.tick.toString(),
          time: snapshot.time,
          elapsed: transition?.elapsed,
          duration: transition?.duration,
        };
      };
      await send({ kind: "pointerMove", pointer: 3n, point: start });
      await send({ kind: "pointerDown", pointer: 3n, point: start });
      const samples = [];
      const handlerMs: number[] = [];
      for (let batch = 0; batch < 3; batch++) {
        const from = Number(app.options.carousel);
        const began = performance.now();
        let completed = false;
        let capturedInterior = false;
        const pending = Promise.all(
          Array.from({ length: 4 }, async (_, offset) => {
            const command = {
              kind: "pointerMove" as const,
              pointer: 3n,
              point: [
                start[0] - (batch * 4 + offset + 1) * 0.015,
                start[1],
              ] as const,
            };
            const outcome = await input.send(command);
            assert.equal(
              outcome.rejected,
              0,
              "held physical input is accepted",
            );
            const beganHandler = performance.now();
            await app.handlePhysicalInput(command, outcome);
            handlerMs.push(performance.now() - beganHandler);
          }),
        ).finally(() => {
          completed = true;
        });
        // Real held input arrives as a burst. Read actual evaluated poses while
        // its ordered submissions and native transitions are in progress.
        do {
          const observed = await observe();
          if (
            !capturedInterior &&
            observed.pose > from + 0.0001 &&
            observed.pose < Number(app.options.carousel) - 0.0001 &&
            observed.elapsed !== undefined
          ) {
            capturedInterior = true;
            // Capture completes near this observation; the sample owns its exact tick.
            images[`cockpit-held-interior-${batch}`] = image(
              await context.host.presentation.capture(view),
            );
          }
          samples.push({
            from,
            target: Number(app.options.carousel),
            observationMs: performance.now() - began,
            ...observed,
          });
        } while (!completed);
        await pending;
        for (let frameIndex = 0; frameIndex < 3; frameIndex++) {
          await context.host.presentation.frame(view);
          samples.push({
            from,
            target: Number(app.options.carousel),
            observationMs: performance.now() - began,
            ...(await observe()),
          });
        }
        if (batch < 2)
          images[`cockpit-held-${batch}`] = image(
            await context.host.presentation.capture(view),
          );
      }
      const target = Number(app.options.carousel);
      // Completed Host frames settle the held target; no client clock is advanced.
      for (let index = 0; index < 8; index++)
        await context.host.presentation.frame(view);
      const converged = (await observe()).pose;
      await send({
        kind: "pointerUp",
        pointer: 3n,
        point: [start[0] - 0.18, start[1]],
      });
      await setTimeout(300);
      await frame();
      const released = (await observe()).pose;
      const retainedController = (
        await app.sceneClient!.inspectPage({
          collection: "controllers",
          after: controllerId - 1n,
          limit: 1,
        })
      ).controllers?.[0]?.id.toString();
      await app.update({ carousel: 1 });
      await setTimeout(300);
      await frame();
      return {
        samples,
        handlerMs,
        viewport: [viewport.width, viewport.height] as const,
        controller: controllerId.toString(),
        retainedController,
        target,
        converged,
        released,
      };
    },
    async state() {
      return {
        options: app.options,
        navigation: app.navigationMode,
        scene: await app.sceneClient!.inspect(),
        panels: await Promise.all(state.panels.map((panel) => panel.inspect())),
        panelWorlds: state.panels.map((panel) => {
          assert.ok(panel.worldReference);
          return panel.worldReference;
        }),
        componentIds,
      };
    },
    async press(panel, id, fraction = 0.5) {
      await frame();
      const target = await point(panel, id, fraction);
      assert.ok(
        target.every((value) => value >= 0 && value <= 1),
        `${id} is inside the viewport: ${target}`,
      );
      const client = panel === "shell" ? state.root : state.panels[panel]!;
      const snapshot = await client.inspect();
      const expected = snapshot.entities.find(
        (entity) => entity.metadata.symbolicId === id,
      )!.id;
      await send({ kind: "pointerMove", pointer: 1n, point: target });
      const feedback = await client.inspectPage({
        collection: "guiPointers",
      });
      assert.ok(
        feedback.guiPointers?.some(
          (record) => record.target.entity === expected && record.state.hovered,
        ),
        `${id} receives native hover at ${target}`,
      );
      const down = await send({
        kind: "pointerDown",
        pointer: 1n,
        point: target,
      });
      const up = await send({
        kind: "pointerUp",
        pointer: 1n,
        point: target,
      });
      assert.equal(down.disposition, "routed");
      assert.equal(up.rejected, 0);
      await frame();
    },
    async options(patch) {
      await app.update(patch);
      await frame();
    },
    async drag(panel, id, distance, fast = false) {
      await frame();
      const start = await point(panel, id, 0.5);
      const snapshots = [];
      const pending: Promise<void>[] = [];
      const route = async (command: GuiPhysicalInput) => {
        const outcome = await input.send(command);
        assert.equal(outcome.rejected, 0);
        const work = app.handlePhysicalInput(command, outcome);
        // Match the adapter admission barrier: native capture must be qualified
        // before later raw input can release it. Moves and up may then overlap.
        if (fast && command.kind !== "pointerDown") pending.push(work);
        else await work;
      };
      await route({ kind: "pointerMove", pointer: 2n, point: start });
      await route({ kind: "pointerDown", pointer: 2n, point: start });
      for (let step = 1; step <= 4; step++) {
        await route({
          kind: "pointerMove",
          pointer: 2n,
          point: [start[0] + (distance * step) / 4, start[1]],
        });
        if (!fast) {
          await frame();
          snapshots.push(await this.state());
          if (step === 2)
            await this.capture(
              `cockpit-drag-${id.replaceAll("/", "-")}-${distance > 0 ? "right" : "left"}`,
            );
        }
      }
      await route({
        kind: "pointerUp",
        pointer: 2n,
        point: [start[0] + distance, start[1]],
      });
      await Promise.all(pending);
      await frame();
      return snapshots;
    },
    async capture(name) {
      const result = await capture();
      images[name] = result;
      return result;
    },
    action: (name, value) => app.action(name, value),
    async advance(seconds) {
      // Waiting observes the Host clock; it does not step the simulation.
      await setTimeout(seconds * 1000);
      await frame();
    },
    navigate: () =>
      app.navigateCamera({ kind: "rotate", yaw: 0.3, pitch: 0.1 }, binding()),
  };
}

export default defineClient<State>({
  async open(context, args) {
    const diagnosticsIndex = args.indexOf("diagnostics");
    const diagnosticsTransport =
      diagnosticsIndex < 0
        ? undefined
        : nativePresentationTransport(
            args[diagnosticsIndex + 1]!,
            args[diagnosticsIndex + 2]!,
          );
    const diagnosticsHost = diagnosticsTransport
      ? await (
          context.contract
            .IppHostClient as typeof import("@ipp/host-contract").IppHostClient
        ).connectTransport(diagnosticsTransport)
      : undefined;
    const viewport = args.includes("hidpi")
      ? { width: 1920, height: 1080, devicePixelRatio: 2 }
      : args.includes("ultrawide")
        ? { width: 2048, height: 640, devicePixelRatio: 1 }
        : args.includes("portrait")
          ? { width: 800, height: 1200, devicePixelRatio: 1 }
          : args.includes("narrow")
            ? { width: 960, height: 640, devicePixelRatio: 1 }
            : VIEWPORT;
    const gallery = new NativeGallerySession(
      context,
      "cockpit",
      "examples/demos/scene-registry.ts",
      args.find((argument) => argument.startsWith("/")) ?? context.assetRoot,
      viewport,
      {},
    );
    const panels: Client[] = [];
    let root: Client | undefined;
    try {
      await gallery.open();
      const controller = gallery.application.controller as {
        panels: readonly { index: number; world: WorldReference }[];
      };
      for (const panel of [...controller.panels].sort(
        (a, b) => a.index - b.index,
      ))
        panels.push(await context.host.openWorld(panel.world));
      root = await context.host.openWorld(gallery.application.output.world);
      return {
        gallery,
        root,
        panels,
        viewport,
        ...(diagnosticsHost ? { diagnosticsHost } : {}),
      };
    } catch (error) {
      await Promise.all(panels.map((panel) => panel.close()));
      await root?.close();
      await gallery.close();
      await diagnosticsHost?.close();
      throw error;
    }
  },
  async capture(state, context, args) {
    const images: Record<string, RgbaImage> = {};
    const binding = state.gallery.application.sceneCanvas?.view?.binding;
    assert.ok(binding);
    // Popups and focus belong to one input context. Keep it through complete
    // interactions, as the production adapter does; capture within this view.
    return state.gallery.present(async (view) => {
      const input = await context.host.input.open(view);
      try {
        const subject = driver(state, context, images, view, input);
        let failure: string | undefined;
        let carouselEvidence: unknown;
        try {
          if (args[0] === "spatial") {
            carouselEvidence = await exerciseCockpitSpatial(subject);
          } else if (args[0] === "quality") {
            carouselEvidence = await exerciseCockpitQuality({
              ...subject,
              statistics: () =>
                renderDiagnostics(state.diagnosticsHost!)!.statistics(),
            });
          } else if (args[0] === "wheel")
            carouselEvidence = await exerciseCockpitWheel(subject);
          else if (args[0] === "tracking")
            carouselEvidence = await exerciseCockpitTracking(subject);
          else if (
            args[0] === "exercise" ||
            args[0] === "carousel" ||
            args[0] === "exterior" ||
            args[0] === "forms"
          )
            carouselEvidence = await exerciseCockpit(
              subject,
              args[0] === "exercise" ? "presentation" : args[0],
            );
          else await subject.capture("cockpit-glass");
        } catch (error) {
          failure = error instanceof Error ? error.stack : String(error);
        }
        if (failure) await subject.capture("failure");
        const snapshot = await subject.state();
        return {
          images,
          report: {
            failure,
            carouselEvidence,
            frames: Object.keys(images),
            options: snapshot.options,
            diagnostics: [snapshot.scene, ...snapshot.panels].flatMap(
              (world) => world.renderDiagnostics,
            ),
          },
        };
      } finally {
        await input.close();
      }
    });
  },
  async close(state, context) {
    const worlds = new Set([
      state.gallery.application.output.world.id,
      state.gallery.application.sceneOutput?.world.id,
      ...state.panels.map((panel) => panel.worldReference?.id),
    ]);
    await Promise.all([
      state.root.close(),
      ...state.panels.map((panel) => panel.close()),
    ]);
    await state.gallery.close();
    await state.diagnosticsHost?.close();
    assert.ok(
      (await context.host.listWorlds()).every((world) => !worlds.has(world.id)),
      "cockpit releases its shell, scene and all six Canvas Worlds",
    );
  },
});
