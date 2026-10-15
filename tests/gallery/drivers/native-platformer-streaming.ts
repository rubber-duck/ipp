/** Actual demo application, native transport, gated gait assets and GLES frames. */
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import type { AnimationWorldClient, WorldReference } from "@ipp/client";
import type { BlenderDiskManifest } from "../../../integrations/blender/client/disk-import.js";
import {
  defineClient,
  type ClientContext,
} from "../../../tools/shared-host/client.js";
import { NativeGallerySession } from "../../../tools/shared-host/gallery.js";
import type { RgbaImage } from "../../../tools/shared-host/images.js";
import {
  exercisePlatformerContinuity,
  exercisePlatformerCancellation,
  exercisePlatformerRoute,
  type PlatformerStreamingDriver,
} from "../scenarios/platformer-streaming.js";

interface State {
  readonly prefix: string;
  readonly assetRoot: string;
}
async function subject(
  context: ClientContext,
  state: State,
  kind: string,
  images: Record<string, RgbaImage>,
) {
  const prefix = state.prefix + kind + "/";
  const gallery = new NativeGallerySession(
    context,
    "platformer",
    kind === "route"
      ? "examples/demos/scenes/platformer/scene.tsx"
      : "tests/gallery/fixtures/platformer-streaming-scene.ts",
    state.assetRoot,
    { width: 960, height: 640, devicePixelRatio: 1 },
    { playing: true, streamPrefix: prefix },
  );
  await gallery.open();
  const app = gallery.application;
  const client = app.sceneClient! as AnimationWorldClient;
  const manifest = JSON.parse(
    await readFile(
      resolve(
        state.assetRoot,
        "gallery-platformer-native-assets/manifest.json",
      ),
      "utf8",
    ),
  ) as BlenderDiskManifest;
  const clips = Object.fromEntries(
    ["walk", "run", "crawl"].map((mode) => {
      const clip = manifest.clips.find(
        (entry) =>
          entry.name ===
            `Platformer_${mode[0]!.toUpperCase()}${mode.slice(1)}` &&
          entry.target === "platformer-rig",
      )!.clip;
      return [
        mode,
        {
          duration: clip.duration,
          source:
            kind === "route"
              ? clip.source
              : prefix + clip.source.split("/").at(-1)!,
        },
      ];
    }),
  ) as PlatformerStreamingDriver["clips"];
  let closed = false;
  const poseLayers = async (id: bigint) => {
    const state = await client.inspect();
    const selected = state.controllers?.find(
      (controller) => controller.id === id,
    );
    const gaitSources = new Set(
      Object.values(clips).map((clip) => clip.source),
    );
    if (
      !selected?.description.drivers.some((binding) =>
        gaitSources.has(binding.source),
      )
    )
      return [id];
    const origin = state.controllers?.find((controller) =>
      controller.description.drivers.some((binding) =>
        binding.source.includes("platformer-pose-origin-"),
      ),
    );
    assert.ok(origin, "gait iteration keeps its pose-origin layer in step");
    return [origin.id, id];
  };
  const driver: PlatformerStreamingDriver = {
    clips,
    state: () => client.inspect(),
    mode: async (mode) => {
      await app.action("setMode", mode);
    },
    playing: async (playing) => {
      await app.action("playback", playing);
    },
    reverse: async () => {
      await app.action("reverse");
    },
    async seek(id, time) {
      await client.playback(id, { action: "seek", time });
      const observed = await client.inspect();
      await client.waitForFrame(observed.tick);
    },
    async play(id, speed) {
      for (const layer of await poseLayers(id))
        client.playback(
          layer,
          layer === id
            ? speed === undefined
              ? { action: "play" }
              : { action: "playAtSpeed", speed }
            : { action: "playAtSpeed", speed: 0 },
        );
    },
    async pause(id) {
      for (const layer of await poseLayers(id))
        client.playback(layer, { action: "pause" });
    },
    async capture(name) {
      const frame = (await gallery.capture()).images!.platformer!;
      images[`${kind}-${name}`] = frame;
      return frame;
    },
    async release() {
      const result = await fetch(prefix + "release");
      assert.equal(result.status, 200);
    },
    async requests() {
      const response = await fetch(prefix + "requests");
      assert.equal(response.status, 200);
      return (await response.json()) as Record<
        "walk" | "run" | "crawl",
        number
      >;
    },
    async leave() {
      const sceneWorld = app.sceneOutput!.world.id;
      await app.action("scene", "shapes");
      assert.equal(
        app.sceneId,
        "shapes",
        "navigation commits its successor before the pending clip arrives",
      );
      assert.ok(
        (await context.host.listWorlds()).every(
          (world) => world.id !== sceneWorld,
        ),
        "navigation retires the exact pending robot World",
      );
      assert.equal(
        (await app.inspectApplication()).error,
        undefined,
        "late retired action leaves successor application state intact",
      );
      await driver.capture("departed-to-shapes");
    },
    async close() {
      if (closed) return;
      closed = true;
      const worlds = new Set([
        app.output.world.id,
        app.sceneOutput?.world.id,
        (app.controller as { movementWorld?: WorldReference } | undefined)
          ?.movementWorld?.id,
      ]);
      // Closing the whole application must abort pending scene work without
      // waiting for its gated producer. A later completion cannot repaint it.
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([
          gallery.close(),
          new Promise<never>((_, reject) => {
            timer = setTimeout(
              () =>
                reject(new Error("pending robot close exceeded five seconds")),
              5_000,
            );
          }),
        ]);
      } finally {
        clearTimeout(timer);
      }
      assert.ok(
        (await context.host.listWorlds()).every(
          (world) => !worlds.has(world.id),
        ),
        "pending close retires every owned World",
      );
    },
  };
  return driver;
}
export default defineClient<State>({
  async open(context, args) {
    return {
      prefix: args.find((value) => value.startsWith("http://")) ?? "",
      assetRoot:
        args.find((value) => value.startsWith("/")) ?? context.assetRoot,
    };
  },
  async capture(state, context, args) {
    const images: Record<string, RgbaImage> = {};
    const cases = args.includes("route-only")
      ? ["route"]
      : ["continuity", "newest", "closing", "leaving", "route"];
    let failure: string | undefined;
    try {
      for (const kind of cases) {
        const driver = await subject(context, state, kind, images);
        try {
          if (kind === "continuity") await exercisePlatformerContinuity(driver);
          else if (kind === "route") await exercisePlatformerRoute(driver);
          else
            await exercisePlatformerCancellation(
              driver,
              kind as "newest" | "closing" | "leaving",
            );
        } finally {
          await driver.close();
        }
      }
    } catch (error) {
      failure = error instanceof Error ? error.stack : String(error);
    }
    return { images, report: { failure, cases, frames: Object.keys(images) } };
  },
  async close() {},
});
