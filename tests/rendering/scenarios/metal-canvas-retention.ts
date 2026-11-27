import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";
import type {
  AssetWorldClient,
  Client,
  Command,
  EntityRef,
  HostClientBase,
} from "@ipp/client";
import type { HostProfiling } from "../../../packages/ipp-client/src/profiling.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  ATTACHMENTS,
  CANVAS,
  SCENE,
  SURFACE,
  selectSystems,
} from "../../fixtures/system-selections.js";

export async function withCanvasRetention<T>(
  host: HostClientBase<Client>,
  fontBytes: ArrayBuffer,
  run: (canvas: {
    capture: () => Promise<Uint8Array>;
    frame: () => Promise<bigint>;
    move: (x: number) => Promise<void>;
    cache: (enabled: boolean) => Promise<void>;
    projectSphere: () => Promise<void>;
    replaceTarget: () => Promise<void>;
  }) => Promise<T>,
): Promise<T> {
  const scene = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, SCENE, SURFACE),
  });
  try {
    const panel = await host.createWorld({
      selectedSystems: selectSystems(CANVAS),
      canvas: { extent: [400, 240], unitsPerMetre: 100 },
    });
    try {
      const [camera, gui] = await Promise.all([
        host.openWorld(scene.reference),
        host.openWorld(panel.reference),
      ]);
      const panelOutput = { kind: "canvas", world: panel.reference } as const;
      const alias = (id: number): EntityRef => ({ kind: "alias", alias: id });
      const box = (
        id: number,
        x: number,
        y: number,
        width: number,
        height: number,
        red: number,
        alpha = 1,
      ) => [
        createEntity(id, `box-${id}`),
        insertComponent(gui, "CanvasStyle", alias(id), {
          x,
          y,
          red,
          green: 0,
          blue: 0,
          alpha,
        }),
        insertComponent(gui, "CanvasBox", alias(id), { width, height }),
      ];
      const font = await (gui as AssetWorldClient).createAsset(17, fontBytes);
      const authored = successfulBatch(
        await gui.batch([
          ...box(1, 0, 0, 400, 240, 0.12),
          ...Array.from({ length: 8 }, (_, i) => [
            createEntity(i + 10, `label-${i}`),
            insertComponent(gui, "CanvasStyle", alias(i + 10), {
              x: 8,
              y: 4 + i * 21,
            }),
            insertComponent(gui, "CanvasText", alias(i + 10), {
              text: "IPP 808 / CANVAS 0123456789",
              source: font.source,
              font_size: 16,
            }),
          ]).flat(),
          ...box(2, 16, 204, 24, 12, 1, 0.5),
          ...box(3, 52, 204, 5, 12, 0.4),
        ]),
      );
      const attachment = camera.components.WorldAttachment!;
      const binding: Command = {
        kind: "insertComponent",
        entity: alias(2),
        component: attachment.id,
        fields: [
          {
            offset: attachment.fields.mode!.offset,
            value: { kind: "u32", value: 1 },
          },
          {
            offset: attachment.fields.child!.offset,
            value: { kind: "world", value: panel.reference },
          },
        ],
      };
      const cacheFields = {
        direct_distance: 0,
        max_refresh_hz: 240,
        force_cache: true,
      };
      const placed = successfulBatch(
        await camera.batch([
          createEntity(1, "camera"),
          insertComponent(camera, "Camera", alias(1), {
            projection: 1,
            ortho_height: 3,
          }),
          createEntity(2, "surface"),
          insertComponent(camera, "Transform", alias(2), { z: -3 }),
          insertComponent(camera, "FlatSurface", alias(2), {
            width: 4,
            height: 2.4,
          }),
          insertComponent(camera, "SurfaceCache", alias(2), cacheFields),
          binding,
        ]),
      );
      let state = await gui.inspect();
      for (;;) {
        const resource = state.resources.find(
          (item) => item.source === font.source,
        );
        if (resource?.status === "loaded") break;
        assert.notEqual(resource?.status, "failed");
        await gui.waitForFrame(state.tick);
        state = await gui.inspect();
      }

      const output = await host.bindOutput(
        scene.reference,
        aliasId(placed, 1),
        "camera",
      );
      let view = await host.presentation.select(
        await host.presentation.surface(),
        await host.setRootOutput(output, {
          width: 640,
          height: 400,
          devicePixelRatio: 1,
        }),
      );
      try {
        const afterOutputs = [output, panelOutput];
        let sequence = 0n;
        return await run({
          async capture() {
            const result = await host.presentation.capture(view, {
              afterOutputs,
              afterSequence: sequence,
            });
            assert.equal(result.failedDrawCalls, 0);
            sequence = result.sequence;
            return new Uint8Array(result.pixels);
          },
          async frame() {
            const result = await host.presentation.frame(view, {
              afterOutputs,
              afterSequence: sequence,
            });
            assert.equal(result.failedDrawCalls, 0);
            sequence = result.sequence;
            return sequence;
          },
          async move(x) {
            successfulBatch(
              await gui.batch([
                {
                  kind: "setField",
                  entity: { kind: "handle", id: aliasId(authored, 2) },
                  component: gui.components.CanvasStyle!.id,
                  field: {
                    offset: gui.components.CanvasStyle!.fields.x!.offset,
                    value: { kind: "f32", value: x },
                  },
                },
              ]),
            );
          },
          async cache(enabled) {
            successfulBatch(
              await camera.batch([
                insertComponent(
                  camera,
                  "SurfaceCache",
                  { kind: "handle", id: aliasId(placed, 2) },
                  {
                    ...cacheFields,
                    direct_distance: enabled ? 0 : 100,
                  },
                ),
              ]),
            );
          },
          async replaceTarget() {
            for (const width of [642, 640]) {
              view = await host.presentation.resize(view, {
                width,
                height: 400,
                devicePixelRatio: 1,
              });
            }
          },
          async projectSphere() {
            const entity = { kind: "handle", id: aliasId(placed, 2) } as const;
            successfulBatch(
              await camera.batch([
                {
                  kind: "removeComponent",
                  entity,
                  component: camera.components.SurfaceCache!.id,
                },
                {
                  kind: "removeComponent",
                  entity,
                  component: camera.components.FlatSurface!.id,
                },
                insertComponent(camera, "SphereSurface", entity, {
                  width: 4,
                  height: 2.4,
                  curvature: 1,
                }),
              ]),
            );
          },
        });
      } finally {
        await host.presentation.clear(view);
      }
    } finally {
      await host.destroyWorld(panel.reference);
    }
  } finally {
    await host.destroyWorld(scene.reference);
  }
}

export async function assertCanvasRetention(
  host: HostClientBase<Client>,
  fontBytes: ArrayBuffer,
  record: (stage: string, pixels: Uint8Array) => Promise<void>,
  profiler?: HostProfiling,
) {
  return withCanvasRetention(host, fontBytes, async (canvas) => {
    const initial = await canvas.capture();
    await record("initial", initial);
    const id = await profiler?.start({
      counters: false,
      trace: { maxEvents: 50_000 },
      maxArtifactBytes: 64 * 1024 * 1024,
    });
    let flatProfile;
    try {
      await canvas.move(44);
      const moved = await canvas.capture();
      await record("damage", moved);
      let changed = 0;
      for (let i = 0; i < initial.length; i += 4) {
        if (initial[i] !== moved[i]) changed++;
      }
      assert.ok(
        changed >= 100 && changed < 2500,
        `sparse move changed ${changed} pixels`,
      );
      await canvas.cache(false);
      await canvas.frame();
      const direct = await canvas.capture();
      await record("direct", direct);
      await setTimeout(850);
      await canvas.frame();
      const replay = await canvas.capture();
      await record("replay", replay);
      assert.equal(replay.length, direct.length);
      assert.equal(
        replay.findIndex((value, index) => value !== direct[index]),
        -1,
        "retained direct draws changed the presented pixels",
      );
      await canvas.cache(true);
      const fresh = await canvas.capture();
      await record("fresh", fresh);
      assert.equal(moved.length, fresh.length);
      assert.equal(
        moved.findIndex((value, index) => value !== fresh[index]),
        -1,
        "sparse repaint differs from a fresh image",
      );
      flatProfile = await profiler?.stop();
    } finally {
      if (id) await profiler!.release(id);
    }

    // This patch extends beyond the sphere's silhouette in chart space.
    // Both ray roots are scheduled for the same Canvas in every frame.
    await canvas.projectSphere();
    await canvas.frame();
    const sphere = await canvas.capture();
    await record("sphere-direct", sphere);
    assert.ok(sphere.some((value, i) => i % 4 === 0 && value > 70));
    await setTimeout(850);
    await canvas.frame();
    const curvedId = await profiler?.start({
      counters: false,
      trace: { maxEvents: 50_000 },
      maxArtifactBytes: 64 * 1024 * 1024,
    });
    try {
      const replay = await canvas.capture();
      await record("sphere-replay", replay);
      assert.deepEqual(
        replay,
        sphere,
        "retained sphere changed the presented pixels",
      );
      await canvas.move(116);
      const moved = await canvas.capture();
      assert.ok(
        moved.some((value, i) => value !== sphere[i]),
        "sphere edit must change the capture",
      );
      await record("sphere-damage", moved);
      await canvas.replaceTarget();
      const fresh = await canvas.capture();
      assert.equal(
        moved.findIndex((value, i) => value !== fresh[i]),
        -1,
        "sparse sphere edit must match a full redraw",
      );
      const profile = await profiler?.stop();
      if (profile) {
        assert.ok(
          profile.trace?.events.some(
            (event) => event.name === "MetalCanvasBatches.replay",
          ),
          "expected warmed two-root Canvas batches to replay",
        );
        assert.ok(
          profile.trace?.events.some(
            (event) => event.name === "MetalCanvasDamage.root_repaint",
          ),
          "expected the moving mark on a sphere to use the root damage scissor",
        );
      }
    } finally {
      if (curvedId) await profiler!.release(curvedId);
    }
    return flatProfile;
  });
}
