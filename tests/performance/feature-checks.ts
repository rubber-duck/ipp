/** Observable checks outside timing windows, shared by native and browser imports. */
import type { EntitySnapshot, PickingWorldClient } from "@ipp/client";
import type { BlenderClient } from "../../integrations/blender/client/adapter.js";
import { check } from "../integration/animation-fixtures.js";
import type { StressFixture } from "./features.js";

export async function checkStressFeatures(
  client: BlenderClient,
  fixture: StressFixture,
  seek: (time: number) => Promise<void>,
  originalCamera: bigint,
) {
  const state = await client.inspect();
  const entities = new Map(
    state.entities.map((entity) => [entity.metadata.symbolicId, entity]),
  );
  const counts = Object.fromEntries(
    Object.entries(client.components).map(([name, descriptor]) => [
      name,
      state.entities.filter((e) =>
        e.effective.some((c) => c.component === descriptor.id),
      ).length,
    ]),
  );
  for (const [name, count] of Object.entries(counts))
    check(count > 0, `Stress scene does not exercise ${name}`);
  check(counts.MeshPose! >= 8, "Missing rigid and skinned mesh-pose workloads");
  check(
    state.entities.some(
      (e) =>
        e.effective.some((c) => c.component === client.components.Skin!.id) &&
        e.effective.some((c) => c.component === client.components.MeshPose!.id),
    ),
    "Pose-before-skin workload missing",
  );
  const fields = (entity: EntitySnapshot, name: string) => {
    const component = entity.effective.find(
      (c) => c.component === client.components[name]!.id,
    );
    check(component, `${entity.metadata.symbolicId}: missing ${name}`);
    return component.fields;
  };
  const get = async (name: string) => {
    const id = entities.get(name)?.id;
    check(id, `Missing ${name}`);
    return (await client.inspectPage({ collection: "entities", target: id }))
      .entities[0]!;
  };
  const close = (actual: unknown, expected: number, message: string) => {
    check(
      typeof actual === "number" && Math.abs(actual - expected) < 2e-4,
      `${message}: expected ${expected}, got ${String(actual)}`,
    );
  };
  const samples = [];
  const queries = client as BlenderClient & PickingWorldClient;
  try {
    for (const time of [0.5, 1.5, 2.5]) {
      await seek(time);
      const local = time % 2;
      const u = local / 2;
      const expected = {
        linear: 2 * local,
        constraint: 4 * local + 1,
        step: local < 1 ? 1 : 3,
        bezier: 3 * u * u - 2 * u * u * u,
        weighted: 2 + local * 0.5,
        additive: 10 + local,
      };
      for (const [name, value] of Object.entries(expected))
        close(
          fields(await get(`feature-${name}`), "Scalar").value,
          value,
          name,
        );
      const material = (await get("feature-material-0")).effective.find(
        (c) => c.component === client.components.CustomMaterial!.id,
      )!;
      const tint = material.properties?.tint;
      check(tint?.kind === "vec4", "Animated custom tint missing");
      close(tint.value[0], 0.2 + u * 0.8, "dynamic tint");
      const displaced = (await get("feature-material-3")).effective.find(
        (c) => c.component === client.components.CustomMaterial!.id,
      )!.properties?.shift;
      check(displaced?.kind === "f32", "Animated vertex displacement missing");
      close(displaced.value, -0.35 + u * 0.7, "custom vertex displacement");
      const camera = await get("benchmark-camera");
      const baseline = camera.base.find(
        (c) => c.component === client.components.Camera!.id,
      )!;
      close(
        fields(camera, "Camera").fov_y,
        Number(baseline.fields.fov_y) * (1 - time * 0.005),
        "animated projection",
      );
      client.sendCommand({
        type: "CameraActivateCommand",
        entity: entities.get("feature-look-camera")!.id,
      });
      const z = fixture.grid * 0.65 + 11;
      const projected = await queries.query({
        type: "CameraProjectQuery",
        x: 0.5,
        y: 0.5,
        width: 800,
        height: 600,
        plane: { point: [0, 2, z], normal: [0, 0, 1] },
      });
      check(projected.ok && projected.position, "LookAt projection failed");
      close(
        projected.position[0],
        -4 + 4 * local,
        "LookAt follows animated target",
      );
      close(projected.position[1], 2, "LookAt vertical projection");
      samples.push({ time, expected, projected: projected.position });
    }
    const poseSamples = [];
    for (const probe of fixture.pose_probes) {
      await seek((probe.frame - 1) / fixture.fps);
      const weight = fields(await get(probe.name), "MeshPose").weight;
      close(
        weight,
        probe.weight,
        `${probe.name} Blender pose at frame ${probe.frame}`,
      );
      poseSamples.push({ name: probe.name, frame: probe.frame, weight });
    }
    client.sendCommand({
      type: "CameraActivateCommand",
      entity: entities.get("feature-orthographic")!.id,
    });
    const picks = [];
    for (let index = 0; index < 4; index++) {
      // Independent orthographic projection: width = height * viewport aspect.
      const x = (index - 1.5) * 2;
      const result = await queries.query({
        type: "GeometryPickQuery",
        x: 0.5 + x / ((8 * 800) / 600),
        y: 0.5,
        width: 800,
        height: 600,
        includeViewPlane: true,
      });
      check(
        result.ok &&
          result.hit?.entity === entities.get(`feature-shape-${index}`)!.id,
        `Picking shape ${index} failed`,
      );
      close(result.hit.position[0], x, `Picking shape ${index} position`);
      picks.push({ shape: index, hit: result.hit });
    }
    return { components: counts, samples, poseSamples, picks };
  } finally {
    client.sendCommand({
      type: "CameraActivateCommand",
      entity: originalCamera,
    });
    await seek(0.5);
  }
}
