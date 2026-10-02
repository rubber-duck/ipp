import assert from "node:assert/strict";
import test from "node:test";
import { FieldKind, encodeShaderDefinition } from "@ipp/client";
import type { ReactWorldClient } from "../src/contract.js";
import { componentNames } from "../src/components.js";
import { Color, Slider } from "../src/gui/controls.js";
import {
  FRAGMENT_SHADER_HOST_TYPE,
  PAINT_SHADER_HOST_TYPE,
  describeShader,
} from "../src/shaders.js";
import { ReactWorldTree, type ReactWorldElementType } from "../src/tree.js";

function client(): ReactWorldClient {
  return {
    session: 1n,
    schemaHash: 1n,
    components: {
      GuiCheckbox: {
        id: 10,
        fields: {
          label: { offset: 0, kind: FieldKind.String },
          checked: { offset: 8, kind: FieldKind.Bool },
        },
      },
      GuiBehavior: {
        id: 11,
        fields: { enabled: { offset: 0, kind: FieldKind.Bool } },
      },
      GuiTheme: {
        id: 12,
        fields: { parts: { offset: 0, kind: FieldKind.Rows } },
      },
      GuiSkin: {
        id: 13,
        fields: {
          theme: { offset: 0, kind: FieldKind.Entity },
          parts: { offset: 8, kind: FieldKind.Rows },
        },
      },
      GuiSlider: {
        id: 14,
        fields: {
          value: { offset: 16, kind: FieldKind.F32 },
          origin: { offset: 20, kind: FieldKind.F32 },
          axis: { offset: 24, kind: FieldKind.U32 },
        },
      },
      GuiColor: {
        id: 53,
        fields: {
          hue: { offset: 0, kind: FieldKind.F32 },
          saturation: { offset: 4, kind: FieldKind.F32 },
          value: { offset: 8, kind: FieldKind.F32 },
          alpha: { offset: 12, kind: FieldKind.F32 },
          alpha_rail: { offset: 16, kind: FieldKind.Bool },
        },
      },
      CanvasPaint: {
        id: 52,
        dynamicProperties: true,
        fields: {
          source: { offset: 0, kind: FieldKind.String },
          variant: { offset: 16, kind: FieldKind.U32 },
        },
      },
    },
    batch: async () => {
      throw new Error("Declaration validation must not submit");
    },
  };
}

test("GUI declarations use ordinary entities, plain component declarations and explicit links", () => {
  const tree = new ReactWorldTree(client());
  const parent = tree.instance("ipp-entity", { id: "parent" });
  const child = tree.instance("ipp-entity", { bindTo: "child" });
  child.children.push(tree.instance("ipp-gui-checkbox", { checked: true }));
  parent.children.push(
    child,
    tree.instance("ipp-gui-behavior", { enabled: false }),
  );
  tree.children.push(parent);
  const description = tree.describe();
  assert.deepEqual(
    description.entities.map((entity) => [entity.symbolicId, entity.kind]),
    [
      ["parent", "declared"],
      ["child", "bound"],
    ],
  );
  assert.deepEqual(description.links, []);
  assert.deepEqual(
    description.components.map((component) => [
      component.entity,
      component.component,
      component.control,
    ]),
    [
      [child.identity, 10, true],
      [parent.identity, 11, undefined],
    ],
  );
  assert.deepEqual(description.components[0]!.fields.get(8), {
    kind: "bool",
    value: true,
  });
  assert.equal("gui" in description, false);
  // Component modes are gone: `bound` is not a component prop.
  assert.throws(
    () => tree.instance("ipp-gui-checkbox", { bound: true }),
    /Unsupported/,
  );
});

test("A dial is a Slider declared with axis 2", () => {
  const tree = new ReactWorldTree(client());
  const entity = tree.instance("ipp-entity", { id: "gain" });
  const element = Slider({ value: 0.65, origin: 0, axis: 2 });
  entity.children.push(
    tree.instance(element.type as ReactWorldElementType, element.props),
  );
  tree.children.push(entity);
  const [dial] = tree.describe().components;
  assert.equal(dial!.component, 14);
  assert.equal(dial!.control, true);
  assert.deepEqual(dial!.fields.get(24), { kind: "u32", value: 2 });
  assert.deepEqual(dial!.fields.get(20), { kind: "f32", value: 0 });
});

test("A Color declares its HSVA channels and takes its colour callback, not a scalar one", () => {
  // A value callback needs lifecycle watches, which declaration never opens.
  const tree = new ReactWorldTree({
    ...client(),
    watchLifecycle: async () => {
      throw new Error("Declaration validation must not watch");
    },
  });
  const entity = tree.instance("ipp-entity", { id: "picker" });
  const onColorCommit = () => {};
  const element = Color({
    hue: 0.5,
    saturation: 0.25,
    value: 1,
    alpha: 0.75,
    alpha_rail: true,
    onColorCommit,
  });
  entity.children.push(
    tree.instance(element.type as ReactWorldElementType, element.props),
  );
  tree.children.push(entity);
  const [color] = tree.describe().components;
  assert.equal(color!.component, 53);
  assert.equal(color!.control, true);
  assert.equal(color!.controlListeners?.onColorCommit, onColorCommit);
  assert.deepEqual(
    [0, 4, 8, 12, 16].map((offset) => color!.fields.get(offset)),
    [
      { kind: "f32", value: 0.5 },
      { kind: "f32", value: 0.25 },
      { kind: "f32", value: 1 },
      { kind: "f32", value: 0.75 },
      { kind: "bool", value: true },
    ],
  );
  const scalar = Color({ onScalarCommit: () => {} } as never);
  assert.throws(
    () =>
      new ReactWorldTree(client()).instance(
        scalar.type as ReactWorldElementType,
        scalar.props,
      ),
    /Unsupported GuiColor callback: onScalarCommit/,
  );
});

test("Rows and generated sparse writes snapshot their values and retain declaration identity", () => {
  const tree = new ReactWorldTree(client());
  const entity = tree.instance("ipp-entity", { id: "theme" });
  const bytes = new Uint8Array([1, 2, 3]);
  const theme = tree.instance("ipp-gui-theme", { parts: bytes });
  entity.children.push(theme);
  tree.children.push(entity);
  const initial = tree.describe();
  assert.equal(tree.describe().signature, initial.signature);
  // A rerender that reuses a mutated array still compares its content.
  bytes[0] = 9;
  theme.props = { ...theme.props };
  const changed = tree.describe();
  assert.notEqual(changed.signature, initial.signature);
  assert.deepEqual(initial.components[0]!.fields.get(0), {
    kind: "rows",
    value: new Uint8Array([1, 2, 3]),
  });
  assert.equal(
    changed.components[0]!.identity,
    initial.components[0]!.identity,
  );
  theme.props = { fields: [{ offset: 100, value: { kind: "unset" } }] };
  assert.deepEqual(tree.describe().components[0]!.fields.get(100), {
    kind: "unset",
  });
});

test("GUI component names require selected evaluators and do not fall back to GuiRoot", () => {
  const unselected: ReactWorldClient = {
    ...client(),
    manifest: {
      systems: [],
      operations: [],
      components: [],
    },
  };
  const tree = new ReactWorldTree(unselected);
  assert.throws(() => tree.instance("ipp-gui-checkbox", {}), /select/);
  assert.equal(Object.hasOwn(componentNames, "ipp-gui-root"), false);
});

test("Every control accepts a context request callback; other components do not", () => {
  const onContextMenu = () => {};
  const declare = (observing: boolean) => {
    const tree = new ReactWorldTree({
      ...client(),
      ...(observing
        ? {
            subscribeGuiEffects: async () => {
              throw new Error("Declaration validation must not subscribe");
            },
          }
        : {}),
    });
    const entity = tree.instance("ipp-entity", { id: "control" });
    entity.children.push(tree.instance("ipp-gui-checkbox", { onContextMenu }));
    tree.children.push(entity);
    return tree;
  };
  const description = declare(true).describe();
  assert.equal(
    description.components[0]!.controlListeners?.onContextMenu,
    onContextMenu,
  );
  assert.equal(description.guiEffects, true);
  // Like a press, a context request needs the effect observation client.
  assert.throws(() => declare(false).describe(), /observation client/);
  assert.throws(
    () =>
      new ReactWorldTree(client()).instance("ipp-gui-behavior", {
        onContextMenu,
      }),
    /Unsupported/,
  );
});

test("Every control accepts feedback callbacks, which need the observation client alone", () => {
  const onFocusChange = () => {};
  const onInteractionChange = () => {};
  const declare = (observing: boolean) => {
    const tree = new ReactWorldTree({
      ...client(),
      ...(observing
        ? {
            subscribeGuiEffects: async () => {
              throw new Error("Declaration validation must not subscribe");
            },
          }
        : {}),
    });
    const entity = tree.instance("ipp-entity", { id: "control" });
    entity.children.push(
      tree.instance("ipp-gui-checkbox", {
        onFocusChange,
        onInteractionChange,
      }),
    );
    tree.children.push(entity);
    return tree;
  };
  const description = declare(true).describe();
  const listeners = description.components[0]!.controlListeners;
  assert.equal(listeners?.onFocusChange, onFocusChange);
  assert.equal(listeners?.onInteractionChange, onInteractionChange);
  // Feedback has its own observation; no application effect is wanted.
  assert.equal(description.guiFeedback, true);
  assert.equal(description.guiEffects, false);
  assert.throws(() => declare(false).describe(), /observation client/);
  assert.throws(
    () =>
      new ReactWorldTree(client()).instance("ipp-gui-behavior", {
        onFocusChange,
      }),
    /Unsupported/,
  );
});

test("Ordinary GUI rejects obsolete callbacks, unknown props and unqualified refs", () => {
  const tree = new ReactWorldTree(client());
  assert.throws(
    () => tree.instance("ipp-gui-checkbox", { onChange: () => {} }),
    /Unsupported/,
  );
  assert.throws(
    () => tree.instance("ipp-gui-behavior", { enabled: "no" }),
    /boolean/,
  );
  assert.throws(
    () => tree.instance("ipp-gui-checkbox", { controlRef: { current: null } }),
    /ordinary GUI client/,
  );
  assert.throws(
    () =>
      tree.instance("ipp-gui-theme", {
        fields: [{ offset: 1, value: { kind: "world", value: null } }],
      }),
    /field/i,
  );
});

test("Symbolic theme references resolve to root-local declared entity identities", () => {
  const tree = new ReactWorldTree(client());
  const theme = tree.instance("ipp-entity", { bindTo: "shared-theme" });
  const control = tree.instance("ipp-entity", { id: "control" });
  control.children.push(
    tree.instance("ipp-gui-skin", { theme: "shared-theme" }),
  );
  tree.children.push(control, theme);
  const description = tree.describe();
  assert.deepEqual(description.components[0]!.fields.get(0), {
    kind: "entity-reference",
    value: { entity: theme.identity },
  });
});

test("Descriptions reuse unchanged declarations and listener changes keep the signature", () => {
  const unused = async (): Promise<never> => {
    throw new Error("Description must not observe");
  };
  const tree = new ReactWorldTree({
    ...client(),
    subscribeGuiEffects: unused,
    inspectPage: unused,
    watchLifecycle: unused,
  });
  const entity = tree.instance("ipp-entity", { id: "row" });
  const first = () => {};
  const checkbox = tree.instance("ipp-gui-checkbox", {
    checked: true,
    onToggle: first,
  });
  entity.children.push(checkbox);
  tree.children.push(entity);
  const initial = tree.describe();
  const second = () => {};
  checkbox.props = { checked: true, onToggle: second };
  const rerendered = tree.describe();
  assert.equal(rerendered.signature, initial.signature);
  assert.equal(rerendered.entities[0], initial.entities[0]);
  assert.equal(rerendered.components[0]!.fields, initial.components[0]!.fields);
  assert.equal(rerendered.components[0]!.controlListeners?.onToggle, second);
  checkbox.props = { checked: false, onToggle: second };
  const changed = tree.describe();
  assert.notEqual(changed.signature, initial.signature);
  assert.deepEqual(changed.components[0]!.fields.get(8), {
    kind: "bool",
    value: false,
  });
  checkbox.props = { checked: false, onToggle: second };
  assert.equal(tree.describe().signature, changed.signature);
  entity.children.push(tree.instance("ipp-gui-behavior", { enabled: true }));
  assert.notEqual(tree.describe().signature, changed.signature);
});

test("A Paint declares its source like a custom material and its other props as named inputs", () => {
  const tree = new ReactWorldTree(client());
  const panel = tree.instance("ipp-entity", { id: "panel" });
  panel.children.push(
    tree.instance("ipp-canvas-paint", {
      source: "paint:///scanlines",
      spacing: 4,
      tint: [0.2, 0.9, 1, 1],
    }),
  );
  tree.children.push(panel);
  const [paint] = tree.describe().components;
  assert.equal(paint!.component, 52);
  assert.deepEqual(paint!.fields.get(0), {
    kind: "string",
    value: "paint:///scanlines",
  });
  assert.deepEqual(paint!.properties, {
    spacing: { kind: "f32", value: 4 },
    tint: {
      kind: "vec4",
      value: [0.20000000298023224, 0.8999999761581421, 1, 1],
    },
  });
  assert.throws(
    () => tree.instance("ipp-canvas-paint", { "not a name": 1 }),
    /Invalid dynamic property name/,
  );
});

test("A PaintShader stage makes a ShaderAsset a canvas paint definition", () => {
  const paint = describeShader(
    [{ type: PAINT_SHADER_HOST_TYPE, props: { children: "return color;" } }],
    { spacing: "f32" },
    {},
  );
  assert.deepEqual(paint.backends["glsl-es-300"], { paint: "return color;" });
  const bytes = encodeShaderDefinition(paint);
  // IPPH version 3 carries the paint body after the material stages.
  assert.equal(new DataView(bytes.buffer).getUint32(4, true), 3);
  assert.equal(
    new TextDecoder().decode(bytes.subarray(bytes.length - 13)),
    "return color;",
  );
  // A paint has no material stage, recipe flag or texture input.
  const mixed = describeShader(
    [
      { type: PAINT_SHADER_HOST_TYPE, props: { children: "return color;" } },
      {
        type: FRAGMENT_SHADER_HOST_TYPE,
        props: { children: "vec4 materialFragment() { return vec4(1); }" },
      },
    ],
    {},
    {},
  );
  assert.throws(() => encodeShaderDefinition(mixed), /material stages/);
  assert.throws(
    () => encodeShaderDefinition({ ...paint, recipe: { lighting: true } }),
    /recipe flags/,
  );
  assert.throws(
    () =>
      encodeShaderDefinition({ ...paint, parameters: { image: "texture2D" } }),
    /float scalars or vectors/,
  );
});
