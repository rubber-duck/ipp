import assert from "node:assert/strict";
import type {
  Client,
  HostClientBase,
  OutputReference,
  PresentedCapture,
} from "@ipp/client";
import type {
  HostProfiling,
  ProfileCapture,
} from "../../../packages/ipp-client/src/profiling.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import {
  PARTICLES,
  SCENE,
  SKINNING,
  selectSystems,
} from "../../fixtures/system-selections.js";

/** Author and await the shared Camera workload on a connected Host. */
export async function withMetalCamera<T>(
  host: HostClientBase<Client>,
  run: (output: OutputReference) => Promise<T>,
  grid?: { columns: number; rows: number },
): Promise<T> {
  const world = await host.createWorld({
    selectedSystems: selectSystems(SCENE, SKINNING, PARTICLES),
  });
  try {
    const client = await host.openWorld(world.reference);
    const camera = { kind: "alias", alias: 1 } as const;
    const drawables = [
      { name: "front", x: 0, y: 0.4, z: -2, size: 1, rgb: [1, 0, 0] },
      { name: "behind", x: 0, y: 0, z: -4, size: 2, rgb: [0, 0, 1] },
      {
        name: "bottom-left",
        x: -1.6,
        y: -1.2,
        z: -2,
        size: 0.7,
        rgb: [0, 1, 0],
      },
      { name: "textured", x: 1.6, y: 1.1, z: -2, size: 1, rgb: [1, 1, 1] },
    ];
    const texture =
      "ipp://texture/checkerboard?width=16&height=16&cellsX=4&cellsY=4";
    const source = "ipp://mesh/cube?width=1&height=1&length=1";
    const stretched = "ipp://mesh/cube?width=2&height=1&length=1";
    const skeleton = "ipp://skeleton/rig-strip";
    const skin = "ipp://skin/rig-strip";
    const bent = "ipp://pose/rig-strip-bent";
    const strip = "ipp://mesh/rig-strip";
    const litEntity = { kind: "alias", alias: 6 } as const;
    const rig = { kind: "alias", alias: 8 } as const;
    const particles = { kind: "alias", alias: 9 } as const;
    const outcome = successfulBatch(
      await client.batch([
        createEntity(1, "camera"),
        insertComponent(client, "Camera", camera, {
          projection: 1,
          ortho_height: 4,
        }),
        ...drawables.flatMap(
          ({ name, x, y, z, size, rgb: [r, g, b] }, index) => {
            const entity = { kind: "alias", alias: index + 2 } as const;
            return [
              createEntity(entity.alias, name),
              insertComponent(client, "Transform", entity, {
                x,
                y,
                z,
                sx: size,
                sy: size,
                sz: size,
              }),
              insertComponent(client, "MeshInstance", entity, { source }),
              insertComponent(client, "UnlitMaterial", entity, {
                r: r!,
                g: g!,
                b: b!,
              }),
              ...(name === "textured"
                ? [
                    insertComponent(client, "BaseColorTexture", entity, {
                      source: texture,
                    }),
                  ]
                : []),
            ];
          },
        ),
        createEntity(litEntity.alias, "posed-pbr"),
        insertComponent(client, "Transform", litEntity, {
          x: 1.53,
          y: -1.23,
          z: -2,
          sx: 0.7,
          sy: 0.7,
          sz: 0.7,
        }),
        insertComponent(client, "MeshInstance", litEntity, { source }),
        insertComponent(client, "MeshPose", litEntity, {
          source: stretched,
          weight: 0.5,
        }),
        insertComponent(client, "PbrMaterial", litEntity, {
          r: 0.7,
          g: 0.2,
          b: 0.05,
          receive_shadows: false,
        }),
        createEntity(7, "light"),
        insertComponent(client, "Transform", { kind: "alias", alias: 7 }),
        insertComponent(
          client,
          "Light",
          { kind: "alias", alias: 7 },
          { intensity: 1.5 },
        ),
        // A domain can contain more lights than one material's uniform block.
        // These distant point lights must leave the lit foreground unchanged.
        ...Array.from({ length: 9 }, (_, index) => {
          const entity = { kind: "alias", alias: index + 10 } as const;
          return [
            createEntity(entity.alias, `distant-light-${index}`),
            insertComponent(client, "Transform", entity, { x: 100 + index }),
            insertComponent(client, "Light", entity, {
              kind: 1,
              intensity: 1,
              range: 1,
            }),
          ];
        }).flat(),
        createEntity(rig.alias, "skinned-strip"),
        insertComponent(client, "Transform", rig, {
          x: -1.45,
          y: 0.85,
          z: -2.3,
          sx: 0.6,
          sy: 0.6,
          sz: 0.6,
        }),
        insertComponent(client, "Skeleton", rig, {
          source: skeleton,
          pose_source: bent,
        }),
        insertComponent(client, "MeshInstance", rig, { source: strip }),
        insertComponent(client, "UnlitMaterial", rig),
        createEntity(particles.alias, "mesh-particles"),
        insertComponent(client, "Transform", particles, {
          x: -1.62,
          y: -0.15,
          z: -2,
        }),
        insertComponent(client, "ParticleEmitter", particles, {
          burst: 3,
          rate: 0,
          speed: 0,
          lifetime: 1000,
          size: 0.6,
          shape: 1,
          extent_x: 0.2,
          extent_y: 0.2,
          extent_z: 0.2,
        }),
        insertComponent(client, "ParticleMesh", particles, { source }),
        insertComponent(client, "UnlitMaterial", particles, {
          r: 1,
          g: 0,
          b: 1,
        }),
      ]),
    );
    const rigId = aliasId(outcome, rig.alias);
    successfulBatch(
      await client.batch([
        insertComponent(
          client,
          "Skin",
          { kind: "handle", id: rigId },
          { skeleton: rigId, source: skin },
        ),
      ]),
    );
    if (grid) {
      // Issue one bounded batch per row. All cubes fit inside the benchmark's
      // orthographic view and share the existing mesh and material pipeline.
      // At 720p the centers are 36 pixels apart and edges fall between samples.
      for (let row = 0; row < grid.rows; row++) {
        successfulBatch(
          await client.batch(
            Array.from({ length: grid.columns }, (_, column) => {
              const alias = 32 + row * grid.columns + column;
              const entity = { kind: "alias", alias } as const;
              return [
                createEntity(alias, `grid-${row}-${column}`),
                insertComponent(client, "Transform", entity, {
                  x: (column + 0.5 - grid.columns / 2) / 5,
                  y: (row + 0.5 - grid.rows / 2) / 5,
                  z: -6,
                  sx: 2 / 15,
                  sy: 2 / 15,
                  sz: 2 / 15,
                }),
                insertComponent(client, "MeshInstance", entity, { source }),
                insertComponent(client, "UnlitMaterial", entity, {
                  r: 0.2 + (0.6 * column) / grid.columns,
                  g: 0.2 + (0.6 * row) / grid.rows,
                  b: 0.5,
                }),
              ];
            }).flat(),
          ),
        );
      }
    }
    let observed = await client.inspect();
    while (
      ![source, texture, stretched, skeleton, skin, bent, strip].every((uri) =>
        observed.resources.some(
          (resource) => resource.source === uri && resource.status === "loaded",
        ),
      )
    ) {
      assert.ok(
        !observed.resources.some((resource) => resource.status === "failed"),
        "Camera assets decoded and uploaded",
      );
      await client.waitForFrame(observed.tick);
      observed = await client.inspect();
    }
    const output = await host.bindOutput(
      world.reference,
      aliasId(outcome, 1),
      "camera",
    );
    return await run(output);
  } finally {
    await host.destroyWorld(world.reference);
  }
}

/** Deliberately asymmetric placement in an odd-width attachment and readback. */
export async function metalCamera(
  host: HostClientBase<Client>,
  profiler?: HostProfiling,
) {
  return withMetalCamera(host, async (output) => {
    const viewport = { width: 101, height: 81, devicePixelRatio: 1 };
    const root = await host.setRootOutput(output, viewport);
    const view = await host.presentation.select(
      await host.presentation.surface(),
      root,
    );
    const capture = await host.presentation.capture(view, {
      afterOutputs: [output],
    });
    const resized = await host.presentation.resize(view, {
      ...viewport,
      width: 103,
      height: 79,
    });
    await assert.rejects(host.presentation.capture(view), {
      reason: "staleView",
    });
    const after = await host.presentation.capture(resized, {
      afterOutputs: [output],
    });
    assert.ok(after.sequence > capture.sequence);
    let profile: ProfileCapture | undefined;
    if (profiler) {
      const id = await profiler.start({
        counters: false,
        trace: { maxEvents: 2000 },
        gpu: "frame",
      });
      try {
        await host.presentation.frame(resized, {
          afterOutputs: [output],
          afterSequence: after.sequence,
        });
        profile = await profiler.stop();
      } finally {
        await profiler.release(id);
      }
    }
    // Exercise both sprite blend modes over the red cube, and a third sprite behind it.
    const sprites = await host.openWorld(output.world);
    try {
      successfulBatch(
        await sprites.batch([
          ...[
            { x: -0.3, y: 0.4, z: -1, blend: 0 },
            { x: 0.3, y: 0.4, z: -1, blend: 1 },
            { x: 0, y: 0.7, z: -2.9, blend: 1 },
          ].flatMap(({ x, y, z, blend }, i) => {
            const entity = { kind: "alias", alias: i + 1 } as const;
            return [
              createEntity(entity.alias, `fixed-sprite-${i}`),
              insertComponent(sprites, "Transform", entity, { x, y, z }),
              insertComponent(sprites, "ParticleEmitter", entity, {
                burst: 1,
                rate: 0,
                speed: 0,
                lifetime: 1000,
                size: 0.27,
              }),
              insertComponent(sprites, "ParticleSprite", entity, {
                r: 0,
                g: 1,
                b: 0,
                opacity: 0.5,
                end_opacity: 0.5,
                end_size: 1,
                blend,
              }),
            ];
          }),
        ]),
      );
      const fixed = await host.presentation.capture(resized, {
        afterOutputs: [output],
        afterSequence: after.sequence,
      });
      assert.equal(fixed.failedDrawCalls, 0);
      assert.equal(fixed.triangles, after.triangles + 6);
      const pixels = new Uint8Array(fixed.pixels);
      for (const [x, y, expected] of [
        [45, 31, [188, 188, 0]],
        [57, 31, [255, 188, 0]],
        [51, 25, [255, 0, 0]],
      ] as const) {
        const at = (y * fixed.view.binding.viewport.width + x) * 4;
        const rgb = pixels.subarray(at, at + 3);
        assert.ok(
          expected.every((v, i) => Math.abs(rgb[i]! - v) <= 2),
          `sprite at (${x}, ${y}) is ${rgb}, expected ${expected}`,
        );
      }
    } finally {
      await sprites.close();
    }
    await host.presentation.clear(resized);
    return { captures: [capture, after], profile };
  });
}

export function assertMetalCamera(
  capture: PresentedCapture,
  drawCalls: number,
) {
  const { width, height } = capture.view.binding.viewport;
  const bytes = new Uint8Array(capture.pixels);
  assert.equal(bytes.length, width * height * 4);
  assert.equal(capture.drawCalls, drawCalls);
  assert.equal(capture.failedDrawCalls, 0);
  assert.equal(capture.triangles, 112);
  const center = Math.floor(width / 2);
  const equator = Math.floor(height / 2);
  for (const [x, y, expected] of [
    [center, equator - 7, [255, 0, 0, 255]],
    [center, equator + 14, [0, 0, 255, 255]],
    [center - 32, equator + 24, [0, 255, 0, 255]],
    [center - 32, equator + 3, [255, 0, 255, 255]],
    [center + 39, equator + 24, [169, 114, 89, 255]],
    [3, 3, [10, 14, 21, 255]],
  ] as const) {
    const actual = bytes.subarray((y * width + x) * 4, (y * width + x) * 4 + 4);
    assert.ok(
      expected.every((channel, i) => Math.abs(channel - actual[i]!) <= 1),
      `Pixel (${x}, ${y}) was ${actual}, expected ${expected}`,
    );
  }
}
