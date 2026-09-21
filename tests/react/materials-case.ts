import * as React from "react";
import { createRoot, Entity } from "@ipp/react";
import type { Client } from "@ipp/client";
import {
  type ReactRuntimeConfiguration,
  connect,
  settleRoots,
} from "./fixture-helpers.js";

export async function customMaterialProperties(
  configuration: ReactRuntimeConfiguration,
) {
  const { client } = await connect(configuration);
  const { CustomMaterial, mat2, i32, u32, asset, texture2D } = await import(
    "@ipp/react"
  );
  const root = createRoot(client);
  const component = client.components.CustomMaterial!.id;
  const observations: unknown[] = [];
  const describe = (
    value: number,
    tint: readonly number[] | undefined,
    extra: boolean,
  ) =>
    React.createElement(
      Entity,
      { id: "custom-owned" },
      React.createElement(CustomMaterial, {
        bound: false,
        source: "file:///custom.shader",
        alpha_mode: 1,
        amount: value,
        tint,
        basis: mat2(1, 0, 0, 1),
        count: i32(3),
        limit: u32(4),
        image: texture2D("file:///texture.png", 2),
        geometry: asset(1, "file:///model.mesh", 3),
        ...(extra ? { enabled: true } : {}),
      }),
    );
  const observe = async () => {
    const state = await client.inspect();
    const entity = state.entities.find(
      (e) => e.metadata.symbolicId === "custom-owned",
    )!;
    const properties = entity.effective.find((v) => v.component === component)!
      .properties!;
    const fields = entity.effective.find(
      (v) => v.component === component,
    )!.fields;
    if (
      fields.source !== "file:///custom.shader" ||
      fields.alpha_mode !== 1 ||
      "source" in properties ||
      "bound" in properties
    )
      throw new Error(
        "Fixed material fields were treated as custom properties",
      );
    observations.push({
      amount: properties.amount,
      tint: properties.tint,
      basis: properties.basis,
      count: properties.count,
      limit: properties.limit,
      image: properties.image,
      geometry: properties.geometry,
    });
    return entity.id;
  };
  try {
    await root.render(describe(0.25, [1, 0, 0, 1], false));
    await observe();
    await root.render(describe(0.75, [0, 0, 1, 1], true));
    const entity = await observe();
    const result = await client.batch([
      {
        kind: "setDynamicProperty",
        entity: { kind: "handle", id: entity },
        component,
        name: "tint",
        value: { kind: "vec4", value: [0, 1, 0, 1] },
      },
    ]);
    if (!result.ok) throw new Error("Producer tint update failed");
    await root.render(describe(0.5, undefined, false));
    await observe();
    await root.unmount();
    const state = await client.inspect();
    if (state.entities.some((e) => e.metadata.symbolicId === "custom-owned"))
      throw new Error("Custom material owner leaked after unmount");
    return observations;
  } finally {
    await settleRoots([root]);
    await client.close();
  }
}

export async function createShaderPreview(client: Client) {
  const {
    CustomMaterial,
    VertexShader,
    FragmentShader,
    ShaderAsset,
    assetRef,
  } = await import("@ipp/react");
  const { ResourceNotificationGate } = await import(
    "./resource-notification-gate.js"
  );
  const gate = new ResourceNotificationGate();
  const root = createRoot(gate.wrap(client), { onError() {} });
  const settleAsset = async () => {
    for (let n = 0; n < 100; n++) {
      const state = root.getAsset("preview");
      if (state?.status === "loaded" || state?.status === "failed") {
        await root.flush();
        return;
      }
      await client.waitForFrame();
      await root.flush();
    }
    throw new Error("Shader preview did not finish loading");
  };
  let edit: React.Dispatch<
    React.SetStateAction<{ body: string; tint: readonly number[] }>
  >;
  function Editor() {
    const [state, setState] = React.useState({
      body: "vec4 materialFragment() { return p_tint; }",
      tint: [0, 0, 1, 1] as readonly number[],
    });
    edit = setState;
    return React.createElement(
      Entity,
      { bindTo: "left" },
      React.createElement(
        CustomMaterial,
        { bound: true, tint: state.tint, source: assetRef("preview") },
        React.createElement(
          ShaderAsset,
          { id: "preview", recipe: {}, parameters: { tint: "vec4" } },
          React.createElement(VertexShader, {
            children: "void materialVertex() { ippDefaultVertex(); }",
          }),
          React.createElement(FragmentShader, {
            references: "tint",
            children: state.body,
          }),
        ),
      ),
    );
  }
  try {
    await root.render(React.createElement(Editor));
    await settleAsset();
  } catch (error) {
    await root.unmount();
    throw error;
  }
  return {
    async update(body: string, tint: readonly number[]) {
      edit!({ body, tint });
      await root.flush();
      await settleAsset();
    },
    async beginUpdate(body: string, tint: readonly number[]) {
      gate.hold(
        (resource) => resource.kind === 13 && resource.status === "loaded",
      );
      edit!({ body, tint });
      await root.flush();
      await gate.wait(client);
    },
    async completeUpdate() {
      gate.release();
      await settleAsset();
    },
    close: () => root.unmount(),
  };
}
