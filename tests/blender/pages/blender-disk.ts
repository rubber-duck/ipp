import type {
  PresentedCapture,
  PresentationView,
  RootBinding,
  WorldGraphLoadResult,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { AnimationFixture } from "../../fixtures/animation.js";
import { check } from "../../harness/page/checks.js";
import type { BlenderClient } from "../../../integrations/blender/client/adapter.js";
import type { BlenderDiskManifest } from "../../../integrations/blender/client/disk-import.js";
import { createViewingCamera } from "../../../examples/blender-viewer/camera.js";
import { compareImages, rgbaDataUrl } from "../../harness/page/images.js";
import { capturedPixels } from "../../rendering/canvas/support/canvas-page.js";

declare global {
  interface Window {
    recordDisk(
      label: string,
      url: string,
      frame: Omit<PresentedCapture, "pixels">,
    ): Promise<void>;
  }
}

/** Real saved World and named exported clips, without a Blender connection or demo adapter. */
export async function run(
  urls: { generated: string; workerScript: string; wasm: string },
  bundle: string,
) {
  const contract = await import(urls.generated);
  const canvas = document.createElement("canvas");
  canvas.width = 400;
  canvas.height = 300;
  document.body.replaceChildren(canvas);
  const host: WorldPersistenceHostClient<BlenderClient> =
    await contract.IppHostClient.connectWorker(urls.workerScript, urls.wasm, {
      canvas: canvas.transferControlToOffscreen(),
      timeoutMs: 30000,
      resourceUrls: [
        { prefix: "https://disk-test.ipp.invalid/", baseUrl: bundle },
      ],
    });
  let graph: WorldGraphLoadResult | undefined;
  let binding: RootBinding | undefined;
  let view: PresentationView | undefined;
  try {
    const manifest: BlenderDiskManifest = await (
      await fetch(bundle + "manifest.json")
    ).json();
    graph = await host.loadWorld(
      new Uint8Array(await (await fetch(bundle + "world.ipp")).arrayBuffer()),
    );
    const client = await host.openWorld(graph.root);
    const state = await client.inspect();
    check(
      state.controllers !== undefined && state.controllers.length === 0,
      "clips-only import must not activate base actions",
    );
    const find = (name: string) => {
      const entity = state.entities.find(
        (entity) => entity.metadata.symbolicId === name,
      );
      check(entity, `Saved Blender entity ${name} is missing`);
      return entity.id;
    };
    const camera = manifest.camera
      ? find(manifest.camera)
      : await createViewingCamera(client);
    const output = await host.bindOutput(graph.root, camera, "camera");
    binding = await host.setRootOutput(output, {
      width: canvas.width,
      height: canvas.height,
      devicePixelRatio: 1,
    });
    view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    const selectedView = view;
    const fixture = new AnimationFixture(client, contract, async () => {});
    const capture = async (label: string) => {
      const until = performance.now() + 20000;
      let image: PresentedCapture;
      for (;;) {
        const state = await client.inspect();
        await client.waitForFrame(state.tick);
        image = await host.presentation.capture(selectedView);
        if (
          image.drawCalls > 0 &&
          state.resources.every((resource) => resource.status === "loaded")
        )
          break;
        check(
          performance.now() < until,
          "disk scene did not become renderable",
        );
      }
      const { width, height } = image.view.binding.viewport;
      const png = rgbaDataUrl({ width, height, pixels: image.pixels });
      const { pixels: _pixels, ...frame } = image;
      await window.recordDisk(label, png, frame);
      return image;
    };
    const walk = manifest.clips.find((clip) => clip.name === "Walk")!;
    check(walk !== undefined, "standard stashed Walk action missing");
    const controller = await fixture.controller(
      walk.clip.properties.map((property, track) => ({
        source: walk.clip.source,
        track,
        target: find(walk.target),
        property,
      })),
    );
    await fixture.seekPaused(controller, walk.clip.duration * 0.1);
    const first = await capture("walk-first");
    await fixture.seekPaused(controller, walk.clip.duration * 0.65);
    const second = await capture("walk-second");
    const difference = compareImages(
      capturedPixels(first),
      capturedPixels(second),
    );
    check(
      difference.changedPixels > 100,
      "exported stashed clip must visibly animate the deserialized rig",
    );
    await client.deleteAnimationController(controller);
    return {
      clips: manifest.clips.map((clip) => clip.name),
      changedPixels: difference.changedPixels,
      entities: state.entities.length,
    };
  } finally {
    try {
      if (view) await host.presentation.clear(view);
      if (binding) await host.clearRootOutput(binding);
      for (const client of host.sessions.values()) await client.close();
      for (const world of graph?.created.values() ?? [])
        await host.destroyWorld(world);
    } finally {
      await host.close();
    }
  }
}
