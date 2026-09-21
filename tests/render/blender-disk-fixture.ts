import type { FrameCapture } from "@ipp/client";
import { AnimationFixture, check } from "../integration/animation-fixtures.js";
import type { BlenderDiskManifest } from "../../integrations/blender/client/disk-import.js";
import { compareImages } from "./image-assertions.js";

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
  const host = await contract.IppHostClient.connectWorker(
    urls.workerScript,
    urls.wasm,
    {
      canvas: canvas.transferControlToOffscreen(),
      timeoutMs: 30000,
      resourceUrls: [
        { prefix: "https://disk-test.ipp.invalid/", baseUrl: bundle },
      ],
    },
  );
  try {
    const manifest: BlenderDiskManifest = await (
      await fetch(bundle + "manifest.json")
    ).json();
    const client = await host.loadWorld(
      new Uint8Array(await (await fetch(bundle + "world.ipp")).arrayBuffer()),
    );
    const state = await client.inspect();
    check(
      state.controllers.length === 0,
      "clips-only import must not activate base actions",
    );
    const find = (name: string) =>
      state.entities.find(
        (entity: { metadata: { symbolicId: string } }) =>
          entity.metadata.symbolicId === name,
      )!.id;
    const camera = manifest.camera
      ? find(manifest.camera)
      : state.entities.find((entity: { base: { component: number }[] }) =>
          entity.base.some(
            (component) => component.component === client.components.Camera.id,
          ),
        )!.id;
    client.sendCommand({ type: "CameraActivateCommand", entity: camera });
    const fixture = new AnimationFixture(client, contract, async () => {});
    const capture = async (label: string) => {
      const until = performance.now() + 20000;
      let image: FrameCapture;
      for (;;) {
        const state = await client.inspect();
        image = await client.presentation.capture(state.tick);
        if (
          image.drawCalls > 0 &&
          state.resources.every(
            (resource: { status: string }) => resource.status === "loaded",
          )
        )
          break;
        check(
          performance.now() < until,
          "disk scene did not become renderable",
        );
      }
      const png = document.createElement("canvas");
      png.width = image.width;
      png.height = image.height;
      png
        .getContext("2d")!
        .putImageData(
          new ImageData(
            new Uint8ClampedArray(image.pixels),
            image.width,
            image.height,
          ),
          0,
          0,
        );
      await (
        window as unknown as {
          recordDisk(label: string, url: string): Promise<void>;
        }
      ).recordDisk(label, png.toDataURL());
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
    const a = await capture("walk-first");
    await fixture.seekPaused(controller, walk.clip.duration * 0.65);
    const b = await capture("walk-second");
    const difference = compareImages(a, b);
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
    await host.close();
  }
}
