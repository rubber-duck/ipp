/** Existing ordinary mesh path proves the shader while native Plot support awaits review. */
import assert from "node:assert/strict";
import {
  Camera,
  CustomMaterial,
  Entity,
  FragmentShader,
  MeshInstance,
  ShaderAsset,
  Transform,
  VertexShader,
  assetRef,
  createRoot,
  type ReactWorldRoot,
} from "@ipp/react";
import type { RootBinding, CameraOutputReference } from "@ipp/client";
import {
  defineClient,
  type ClientContext,
} from "../../../../tools/shared-host/client.js";
import { glassColor } from "../../../../packages/ipp-react/src/gui-glass/palette.js";
import {
  GLASS_CHART_MESH_PARAMETERS,
  GLASS_CHART_MESH_VERTEX,
  GLASS_CHART_MESH_FRAGMENT,
} from "../../../../packages/ipp-react/src/gui-glass/chart-mesh.js";

interface State {
  world: Awaited<ReturnType<ClientContext["host"]["createWorld"]>>;
  client: Awaited<ReturnType<ClientContext["host"]["openWorld"]>>;
  root: ReactWorldRoot;
  binding: RootBinding;
}

function scene(right = false, violet = false) {
  const tint = glassColor(violet ? "#9F58E8" : "#00ACDB");
  const eye = [5, 4.3, 7],
    target = [0, 1.1, 0];
  const dx = eye[0]! - target[0]!,
    dy = eye[1]! - target[1]!,
    dz = eye[2]! - target[2]!;
  const yaw = Math.atan2(dx, dz) / 2,
    pitch = -Math.asin(dy / Math.hypot(dx, dy, dz)) / 2;
  return (
    <>
      <ShaderAsset
        id="finish"
        recipe={{ normals: true, lighting: true }}
        parameters={GLASS_CHART_MESH_PARAMETERS}
      >
        <VertexShader>
          {GLASS_CHART_MESH_VERTEX.replace(
            "ippDefaultVertex();",
            `ippDefaultVertex();v_color=vec3(${tint.slice(0, 3).join(",")});`,
          )}
        </VertexShader>
        <FragmentShader>{GLASS_CHART_MESH_FRAGMENT}</FragmentShader>
      </ShaderAsset>
      <Entity id="camera">
        <Transform
          x={eye[0]}
          y={eye[1]}
          z={eye[2]}
          qx={Math.cos(yaw) * Math.sin(pitch)}
          qy={Math.sin(yaw) * Math.cos(pitch)}
          qz={-Math.sin(yaw) * Math.sin(pitch)}
          qw={Math.cos(yaw) * Math.cos(pitch)}
        />
        <Camera projection={1} ortho_height={4.4} near={0.1} far={200} />
      </Entity>
      <Entity id="bar">
        <Transform y={1.35} sx={1.4} sy={2.7} sz={1.25} />
        <MeshInstance source="ipp://mesh/cube?width=1&height=1&length=1" />
        <CustomMaterial
          source={assetRef("finish")}
          accent={glassColor(violet ? "#BF9CFF" : "#00D9FF")}
          light={right ? [7, 4, -2, 1] : [-4, 8, 6, 1]}
          finish={[1.5, 0.2, 0.8, 1]}
          tint={[0.18, 0.34, 0.39, 1]}
          lighting={[0, 0, 0, 0]}
          alpha_mode={2}
          receives_light={true}
          conservative_bounds={true}
        />
      </Entity>
    </>
  );
}

export default defineClient<State>({
  async open(context) {
    const world = await context.host.createWorld({
      symbolicId: `chart-mesh/${context.name}`,
      temporary: true,
      selectedSystems: [
        "ipp.hierarchy",
        "ipp.look-at",
        "ipp.final-propagation",
        "ipp.asset-dependencies",
        "ipp.geometry",
        "ipp.camera",
        "ipp.render",
      ],
    });
    const client = await context.host.openWorld(world.reference),
      root = createRoot(client, { host: context.host });
    await root.render(scene());
    const camera = (await root.bindOutput(
      "camera",
      "camera",
    )) as CameraOutputReference;
    const binding = await context.host.setRootOutput(camera, {
      width: 800,
      height: 760,
      devicePixelRatio: 2,
    });
    return { world, client, root, binding };
  },
  async capture(state, context) {
    const images: Record<
      string,
      Awaited<ReturnType<ClientContext["capture"]>>
    > = {};
    for (const [name, right, violet] of [
      ["cyan", false, false],
      ["moving", true, false],
      ["violet", false, true],
    ] as const) {
      await state.root.render(scene(right, violet));
      await context.capture(state.binding);
      const deadline = Date.now() + 30000;
      while (state.root.getAsset("finish")?.status !== "loaded") {
        assert(
          state.root.getAsset("finish")?.status !== "failed",
          "Shader compilation failed",
        );
        assert(Date.now() < deadline, "Shader readiness timeout");
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
      const frame = await context.capture(state.binding);
      assert(
        frame.pixels.some((value, index) => index % 4 === 1 && value > 100),
        "No visible chart mesh",
      );
      for (let p = 0; p < frame.pixels.length; p += 4)
        assert(
          !(
            frame.pixels[p]! > 220 &&
            frame.pixels[p + 1]! < 20 &&
            frame.pixels[p + 2]! < 20
          ),
          "Custom shader fallback",
        );
      images[name] = frame;
    }
    const delta = (name: string) => {
      let sum = 0;
      for (let y = 230; y < 500; y++)
        for (let x = 270; x < 380; x++) {
          const p = (y * 800 + x) * 4;
          for (let lane = 0; lane < 3; lane++)
            sum += Math.abs(
              images.cyan!.pixels[p + lane]! - images[name]!.pixels[p + lane]!,
            );
        }
      return sum / (270 * 110 * 3);
    };
    const moving = delta("moving"),
      palette = delta("violet");
    assert(moving > 1, `Source movement must change the face: ${moving}`);
    assert(palette > 10, `Palette must change the face: ${palette}`);
    return {
      images,
      report: { moving, palette },
      summary: [
        "Ordinary native mesh shader compilation and completed captures; native Plot integration pending",
      ],
    };
  },
  async close(state, context) {
    await state.root.unmount();
    await state.client.close();
    await context.host.destroyWorld(state.world.reference);
  },
});
