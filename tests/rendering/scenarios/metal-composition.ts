import assert from "node:assert/strict";
import { clientAssetSource, encodeShaderDefinition } from "@ipp/client";
import type {
  AssetWorldClient,
  Client,
  Command,
  EntityRef,
  HostClientBase,
  OutputReference,
  ShaderDefinition,
} from "@ipp/client";
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

export async function metalComposition(host: HostClientBase<Client>) {
  const root = await host.createWorld({
    selectedSystems: selectSystems(ATTACHMENTS, CANVAS, SURFACE),
    canvas: { extent: [128, 96], unitsPerMetre: 32 },
  });
  try {
    const overlay = await host.createWorld({
      selectedSystems: selectSystems(CANVAS),
      canvas: { extent: [48, 32], unitsPerMetre: 32 },
    });
    const camera = await host.createWorld({
      selectedSystems: selectSystems(ATTACHMENTS, SCENE, SURFACE),
    });
    const panel = await host.createWorld({
      selectedSystems: selectSystems(CANVAS),
      canvas: { extent: [80, 40], unitsPerMetre: 40 },
    });
    const curvedPanel = await host.createWorld({
      selectedSystems: selectSystems(CANVAS),
      canvas: { extent: [30, 18], unitsPerMetre: 40 },
    });
    try {
      const [shell, child, scene, sign, curved] = await Promise.all(
        [root, overlay, camera, panel, curvedPanel].map((world) =>
          host.openWorld(world.reference),
        ),
      );
      assert.ok(shell && child && scene && sign && curved);
      const mark = { kind: "alias", alias: 1 } as const;
      const surface = { kind: "alias", alias: 2 } as const;
      const cylinder = { kind: "alias", alias: 4 } as const;
      const foreground = { kind: "alias", alias: 3 } as const;
      const paintBox = (
        client: Client,
        entity: EntityRef,
        width: number,
        height: number,
        [red, green, blue, alpha]: number[],
        x = 0,
        y = 0,
      ) => [
        insertComponent(client, "CanvasStyle", entity, {
          red: red!,
          green: green!,
          blue: blue!,
          alpha: alpha!,
          x,
          y,
        }),
        insertComponent(client, "CanvasBox", entity, { width, height }),
      ];
      const attach = (
        client: Client,
        entity: EntityRef,
        output: OutputReference,
      ): Command => {
        const attachment = client.components.WorldAttachment!;
        return {
          kind: "insertComponent",
          entity,
          component: attachment.id,
          fields: [
            {
              offset: attachment.fields.mode!.offset,
              value: { kind: "u32", value: output.kind === "canvas" ? 1 : 2 },
            },
            {
              offset: attachment.fields.child!.offset,
              value: { kind: "world", value: output.world },
            },
            ...(output.kind === "camera"
              ? [
                  {
                    offset: attachment.fields.output!.offset,
                    value: { kind: "output", value: output } as const,
                  },
                ]
              : []),
          ],
        };
      };

      successfulBatch(
        await child.batch([
          createEntity(1, "green-background"),
          ...paintBox(child, mark, 48, 32, [0, 1, 0, 1]),
          createEntity(3, "translucent-inner"),
          ...paintBox(child, foreground, 28, 20, [1, 0, 0, 0.5], 16, 8),
        ]),
      );
      const upload = async (client: Client, definition: ShaderDefinition) => {
        const asset = clientAssetSource(client.session, 13, 1n);
        await (client as AssetWorldClient).registerAsset(
          asset,
          encodeShaderDefinition(definition).buffer,
        );
        return asset.source;
      };
      const paint = await upload(sign, {
        parameters: { red: "f32" },
        backends: {
          ipp: { paint: "return vec4(p_red, color.g, 0.0, color.a);" },
          "glsl-es-300": {
            paint: `mat2 a = mat2(1.0, p_red, -1.0, 2.0);
mat3 b = mat3(vec3(a[0], 0.5), vec3(a[1], 0.0), vec3(0.25, 1.0, 1.0));
mat4 m = mat4(vec4(b[0], 0.0), vec4(b[1], 0.25), vec4(b[2], 0.0), vec4(2.0, 1.0, 0.0, 1.0));
vec2 xy = inverse(a) * (a * color.rg);
vec3 rgb = inverse(b) * (b * vec3(p_red, xy.y, 0.0));
return inverse(m) * (m * vec4(rgb, color.a));`,
          },
        },
      });
      const material = await upload(scene, {
        recipe: { shadowPass: true },
        parameters: { tint: "vec4", shadow_coverage: "f32" },
        backends: {
          ipp: {
            fragment: `vec4 materialFragment() {
#if IPP_PASS_SHADOW
  return vec4(p_tint.rgb, p_shadow_coverage);
#else
  return vec4(p_tint.r + atan(0.0, 1.0), p_tint.gba);
#endif
}`,
          },
        },
      });
      successfulBatch(
        await sign.batch([
          createEntity(1, "yellow-panel"),
          ...paintBox(sign, mark, 80, 40, [0, 1, 0, 1]),
          insertComponent(sign, "CanvasPaint", mark, { source: paint }),
          {
            kind: "setDynamicProperty",
            entity: mark,
            component: sign.components.CanvasPaint!.id,
            name: "red",
            value: { kind: "f32", value: 1 },
          },
        ]),
      );
      successfulBatch(
        await curved.batch([
          createEntity(1, "cyan-on-cylinder"),
          ...paintBox(curved, mark, 30, 18, [0, 1, 1, 1]),
        ]),
      );
      const curvedOutput = {
        kind: "canvas",
        world: curvedPanel.reference,
      } as const;
      successfulBatch(
        await sign.batch([
          createEntity(3, "blue-cutout"),
          ...paintBox(sign, foreground, 24, 18, [0, 0, 1, 0.5], 32, 20),
          {
            kind: "setField",
            entity: foreground,
            component: sign.components.CanvasStyle!.id,
            field: {
              offset: sign.components.CanvasStyle!.fields.layer!.offset,
              value: { kind: "u32", value: 3 },
            },
          },
        ]),
      );
      const panelOutput = { kind: "canvas", world: panel.reference } as const;
      const sceneBatch = successfulBatch(
        await scene.batch([
          createEntity(1, "camera"),
          insertComponent(scene, "Camera", mark, {
            projection: 1,
            ortho_height: 4,
          }),
          createEntity(2, "spatial-panel"),
          insertComponent(scene, "Transform", surface, {
            x: 0.5,
            y: 0.5,
            z: -2,
          }),
          insertComponent(scene, "FlatSurface", surface, {
            width: 2,
            height: 1,
            layer_spacing: 0.35,
          }),
          attach(scene, surface, panelOutput),
          insertComponent(scene, "SurfaceCache", surface, {
            direct_distance: 0,
            resolution_scale: 1,
            max_refresh_hz: 1,
          }),
          createEntity(4, "curved-panel"),
          insertComponent(scene, "Transform", cylinder, {
            x: -2.1,
            y: 1.35,
            z: -2,
          }),
          insertComponent(scene, "CylinderSurface", cylinder, {
            width: 0.75,
            height: 0.45,
            curvature: 1.9,
          }),
          attach(scene, cylinder, curvedOutput),
          createEntity(3, "authored-material-cube"),
          insertComponent(scene, "Transform", foreground, {
            x: 1.75,
            y: -1.25,
            z: -2,
            sx: 0.5,
            sy: 0.5,
            sz: 0.5,
          }),
          insertComponent(scene, "MeshInstance", foreground, {
            source: "ipp://mesh/cube?width=1&height=1&length=1",
          }),
          insertComponent(scene, "CustomMaterial", foreground, {
            source: material,
            alpha_mode: 1,
            casts_shadows: true,
          }),
          {
            kind: "setDynamicProperty",
            entity: foreground,
            component: scene.components.CustomMaterial!.id,
            name: "shadow_coverage",
            value: { kind: "f32", value: 1 },
          },
          {
            kind: "setDynamicProperty",
            entity: foreground,
            component: scene.components.CustomMaterial!.id,
            name: "tint",
            value: { kind: "vec4", value: [1, 0, 0, 1] },
          },
        ]),
      );
      const cameraOutput = await host.bindOutput(
        camera.reference,
        aliasId(sceneBatch, 1),
        "camera",
      );
      const overlayOutput = {
        kind: "canvas",
        world: overlay.reference,
      } as const;
      successfulBatch(
        await shell.batch([
          createEntity(1, "background-camera"),
          insertComponent(shell, "FlatSurface", mark, { width: 4, height: 3 }),
          attach(shell, mark, cameraOutput),
          createEntity(2, "clipped-overlay"),
          insertComponent(shell, "CanvasStyle", surface, {
            x: 8,
            y: 24,
            opacity: 0.5,
            clipped: true,
            clip_max_x: 44,
            clip_max_y: 32,
          }),
          insertComponent(shell, "FlatSurface", surface, {
            width: 1.5,
            height: 1,
          }),
          attach(shell, surface, overlayOutput),
          createEntity(3, "foreground"),
          ...paintBox(shell, foreground, 14, 12, [0, 0, 1, 1], 28, 44),
        ]),
      );

      for (const [client, source] of [
        [sign, paint],
        [scene, material],
      ] as const) {
        let state = await client.inspect();
        for (;;) {
          const resource = state.resources.find(
            (item) => item.source === source,
          );
          if (resource?.status === "loaded") break;
          if (resource?.status === "failed") {
            throw new Error(
              JSON.stringify(resource, (_, value) =>
                typeof value === "bigint" ? value.toString() : value,
              ),
            );
          }
          await client.waitForFrame(state.tick);
          state = await client.inspect();
        }
      }
      const rootOutput = { kind: "canvas", world: root.reference } as const;
      const viewport = { width: 128, height: 96, devicePixelRatio: 1 };
      let view = await host.presentation.select(
        await host.presentation.surface(),
        await host.setRootOutput(rootOutput, viewport),
      );
      try {
        const afterOutputs = [
          rootOutput,
          cameraOutput,
          panelOutput,
          curvedOutput,
          overlayOutput,
        ];
        const initial = await host.presentation.capture(view, { afterOutputs });
        view = await host.presentation.resize(view, {
          ...viewport,
          width: 144,
          height: 104,
        });
        const resized = await host.presentation.capture(view, { afterOutputs });
        assert.ok(resized.sequence > initial.sequence);
        assert.equal(initial.failedDrawCalls, 0);
        assert.equal(resized.failedDrawCalls, 0);
        const caster = { kind: "handle", id: aliasId(sceneBatch, 3) } as const;
        const receiver = { kind: "alias", alias: 1 } as const;
        const spotlight = { kind: "alias", alias: 2 } as const;
        successfulBatch(
          await scene.batch([
            createEntity(1, "shadow-receiver"),
            insertComponent(scene, "Transform", receiver, {
              x: 1.0,
              y: -1.0,
              z: -3.1,
              sx: 3.2,
              sy: 1.9,
              sz: 0.05,
            }),
            insertComponent(scene, "MeshInstance", receiver, {
              source: "ipp://mesh/cube?width=1&height=1&length=1",
            }),
            insertComponent(scene, "PbrMaterial", receiver, {
              r: 0.65,
              g: 0.65,
              b: 0.65,
              cast_shadows: false,
            }),
            createEntity(2, "shadow-spotlight"),
            insertComponent(scene, "Transform", spotlight, {
              x: 0.15,
              y: 0.25,
              z: 0.75,
            }),
            insertComponent(scene, "Light", spotlight, {
              kind: 2,
              intensity: 55,
              range: 15,
              inner_cone: 0.52,
              outer_cone: 0.85,
              cast_shadows: true,
            }),
          ]),
        );
        const shadowed = await host.presentation.capture(view, {
          afterOutputs,
        });
        successfulBatch(
          await scene.batch([
            {
              kind: "setDynamicProperty",
              entity: caster,
              component: scene.components.CustomMaterial!.id,
              name: "shadow_coverage",
              value: { kind: "f32", value: 0 },
            },
          ]),
        );
        const lit = await host.presentation.capture(view, { afterOutputs });
        assert.equal(shadowed.failedDrawCalls, 0);
        assert.equal(lit.failedDrawCalls, 0);
        const dark = new Uint8Array(shadowed.pixels);
        const bright = new Uint8Array(lit.pixels);
        let shadowPixels = 0;
        for (let i = 0; i < dark.length; i += 4) {
          if ([0, 1, 2].every((c) => bright[i + c]! - dark[i + c]! > 12))
            shadowPixels++;
        }
        assert.ok(
          shadowPixels > 15,
          `cutout covered ${shadowPixels} receiver pixels`,
        );
        // Centre a perspective camera inside an upright cylinder. Odd output
        // dimensions exercise the lookup's helper fragments at the viewport.
        const setFloat = (
          entity: EntityRef,
          component: string,
          name: string,
          value: number,
        ): Command => ({
          kind: "setField",
          entity,
          component: scene.components[component]!.id,
          field: {
            offset: scene.components[component]!.fields[name]!.offset,
            value: { kind: "f32", value },
          },
        });
        const cameraEntity = {
          kind: "handle",
          id: aliasId(sceneBatch, 1),
        } as const;
        const cylinderEntity = {
          kind: "handle",
          id: aliasId(sceneBatch, 4),
        } as const;
        const flatEntity = {
          kind: "handle",
          id: aliasId(sceneBatch, 2),
        } as const;
        successfulBatch(
          await scene.batch([
            {
              kind: "setField",
              entity: cameraEntity,
              component: scene.components.Camera!.id,
              field: {
                offset: scene.components.Camera!.fields.projection!.offset,
                value: { kind: "u32", value: 0 },
              },
            },
            setFloat(flatEntity, "Transform", "x", 10),
            setFloat(cylinderEntity, "Transform", "x", 0),
            setFloat(cylinderEntity, "Transform", "y", 0),
            setFloat(cylinderEntity, "CylinderSurface", "curvature", -0.5),
          ]),
        );

        const curvedPaint = await upload(curved, {
          parameters: { red: "f32" },
          backends: {
            ipp: { paint: "return vec4(p_red, 0.0, 0.0, color.a);" },
          },
        });
        for (const [alias, x, red] of [
          [2, 6, 0.25],
          [3, 16, 1],
        ] as const) {
          const entity = { kind: "alias", alias } as const;
          successfulBatch(
            await curved.batch([
              createEntity(alias, `curved-paint-${alias}`),
              ...paintBox(curved, entity, 8, 10, [1, 1, 1, 1], x, 4),
              insertComponent(curved, "CanvasPaint", entity, {
                source: curvedPaint,
              }),
              {
                kind: "setDynamicProperty",
                entity,
                component: curved.components.CanvasPaint!.id,
                name: "red",
                value: { kind: "f32", value: red },
              },
            ]),
          );
        }
        let resources = await curved.inspect();
        while (
          !resources.resources.some(
            (item) => item.source === curvedPaint && item.status === "loaded",
          )
        ) {
          assert.ok(
            !resources.resources.some(
              (item) => item.source === curvedPaint && item.status === "failed",
            ),
          );
          await curved.waitForFrame(resources.tick);
          resources = await curved.inspect();
        }

        view = await host.presentation.resize(view, {
          width: 129,
          height: 97,
          devicePixelRatio: 1,
        });
        const curveOutputs = [rootOutput, cameraOutput, curvedOutput];
        const first = await host.presentation.capture(view, {
          afterOutputs: curveOutputs,
        });
        const second = await host.presentation.capture(view, {
          afterOutputs: curveOutputs,
          afterSequence: first.sequence,
        });
        assert.equal(first.failedDrawCalls, 0);
        assert.equal(second.failedDrawCalls, 0);
        assert.deepEqual(
          new Uint8Array(first.pixels),
          new Uint8Array(second.pixels),
        );
        // A moving opaque mesh must occlude paint retained at the cylinder's screen depth.
        const blocker = { kind: "alias", alias: 1 } as const;
        const added = successfulBatch(
          await scene.batch([
            createEntity(1, "panel-occluder"),
            insertComponent(scene, "Transform", blocker, {
              z: -1,
              sx: 0.4,
              sy: 0.4,
              sz: 0.1,
            }),
            insertComponent(scene, "MeshInstance", blocker, {
              source: "ipp://mesh/cube?width=1&height=1&length=1",
            }),
            insertComponent(scene, "UnlitMaterial", blocker, {
              r: 0,
              g: 1,
              b: 0,
            }),
          ]),
        );
        const covered = await host.presentation.capture(view, {
          afterOutputs: curveOutputs,
          afterSequence: second.sequence,
        });
        const at = (48 * 129 + 58) * 4;
        assert.deepEqual(
          [...new Uint8Array(covered.pixels).slice(at, at + 3)],
          [0, 255, 0],
        );
        successfulBatch(
          await scene.batch([
            setFloat(
              { kind: "handle", id: aliasId(added, 1) },
              "Transform",
              "x",
              10,
            ),
          ]),
        );
        const revealed = await host.presentation.capture(view, {
          afterOutputs: curveOutputs,
          afterSequence: covered.sequence,
        });
        assert.deepEqual(
          new Uint8Array(revealed.pixels),
          new Uint8Array(second.pixels),
        );
        return [initial, resized, second].map((capture) => {
          const { width, height } = capture.view.binding.viewport;
          return { width, height, pixels: new Uint8Array(capture.pixels) };
        });
      } finally {
        await host.presentation.clear(view);
      }
    } finally {
      await host.destroyWorld(curvedPanel.reference);
      await host.destroyWorld(panel.reference);
      await host.destroyWorld(camera.reference);
      await host.destroyWorld(overlay.reference);
    }
  } finally {
    await host.destroyWorld(root.reference);
  }
}

export function assertCompositionPixels(
  actual: { width: number; pixels: Uint8Array },
  i: number,
  evidence: string,
) {
  if (i === 2) {
    for (const [x, y, rgb] of [
      [58, 48, [137, 0, 0]],
      [64, 38, [0, 255, 255]],
      [72, 48, [255, 0, 0]],
    ] as const) {
      const pixel = y * actual.width + x;
      const lanes = actual.pixels.subarray(pixel * 4, pixel * 4 + 3);
      assert.ok(
        rgb.every((value, lane) => Math.abs(value - lanes[lane]!) <= 3),
        `cylinder paint at (${x}, ${y}) is ${lanes}, expected ${rgb}; see ${evidence}`,
      );
    }
    return;
  }
  // Sample away from analytic edges and texture filtering boundaries.
  for (const [x, y, rgb] of [
    [12, 30, [5, 188, 12]],
    [33, 48, [0, 0, 255]],
    [73, 30, [255, 255, 0]],
    [78, 40, [188, 188, 188]],
    [14, 15, [0, 255, 255]],
    [66, 83, [10, 14, 20]],
    [106, 78, [255, 0, 0]],
  ] as const) {
    const pixel = y * actual.width + x;
    const lanes = actual.pixels.subarray(pixel * 4, pixel * 4 + 3);
    assert.ok(
      rgb.every((value, lane) => Math.abs(value - lanes[lane]!) <= 4),
      `composition-${i} pixel (${x}, ${y}) is ${lanes}, expected ${rgb}; see ${evidence}`,
    );
  }
}
