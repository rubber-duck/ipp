/** Maintained robot scenario through the full native demo application. */
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import type {
  AnimationWorldClient,
  AnimationJointTransform,
  Client,
  WorldReference,
} from "@ipp/client";
import {
  defineClient,
  type ClientContext,
} from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import {
  exercisePlatformerControls,
  type PlatformerControlsDriver,
} from "../scenarios/platformer-controls.js";

import { exercisePlatformerPoses } from "../scenarios/platformer-poses.js";
import type { PlatformerPoseOrigins } from "../../../examples/demos/scenes/platformer/pose-origins.js";

interface State {
  readonly assetRoot: string;
  readonly gallery: NativeGallerySession;
  readonly panel: Client;
}

const VIEWPORT = { width: 960, height: 640, devicePixelRatio: 1 };

function driver(
  state: State,
  context: ClientContext,
  images: Record<string, RgbaImage>,
): PlatformerControlsDriver {
  const app = state.gallery.application;
  const points = new Map<string, readonly [number, number]>();
  const binding = () => {
    const result = app.sceneCanvas?.view?.binding;
    assert.ok(
      result,
      "the full application has a selected presentation binding",
    );
    return result;
  };
  return {
    async state() {
      const client = app.sceneClient!;
      return {
        scene: await client.inspect(),
        panel: await state.panel.inspect(),
        options: app.options,
        componentIds: Object.fromEntries(
          Object.entries(client.components).map(([name, value]) => [
            name,
            value!.id,
          ]),
        ),
      };
    },
    async capture(name) {
      const result = await state.gallery.capture();
      const frame = result.images?.platformer;
      assert.ok(frame, "native gallery captures its actual application output");
      images[name] = frame;
      return frame;
    },
    async press(id) {
      const snapshot = await state.panel.inspect();
      const names = new Map(
        snapshot.entities.map((entity) => [
          entity.id,
          entity.metadata.symbolicId,
        ]),
      );
      await context.present(binding(), async (view) => {
        const input = await context.host.input.open(view);
        const move = async (point: readonly [number, number]) => {
          await input.send({ kind: "pointerMove", pointer: 1n, point });
          const feedback = await state.panel.inspectPage({
            collection: "guiPointers",
          });
          const hovered = new Set<string>();
          for (const record of feedback.guiPointers ?? []) {
            const name = names.get(record.target.entity);
            if (name && record.state.hovered) {
              points.set(name, point);
              hovered.add(name);
            }
          }
          return hovered;
        };
        try {
          if (points.has(id) && !(await move(points.get(id)!)).has(id))
            points.delete(id);
          if (!points.has(id)) {
            // Discover real hits in the central rendered region; there is no
            // copied camera or hierarchy-matrix evaluator in this driver.
            search: for (let y = 0.14; y <= 0.7; y += 0.025) {
              for (let x = 0.25; x <= 0.76; x += 0.025) {
                await move([x, y]);
                if (points.has(id)) break search;
              }
            }
          }
          const point = points.get(id);
          assert.ok(point, `actual projected input finds ${id}`);
          await move(point);
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
          await context.host.presentation.frame(view);
        } finally {
          await input.close();
        }
      });
      // Application callbacks observe native effects asynchronously.
      const before = await app.sceneClient!.inspect();
      await app.sceneClient!.waitForFrame(before.tick);
      await app.sceneCanvas!.flush();
    },
    async rotate(yaw) {
      await app.navigateCamera({ kind: "rotate", yaw, pitch: 0.04 }, binding());
      points.clear();
      await app.sceneCanvas!.flush();
    },
    action: (name, args) => app.action(name, args),
    async advance(seconds) {
      const initial = await app.sceneClient!.inspect();
      for (;;) {
        const frame = await app.sceneClient!.waitForFrame();
        if (frame.time >= initial.time + seconds) return;
      }
    },
  };
}

export default defineClient<State>({
  async open(context, args) {
    const assetRoot =
      args.find((argument) => argument.startsWith("/")) ?? context.assetRoot;
    const gallery = new NativeGallerySession(
      context,
      "platformer",
      "examples/demos/scenes/platformer/scene.tsx",
      assetRoot,
      VIEWPORT,
      { playing: false },
    );
    try {
      await gallery.open();
      const world = (
        gallery.application.controller as { movementWorld?: WorldReference }
      ).movementWorld;
      assert.ok(
        world,
        "the movement panel exposes its exact owned Canvas World",
      );
      const panel = await context.host.openWorld(world);
      return { gallery, panel, assetRoot };
    } catch (error) {
      await gallery.close();
      throw error;
    }
  },
  async capture(state, context, args) {
    const images: Record<string, RgbaImage> = {};
    const subject = driver(state, context, images);
    let failure: string | undefined;
    let poseEvidence: unknown;
    try {
      if (args[0] === "exercise") await exercisePlatformerControls(subject);
      else if (args[0] === "poses") {
        const client = state.gallery.application
          .sceneClient! as AnimationWorldClient;
        const snapshot = await client.inspect();
        const rig = snapshot.entities.find(
          (entity) => entity.metadata.symbolicId === "platformer-rig",
        )!;
        const skeleton = client.components.Skeleton!;
        const baselineSource = rig.components.find(
          (component) => component.component === skeleton.id,
        )!.fields.pose_source as string;
        const poses = JSON.parse(
          await readFile(
            resolve(
              state.assetRoot,
              "gallery-platformer-native-assets/pose-origins.json",
            ),
            "utf8",
          ),
        ) as PlatformerPoseOrigins;
        let resource:
          | Awaited<ReturnType<AnimationWorldClient["createAsset"]>>
          | undefined;
        poseEvidence = await exercisePlatformerPoses({
          poses,
          state: () => client.inspect(),
          action: subject.action,
          capture: subject.capture,
          async control(id, value) {
            await client.controlAnimationController(id, value);
          },
          async reference(pose) {
            const encode = context.contract.encodePoseAsset as (
              pose: readonly AnimationJointTransform[],
            ) => Uint8Array<ArrayBuffer>;
            const wire = context.contract.WIRE as { ASSET_POSE: number };
            const previous = resource;
            resource = pose
              ? await client.createAsset(wire.ASSET_POSE, encode(pose).buffer)
              : undefined;
            const outcome = await client.batch([
              {
                kind: "setField",
                entity: { kind: "handle", id: rig.id },
                component: skeleton.id,
                field: {
                  offset: skeleton.fields.pose_source!.offset,
                  value: {
                    kind: "string",
                    value: resource?.source ?? baselineSource,
                  },
                },
              },
            ]);
            assert.ok(
              outcome.ok,
              "absolute Blender reference pose is accepted",
            );
            for (;;) {
              const current = await client.inspect();
              if (current.resources.every((entry) => entry.status === "loaded"))
                break;
              assert.ok(
                current.resources.every((entry) => entry.status !== "failed"),
              );
              await client.waitForFrame(current.tick);
            }
            if (previous) await client.releaseAsset(previous);
          },
        });
      } else await subject.capture("robot-root-controls");
    } catch (error) {
      failure = error instanceof Error ? error.stack : String(error);
    }
    const observed = await subject.state();
    const live = new Set(observed.scene.entities.map((entity) => entity.id));
    assert.ok(
      !observed.scene.entities.some(
        (entity) => entity.metadata.symbolicId === "platformer-orb",
      ),
    );
    assert.equal(observed.scene.controllers?.length, 4);
    assert.ok(
      observed.scene.controllers?.every((controller) =>
        controller.description.drivers.every((binding) =>
          live.has(binding.target),
        ),
      ),
      "every remaining native animation target is live",
    );
    return {
      images,
      report: {
        failure,
        poseEvidence,
        options: state.gallery.application.options,
        frames: Object.keys(images),
        state: observed,
      },
    };
  },
  async close(state, context) {
    const worlds = new Set([
      state.gallery.application.output.world.id,
      state.gallery.application.sceneOutput?.world.id,
      state.panel.worldReference?.id,
    ]);
    await state.panel.close();
    await state.gallery.close();
    assert.ok(
      (await context.host.listWorlds()).every((world) => !worlds.has(world.id)),
      "robot session releases its shell, scene and movement Worlds",
    );
  },
});
