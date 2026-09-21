/** Retained mixed numeric/resource benchmark, authored through native WebSocket. */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type {
  AnimationDriverDescription,
  AnimationWorldClient,
  RenderWorldClient,
  WorldPersistenceHostClient,
} from "@ipp/client";
import { runNativeEnvironment } from "../integration/environment.js";
import { successfulBatch } from "../integration/camera-fixtures.js";
import { createCubeMesh } from "../../examples/world-gallery/assets/cube-mesh.js";
import { AnimationFixture } from "../integration/animation-fixtures.js";

const [outputArg, nativeArg] = process.argv.slice(2);
assert.ok(
  outputArg && nativeArg,
  "Expected output and native host directories",
);
const output = resolve(outputArg),
  native = resolve(nativeArg);
await mkdir(output, { recursive: true });
const contract = await import(
  pathToFileURL(resolve(native, "generated.js")).href
);
const prefix = "https://stress.ipp.invalid/";
const publish = async (bytes: Uint8Array) => {
  const digest = createHash("sha256").update(bytes).digest("hex");
  await writeFile(resolve(output, digest), bytes);
  return prefix + digest;
};
const texture = async (rgb: number[]) => {
  const bytes = new Uint8Array(20),
    view = new DataView(bytes.buffer);
  bytes.set([73, 80, 80, 84]);
  [3, 1, 1].forEach((v, i) => view.setUint32(4 + i * 4, v, true));
  bytes.set([...rgb, 255], 16);
  return publish(bytes);
};
const red = await texture([255, 0, 0]),
  green = await texture([0, 255, 0]),
  blue = await texture([0, 0, 255]);
const mesh = await publish(new Uint8Array(createCubeMesh()));
const shader = await publish(
  contract.encodeShaderDefinition({
    parameters: { value0: "f32", image: "texture2D" },
    backends: {
      "glsl-es-300": {
        fragment:
          "vec4 materialFragment() { return vec4(p_value0 * texture(p_image, v_uv).rgb, 1); }",
      },
    },
  }),
);
const result = await runNativeEnvironment(
  "mixed animation benchmark import",
  {
    executable: resolve(native, "ipp-server"),
    schemaArtifact: resolve(native, "contract.bin"),
    workingDirectory: process.cwd(),
    extraArguments: ["--file-root", output, "--file-prefix", prefix],
    operationTimeoutMs: 120000,
    evidenceParent: resolve(output, "../import-evidence"),
  },
  AbortSignal.timeout(300000),
  async (environment) => {
    const host = await environment.track<
      WorldPersistenceHostClient<AnimationWorldClient & RenderWorldClient>
    >(
      contract.IppHostClient.connectWebSocket(environment.url, {
        timeoutMs: 30000,
        logLevel: "error",
        signal: environment.signal,
      }),
    );
    const client = await host.createWorld({ symbolicId: "mixed-benchmark" });
    const fixture = new AnimationFixture(client, contract, async () => {});
    await fixture.create("benchmark-camera", {
      Transform: { z: 9 },
      Camera: { projection: 1, ortho_height: 8, focus_distance: 9 },
    });
    const material = client.components.CustomMaterial!.id,
      scalar = client.components.Scalar!;
    const numeric = Array.from({ length: 8 }, (_, i) => ({
      property: { component: material, name: `value${i}` },
      keys: [
        {
          time: 0,
          value: { kind: "dynamic", value: { kind: "f32", value: 0.2 } },
          interpolation: { kind: "linear" },
        },
        {
          time: 10,
          value: { kind: "dynamic", value: { kind: "f32", value: 0.8 } },
          interpolation: { kind: "step" },
        },
      ],
    }));
    const source = await publish(
      contract.encodeAnimationClip({
        duration: 10,
        tracks: [
          ...numeric,
          {
            property: { component: material, name: "image" },
            keys: [
              {
                time: 0,
                value: {
                  kind: "dynamic",
                  value: { kind: "asset", value: { kind: 2, source: red } },
                },
                interpolation: { kind: "step" },
              },
              {
                time: 5,
                value: {
                  kind: "dynamic",
                  value: { kind: "asset", value: { kind: 2, source: green } },
                },
                interpolation: { kind: "step" },
              },
            ],
          },
          {
            property: {
              component: scalar.id,
              offsets: [scalar.fields.value!.offset],
            },
            keys: [
              {
                time: 0,
                value: { kind: "f32", value: 1 },
                interpolation: { kind: "linear" },
              },
              {
                time: 10,
                value: { kind: "f32", value: 2 },
                interpolation: { kind: "step" },
              },
            ],
          },
        ],
      }),
    );
    const drivers: AnimationDriverDescription[] = [];
    for (let i = 0; i < 256; i++) {
      const target = await fixture.create(`mixed-${i}`, {
        Transform: {
          x: ((i % 16) - 7.5) * 0.4,
          y: (Math.floor(i / 16) - 7.5) * 0.4,
          sx: 0.15,
          sy: 0.15,
          sz: 0.15,
        },
        MeshInstance: { source: mesh },
        UnlitMaterial: { r: 0, g: 0, b: 1 },
        CustomMaterial: { source: shader },
        Scalar: { value: 9 },
        BoundingGeometry: {},
      });
      const properties = [
        ...Array.from({ length: 64 }, (_, n) => ({
          name: `padding${n}`,
          value: { kind: "f32", value: n } as const,
        })),
        ...Array.from({ length: 8 }, (_, n) => ({
          name: `value${n}`,
          value: { kind: "f32", value: 1 } as const,
        })),
        {
          name: "image",
          value: { kind: "asset", value: { kind: 2, source: blue } } as const,
        },
      ];
      const outcome = await client.batch(
        properties.map(({ name, value }) => ({
          kind: "setDynamicProperty",
          entity: { kind: "handle", id: target },
          component: material,
          name,
          value,
        })),
      );
      successfulBatch(outcome);
      for (let track = 0; track < 10; track++)
        drivers.push({
          source,
          track,
          target,
          property:
            track < 8
              ? { component: material, name: `value${track}` }
              : track === 8
                ? { component: material, name: "image" }
                : {
                    component: scalar.id,
                    offsets: [scalar.fields.value!.offset],
                  },
        });
    }
    for (let start = 0; start < drivers.length; start += 200) {
      await client.createAnimationController({
        drivers: drivers.slice(start, start + 200),
        speed: 1,
        looping: false,
      });
    }
    await writeFile(resolve(output, "benchmark.ipp"), await host.saveWorld());
    await writeFile(resolve(output, "camera.txt"), "benchmark-camera\n");
    return {
      entities: 257,
      drivers: drivers.length,
      animatedNumericProperties: 2048,
      scalarDrivers: 256,
      resourceDrivers: 256,
      paddingProperties: 16384,
    };
  },
);
await writeFile(
  resolve(output, "fixture.json"),
  JSON.stringify(result.value, null, 2) + "\n",
);
console.log(result.value);
