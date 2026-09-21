/** React GuiRoot producer lifecycle against a production-faithful client.
 *
 * The mock below mirrors the production contract: overlay declarations that
 * would create a GuiRoot (or write its node tree) are rejected with
 * InvalidField, a Bound property-only overlay on an existing producer is
 * accepted, roots are created with insertComponent and removed with
 * removeComponent, and node IDs are never reused. The mock must stay this
 * strict: production rejects what it rejects.
 *
 * Coverage: mount creates the producer without any GuiRoot overlay, updates
 * patch content without replaying values, unmount removes nodes and the
 * producer, remount adopts a fresh incarnation with stale handles fenced,
 * pre-existing producers are adopted empty or refused occupied, partial
 * failures surface and still clean up, and control/action listeners are
 * retained JS-only without transport.
 */
import assert from "node:assert/strict";
import test from "node:test";
import { createElement as h, Fragment, StrictMode } from "react";
import type {
  BatchOutcome,
  Command,
  ComponentDescriptor,
  EntityRef,
  GuiEdit,
  GuiInspectedNode,
  GuiInspectResponse,
  GuiNodeContent,
  GuiNodeHandle,
  GuiNodeStyle,
  StateOverlayAlias,
  StateOverlayLifecycleDiagnostic,
  StateOverlayRef,
} from "@ipp/client";
import { guiNodeHandle } from "@ipp/client";
import {
  createRoot,
  Entity,
  ParticleMesh,
  ParticleSprite,
  Surface,
  UnlitMaterial,
  type ReactWorldClient,
} from "../../packages/ipp-react/src/index.js";
import { GuiRoot, Row, Text } from "../../packages/ipp-react/src/gui.js";
import {
  ReactWorldTree,
  retainedNodeCallbacks,
} from "../../packages/ipp-react/src/tree.js";
import {
  guiRootSignature,
  type GuiDescribedNode,
} from "../../packages/ipp-react/src/gui/description.js";

const GUI_ROOT_COMPONENT = 90;
const SURFACE_COMPONENT = 41;
const TRANSFORM_COMPONENT = 7;

const mockComponents: Record<string, ComponentDescriptor> = {
  GuiRoot: { id: GUI_ROOT_COMPONENT, fields: {} },
  Surface: {
    id: SURFACE_COMPONENT,
    fields: {
      width: { offset: 0, kind: 1 },
      height: { offset: 1, kind: 1 },
      items: { offset: 2, kind: 6 },
    },
  },
  Transform: { id: TRANSFORM_COMPONENT, fields: {} },
  ParticleSprite: { id: 23, fields: { r: { offset: 0, kind: 1 } } },
  ParticleMesh: { id: 24, fields: { source: { offset: 0, kind: 5 } } },
  UnlitMaterial: { id: 4, fields: { r: { offset: 0, kind: 1 } } },
};

function rejected(reason: string): Error {
  const error = new Error(reason) as Error & { code: string };
  error.code = "IPP_REQUEST_REJECTED";
  return error;
}

interface MockGuiNode {
  id: number;
  parent: number | undefined;
  lifetime: number;
  content: GuiNodeContent;
  style: GuiNodeStyle;
}

interface MockGuiRoot {
  incarnation: bigint;
  nodes: Map<number, MockGuiNode>;
  order: Map<number | undefined, number[]>;
  usedIds: Set<number>;
}

interface MockEntity {
  id: bigint;
  symbolicId: string;
  gui: MockGuiRoot | null;
}

/** In-memory generated-client stand-in with production root semantics. */
class MockGuiClient {
  readonly session = 1n;
  readonly schemaHash = 1n;
  readonly components = mockComponents;
  readonly capabilities = {
    stateOverlays: true,
    spatial: false,
    textures: false,
    builtinAssets: false,
    picking: false,
    debugGeometry: false,
    pbr: false,
    shadows: false,
    skeletalAnimation: false,
    meshPoses: false,
    surfaces: true,
    gui: true,
  };
  entities = new Map<bigint, MockEntity>();
  batches: Command[][] = [];
  edits: GuiEdit[] = [];
  producerCreates = 0;
  producerRemoves = 0;
  failNextInsertComponent = false;
  failNextEdit = false;
  failReleaseBatches = 0;
  omitBindingEntity = false;
  private nextEntity = 1001n;
  private nextHandle = 1n;
  private nextIncarnation = 1n;
  private nextBatch = 1n;
  private tick = 1n;
  private owners = new Set<bigint>();
  private bindings = new Map<
    bigint,
    { entity: bigint; mode: string; owner: bigint }
  >();
  private overlays = new Map<
    bigint,
    { entity: bigint; component: number; owner: bigint }
  >();

  onDiagnostic(
    _listener: (diagnostic: StateOverlayLifecycleDiagnostic) => void,
  ): () => void {
    return () => {};
  }

  private resolveOverlayRef(
    ref: StateOverlayRef,
    local: ReadonlyMap<number, bigint>,
  ): bigint {
    if (ref.kind === "handle") return ref.id;
    const id = local.get(ref.alias);
    if (id === undefined) throw rejected("UnknownAlias");
    return id;
  }

  private resolveEntityRef(
    ref: EntityRef,
    local: ReadonlyMap<number, bigint>,
  ): bigint {
    if (ref.kind === "handle") return ref.id;
    const id = local.get(ref.alias);
    if (id === undefined) throw rejected("UnknownAlias");
    return id;
  }

  private failure(index: number, reason: string): BatchOutcome {
    return {
      batchId: this.nextBatch++,
      tick: this.tick++,
      aliases: [],
      stateOverlays: [],
      ok: false,
      error: { scope: "operation", operation: index, reason },
    };
  }

  async batch(operations: Command[]): Promise<BatchOutcome> {
    this.batches.push(operations);
    if (
      this.failReleaseBatches > 0 &&
      operations.some((operation) => operation.kind.startsWith("release"))
    ) {
      this.failReleaseBatches -= 1;
      return this.failure(0, "SimulatedReleaseFailure");
    }
    const local = new Map<number, bigint>();
    const stateOverlays: StateOverlayAlias[] = [];
    for (let index = 0; index < operations.length; index += 1) {
      const operation = operations[index]!;
      switch (operation.kind) {
        case "createStateOverlayOwner": {
          const id = this.nextHandle++;
          this.owners.add(id);
          stateOverlays.push({
            alias: operation.alias,
            id,
            kind: "owner",
            entity: null,
          });
          local.set(operation.alias, id);
          break;
        }
        case "attachEntityOverlayBinding": {
          const owner = this.resolveOverlayRef(operation.owner, local);
          if (!this.owners.has(owner))
            return this.failure(index, "UnknownOwner");
          const id = this.nextHandle++;
          let entity: bigint;
          if (operation.mode === "owned") {
            entity = this.nextEntity++;
            this.entities.set(entity, {
              id: entity,
              symbolicId: operation.symbolicId,
              gui: null,
            });
          } else {
            const found = [...this.entities.values()].find(
              (entry) => entry.symbolicId === operation.symbolicId,
            );
            if (!found) return this.failure(index, "MissingEntity");
            entity = found.id;
          }
          this.bindings.set(id, { entity, mode: operation.mode, owner });
          local.set(operation.alias, id);
          stateOverlays.push({
            alias: operation.alias,
            id,
            kind: "entityOverlayBinding",
            entity: this.omitBindingEntity ? null : entity,
          });
          break;
        }
        case "attachComponentStateOverlay": {
          const binding = this.bindings.get(
            this.resolveOverlayRef(operation.binding, local),
          );
          if (!binding) return this.failure(index, "UnknownBinding");
          const entity = this.entities.get(binding.entity);
          if (!entity) return this.failure(index, "MissingEntity");
          if (operation.component === GUI_ROOT_COMPONENT) {
            // Production parity: overlays never create a GuiRoot and never
            // carry its node tree. A Bound property-only overlay on an
            // existing producer is accepted; anything else is InvalidField.
            if (
              entity.gui === null ||
              operation.mode !== "bound" ||
              operation.fields.length > 0
            )
              return this.failure(index, "InvalidField");
          }
          if (
            (operation.component === 23 || operation.component === 24) &&
            [...this.overlays.values()].some(
              (overlay) =>
                overlay.entity === entity.id &&
                ((overlay.component === 23 && operation.component === 24) ||
                  (overlay.component === 24 && operation.component === 23)),
            )
          )
            return this.failure(index, "InvalidValue");
          const id = this.nextHandle++;
          this.overlays.set(id, {
            entity: entity.id,
            component: operation.component,
            owner: this.resolveOverlayRef(operation.owner, local),
          });
          local.set(operation.alias, id);
          stateOverlays.push({
            alias: operation.alias,
            id,
            kind: "componentStateOverlay",
            entity: entity.id,
          });
          break;
        }
        case "updateComponentStateOverlay":
        case "updateDynamicComponentStateOverlay": {
          const id = this.resolveOverlayRef(operation.overlay, local);
          if (!this.overlays.has(id))
            return this.failure(index, "UnknownOverlay");
          break;
        }
        case "releaseComponentStateOverlay": {
          const id = this.resolveOverlayRef(operation.overlay, local);
          if (!this.overlays.delete(id))
            return this.failure(index, "UnknownOverlay");
          break;
        }
        case "releaseEntityOverlayBinding": {
          const id = this.resolveOverlayRef(operation.binding, local);
          const binding = this.bindings.get(id);
          if (!binding) return this.failure(index, "UnknownBinding");
          if (binding.mode === "owned") this.entities.delete(binding.entity);
          this.bindings.delete(id);
          break;
        }
        case "releaseStateOverlayOwner": {
          const id = this.resolveOverlayRef(operation.owner, local);
          for (const [bindingId, binding] of [...this.bindings]) {
            if (binding.owner !== id) continue;
            if (binding.mode === "owned") this.entities.delete(binding.entity);
            this.bindings.delete(bindingId);
          }
          for (const [overlayId, overlay] of [...this.overlays]) {
            if (overlay.owner === id || !this.entities.has(overlay.entity))
              this.overlays.delete(overlayId);
          }
          this.owners.delete(id);
          break;
        }
        case "insertComponent": {
          if (this.failNextInsertComponent) {
            this.failNextInsertComponent = false;
            return this.failure(index, "InvalidValue");
          }
          const entity = this.entities.get(
            this.resolveEntityRef(operation.entity, local),
          );
          if (!entity) return this.failure(index, "InvalidEntity");
          if (operation.component === GUI_ROOT_COMPONENT) {
            if (entity.gui !== null) return this.failure(index, "InvalidValue");
            entity.gui = {
              incarnation: this.nextIncarnation++,
              nodes: new Map(),
              order: new Map(),
              usedIds: new Set(),
            };
            this.producerCreates += 1;
          }
          break;
        }
        case "removeComponent": {
          const entity = this.entities.get(
            this.resolveEntityRef(operation.entity, local),
          );
          if (!entity) return this.failure(index, "InvalidEntity");
          if (operation.component === GUI_ROOT_COMPONENT) {
            if (entity.gui === null)
              return this.failure(index, "MissingComponent");
            entity.gui = null;
            this.producerRemoves += 1;
          }
          break;
        }
        default:
          return this.failure(index, "UnsupportedMockOperation");
      }
    }
    return {
      batchId: this.nextBatch++,
      tick: this.tick++,
      aliases: [],
      stateOverlays,
      ok: true,
    };
  }

  private fenced(handle: GuiNodeHandle): {
    root: MockGuiRoot;
    node: MockGuiNode;
  } {
    const entity = this.entities.get(handle.entity);
    const root = entity?.gui;
    if (!root) throw rejected("MissingComponent");
    if (handle.session !== this.session) throw rejected("InvalidHandle");
    if (handle.rootIncarnation !== root.incarnation)
      throw rejected("InvalidHandle");
    const node = root.nodes.get(handle.nodeId);
    if (!node || node.lifetime !== handle.nodeLifetime)
      throw rejected("InvalidHandle");
    return { root, node };
  }

  async editGui(edit: GuiEdit): Promise<void> {
    this.edits.push(edit);
    if (this.failNextEdit) {
      this.failNextEdit = false;
      throw rejected("InvalidValue");
    }
    switch (edit.action) {
      case "insert": {
        const entity = this.entities.get(edit.entity);
        const root = entity?.gui;
        if (!root) throw rejected("MissingComponent");
        if (edit.rootIncarnation !== root.incarnation)
          throw rejected("InvalidValue");
        if (edit.id <= 0 || root.usedIds.has(edit.id))
          throw rejected("InvalidValue");
        if (edit.parent !== undefined && !root.nodes.has(edit.parent))
          throw rejected("InvalidValue");
        const siblings = root.order.get(edit.parent) ?? [];
        if (edit.index < 0 || edit.index > siblings.length)
          throw rejected("InvalidValue");
        root.nodes.set(edit.id, {
          id: edit.id,
          parent: edit.parent,
          lifetime: 1,
          content: edit.content,
          style: edit.style ?? {},
        });
        root.usedIds.add(edit.id);
        root.order.set(edit.parent, [
          ...siblings.slice(0, edit.index),
          edit.id,
          ...siblings.slice(edit.index),
        ]);
        break;
      }
      case "update": {
        const { node } = this.fenced(edit.handle);
        if (edit.patch.content !== undefined) node.content = edit.patch.content;
        if (edit.patch.style !== undefined) {
          const current = { ...node.style } as Record<string, unknown>;
          for (const [key, value] of Object.entries(edit.patch.style))
            if (value === null || value === undefined) delete current[key];
            else current[key] = value;
          node.style = current as unknown as GuiNodeStyle;
        }
        break;
      }
      case "move": {
        const { root, node } = this.fenced(edit.handle);
        if (edit.parent !== undefined && !root.nodes.has(edit.parent))
          throw rejected("InvalidValue");
        const from = root.order.get(node.parent) ?? [];
        root.order.set(
          node.parent,
          from.filter((id) => id !== node.id),
        );
        const siblings = root.order.get(edit.parent) ?? [];
        if (edit.index < 0 || edit.index > siblings.length)
          throw rejected("InvalidValue");
        node.parent = edit.parent;
        root.order.set(edit.parent, [
          ...siblings.slice(0, edit.index),
          node.id,
          ...siblings.slice(edit.index),
        ]);
        break;
      }
      case "remove": {
        const { root, node } = this.fenced(edit.handle);
        const doomed = [node.id];
        for (let scan = 0; scan < doomed.length; scan += 1)
          for (const [id, candidate] of root.nodes)
            if (candidate.parent === doomed[scan]) doomed.push(id);
        for (const id of doomed) {
          root.nodes.delete(id);
          root.order.delete(id);
        }
        const siblings = root.order.get(node.parent) ?? [];
        root.order.set(
          node.parent,
          siblings.filter((id) => id !== node.id),
        );
        break;
      }
      case "setControlValue":
        throw rejected("UnexpectedControlWrite");
    }
  }

  async editGuiBatch(
    edits: readonly GuiEdit[],
  ): Promise<import("@ipp/client").GuiEditBatchOutcome> {
    let applied = 0;
    for (const edit of edits) {
      try {
        await this.editGui(edit);
        applied += 1;
      } catch (error) {
        return {
          ok: false,
          applied,
          requests: 1,
          error: {
            kind: "runtime",
            reason: error instanceof Error ? error.message : String(error),
          },
        };
      }
    }
    return { ok: true, applied, requests: 1 };
  }

  async inspectGui(query: {
    entity: bigint;
    nodeId?: number;
    maxDepth?: number;
    limit?: number;
  }): Promise<GuiInspectResponse> {
    const entity = this.entities.get(query.entity);
    const root = entity?.gui;
    if (!root) throw rejected("MissingComponent");
    const nodes: GuiInspectedNode[] = [];
    const visit = (id: number): void => {
      const node = root.nodes.get(id);
      if (!node) return;
      nodes.push({
        id: node.id,
        ...(node.parent === undefined ? {} : { parent: node.parent }),
        lifetime: node.lifetime,
        controlRevision: 0,
        children: root.order.get(node.id) ?? [],
        content: node.content,
        controlValue: { kind: "none" },
        style: node.style,
      });
      for (const child of root.order.get(node.id) ?? []) visit(child);
    };
    for (const id of root.order.get(undefined) ?? []) visit(id);
    return {
      rootEntity: entity!.id,
      rootIncarnation: root.incarnation,
      nodes,
    };
  }

  createGuiNodeHandle(
    entity: bigint,
    rootIncarnation: bigint,
    nodeId: number,
    nodeLifetime: number,
  ): GuiNodeHandle {
    return guiNodeHandle(
      this.session,
      entity,
      rootIncarnation,
      nodeId,
      nodeLifetime,
    );
  }

  resourceCounts(): {
    owners: number;
    bindings: number;
    overlays: number;
  } {
    return {
      owners: this.owners.size,
      bindings: this.bindings.size,
      overlays: this.overlays.size,
    };
  }
}

function mockClient(): MockGuiClient {
  return new MockGuiClient();
}

function asClient(mock: MockGuiClient): ReactWorldClient {
  return mock as unknown as ReactWorldClient;
}

function onlyEntity(mock: MockGuiClient): bigint {
  const entities = [...mock.entities.values()];
  assert.equal(entities.length, 1);
  return entities[0]!.id;
}

function panelElement(text: string) {
  return h(
    Entity,
    { id: "panel", key: "panel" },
    h(Surface, { width: 4, height: 3 }),
    h(GuiRoot, null, h(Row, null, h(Text, { text }))),
  );
}

function particlePanel(meshes: boolean) {
  return h(
    Entity,
    { id: "particle-panel", key: "particle-panel" },
    h(Surface, { width: 4, height: 3 }),
    h(GuiRoot, null, h(Row, null, h(Text, { text: "particles" }))),
    meshes
      ? h(
          Fragment,
          null,
          h(ParticleMesh, { source: "memory:cube" }),
          h(UnlitMaterial, { r: 0.5 }),
        )
      : h(ParticleSprite, { r: 1 }),
  );
}

function panelWithRef(text: string, ref: { current: GuiNodeHandle | null }) {
  return h(
    Entity,
    { id: "panel", key: "panel" },
    h(Surface, { width: 4, height: 3 }),
    h(GuiRoot, null, h(Row, null, h(Text, { text, nodeRef: ref }))),
  );
}

function panelWithAction(text: string, onAction: () => void) {
  return h(
    Entity,
    { id: "panel", key: "panel" },
    h(Surface, { width: 4, height: 3 }),
    h(GuiRoot, null, h(Row, null, h(Text, { text, onAction }))),
  );
}

function boundPanelElement(symbolicId: string, text: string) {
  return h(
    Entity,
    { bindTo: symbolicId, key: symbolicId },
    h(Surface, { width: 4, height: 3 }),
    h(GuiRoot, null, h(Row, null, h(Text, { text }))),
  );
}

function panelAndBoundElement(
  panelText: string,
  symbolicId: string,
  boundText: string,
) {
  return h(
    StrictMode,
    null,
    h(
      Entity,
      { id: "panel", key: "panel" },
      h(Surface, { width: 4, height: 3 }),
      h(GuiRoot, null, h(Row, null, h(Text, { text: panelText }))),
    ),
    h(
      Entity,
      { bindTo: symbolicId, key: "bound" },
      h(Surface, { width: 4, height: 3 }),
      h(GuiRoot, null, h(Row, null, h(Text, { text: boundText }))),
    ),
  );
}

test("mount creates the producer root without any GuiRoot overlay", async () => {
  const mock = mockClient();
  const errors: Error[] = [];
  const root = createRoot(asClient(mock), {
    onError: (error) => {
      errors.push(error);
    },
  });
  const ref: { current: GuiNodeHandle | null } = { current: null };
  await root.render(
    h(
      Entity,
      { id: "panel" },
      h(Surface, { width: 4, height: 3 }),
      h(GuiRoot, null, h(Row, null, h(Text, { text: "hi", nodeRef: ref }))),
    ),
  );
  assert.equal(mock.producerCreates, 1);
  assert.equal(mock.producerRemoves, 0);
  for (const operations of mock.batches)
    for (const operation of operations)
      assert.equal(
        operation.kind === "attachComponentStateOverlay" &&
          operation.component === GUI_ROOT_COMPONENT,
        false,
        "a GuiRoot overlay was submitted",
      );
  const inspection = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.deepEqual(
    inspection.nodes.map((node) => node.content),
    [
      { kind: "container", containerKind: "row" },
      { kind: "text", text: "hi" },
    ],
  );
  assert.ok(ref.current !== null);
  assert.equal(ref.current.nodeId, 2);
  assert.equal(ref.current.rootIncarnation, inspection.rootIncarnation);
  assert.deepEqual(errors, []);
  await root.unmount();
});

test("updates patch content without touching the producer", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  await root.render(panelElement("hi"));
  const incarnation = (await mock.inspectGui({ entity: onlyEntity(mock) }))
    .rootIncarnation;
  const editsBefore = mock.edits.length;
  const batchesBefore = mock.batches.length;
  await root.render(panelElement("hello"));
  assert.equal(mock.producerCreates, 1);
  assert.equal(mock.producerRemoves, 0);
  assert.equal(mock.batches.length, batchesBefore);
  const updateEdits = mock.edits.slice(editsBefore);
  assert.equal(updateEdits.length, 1);
  assert.equal(updateEdits[0]!.action, "update");
  const after = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.equal(after.rootIncarnation, incarnation);
  assert.deepEqual(
    after.nodes.map((node) => node.id),
    [1, 2],
  );
  assert.deepEqual(after.nodes[1]!.content, {
    kind: "text",
    text: "hello",
  });
  // A listener-only change resubmits nothing: listeners are JS-only.
  const editsBeforeListeners = mock.edits.length;
  await root.render(panelWithAction("hello", () => {}));
  assert.equal(mock.edits.length, editsBeforeListeners);
  await root.unmount();
});

test("particle replacement stays release-first on an entity with a GuiRoot", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), { onError: () => {} });
  await root.render(particlePanel(false));
  const batchesBefore = mock.batches.length;

  await root.render(particlePanel(true));

  assert.deepEqual(
    mock.batches
      .slice(batchesBefore)
      .map((batch) => batch.map((operation) => operation.kind)),
    [
      [
        "releaseComponentStateOverlay",
        "attachComponentStateOverlay",
        "attachComponentStateOverlay",
      ],
    ],
  );
  assert.equal(mock.producerCreates, 1);
  assert.equal(mock.producerRemoves, 0);
  await root.unmount();
});

test("strict mode keeps one producer with stable identities", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  await root.render(h(StrictMode, null, panelElement("hi")));
  await root.render(h(StrictMode, null, panelElement("hi")));
  assert.equal(mock.producerCreates, 1);
  const inspection = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.deepEqual(
    inspection.nodes.map((node) => node.id),
    [1, 2],
  );
  await root.unmount();
});

test("unmount removes the tree and the created producer", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  const ref: { current: GuiNodeHandle | null } = { current: null };
  await root.render(panelWithRef("hi", ref));
  assert.ok(ref.current !== null);
  await root.unmount();
  assert.equal(ref.current, null);
  assert.equal(mock.entities.size, 0);
  assert.equal(mock.producerRemoves, 1);
});

test("remount adopts a fresh incarnation and fences stale handles", async () => {
  const mock = mockClient();
  const first = createRoot(asClient(mock), {
    onError: () => {},
  });
  const ref: { current: GuiNodeHandle | null } = { current: null };
  await first.render(panelWithRef("hi", ref));
  const stale = ref.current;
  assert.ok(stale !== null);
  const firstIncarnation = stale.rootIncarnation;
  await first.unmount();
  const second = createRoot(asClient(mock), {
    onError: () => {},
  });
  await second.render(panelElement("hi"));
  assert.equal(mock.producerCreates, 2);
  const inspection = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.notEqual(inspection.rootIncarnation, firstIncarnation);
  await assert.rejects(
    mock.editGui({ action: "remove", handle: stale }),
    /MissingComponent|InvalidHandle/,
  );
  await second.unmount();
});

async function externalProducer(
  mock: MockGuiClient,
  symbolicId: string,
): Promise<bigint> {
  const setup = await mock.batch([
    { kind: "createStateOverlayOwner", alias: 1 },
    {
      kind: "attachEntityOverlayBinding",
      owner: { kind: "alias", alias: 1 },
      alias: 2,
      symbolicId,
      mode: "owned",
    },
  ]);
  assert.ok(setup.ok);
  const entity = setup.stateOverlays.find(
    (entry) => entry.kind === "entityOverlayBinding",
  )!.entity!;
  const created = await mock.batch([
    {
      kind: "insertComponent",
      entity: { kind: "handle", id: entity },
      component: GUI_ROOT_COMPONENT,
      fields: [],
    },
  ]);
  assert.ok(created.ok);
  return entity;
}

test("a pre-existing empty producer is adopted and survives unmount", async () => {
  const mock = mockClient();
  const external = await externalProducer(mock, "external");
  const createsBefore = mock.producerCreates;
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  await root.render(boundPanelElement("external", "hi"));
  assert.equal(mock.producerCreates, createsBefore);
  const inspection = await mock.inspectGui({ entity: external });
  assert.equal(inspection.nodes.length, 2);
  // Unmount removes the reconciler's nodes but never an adopted producer.
  await root.unmount();
  assert.equal(mock.producerRemoves, 0);
  const after = await mock.inspectGui({ entity: external });
  assert.equal(after.nodes.length, 0);
});

test("an occupied foreign root is refused without damage", async () => {
  const mock = mockClient();
  const foreign = await externalProducer(mock, "foreign");
  const empty = await mock.inspectGui({ entity: foreign });
  await mock.editGui({
    action: "insert",
    entity: foreign,
    rootIncarnation: empty.rootIncarnation,
    id: 1,
    index: 0,
    content: { kind: "text", text: "foreign" },
  });
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  await assert.rejects(
    root.render(boundPanelElement("foreign", "hi")),
    /another writer/,
  );
  const inspection = await mock.inspectGui({ entity: foreign });
  assert.equal(inspection.nodes.length, 1);
  assert.deepEqual(inspection.nodes[0]!.content, {
    kind: "text",
    text: "foreign",
  });
  await root.unmount();
});

test("a rejected adoption cleanup retains exact handles and retries", async () => {
  const mock = mockClient();
  const errors: Error[] = [];
  const root = createRoot(asClient(mock), {
    onError: (error) => {
      errors.push(error);
    },
  });
  await root.render(h(StrictMode, null, panelElement("hello")));
  const panel = onlyEntity(mock);
  const incarnation = (await mock.inspectGui({ entity: panel }))
    .rootIncarnation;
  const foreign = await externalProducer(mock, "foreign");
  const empty = await mock.inspectGui({ entity: foreign });
  await mock.editGui({
    action: "insert",
    entity: foreign,
    rootIncarnation: empty.rootIncarnation,
    id: 1,
    index: 0,
    content: { kind: "text", text: "foreign" },
  });

  mock.failReleaseBatches = 1;
  await assert.rejects(
    root.render(panelAndBoundElement("hello", "foreign", "refused")),
    /another writer/,
  );
  assert.ok(
    errors.some((error) => /SimulatedReleaseFailure/.test(error.message)),
  );
  assert.deepEqual(
    (await mock.inspectGui({ entity: foreign })).nodes.map(
      (node) => node.content,
    ),
    [{ kind: "text", text: "foreign" }],
  );

  await mock.editGui({
    action: "remove",
    handle: mock.createGuiNodeHandle(foreign, empty.rootIncarnation, 1, 1),
  });
  const removed = await mock.batch([
    {
      kind: "removeComponent",
      entity: { kind: "handle", id: foreign },
      component: GUI_ROOT_COMPONENT,
    },
  ]);
  assert.ok(removed.ok);

  await root.render(panelAndBoundElement("hello", "foreign", "recovered"));
  assert.equal(
    (await mock.inspectGui({ entity: panel })).rootIncarnation,
    incarnation,
    "retry recreated the previously acknowledged panel",
  );
  assert.ok(
    (await mock.inspectGui({ entity: foreign })).nodes.some(
      (node) =>
        node.content.kind === "text" && node.content.text === "recovered",
    ),
  );
  const releaseBatches = mock.batches.filter((batch) =>
    batch.some((operation) => operation.kind.startsWith("release")),
  );
  assert.ok(releaseBatches.length >= 2, "the retained cleanup was not retried");
  await root.unmount();
});

test("missing binding entity enrichment stays disposable and recoverable", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  mock.omitBindingEntity = true;
  await assert.rejects(
    root.render(panelElement("missing")),
    /omitted entity identity/,
  );
  assert.deepEqual(mock.resourceCounts(), {
    owners: 1,
    bindings: 1,
    overlays: 1,
  });

  mock.omitBindingEntity = false;
  await root.render(panelElement("recovered"));
  assert.deepEqual(mock.resourceCounts(), {
    owners: 1,
    bindings: 1,
    overlays: 1,
  });
  const inspection = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.ok(
    inspection.nodes.some(
      (node) =>
        node.content.kind === "text" && node.content.text === "recovered",
    ),
  );
  await root.unmount();
  assert.deepEqual(mock.resourceCounts(), {
    owners: 0,
    bindings: 0,
    overlays: 0,
  });
});

test("producer creation failure surfaces and the next render recovers", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  mock.failNextInsertComponent = true;
  await assert.rejects(root.render(panelElement("hi")), /rejected/);
  assert.equal(mock.producerCreates, 0);
  // Rejected commits never retry unchanged work: render corrected content.
  await root.render(panelElement("hi!"));
  assert.equal(mock.producerCreates, 1);
  const inspection = await mock.inspectGui({ entity: onlyEntity(mock) });
  assert.deepEqual(
    inspection.nodes.map((node) => node.content),
    [
      { kind: "container", containerKind: "row" },
      { kind: "text", text: "hi!" },
    ],
  );
  await root.unmount();
});

test("edit failure during flush surfaces and unmount still cleans up", async () => {
  const mock = mockClient();
  const root = createRoot(asClient(mock), {
    onError: () => {},
  });
  mock.failNextEdit = true;
  await assert.rejects(root.render(panelElement("hi")), /InvalidValue/);
  assert.equal(mock.producerCreates, 1);
  const entity = onlyEntity(mock);
  assert.equal((await mock.inspectGui({ entity })).nodes.length, 0);
  await root.render(panelElement("hi!"));
  assert.equal(mock.producerCreates, 1);
  assert.equal((await mock.inspectGui({ entity })).nodes.length, 2);
  await root.unmount();
  assert.equal(mock.entities.size, 0);
  assert.equal(mock.producerRemoves, 1);
});

test("the mock rejects overlay root declarations like production", async () => {
  const mock = mockClient();
  const setup = await mock.batch([
    { kind: "createStateOverlayOwner", alias: 1 },
    {
      kind: "attachEntityOverlayBinding",
      owner: { kind: "alias", alias: 1 },
      alias: 2,
      symbolicId: "panel",
      mode: "owned",
    },
  ]);
  assert.ok(setup.ok);
  const owner = setup.stateOverlays.find((entry) => entry.kind === "owner")!.id;
  const binding = setup.stateOverlays.find(
    (entry) => entry.kind === "entityOverlayBinding",
  )!.id;
  const entity = setup.stateOverlays.find(
    (entry) => entry.kind === "entityOverlayBinding",
  )!.entity!;
  const overlay = (
    mode: "auto" | "bound" | "owned",
    alias: number,
  ): Command => ({
    kind: "attachComponentStateOverlay",
    owner: { kind: "handle", id: owner },
    binding: { kind: "handle", id: binding },
    alias,
    component: GUI_ROOT_COMPONENT,
    mode,
    fields: [],
  });
  // Auto and Owned creation is rejected, matching production InvalidField.
  for (const mode of ["auto", "owned"] as const) {
    const outcome = await mock.batch([overlay(mode, 3)]);
    assert.equal(outcome.ok, false);
    if (!outcome.ok) {
      assert.equal(outcome.error.scope, "operation");
      assert.match(outcome.error.reason, /InvalidField/);
    }
  }
  // A Bound overlay without a producer is still rejected.
  const unbound = await mock.batch([overlay("bound", 3)]);
  assert.equal(unbound.ok, false);
  // A Bound property-only overlay on an existing producer is accepted,
  // preserving the bound workaround for pre-created roots.
  const created = await mock.batch([
    {
      kind: "insertComponent",
      entity: { kind: "handle", id: entity },
      component: GUI_ROOT_COMPONENT,
      fields: [],
    },
  ]);
  assert.ok(created.ok);
  const bound = await mock.batch([overlay("bound", 4)]);
  assert.ok(bound.ok);
});

function describePanel(mock: MockGuiClient, withListeners: boolean) {
  const tree = new ReactWorldTree(asClient(mock));
  const noop = (): void => {};
  const entity = tree.instance("ipp-entity", { id: "e" });
  const surface = tree.instance("ipp-surface", { width: 1, height: 1 });
  const root = tree.instance("ipp-gui-root", { bound: false });
  const row = tree.instance("ipp-gui-row", {});
  const button = tree.instance("ipp-gui-button", {
    label: "go",
    ...(withListeners ? { onPress: noop, onAction: noop } : {}),
  });
  const checkbox = tree.instance("ipp-gui-checkbox", {
    ...(withListeners ? { onToggle: noop } : {}),
  });
  const slider = tree.instance("ipp-gui-slider", {
    value: 0.5,
    ...(withListeners ? { onScalarCommit: noop } : {}),
  });
  const input = tree.instance("ipp-gui-text-input", {
    text: "a",
    ...(withListeners ? { onTextCommit: noop } : {}),
  });
  entity.children.push(surface, root);
  root.children.push(row);
  row.children.push(button, checkbox, slider, input);
  tree.children.push(entity);
  return { description: tree.describe(), noop };
}

test("listeners are retained JS-only without transport", () => {
  const mock = mockClient();
  const { description, noop } = describePanel(mock, true);
  // No overlay is ever described for GuiRoot, whatever the bound prop says.
  assert.equal(
    description.overlays.some(
      (overlay) => overlay.component === GUI_ROOT_COMPONENT,
    ),
    false,
  );
  const nodes = description.gui[0]!.nodes;
  const callbacks = (type: string) =>
    retainedNodeCallbacks(nodes.find((node) => node.type === type)!);
  assert.equal(callbacks("ipp-gui-button").onPress, noop);
  assert.equal(callbacks("ipp-gui-button").onAction, noop);
  assert.equal(callbacks("ipp-gui-checkbox").onToggle, noop);
  assert.equal(callbacks("ipp-gui-slider").onScalarCommit, noop);
  assert.equal(callbacks("ipp-gui-text-input").onTextCommit, noop);
  // Listeners never reach the commit signature.
  const plain = describePanel(mock, false).description;
  assert.equal(
    guiRootSignature(description.gui[0]!.nodes),
    guiRootSignature(plain.gui[0]!.nodes),
  );
  // Nodes built elsewhere report no listeners.
  const foreign: GuiDescribedNode = {
    identity: 1,
    parent: undefined,
    type: "ipp-gui-text",
    content: { kind: "text", text: "x" },
    style: {},
    nodeRef: null,
    onAction: undefined,
    onActionCapture: undefined,
  };
  assert.deepEqual(retainedNodeCallbacks(foreign), {
    onAction: undefined,
    onActionCapture: undefined,
    onPress: undefined,
    onToggle: undefined,
    onScalarCommit: undefined,
    onTextCommit: undefined,
  });
});
