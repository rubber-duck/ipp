import assert from "node:assert/strict";
import test from "node:test";
import { FieldKind } from "@ipp/client";
import type { ReactWorldClient } from "../src/contract.js";
import { componentNames } from "../src/components.js";
import { ReactWorldTree } from "../src/tree.js";

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
