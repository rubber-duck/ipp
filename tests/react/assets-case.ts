import * as React from "react";
import { createRoot, Entity, Scalar } from "@ipp/react";
import {
  type ReactRuntimeConfiguration,
  type GeneratedClient,
  connect,
  settleRoots,
  findEntity,
  rejectedMessage,
} from "./fixture-helpers.js";

export async function namedAssets(configuration: ReactRuntimeConfiguration) {
  const { client } = await connect(configuration);
  const {
    Asset,
    AnimationAsset,
    MeshInstance,
    assetRef,
    ShaderAsset,
    FragmentShader,
  } = await import("@ipp/react");
  const { poseMesh } = await import("../render/mesh-pose-assets.js");
  const animation = client as import("@ipp/client").AnimationWorldClient;
  const { ResourceNotificationGate } = await import(
    "./resource-notification-gate.js"
  );
  const gate = new ResourceNotificationGate();
  const root = createRoot(gate.wrap(client), { onError() {} });
  const other = createRoot(client, { onError() {} });
  const checks: string[] = [];
  const check = (ok: boolean, message: string) => {
    if (!ok) throw new Error(message);
    checks.push(message);
  };
  const wait = async (id: string, previous?: string, selectedRoot = root) => {
    for (let attempts = 0; attempts < 100; attempts++) {
      await client.waitForFrame();
      await selectedRoot.flush();
      const state = selectedRoot.getAsset(id);
      if (state?.status === "failed") throw new Error(state.error);
      if (state?.current && state.current.source !== previous)
        return state.current.source;
    }
    throw new Error(`Asset ${id} did not become ready`);
  };
  const geometry = (weight: number) =>
    React.createElement(Asset<number>, {
      id: "mesh",
      kind: 1,
      data: weight,
      encode: poseMesh,
    });
  const scene = (weight: number) =>
    React.createElement(
      React.Fragment,
      null,
      React.createElement(
        Entity,
        { id: "asset-target" },
        React.createElement(MeshInstance, { source: assetRef("mesh") }),
        React.createElement(Scalar, { value: 0 }),
      ),
      React.createElement(
        Entity,
        { id: "asset-peer" },
        React.createElement(MeshInstance, { source: assetRef("mesh") }),
      ),
      geometry(weight),
    );
  const selection = async () =>
    findEntity(await client.inspect(), "asset-target")!.effective.find(
      (value) => value.component === client.components.MeshInstance!.id,
    )!.fields.source;
  let controller: bigint | undefined;
  try {
    gate.hold(
      (resource) =>
        resource.source.includes("/mesh#") && resource.status === "loaded",
    );
    await root.render(scene(0));
    await gate.wait(client);
    check(
      (await selection()) === "",
      "initial consumers acknowledge with an empty selection before readiness",
    );
    gate.release();
    const first = await wait("mesh");
    check(
      first.startsWith(`client://${client.session}/`) &&
        first.includes("/mesh#") &&
        (await selection()) === first &&
        (await client.inspect()).entities
          .filter((entity) =>
            ["asset-target", "asset-peer"].includes(
              entity.metadata.symbolicId ?? "",
            ),
          )
          .every(
            (entity) =>
              entity.effective.find(
                (component) =>
                  component.component === client.components.MeshInstance!.id,
              )?.fields.source === first,
          ),
      "forward named references bind after real resource loading",
    );
    await root.render(scene(0));
    check(
      root.getAsset("mesh")?.current?.source === first,
      "equal encoded content reuses the resource",
    );
    gate.hold(
      (resource) =>
        resource.source.includes("/mesh#") && resource.status === "loaded",
    );
    await root.render(scene(0.5));
    await gate.wait(client);
    check(
      (await selection()) === first,
      "previous selection survives a pending replacement",
    );
    gate.hold(() => false);
    await root.render(scene(1));
    const second = await wait("mesh", first);
    gate.release();
    await root.flush();
    check(
      root.getAsset("mesh")?.current?.source === second &&
        (await selection()) === second,
      "superseded completion cannot replace the newest loaded asset",
    );
    check(
      (await selection()) === second,
      "changed content switches the existing consumer",
    );
    await other.render(geometry(0));
    check(
      (await wait("mesh", undefined, other)) !== first,
      "roots sharing a client have isolated asset scopes",
    );
    check(
      (
        await rejectedMessage(
          root.render(
            React.createElement(React.Fragment, null, geometry(0), geometry(1)),
          ),
        )
      ).includes("Duplicate asset id"),
      "duplicate asset ids reject before registration",
    );
    await root.render(scene(1));
    check(
      (
        await rejectedMessage(
          root.render(
            React.createElement(
              Entity,
              { id: "missing-asset" },
              React.createElement(MeshInstance, {
                source: assetRef("missing"),
              }),
            ),
          ),
        )
      ).includes("Unknown asset id"),
      "missing named references reject before submission",
    );
    const scalar = client.components.Scalar!;
    const clip = {
      duration: 1,
      tracks: [
        {
          property: {
            component: scalar.id,
            offsets: [scalar.fields.value!.offset],
          },
          keys: [
            {
              time: 0,
              interpolation: { kind: "linear" as const },
              value: { kind: "f32" as const, value: 2 },
            },
            { time: 1, value: { kind: "f32" as const, value: 6 } },
          ],
        },
      ],
    };
    await root.render(
      React.createElement(
        React.Fragment,
        null,
        scene(1),
        React.createElement(AnimationAsset, { id: "motion", clip }),
      ),
    );
    const source = await wait("motion");
    const target = findEntity(await client.inspect(), "asset-target")!.id;
    controller = await animation.createAnimationController({
      speed: 0,
      drivers: [
        { source, track: 0, target, property: clip.tracks[0]!.property },
      ],
    });
    await animation.controlAnimationController(controller, { action: "play" });
    await animation.controlAnimationController(controller, { action: "pause" });
    await animation.controlAnimationController(controller, {
      action: "seek",
      time: 0.5,
    });
    const value = findEntity(
      await client.inspect(),
      "asset-target",
    )!.effective.find((value) => value.component === scalar.id)!.fields.value;
    check(
      value === 4,
      "AnimationAsset feeds the real controller through the generated client",
    );
    await animation.deleteAnimationController(controller);
    controller = undefined;
    check(
      (
        await rejectedMessage(
          root.render(
            React.createElement(
              ShaderAsset,
              { id: "invalid", recipe: {}, parameters: {} },
              React.createElement(FragmentShader, {
                references: "missing",
                children: "vec4 materialFragment() { return p_missing; }",
              }),
            ),
          ),
        )
      ).includes("declared parameter type"),
      "shader references require explicit declared types",
    );
    gate.hold(
      (resource) =>
        resource.source.includes("/mesh#") && resource.status === "loaded",
    );
    await root.render(scene(0.25));
    await gate.wait(client);
    await root.unmount();
    gate.release();
    check(
      !findEntity(await client.inspect(), "asset-target"),
      "unmount releases declarations and producer ownership",
    );
    return checks;
  } finally {
    if (controller !== undefined)
      await animation.deleteAnimationController(controller);
    await settleRoots([root, other]);
    await client.close();
  }
}

export async function createPendingAsset(client: GeneratedClient) {
  const { Asset } = await import("@ipp/react");
  const { ResourceNotificationGate } = await import(
    "./resource-notification-gate.js"
  );
  const { poseMesh } = await import("../render/mesh-pose-assets.js");
  const gate = new ResourceNotificationGate();
  const root = createRoot(gate.wrap(client), { onError() {} });
  gate.hold((resource) => resource.kind === 1 && resource.status === "loaded");
  try {
    await root.render(
      React.createElement(Asset<number>, {
        id: "session-asset",
        kind: 1,
        data: 0,
        encode: poseMesh,
      }),
    );
    await gate.wait(client);
  } catch (error) {
    await root.unmount();
    throw error;
  }
  return {
    source: root.getAsset("session-asset")!.pendingSource,
    release() {
      gate.release();
    },
    state: () => root.getAsset("session-asset"),
    close: () => root.unmount(),
  };
}
