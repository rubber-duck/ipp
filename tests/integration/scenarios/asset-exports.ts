/** Shared export behavior; environment drivers own processes, connections and rendering. */
import type {
  AssetWorldClient,
  ClientAssetSource,
  HostClientBase,
  PresentedCapture,
} from "@ipp/client";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
  createFixtureCamera,
  componentFields,
} from "../camera-fixtures.js";
import { settledAsset } from "../asset-fixtures.js";
import { SCENE, selectSystems } from "../system-selections.js";

export const PUBLIC_EXPORT_SOURCE: ClientAssetSource = {
  kind: 2,
  source: "fixture-export:texture",
  variant: 0,
};
const rgba = [
  37, 83, 149, 17, 211, 67, 129, 63, 94, 203, 51, 127, 173, 121, 237, 191, 53,
  229, 181, 223, 241, 157, 43, 255,
];

export function exportTexture(): ArrayBuffer {
  const bytes = new Uint8Array(40);
  bytes.set([73, 80, 80, 84]);
  const view = new DataView(bytes.buffer);
  for (const [offset, value] of [
    [4, 3],
    [8, 3],
    [12, 2],
  ])
    view.setUint32(offset!, value!, true);
  bytes.set(rgba, 16);
  return bytes.buffer;
}

function check(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function denied(
  operation: Promise<unknown>,
  reason?: string,
): Promise<void> {
  const error = await operation.then(
    () => undefined,
    (error: unknown) => error,
  );
  check(
    error instanceof Error && (!reason || error.message.includes(reason)),
    `Expected export rejection${reason ? ` containing ${reason}` : ""}, received ${String(error)}`,
  );
}

function exact(actual: Uint8Array, expected: ArrayBuffer, label: string): void {
  const bytes = new Uint8Array(expected);
  check(
    actual.length === bytes.length &&
      actual.every((byte, index) => byte === bytes[index]),
    `${label}: dimensions, row order, sRGB RGB or straight alpha changed`,
  );
}

export interface AssetExportEvidence {
  record(value: object): Promise<void>;
  capture?(name: string, image: PresentedCapture): Promise<void>;
}

export async function assetExportRoundTrip(
  host: HostClientBase<AssetWorldClient>,
  peer: HostClientBase<AssetWorldClient>,
  fixture: ((operation: number) => Promise<unknown>) | undefined,
  options: {
    graphics: boolean;
    evidence: AssetExportEvidence;
    fenceGate?: (
      operation: "hold" | "suspend" | "release" | "resume",
      world: bigint,
    ) => Promise<string>;
  },
) {
  const world = await host.createWorld({
    selectedSystems: selectSystems(SCENE),
    temporary: true,
  });
  const client = await host.openWorld(world.reference);
  let root: Awaited<ReturnType<typeof host.setRootOutput>> | undefined;
  let worldLive = true;
  try {
    await denied(host.assets.find(PUBLIC_EXPORT_SOURCE), "denied");
    await denied(
      host.assets.find({ kind: 1, source: "ipp://mesh/cube?width=1" }),
      "denied",
    );
    if (fixture) {
      await fixture(3);
      const publicGrant = await peer.assets.find(PUBLIC_EXPORT_SOURCE);
      exact(
        await peer.assets.readAll(publicGrant.capability, {
          representation: "original",
        }),
        exportTexture(),
        "explicit public original",
      );
      await fixture(4);
      await denied(
        peer.assets.read(publicGrant.capability, {
          representation: "original",
        }),
      );
    }

    const source = await client.createAsset(2, exportTexture());
    const capability = await host.assets.find(source);
    check(
      capability.original,
      "Own immutable producer registration lacked original authority",
    );
    await denied(peer.assets.find(source), "denied");
    await denied(
      peer.assets.read(capability.capability, { representation: "original" }),
      "denied",
    );
    const created = successfulBatch(
      await client.batch([
        createEntity(1, "export-textured-mesh"),
        insertComponent(client, "Transform", { kind: "alias", alias: 1 }),
        insertComponent(
          client,
          "MeshInstance",
          { kind: "alias", alias: 1 },
          { source: "ipp://mesh/cube?width=2&height=2&length=2" },
        ),
        insertComponent(
          client,
          "UnlitMaterial",
          { kind: "alias", alias: 1 },
          { r: 1, g: 1, b: 1 },
        ),
        insertComponent(
          client,
          "UnlitTexture",
          { kind: "alias", alias: 1 },
          { source: source.source },
        ),
      ]),
    );
    const entity = aliasId(created, 1);
    const resource = await settledAsset(client, source);
    check(
      resource.status === "loaded",
      "Actual typed representation failed to decode",
    );
    const ready = await host.assets.find(source);
    check(
      options.graphics
        ? ready.gpu.includes("texture-v3")
        : ready.cpu.includes("texture-v3"),
      "Host did not report the supported live semantic encoding",
    );
    if (options.graphics)
      check(
        !ready.cpu.includes("texture-v3"),
        "GPU layout metadata was advertised as a complete CPU texture",
      );
    await denied(
      host.assets.read(ready.capability, {
        representation: options.graphics ? "gpu" : "cpu",
        format: "mesh-v3",
      }),
    );
    if (options.graphics)
      await denied(
        host.assets.read(ready.capability, {
          representation: "cpu",
          format: "texture-v3",
        }),
        "CPU",
      );
    if (!options.graphics)
      await denied(
        host.assets.read(ready.capability, {
          representation: "gpu",
          format: "texture-v3",
        }),
      );

    let before: PresentedCapture | undefined;
    let view: Awaited<ReturnType<typeof host.presentation.select>> | undefined;
    if (options.graphics) {
      const camera = await createFixtureCamera(client);
      const output = await host.bindOutput(world.reference, camera, "camera");
      root = await host.setRootOutput(output, {
        width: 96,
        height: 64,
        devicePixelRatio: 1,
      });
      view = await host.presentation.select(
        await host.presentation.surface(),
        root,
      );
      before = await host.presentation.capture(view, {
        afterOutputs: [output],
      });
      check(
        before.drawCalls > 0 && before.triangles > 0,
        "Texture export fixture produced no actual draw",
      );
      const pixels = new Uint8Array(before.pixels);
      const unique = new Set(
        Array.from(
          { length: pixels.length / 4 },
          (_, i) =>
            `${pixels[i * 4]},${pixels[i * 4 + 1]},${pixels[i * 4 + 2]}`,
        ),
      );
      check(
        unique.size > 4,
        "Asymmetric textured scene lacked meaningful image variation",
      );
      await options.evidence.capture?.("original", before);
    }

    const representation = options.graphics ? "gpu" : "cpu";
    await options.fenceGate?.("hold", world.id);
    const encoding = host.assets.readAll(ready.capability, {
      representation,
      format: "texture-v3",
    });
    void encoding.catch(() => {});
    const tick = await options.fenceGate?.("suspend", world.id);
    await options.fenceGate?.("release", world.id);
    const encoded = await encoding;
    exact(encoded, exportTexture(), `${representation} semantic export`);
    const afterTick = await options.fenceGate?.("resume", world.id);
    if (tick !== undefined)
      check(afterTick === tick, "Export advanced a suspended World");
    const replacement = await client.createAsset(2, encoded.buffer);
    successfulBatch(
      await client.batch(
        componentFields(client, "UnlitTexture", {
          source: replacement.source,
        }).map((field) => ({
          kind: "setField" as const,
          entity: { kind: "handle" as const, id: entity },
          component: client.components.UnlitTexture!.id,
          field,
        })),
      ),
    );
    check(
      (await settledAsset(client, replacement)).status === "loaded",
      "Semantic export did not reload through actual provider",
    );
    if (view && before) {
      const after = await host.presentation.capture(view, {
        afterOutputs: [root!.output],
      });
      check(
        new Uint8Array(after.pixels).every(
          (byte, index) => byte === new Uint8Array(before!.pixels)[index],
        ),
        "Reloaded semantic export changed the completed image",
      );
      await options.evidence.capture?.("reloaded", after);
    }

    const output = await host.assets.read(ready.capability, {
      representation,
      format: "texture-v3",
    });
    const original = await host.assets.read(ready.capability, {
      representation: "original",
    });
    if (fixture) await fixture(6);
    await client.releaseAsset(source);
    exact(
      await host.reads.readAll(original.read),
      exportTexture(),
      "original read after producer release",
    );
    await host.assets.revoke(ready.capability);
    exact(
      await host.reads.readAll(output.read),
      exportTexture(),
      "detached semantic output after grant revoke",
    );
    await denied(
      host.assets.read(ready.capability, { representation: "original" }),
    );

    const replacementGrant = await host.assets.find(replacement);
    if (fixture) {
      const pressureRead = await host.assets.read(replacementGrant.capability, {
        representation: "original",
      });
      await fixture(1);
      await denied(host.reads.readAll(pressureRead.read));
      await peer.listWorlds();
    }
    await host.assets.revoke(replacementGrant.capability);
    await denied(
      host.assets.read(replacementGrant.capability, {
        representation,
        format: "texture-v3",
      }),
    );
    if (fixture) {
      await fixture(5);
      await denied(peer.assets.find(PUBLIC_EXPORT_SOURCE));
    }
    await options.evidence.record({
      representation,
      format: "texture-v3",
      bytes: encoded.length,
      source,
      resource,
      authorization: true,
      exactRgba: true,
      reloaded: true,
      detached: true,
      cancelled: true,
    });
    if (root) {
      await host.clearRootOutput(root);
      root = undefined;
    }
    await client.close();
    await host.destroyWorld(world.reference);
    worldLive = false;
    return {
      representation,
      bytes: encoded.length,
      exactRgba: true,
      reloaded: true,
      detached: true,
      authorization: true,
    };
  } finally {
    await options.fenceGate?.("resume", world.id);
    if (root) await host.clearRootOutput(root).catch(() => {});
    await client.close();
    if (worldLive) await host.destroyWorld(world.reference);
  }
}
