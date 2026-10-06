import { sameOutputReference } from "../../../packages/ipp-client/src/references.js";
import type {
  ClientAssetSource,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  BLENDER_SYSTEMS,
  BlenderAdapter,
  type BlenderClient,
  type BlenderContract,
} from "../../../integrations/blender/client/adapter.js";
import type {
  BlenderClip,
  BlenderSnapshot,
} from "../../../integrations/blender/client/types.js";
import { check } from "../../harness/page/checks.js";

/** Skeletal and particle Systems, selected only by a World whose scene authors joints. */
const JOINT_SYSTEMS = new Set([
  "ipp.skeleton",
  "ipp.skinning",
  "ipp.particles",
]);

function blenderSystems(joints: boolean): readonly string[] {
  return joints
    ? BLENDER_SYSTEMS
    : BLENDER_SYSTEMS.filter((system) => !JOINT_SYSTEMS.has(system));
}

export interface BlenderCleanupGate {
  hold(kind: "controller" | "asset", session: bigint): Promise<void>;
  release(): void;
  readonly published: readonly ClientAssetSource[];
  readonly released: readonly ClientAssetSource[];
}

export async function blenderCleanup(
  host: WorldPersistenceHostClient<BlenderClient>,
  contract: BlenderContract,
  gate: BlenderCleanupGate,
  kind: "controller" | "asset",
) {
  const created = await host.createWorld({
    selectedSystems: blenderSystems(false),
    symbolicId: `blender-cleanup-${kind}`,
  });
  const client = await host.openWorld(created.reference);
  const session = client.session;
  const observer = await host.openWorld(created.reference);
  const clip: BlenderClip = {
    duration: 1,
    tracks: [
      {
        property: { component: "Transform", fields: ["x"] },
        keys: [
          { time: 0, value: { kind: "f32", value: 2 } },
          { time: 1, value: { kind: "f32", value: 4 } },
        ],
      },
    ],
  };
  class FixtureAdapter extends BlenderAdapter {
    override source(source: string): string {
      return source === "/assets/cleanup-clip"
        ? `data:application/json,${encodeURIComponent(JSON.stringify(clip))}`
        : super.source(source);
    }
  }
  const adapter = new FixtureAdapter(
    client,
    contract,
    new URL("https://localhost"),
    "",
  );
  let pending: Promise<unknown> | undefined;
  let sentinel: bigint | undefined;
  let borrowedAsset: ClientAssetSource | undefined;
  try {
    const initial = await adapter.apply(snapshot(0, null));
    const property = {
      component: client.components.Transform!.id,
      offsets: [client.components.Transform!.fields.x!.offset],
    };
    const encoded = contract.encodeAnimationClip({
      duration: clip.duration,
      tracks: [{ property, keys: clip.tracks[0]!.keys }],
    });
    borrowedAsset = await client.createAsset(
      contract.WIRE.ASSET_ANIMATION,
      encoded.buffer,
    );
    sentinel = await client.createAnimationController({
      speed: 1,
      looping: false,
      drivers: [
        {
          source: borrowedAsset.source,
          track: 0,
          target: initial.entities.get("parent")!,
          property,
        },
      ],
    });
    const publishedStart = gate.published.length;
    const releasedStart = gate.released.length;
    const held = gate.hold(kind, session);
    const desired = snapshot(1, null);
    desired.scene.animations = [
      {
        id: "cleanup-animation",
        target: "parent",
        source: "/assets/cleanup-clip",
      },
    ];
    const result = adapter.apply(desired).then(
      () => false,
      () => true,
    );
    pending = result;
    await held;
    const queued = adapter.apply({ ...desired, revision: 2 }).then(
      () => false,
      () => true,
    );
    let disposed = false;
    const disposal = adapter.dispose().then(() => {
      disposed = true;
    });
    void disposal.catch(() => {});
    const concurrent = adapter.dispose();
    void concurrent.catch(() => {});
    const fenced = await adapter.apply({ ...desired, revision: 3 }).then(
      () => false,
      () => true,
    );
    const before = await observer.inspect();
    const controllersBefore = before.controllers?.length ?? 0;
    check(
      controllersBefore === (kind === "controller" ? 2 : 1),
      "Held ACK did not follow the expected committed effect",
    );
    const disposedBeforeAck = disposed;
    gate.release();
    check(await result, "Closed revision unexpectedly completed");
    check(await queued, "Queued revision escaped the close fence");
    await Promise.all([disposal, concurrent]);
    await adapter.dispose();
    const after = await client.inspect();
    const controllersAfter = after.controllers ?? [];
    check(
      controllersAfter.length === 1 && controllersAfter[0]?.id === sentinel,
      `Late ${kind} ACK leaked controller ownership: ${controllersBefore} before, ${controllersAfter.length} after disposal`,
    );
    check(
      !disposedBeforeAck,
      "Disposal completed before the held ACK was drained",
    );
    check(fenced, "Disposal admitted new work");
    const published = gate.published.slice(publishedStart);
    const released = gate.released.slice(releasedStart);
    check(
      published.length === 1 &&
        released.length === 1 &&
        released[0]?.source === published[0]?.source,
      "Disposal did not acknowledge exactly its owned asset release",
    );
    check(
      client.session === session,
      "Cleanup replaced the borrowed client session",
    );
    check(
      controllersAfter[0]?.description.drivers[0]?.source ===
        borrowedAsset.source,
      "Cleanup replaced the caller's asset reference",
    );
    check(
      (await host.resolveWorld(created.reference.id)).incarnation ===
        created.reference.incarnation,
      "Cleanup destroyed the borrowed World",
    );
    return {
      kind,
      controllersBefore,
      controllersAfter: controllersAfter.length,
      published: published.length,
      released: released.length,
      disposedBeforeAck,
    };
  } finally {
    gate.release();
    await pending;
    try {
      await adapter.dispose();
    } finally {
      try {
        if (sentinel !== undefined)
          await client.deleteAnimationController(sentinel);
      } finally {
        try {
          if (borrowedAsset) await client.releaseAsset(borrowedAsset);
        } finally {
          try {
            await observer.close();
          } finally {
            try {
              await client.close();
            } finally {
              await host.destroyWorld(created.reference);
            }
          }
        }
      }
    }
  }
}

function snapshot(
  revision: number,
  extra: { parent?: string; parent_bone?: number } | null,
  camera = false,
): BlenderSnapshot {
  return {
    type: "snapshot",
    session: "blender-export-headless",
    revision,
    scene: {
      ...(camera ? { active_camera: "camera" } : {}),
      entities: [
        {
          id: "parent",
          name: "Blender parent",
          transform: {
            x: 2,
            y: 0,
            z: 0,
            qx: 0,
            qy: 0,
            qz: 0,
            qw: 1,
            sx: 1,
            sy: 1,
            sz: 1,
          },
        },
        {
          id: "child",
          name: "Blender child",
          parent: "parent",
          transform: {
            x: 3,
            y: 0,
            z: 0,
            qx: 0,
            qy: 0,
            qz: 0,
            qw: 1,
            sx: 1,
            sy: 1,
            sz: 1,
          },
        },
        ...(extra ? [{ id: "extra", name: "Blender extra", ...extra }] : []),
        ...(camera
          ? [
              {
                id: "camera",
                name: "Blender camera",
                camera: {
                  projection: 0 as const,
                  fov_y: Math.PI / 4,
                  near: 0.1,
                  far: 100,
                  ortho_height: 4,
                  focus_distance: 6,
                },
                transform: {
                  x: 0,
                  y: 0,
                  z: 6,
                  qx: 0,
                  qy: 0,
                  qz: 0,
                  qw: 1,
                  sx: 1,
                  sy: 1,
                  sz: 1,
                },
              },
            ]
          : []),
      ],
    },
  };
}

export async function blenderHeadless(
  host: WorldPersistenceHostClient<BlenderClient>,
  contract: BlenderContract,
  joint: boolean,
) {
  const created = await host.createWorld({
    selectedSystems: blenderSystems(joint),
    symbolicId: "blender-headless",
  });
  const client = await host.openWorld(created.reference);
  const adapter = new BlenderAdapter(
    client,
    contract,
    new URL("https://localhost"),
    "",
  );
  let copy: Awaited<ReturnType<typeof host.loadWorld>> | undefined;
  let restored: BlenderClient | undefined;
  let binding: Awaited<ReturnType<typeof host.setRootOutput>> | undefined;
  let scenarioFailure: unknown;
  try {
    const first = await adapter.apply(snapshot(1, null));
    check(first.selectedCamera === null, "Adapter selected an implicit camera");
    const parent = first.entities.get("parent");
    const child = first.entities.get("child");
    check(
      parent !== undefined && child !== undefined,
      "Missing export identity",
    );
    const initial = await client.inspect();
    const childState = initial.entities.find((entity) => entity.id === child);
    check(childState?.link.parent === parent, "Object link was not authored");
    check(
      childState.components.some(
        (component) =>
          component.component === client.components.Transform?.id &&
          component.fields.x === 3,
      ),
      "Child transform must remain local to its parent",
    );

    let rejected = false;
    try {
      await adapter.apply(snapshot(2, { parent: "extra" }));
    } catch {
      rejected = true;
    }
    check(rejected, "Self-parenting revision unexpectedly succeeded");
    const partial = await client.inspect();
    const extra = partial.entities.find(
      (entity) => entity.metadata.symbolicId === "Blender extra",
    );
    check(extra !== undefined, "Failed batch rolled back its created prefix");
    check(
      extra.link.parent === extra.id,
      "Failed operation did not retain its applied link scope",
    );

    const corrected = await adapter.apply(
      snapshot(3, { parent: "parent", ...(joint ? { parent_bone: 0 } : {}) }),
    );
    check(
      corrected.entities.get("extra") === extra.id,
      "Correction replaced an acknowledged identity",
    );
    const current = await client.inspect();
    const correctedExtra = current.entities.find(
      (entity) => entity.id === extra.id,
    );
    check(correctedExtra?.link.parent === parent, "Correction did not link");
    if (joint)
      check(
        correctedExtra.components.some(
          (component) =>
            component.component === client.components.ParentJoint?.id &&
            component.fields.ordinal === 0,
        ),
        "Joint selection was not an independent component",
      );

    const presented = await adapter.apply(
      snapshot(
        4,
        { parent: "parent", ...(joint ? { parent_bone: 0 } : {}) },
        true,
      ),
    );
    const selected = presented.entities.get("camera");
    check(
      selected !== undefined && presented.selectedCamera === selected,
      "Authored camera selection did not resolve its exact entity",
    );
    const output = await host.bindOutput(created.reference, selected, "camera");
    binding = await host.setRootOutput(output, {
      width: 320,
      height: 240,
      devicePixelRatio: 1,
    });
    const peer = await host.openWorld(created.reference);
    await peer.close();
    check(
      sameOutputReference(await host.resolveOutput(output), output),
      "Camera OutputRef depended on the peer authoring session",
    );

    const bytes = await host.saveWorld(client.session);
    const graph = await host.inspectWorldGraph(bytes);
    check(graph.nodes.length === 1, "Ordinary links created a second World");
    copy = await host.loadWorld(bytes, { symbolicId: "blender-headless-copy" });
    restored = await host.openWorld(copy.root);
    check(
      (await host.getRootOutputBinding(copy.root)) === null,
      "Graph file persisted application presentation selection",
    );
    const loaded = await restored.inspect();
    const loadedParent = loaded.entities.find(
      (entity) => entity.metadata.symbolicId === "Blender parent",
    );
    const loadedExtra = loaded.entities.find(
      (entity) => entity.metadata.symbolicId === "Blender extra",
    );
    check(
      loadedExtra?.link.parent === loadedParent?.id,
      "Reference-only graph load lost the Blender link",
    );
    await adapter.dispose();
    await client.close();
    check(
      sameOutputReference(await host.resolveOutput(output), output),
      "Camera OutputRef expired with its authoring session",
    );
    const reopened = await host.openWorld(created.reference);
    try {
      check(
        (await reopened.inspect()).entities.length === 4,
        "Closing the Blender session destroyed its World",
      );
    } finally {
      await reopened.close();
    }
    return { entities: loaded.entities.length, bytes: bytes.length, joint };
  } catch (error) {
    scenarioFailure = error;
    throw error;
  } finally {
    const cleanupFailures: unknown[] = [];
    const cleanup = async (action: () => Promise<unknown>) => {
      try {
        await action();
      } catch (error) {
        cleanupFailures.push(error);
      }
    };
    await cleanup(() => adapter.dispose());
    if (binding) {
      const exactBinding = binding;
      await cleanup(() => host.clearRootOutput(exactBinding));
    }
    if (restored) {
      const exactRestored = restored;
      await cleanup(() => exactRestored.close());
    }
    if (copy)
      for (const world of copy.created.values())
        await cleanup(() => host.destroyWorld(world));
    await cleanup(() => client.close());
    await cleanup(() => host.destroyWorld(created.reference));
    if (cleanupFailures.length)
      throw new AggregateError(
        scenarioFailure === undefined
          ? cleanupFailures
          : [scenarioFailure, ...cleanupFailures],
        "Blender headless cleanup failed",
      );
  }
}
