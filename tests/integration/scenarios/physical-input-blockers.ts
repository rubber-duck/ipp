import type {
  Client,
  GuiWorldClient,
  PickingWorldClient,
  HostClientBase,
  GuiPhysicalContext,
  GuiPickingBlocker,
  GuiInputRoutingOutcome,
  RowsInput,
} from "@ipp/client";
import { controlState } from "./gui-lifecycle.js";
import { attachCanvasGuiInput } from "../../../packages/ipp-react/src/gui/input.js";
import { createGuiUnhandledInputGate } from "../../../packages/ipp-react/src/gui/scene-input.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import {
  ATTACHMENTS,
  LIFECYCLE,
  CAMERA,
  SURFACE,
  GUI,
  selectSystems,
} from "../system-selections.js";

interface Contract {
  encodeBoundingShape(shape: {
    type: "box";
    min: [number, number, number];
    max: [number, number, number];
  }): Uint8Array<ArrayBuffer>;
  GuiTheme: { encodeParts(input: RowsInput): Uint8Array<ArrayBuffer> };
  guiPaintPartIndex(input: { part: "background" }): number;
}

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** The caller drives real DOM mouse events; this fixture records only their actual terminals. */
export async function preparePhysicalBlockers(
  host: HostClientBase<Client>,
  contract: Contract,
  canvas: HTMLCanvasElement,
) {
  const cameraWorld = (
    await host.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, CAMERA, SURFACE, LIFECYCLE),
    })
  ).reference;
  const panelWorld = (
    await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      canvas: { extent: [96, 64], unitsPerMetre: 1 },
    })
  ).reference;
  const camera = (await host.openWorld(cameraWorld)) as Client &
    PickingWorldClient;
  const panel = (await host.openWorld(panelWorld)) as Client & GuiWorldClient;
  const alias = (value: number) => ({ kind: "alias" as const, alias: value });
  const panelCreated = successfulBatch(
    await panel.batch([
      createEntity(1, "blocked-panel"),
      createEntity(3, "blocked-theme"),
      {
        kind: "insertComponent",
        entity: alias(3),
        component: panel.components.GuiTheme!.id,
        fields: [
          {
            offset: panel.components.GuiTheme!.fields.parts!.offset,
            value: {
              kind: "rows",
              value: contract.GuiTheme.encodeParts({
                nextSlot: 1,
                rows: new Map([
                  [
                    0,
                    // A plain red box: no line and none of the default
                    // look's corner cuts, so the tested ray meets its fill.
                    {
                      part: contract.guiPaintPartIndex({ part: "background" }),
                      color: [1, 0, 0, 1],
                      corner_radius: [0, 0],
                      corner_cut: [0, 0, 0, 0],
                      border_width: 0,
                    },
                  ],
                ]),
              }),
            },
          },
        ],
      },
      createEntity(2, "blocked-button"),
      insertComponent(panel, "GuiButton", alias(2)),
      insertComponent(panel, "GuiLayout", alias(2), { width: 96, height: 64 }),
      {
        kind: "placeEntity",
        entity: alias(2),
        placement: { parent: alias(1), before: null },
      },
    ]),
  );
  const entity = { kind: "handle" as const, id: aliasId(panelCreated, 2) };
  successfulBatch(
    await panel.batch([
      insertComponent(panel, "GuiSkin", entity, {
        theme: aliasId(panelCreated, 3),
      }),
    ]),
  );
  const geometry = contract.encodeBoundingShape({
    type: "box",
    min: [-0.4, -0.4, -0.1],
    max: [0.4, 0.4, 0.1],
  });
  const created = successfulBatch(
    await camera.batch([
      createEntity(1, "physical-camera"),
      insertComponent(camera, "Camera", alias(1), {
        projection: 1,
        ortho_height: 4,
      }),
      insertComponent(camera, "Transform", alias(1), { z: 5 }),
      createEntity(2, "panel-anchor"),
      insertComponent(camera, "Surface", alias(2), { width: 2, height: 2 }),
      {
        kind: "insertComponent",
        entity: alias(2),
        component: camera.components.WorldAttachment!.id,
        fields: [
          {
            offset: camera.components.WorldAttachment!.fields.child!.offset,
            value: { kind: "world", value: panelWorld },
          },
          {
            offset: camera.components.WorldAttachment!.fields.mode!.offset,
            value: { kind: "u32", value: 1 },
          },
        ],
      },
      createEntity(3, "transformed-blocker"),
      insertComponent(camera, "PickingGeometry", alias(3), { geometry }),
      insertComponent(camera, "Transform", alias(3), {
        x: 0.2,
        z: 1,
        qy: Math.sin(Math.PI / 8),
        qw: Math.cos(Math.PI / 8),
        sx: 1.5,
      }),
    ]),
  );
  const blockerEntity = { kind: "handle" as const, id: aliasId(created, 3) };
  const output = await host.bindOutput(
    cameraWorld,
    aliasId(created, 1),
    "camera",
  );
  const binding = await host.setRootOutput(output, {
    width: 96,
    height: 64,
    devicePixelRatio: 1,
  });
  const view = await host.presentation.select(
    await host.presentation.surface(),
    binding,
  );
  await host.presentation.frame(view);
  const pick = async (): Promise<GuiPickingBlocker> => {
    const result = await camera.query({
      type: "GeometryPickQuery",
      view: { kind: "bound", binding },
      x: 0.5,
      y: 0.5,
      includeViewPlane: false,
    });
    check(
      result.ok && result.hit?.entity === blockerEntity.id,
      "Transformed foreground did not intersect the composed ray",
    );
    return {
      world: result.hit.world,
      entity: result.hit.entity,
      incarnation: result.hit.incarnation,
    };
  };
  let blocker = await pick();
  let context: GuiPhysicalContext;
  let detach: (() => void) | undefined;
  const gate = createGuiUnhandledInputGate();
  let gateResult: Promise<boolean> | undefined;
  let presses = 0;
  let downs: GuiInputRoutingOutcome[] = [];
  let wheels: GuiInputRoutingOutcome[] = [];
  const failures: string[] = [];
  const observation = await panel.subscribeGuiEffects((effect) => {
    if (effect.effect.kind === "pressed") presses += 1;
  });
  const open = async (marked: boolean) => {
    detach?.();
    await context?.close();
    context = await host.input.open(view, {
      blockers: marked ? [blocker] : [],
    });
    const send = context.send.bind(context);
    context.send = async (input) => {
      const result = await send(input);
      if (input.kind === "pointerDown") downs.push(result);
      if (input.kind === "wheel") wheels.push(result);
      return result;
    };
    // One 100 px wheel notch scrolls 100 logical units: a unit per pixel.
    detach = attachCanvasGuiInput(canvas, context, {
      wheelStep: 100,
      unhandledInputGate: gate,
      onError: (error) => failures.push(error.message),
    });
    downs = [];
    wheels = [];
  };
  await open(true);
  return {
    open,
    armButton(button: "secondary" | "auxiliary") {
      gateResult = gate.pointerDown(1, button, new AbortController().signal);
    },
    async gateResult(expected: boolean) {
      check(gateResult !== undefined, "Gate was not armed");
      check(
        (await gateResult) === expected,
        `Wrong non-primary gate, expected ${expected}`,
      );
      gateResult = undefined;
    },
    async replacedBlocker() {
      const previous = blocker;
      successfulBatch(
        await camera.batch([
          {
            kind: "removeComponent",
            entity: blockerEntity,
            component: camera.components.PickingGeometry!.id,
          },
          insertComponent(camera, "PickingGeometry", blockerEntity, {
            geometry,
          }),
        ]),
      );
      await host.presentation.frame(view);
      blocker = await pick();
      check(
        previous.incarnation !== blocker.incarnation,
        "Blocker replacement reused incarnation",
      );
      return { previous, replacement: blocker };
    },
    async scrollRole() {
      successfulBatch(
        await panel.batch([
          {
            kind: "removeComponent",
            entity,
            component: panel.components.GuiButton!.id,
          },
          insertComponent(panel, "GuiVirtualList", entity, {
            item_count: 20,
            item_extent: 10,
            axis: 1,
            overscan: 0,
          }),
        ]),
      );
      await host.presentation.frame(view);
      await open(true);
    },
    async verify(expectedPresses: number, blocked: boolean, wheel = false) {
      const deadline = performance.now() + 10_000;
      while (
        (wheel ? wheels : downs).length === 0 ||
        presses !== expectedPresses
      ) {
        check(
          performance.now() < deadline,
          `Missing physical terminal or wrong press count: ${presses}/${expectedPresses}; errors=${failures}`,
        );
        await host.presentation.frame(view);
      }
      const result = (wheel ? wheels : downs).at(-1)!;
      check(failures.length === 0, `Physical blocker errors: ${failures}`);
      check(
        result.disposition === (blocked ? "blocked" : "routed") &&
          !result.rejected &&
          !result.cancelled,
        `Wrong physical result ${JSON.stringify(result)}`,
      );
      const snapshot = await controlState(panel, entity.id);
      if (wheel)
        check(
          snapshot?.value.kind === "scroll" &&
            snapshot.value.offset[1] === (blocked ? 0 : 20),
          "Blocking changed scroll authority",
        );
      const capture = await host.presentation.capture(view);
      const pixels = new Uint8Array(capture.pixels);
      const ray = (32 * 96 + 48) * 4;
      check(
        pixels[ray]! > 200,
        `Expected rendered child panel at the tested physical ray: ${[...pixels.subarray(ray, ray + 4)]}`,
      );
      return {
        result,
        presses,
        blocker,
        image: { width: 96, height: 64, pixels: [...pixels] },
      };
    },
    async close() {
      detach?.();
      await context.close();
      await observation.unsubscribe();
      await host.presentation.clear(view);
      await camera.close();
      await panel.close();
      await host.destroyWorld(cameraWorld);
      await host.destroyWorld(panelWorld);
    },
  };
}
