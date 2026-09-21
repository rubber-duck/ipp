/** Scene features that have no Blender exporter mapping. Shared by both Hosts. */
import type {
  AnimationClipSource,
  AnimationDriverDescription,
  AnimationInterpolation,
  AnimationValue,
  BoundingShape,
  DynamicValue,
  GeometryEncoder,
  ShaderDefinition,
} from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";
import { check } from "../integration/animation-fixtures.js";
import {
  aliasId,
  createEntity,
  insertComponent,
} from "../integration/camera-fixtures.js";

export interface FeatureContract {
  encodeAnimationClip(clip: AnimationClipSource): Uint8Array<ArrayBuffer>;
  encodeShaderDefinition(definition: ShaderDefinition): Uint8Array<ArrayBuffer>;
  encodeBoundingShape: GeometryEncoder;
}

export interface StressFixture {
  version: number;
  grid: number;
  fps: number;
  pose_probes: {
    name: string;
    frame: number;
    weight: number;
    vertices_blender: number[][];
  }[];
}

type Values = Record<
  string,
  number | string | boolean | bigint | Uint8Array<ArrayBuffer>
>;
export type PublishFeatureAsset = (
  kind: number,
  bytes: Uint8Array<ArrayBuffer>,
) => Promise<string>;

export async function addStressFeatures(
  client: BlenderClient,
  contract: FeatureContract,
  fixture: StressFixture,
  publish: PublishFeatureAsset,
) {
  check(
    fixture.version === 2,
    "Regenerate the stress scene: feature coverage requires fixture version 2",
  );
  const imported = await client.inspect();
  const find = (name: string) =>
    imported.entities.find((e) => e.metadata.symbolicId === name)!;
  const source = (component: string) => {
    const id = client.components[component]!.id;
    const value = imported.entities
      .flatMap((e) => e.base)
      .find((c) => c.component === id)?.fields.source;
    check(
      typeof value === "string" && value.length > 0,
      `Missing imported ${component} asset`,
    );
    return value;
  };
  const mesh = String(
    find("drop-000-000").base.find(
      (c) => c.component === client.components.MeshInstance!.id,
    )!.fields.source,
  );
  const texture = source("BaseColorTexture");
  // Beside the deformation row, outside the falling grid in either preset.
  const z = fixture.grid * 0.65 + 11;
  const entities = new Map<string, bigint>();
  const create = async (
    name: string,
    components: Record<string, Values>,
    properties?: Record<string, DynamicValue>,
  ) => {
    const entity = { kind: "alias", alias: 1 } as const;
    const outcome = await client.batch([
      createEntity(1, name),
      ...Object.entries(components).map(([name, values]) =>
        insertComponent(client, name, entity, values),
      ),
      ...Object.entries(properties ?? {}).map(([name, value]) => ({
        kind: "setDynamicProperty" as const,
        entity,
        component: client.components.CustomMaterial!.id,
        name,
        value,
      })),
    ]);
    const id = aliasId(outcome, 1);
    entities.set(name, id);
    return id;
  };
  const drivers: AnimationDriverDescription[] = [];
  const track = async (
    target: bigint,
    component: string,
    field: string,
    values: readonly [AnimationValue, AnimationValue],
    interpolation: AnimationInterpolation = { kind: "linear" },
    options: Partial<AnimationDriverDescription> = {},
    dynamic = false,
    duration = 2,
  ) => {
    const descriptor = client.components[component]!;
    const property = dynamic
      ? { component: descriptor.id, name: field }
      : {
          component: descriptor.id,
          offsets: [descriptor.fields[field]!.offset],
        };
    const keys =
      interpolation.kind === "step"
        ? [
            { time: 0, value: values[0], interpolation },
            { time: 1, value: values[1], interpolation },
            { time: 2, value: values[0] },
          ]
        : [
            { time: 0, value: values[0], interpolation },
            { time: duration, value: values[1] },
          ];
    const source = await publish(
      10,
      contract.encodeAnimationClip({ duration, tracks: [{ property, keys }] }),
    );
    drivers.push({
      source,
      target,
      track: 0,
      property,
      repeat: true,
      ...options,
    });
  };
  const floats = (a: number, b: number): [AnimationValue, AnimationValue] => [
    { kind: "f32", value: a },
    { kind: "f32", value: b },
  ];
  // A ten-second projection track gives this controller the same seek range as
  // the imported timeline; short per-driver clips repeat within that interval.
  const camera = find("benchmark-camera");
  const fov = Number(
    camera.base.find((c) => c.component === client.components.Camera!.id)!
      .fields.fov_y,
  );
  await track(
    camera.id,
    "Camera",
    "fov_y",
    floats(fov, fov * 0.95),
    undefined,
    {},
    false,
    10,
  );
  const scalar = await create("feature-linear", { Scalar: { value: 0 } });
  await track(scalar, "Scalar", "value", floats(0, 4));
  await create("feature-constraint", {
    Scalar: {},
    LinearDriver: { source: scalar, scale: 2, bias: 1 },
  });
  for (const [name, baseline, from, to, interpolation, options] of [
    ["step", 0, 1, 3, { kind: "step" }, {}],
    [
      "bezier",
      0,
      0,
      1,
      {
        kind: "bezier",
        time1: 2 / 3,
        time2: 4 / 3,
        value1: floats(0, 0)[0],
        value2: floats(1, 1)[0],
      },
      {},
    ],
    ["weighted", 2, 2, 6, { kind: "linear" }, { weight: 0.25 }],
    [
      "additive",
      10,
      2,
      6,
      { kind: "linear" },
      { weight: 0.5, additive: true, referenceTime: 0 },
    ],
  ] as const) {
    const id = await create(`feature-${name}`, { Scalar: { value: baseline } });
    await track(
      id,
      "Scalar",
      "value",
      floats(from, to),
      interpolation,
      options,
    );
  }

  const target = await create("feature-look-target", {
    Transform: { x: -4, y: 2, z },
    MeshInstance: { source: mesh },
    UnlitMaterial: { r: 0.2, g: 0.8, b: 0.3 },
  });
  await create("feature-unlit-texture", {
    Transform: { x: -5, y: 4, z },
    MeshInstance: { source: mesh },
    UnlitTexture: { source: texture },
  });
  await track(target, "Transform", "x", floats(-4, 4));
  await create("feature-look-camera", {
    Transform: { x: 0, y: 2, z: z + 10 },
    Camera: { projection: 1, ortho_height: 8, far: 1000 },
    LookAt: { target },
  });
  await create("feature-orthographic", {
    Transform: { x: 0, y: 2, z: z + 10 },
    Camera: { projection: 1, ortho_height: 8, far: 1000 },
  });

  const shapes: BoundingShape[] = [
    { type: "box", min: [-0.6, -0.6, -0.6], max: [0.6, 0.6, 0.6] },
    { type: "sphere", radius: 0.65 },
    { type: "pill", start: [0, -0.5, 0], end: [0, 0.5, 0], radius: 0.4 },
    {
      type: "compound",
      parts: [
        { type: "sphere", radius: 0.45, center: [-0.45, 0, 0] },
        {
          type: "box",
          min: [-0.3, -0.3, -0.3],
          max: [0.3, 0.3, 0.3],
          transform: {
            translation: [0.45, 0, 0],
            rotation: [0, 0, Math.sin(0.3), Math.cos(0.3)],
          },
        },
      ],
    },
  ];
  for (const [index, shape] of shapes.entries()) {
    const geometry = contract.encodeBoundingShape(shape);
    // Alternate inline definitions and shared immutable geometry resources.
    const definition =
      index % 2 ? { source: await publish(6, geometry) } : { geometry };
    await create(`feature-shape-${index}`, {
      Transform: { x: (index - 1.5) * 2, y: 2, z },
      BoundingGeometry: {
        ...definition,
        is_rendered: true,
        outline: index % 2 === 0,
      },
      PickingGeometry: definition,
    });
  }

  const rig = find("walker-00-rig");
  check(rig, "Stress fixture needs a walking rig");
  await create("feature-joint-attachment", {
    Transform: { y: 0.15, sx: 0.25, sy: 0.25, sz: 0.25 },
    Hierarchy: { parent: rig.id, parent_bone: 0 },
    MeshInstance: { source: mesh },
    UnlitMaterial: { r: 1, g: 0.8, b: 0.1 },
  });
  await create("feature-joint-pill", {
    Transform: {},
    PickingGeometry: {
      geometry: contract.encodeBoundingShape({
        type: "pill",
        radius: 0.15,
        joints: [0, 1],
      }),
      skeleton: rig.id,
      is_rendered: true,
      outline: true,
    },
  });

  const shader = async (instancing: boolean, vertex = false) =>
    publish(
      13,
      contract.encodeShaderDefinition({
        recipe: { lighting: true, normals: true, shadowPass: true, instancing },
        parameters: { tint: "vec4", shift: "f32", checker: "texture2D" },
        backends: {
          "glsl-es-300": {
            ...(vertex
              ? {
                  vertex:
                    "void materialVertex() { ippDefaultVertex(); vec4 d = vec4(p_shift, 0, 0, 0); gl_Position += u_mvp * d; v_position += (u_model * d).xyz; }",
                }
              : {}),
            fragment: `vec4 materialFragment() {
        vec3 light = 0.2 + u_ambient;
        vec3 normal = ippSurfaceNormal();
        for (int i = 0; i < u_light_count; ++i) {
          float nl = max(dot(normal, ippLightDirection(i, v_position)), 0.0);
          light += ippLightRadiance(i, v_position) * nl * ippShadowVisibility(i, nl);
        }
        float coverage = fract(v_uv.x * 4.0) < 0.5 ? p_tint.a : 0.0;
        return vec4(texture(p_checker, v_uv).rgb * p_tint.rgb * light, coverage);
      }`,
          },
        },
      }),
    );
  const standard = await shader(false),
    displaced = await shader(false, true),
    instanced = await shader(true);
  const parameters: Record<string, DynamicValue> = {
    tint: { kind: "vec4", value: [0.2, 0.7, 1, 0.8] },
    shift: { kind: "f32", value: 0 },
    checker: { kind: "asset", value: { kind: 2, source: texture } },
  };
  for (let index = 0; index < 4; index++) {
    const id = await create(
      `feature-material-${index}`,
      {
        Transform: { x: (index - 1.5) * 2, y: 4, z },
        MeshInstance: { source: mesh },
        CustomMaterial: {
          source: index === 3 ? displaced : standard,
          alpha_mode: index % 3,
          alpha_cutoff: 0.4,
          receives_light: true,
          receives_shadows: true,
          casts_shadows: index % 3 !== 2,
          conservative_bounds: index === 3,
        },
        ...(index === 3
          ? {
              BoundingGeometry: {
                geometry: contract.encodeBoundingShape({
                  type: "box",
                  min: [-3, -3, -3],
                  max: [3, 3, 3],
                }),
              },
            }
          : {}),
      },
      parameters,
    );
    await track(
      id,
      "CustomMaterial",
      "tint",
      [
        { kind: "dynamic", value: parameters.tint! },
        { kind: "dynamic", value: { kind: "vec4", value: [1, 0.3, 0.2, 0.8] } },
      ],
      undefined,
      {},
      true,
    );
    if (index === 3)
      await track(
        id,
        "CustomMaterial",
        "shift",
        [
          { kind: "dynamic", value: { kind: "f32", value: -0.35 } },
          { kind: "dynamic", value: { kind: "f32", value: 0.35 } },
        ],
        undefined,
        {},
        true,
      );
  }
  for (let shape = 0; shape < 3; shape++) {
    const id = await create(
      `feature-emitter-${shape}`,
      {
        Transform: { x: (shape - 1) * 3, y: 0.5, z: z + 3 },
        ParticleEmitter: {
          seed: 7349 + shape,
          capacity: 256,
          rate: 48,
          lifetime: 2,
          shape,
          space: shape % 2,
          burst: 8,
          speed: 2,
          spread: 0.3,
          size: 0.15,
          extent_x: 0.5,
          extent_y: 0.3,
          extent_z: 0.5,
          acceleration_y: -0.6,
          drag: 0.1,
          spin: 1,
        },
        ...(shape === 2
          ? {
              ParticleMesh: { source: mesh },
              CustomMaterial: {
                source: instanced,
                receives_light: true,
                receives_shadows: true,
                casts_shadows: true,
              },
            }
          : {
              ParticleSprite: {
                source: texture,
                blend: shape,
                alignment: shape,
                end_size: 0.2,
                end_opacity: 0,
              },
            }),
      },
      shape === 2 ? parameters : undefined,
    );
    await track(
      id,
      "Transform",
      "x",
      floats((shape - 1) * 3 - 0.5, (shape - 1) * 3 + 0.5),
    );
  }
  const controller = await client.createAnimationController({
    drivers,
    looping: true,
  });
  return { controller, drivers: drivers.length, entities };
}

/** All asynchronous resource work finishes outside measurement windows. */
export async function readyStressFeatures(
  client: BlenderClient,
  headlessImport = false,
) {
  const deadline = performance.now() + 600000;
  for (;;) {
    let after = 0n,
      pending = false;
    do {
      const page = await client.inspectPage({ collection: "resources", after });
      for (const resource of page.resources) {
        // The disk-import server has no graphics context. The saved World must
        // load these programs successfully in the measurement GLES Host.
        if (
          headlessImport &&
          resource.kind === 13 &&
          resource.status === "failed" &&
          resource.error === "Shader programs require a rendering Host"
        )
          continue;
        if (resource.status === "failed")
          throw new Error(
            `Stress resource failed: ${JSON.stringify(resource, (_, value) => (typeof value === "bigint" ? String(value) : value))}`,
          );
        pending ||= resource.status !== "loaded";
      }
      after = page.next;
    } while (after !== 0n);
    if (!pending) return;
    check(
      performance.now() < deadline,
      "Stress resources did not become ready",
    );
  }
}
