/** Ambiguous insert recovery and allocation high-water preservation (R19).
 *
 * Headless and node-runnable: a fake GUI runtime applies edits, drops
 * selected responses and answers inspections. Real runtime coverage stays in
 * the pipeline `react` suite.
 */
import assert from "node:assert/strict";
import test from "node:test";
import type {
  BatchOutcome,
  Command,
  GuiEdit,
  GuiInspectedNode,
  GuiInspectResponse,
  GuiNodeHandle,
  GuiNodeStyle,
} from "@ipp/client";
import type { ReactWorldClient } from "../src/contract.js";
import type {
  GuiDescribedNode,
  GuiDescribedRoot,
} from "../src/gui/description.js";
import { GuiCommits } from "../src/gui/commits.js";
import type { GuiControlTheme } from "../src/gui/theme.js";
import {
  GUI_CHECKBOX_HOST_TYPE,
  GUI_COLUMN_HOST_TYPE,
  type GuiNodeRef,
} from "../src/gui/components.js";

type FailureMode = "loss" | "reject";

interface FakeStoredNode {
  id: number;
  parent: number | undefined;
  content: GuiDescribedNode["content"];
  style: GuiNodeStyle;
  lifetime: number;
}

function rejected(): Error {
  return Object.assign(new Error("rejected"), {
    code: "IPP_REQUEST_REJECTED",
  });
}

function isRejected(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === "IPP_REQUEST_REJECTED"
  );
}

/** Minimal producer runtime: applies GUI edits, loses chosen responses. */
class FakeGuiRuntime {
  readonly entity = 100n;
  rootIncarnation = 1n;
  producerLive = false;
  producerCreates = 0;
  producerRemoves = 0;
  producerRemoveFailures = 0;
  guiBatchRequests = 0;
  readonly failureSnapshots: number[][] = [];
  readonly nodes = new Map<number, FakeStoredNode>();
  readonly dynamicCommands: Command[] = [];
  private readonly failures: Array<{
    action: GuiEdit["action"];
    mode: FailureMode;
    remaining: number;
  }> = [];

  failNext(action: GuiEdit["action"], mode: FailureMode): void {
    this.failures.push({ action, mode, remaining: 0 });
  }

  failAfter(
    action: GuiEdit["action"],
    successful: number,
    mode: FailureMode,
  ): void {
    this.failures.push({ action, mode, remaining: successful });
  }

  failNextProducerRemove(): void {
    this.producerRemoveFailures += 1;
  }

  injectForeign(node: FakeStoredNode): void {
    this.nodes.set(node.id, node);
  }

  removeForeign(id: number): void {
    this.nodes.delete(id);
  }

  private takeFailure(action: GuiEdit["action"]): FailureMode | undefined {
    const index = this.failures.findIndex(
      (failure) => failure.action === action,
    );
    if (index === -1) return undefined;
    const failure = this.failures[index]!;
    if (failure.remaining > 0) {
      failure.remaining -= 1;
      return undefined;
    }
    return this.failures.splice(index, 1)[0]!.mode;
  }

  async editGui(edit: GuiEdit): Promise<void> {
    const mode = this.takeFailure(edit.action);
    if (edit.action === "insert") {
      if (mode !== "reject") {
        if (this.nodes.has(edit.id)) throw rejected();
        this.nodes.set(edit.id, {
          id: edit.id,
          parent: edit.parent,
          content: edit.content,
          style: { ...edit.style },
          lifetime: 1,
        });
      }
      if (mode === "reject") throw rejected();
      if (mode === "loss") throw new Error("response lost");
      return;
    }
    if (edit.action === "remove") {
      if (mode !== "reject") this.removeSubtree(edit.handle.nodeId);
      if (mode === "reject") throw rejected();
      if (mode === "loss") throw new Error("response lost");
      return;
    }
    if (edit.action === "update") {
      if (mode !== "reject") {
        const node = this.nodes.get(edit.handle.nodeId);
        if (!node) throw rejected();
        if (edit.patch.content !== undefined) node.content = edit.patch.content;
        if (edit.patch.style?.opacity !== undefined)
          node.style = { ...node.style, opacity: edit.patch.style.opacity };
      }
      if (mode === "reject") throw rejected();
      if (mode === "loss") throw new Error("response lost");
      return;
    }
    if (edit.action === "move") {
      if (mode !== "reject") {
        const node = this.nodes.get(edit.handle.nodeId);
        if (!node) throw rejected();
        if (edit.parent !== undefined) node.parent = edit.parent;
      }
      if (mode === "reject") throw rejected();
      if (mode === "loss") throw new Error("response lost");
    }
  }

  async editGuiBatch(
    edits: readonly GuiEdit[],
  ): Promise<import("@ipp/client").GuiEditBatchOutcome> {
    this.guiBatchRequests += 1;
    let applied = 0;
    for (const edit of edits) {
      try {
        await this.editGui(edit);
        applied += 1;
      } catch (error) {
        if (isRejected(error)) this.failureSnapshots.push(liveIds(this));
        if (isRejected(error))
          return {
            ok: false,
            applied,
            requests: 1,
            error: { kind: "runtime", reason: "injected rejection" },
          };
        throw error;
      }
    }
    return { ok: true, applied, requests: 1 };
  }

  private removeSubtree(id: number): void {
    const doomed = [id];
    for (let index = 0; index < doomed.length; index += 1) {
      const current = doomed[index]!;
      for (const node of this.nodes.values())
        if (node.parent === current) doomed.push(node.id);
    }
    for (const gone of doomed) this.nodes.delete(gone);
  }

  async inspectGui(query: { entity: bigint }): Promise<GuiInspectResponse> {
    assert.equal(query.entity, this.entity);
    if (!this.producerLive) throw rejected();
    const nodes = [...this.nodes.values()]
      .sort((a, b) => a.id - b.id)
      .map(
        (node) =>
          ({
            id: node.id,
            parent: node.parent,
            children: [...this.nodes.values()]
              .filter((child) => child.parent === node.id)
              .map((child) => child.id),
            content: node.content,
            style: { ...node.style },
            controlValue: { kind: "none" },
            controlRevision: 0,
            lifetime: node.lifetime,
          }) as unknown as GuiInspectedNode,
      );
    return {
      rootEntity: this.entity,
      rootIncarnation: this.rootIncarnation,
      nodes,
    } as unknown as GuiInspectResponse;
  }

  async batch(commands: Command[]): Promise<BatchOutcome> {
    for (const command of commands) {
      if (command.kind === "insertComponent") {
        this.producerLive = true;
        this.producerCreates += 1;
      } else if (command.kind === "removeComponent") {
        if (this.producerRemoveFailures > 0) {
          this.producerRemoveFailures -= 1;
          return {
            ok: false,
            batchId: 1n,
            tick: 1n,
            aliases: [],
            stateOverlays: [],
            error: {
              scope: "operation",
              operation: 0,
              reason: "injected producer removal rejection",
            },
          };
        }
        this.producerLive = false;
        this.producerRemoves += 1;
      } else if (
        command.kind === "setDynamicProperty" ||
        command.kind === "removeDynamicProperty"
      ) {
        this.dynamicCommands.push(command);
      }
    }
    return { ok: true } as unknown as BatchOutcome;
  }

  createGuiNodeHandle(
    entity: bigint,
    rootIncarnation: bigint,
    nodeId: number,
    nodeLifetime: number,
  ): GuiNodeHandle {
    return {
      session: 7n,
      entity,
      rootIncarnation,
      nodeId,
      nodeLifetime,
    } as GuiNodeHandle;
  }

  client(): ReactWorldClient {
    return {
      session: 7n,
      schemaHash: "test",
      capabilities: [],
      components: { GuiRoot: { id: 26 } },
      batch: (commands: Command[]) => this.batch(commands),
      onDiagnostic: () => {},
      editGui: (edit: GuiEdit) => this.editGui(edit),
      editGuiBatch: (edits: readonly GuiEdit[]) => this.editGuiBatch(edits),
      inspectGui: (query: { entity: bigint }) => this.inspectGui(query),
      createGuiNodeHandle: (
        entity: bigint,
        rootIncarnation: bigint,
        nodeId: number,
        nodeLifetime: number,
      ) =>
        this.createGuiNodeHandle(entity, rootIncarnation, nodeId, nodeLifetime),
    } as unknown as ReactWorldClient;
  }

  commits(): GuiCommits {
    return new GuiCommits(this.client(), {
      checkSession: () => {},
      report: (error: unknown) =>
        error instanceof Error ? error : new Error(String(error)),
    });
  }
}

function ref(): { current: GuiNodeHandle | null } {
  return { current: null };
}

function containerNode(
  identity: number,
  nodeRef: GuiNodeRef,
  style: GuiNodeStyle = {},
  theme?: GuiControlTheme,
): GuiDescribedNode {
  return {
    identity,
    parent: undefined,
    type: GUI_COLUMN_HOST_TYPE,
    content: { kind: "container", containerKind: "column" },
    style,
    nodeRef,
    onAction: undefined,
    onActionCapture: undefined,
    ...(theme === undefined ? {} : { theme }),
  };
}

function checkboxNode(
  identity: number,
  parent: number,
  checked: boolean,
  nodeRef: GuiNodeRef,
  style: GuiNodeStyle = {},
): GuiDescribedNode {
  return {
    identity,
    parent,
    type: GUI_CHECKBOX_HOST_TYPE,
    content: { kind: "checkbox", checked },
    style,
    nodeRef,
    onAction: undefined,
    onActionCapture: undefined,
  };
}

function describe(nodes: GuiDescribedNode[]): GuiDescribedRoot[] {
  return [
    {
      identity: 1,
      entity: 1,
      nodeRef: null,
      nodes,
      signature: JSON.stringify(
        nodes.map((node) => [node.identity, node.parent ?? null, node.content]),
      ),
    },
  ];
}

function liveIds(runtime: FakeGuiRuntime): number[] {
  return [...runtime.nodes.keys()].sort((a, b) => a - b);
}

test("a 50-node mount uses one GUI request", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const nodes = [containerNode(10, ref())];
  for (let identity = 11; identity < 60; identity += 1)
    nodes.push(checkboxNode(identity, 10, false, ref()));

  await commits.apply(describe(nodes), () => runtime.entity);

  assert.equal(runtime.nodes.size, 50);
  assert.equal(runtime.guiBatchRequests, 1);
});

test("a middle batch failure stops the suffix and recovery completes it", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const nodes = [containerNode(10, ref())];
  await commits.apply(describe(nodes), () => runtime.entity);
  runtime.failAfter("insert", 1, "reject");

  const desired = describe([
    ...nodes,
    checkboxNode(11, 10, false, ref()),
    checkboxNode(12, 10, false, ref()),
    checkboxNode(13, 10, false, ref()),
  ]);
  await assert.rejects(
    commits.apply(desired, () => runtime.entity),
    /rejected after 1 edits/,
  );

  assert.deepEqual(runtime.failureSnapshots, [[1, 2]]);
  await commits.apply(desired, () => runtime.entity);
  assert.deepEqual(liveIds(runtime), [1, 2, 3, 4]);
  assert.equal(runtime.guiBatchRequests, 3);
  await commits.dispose();
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, false);
});

test("response loss after an accepted insert adopts the produced id", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const resolveEntity = () => runtime.entity;
  const rootRef = ref();
  const checkRef = ref();
  const pair = () => [
    containerNode(10, rootRef),
    checkboxNode(11, 10, false, checkRef),
  ];
  await commits.apply(describe(pair()), resolveEntity);
  assert.deepEqual(liveIds(runtime), [1, 2]);
  assert.equal(rootRef.current?.nodeId, 1);
  assert.equal(checkRef.current?.nodeId, 2);
  // The runtime applies the next insert but its response is lost.
  const extraRef = ref();
  runtime.failNext("insert", "loss");
  await commits.apply(
    describe([...pair(), checkboxNode(12, 10, false, extraRef)]),
    resolveEntity,
  );
  // Recovery adopted the produced id instead of rejecting it as foreign.
  assert.deepEqual(liveIds(runtime), [1, 2, 3]);
  assert.equal(extraRef.current?.nodeId, 3);
  // Stable refs and no duplicates on re-apply.
  await commits.apply(
    describe([
      containerNode(10, rootRef),
      checkboxNode(11, 10, false, checkRef),
      checkboxNode(12, 10, false, extraRef),
    ]),
    resolveEntity,
  );
  assert.deepEqual(liveIds(runtime), [1, 2, 3]);
  assert.equal(extraRef.current?.nodeId, 3);
});

test("removing the highest id never reuses it and unmount cleans up", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const resolveEntity = () => runtime.entity;
  const aRef = ref();
  const bRef = ref();
  const cRef = ref();
  const trio = () => [
    containerNode(10, aRef),
    checkboxNode(11, 10, false, bRef),
    checkboxNode(12, 10, false, cRef),
  ];
  await commits.apply(describe(trio()), resolveEntity);
  assert.deepEqual(liveIds(runtime), [1, 2, 3]);
  assert.equal(cRef.current?.nodeId, 3);
  // Remove the highest-id node; the allocator must not retreat below it.
  await commits.apply(
    describe([containerNode(10, aRef), checkboxNode(11, 10, false, bRef)]),
    resolveEntity,
  );
  assert.deepEqual(liveIds(runtime), [1, 2]);
  // New content after a response loss takes a fresh id, never the retired one.
  const dRef = ref();
  runtime.failNext("insert", "loss");
  await commits.apply(
    describe([
      containerNode(10, aRef),
      checkboxNode(11, 10, false, bRef),
      checkboxNode(13, 10, false, dRef),
    ]),
    resolveEntity,
  );
  assert.equal(dRef.current?.nodeId, 4);
  assert.deepEqual(liveIds(runtime), [1, 2, 4]);
  // The next allocation advances past adopted content.
  const eRef = ref();
  await commits.apply(
    describe([
      containerNode(10, aRef),
      checkboxNode(11, 10, false, bRef),
      checkboxNode(13, 10, false, dRef),
      checkboxNode(14, 10, false, eRef),
    ]),
    resolveEntity,
  );
  assert.equal(eRef.current?.nodeId, 5);
  assert.deepEqual(liveIds(runtime), [1, 2, 4, 5]);
  // Complete unmount cleanup: no nodes, no producer, no live refs.
  await commits.apply([], resolveEntity);
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, false);
  assert.equal(runtime.producerCreates, 1);
  assert.equal(runtime.producerRemoves, 1);
  for (const target of [aRef, bRef, cRef, dRef, eRef])
    assert.equal(target.current, null);
});

test("refusing foreign work retains produced ids for later recovery", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const resolveEntity = () => runtime.entity;
  const aRef = ref();
  await commits.apply(describe([containerNode(10, aRef)]), resolveEntity);
  assert.deepEqual(liveIds(runtime), [1]);
  // Another writer's node appears; an ambiguous update must refuse it.
  runtime.injectForeign({
    id: 99,
    parent: undefined,
    content: { kind: "container", containerKind: "column" },
    style: {},
    lifetime: 1,
  });
  runtime.failNext("update", "loss");
  await assert.rejects(
    commits.apply(
      describe([containerNode(10, aRef, { opacity: 0.5 })]),
      resolveEntity,
    ),
    /changed beneath/,
  );
  // The produced identity survived the refusal: clearing the foreign node
  // lets the same content recover without duplicates or retargeting.
  runtime.removeForeign(99);
  await commits.apply(
    describe([containerNode(10, aRef, { opacity: 0.5 })]),
    resolveEntity,
  );
  assert.equal(aRef.current?.nodeId, 1);
  assert.deepEqual(liveIds(runtime), [1]);
  assert.equal(runtime.nodes.get(1)?.style.opacity, 0.5);
});

test("rejected root removal is retained and retried before remount", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const rootRef = ref();
  await commits.apply(
    describe([containerNode(10, rootRef)]),
    () => runtime.entity,
  );
  runtime.failNext("remove", "reject");
  await assert.rejects(
    commits.apply([], () => runtime.entity),
    /rejected/,
  );
  assert.equal(runtime.nodes.size, 1);
  assert.equal(runtime.producerLive, true);
  assert.equal(rootRef.current, null);
  await commits.apply([], () => runtime.entity);
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, false);
  assert.equal(runtime.producerRemoves, 1);
});

test("rejected producer removal resumes after the acknowledged node phase", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  await commits.apply(
    describe([containerNode(10, ref())]),
    () => runtime.entity,
  );
  runtime.failNextProducerRemove();
  await assert.rejects(
    commits.apply([], () => runtime.entity),
    /producer cleanup rejected/,
  );
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, true);
  await commits.dispose();
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, false);
  assert.equal(runtime.producerRemoves, 1);
});

test("losing the bound entity uses the same retained cleanup path", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  await commits.apply(
    describe([containerNode(10, ref())]),
    () => runtime.entity,
  );
  runtime.failNext("remove", "reject");
  await assert.rejects(
    commits.apply(describe([]), () => undefined),
    /rejected/,
  );
  assert.equal(runtime.nodes.size, 1);
  await commits.apply(describe([]), () => undefined);
  assert.equal(runtime.nodes.size, 0);
  assert.equal(runtime.producerLive, false);
});

test("themes author GuiRoot named parts and equivalent rerenders are stable", async () => {
  const runtime = new FakeGuiRuntime();
  const commits = runtime.commits();
  const theme: GuiControlTheme = {
    parts: {
      background: {
        base: { color: [1, 0, 0, 1], opacity: 0.75, scale: [1, 1] },
        pressed: { color: [0, 1, 0, 1] },
      },
    },
  };
  await commits.apply(
    describe([containerNode(10, ref(), {}, theme)]),
    () => runtime.entity,
  );
  assert.ok(
    runtime.dynamicCommands.some(
      (command) =>
        command.kind === "setDynamicProperty" &&
        command.name === "node_1_part_background_pressed_color",
    ),
  );
  const count = runtime.dynamicCommands.length;
  await commits.apply(
    describe([
      containerNode(
        10,
        ref(),
        {},
        {
          parts: {
            background: {
              base: { color: [1, 0, 0, 1], opacity: 0.75, scale: [1, 1] },
              pressed: { color: [0, 1, 0, 1] },
            },
          },
        },
      ),
    ]),
    () => runtime.entity,
  );
  assert.equal(runtime.dynamicCommands.length, count);
  await commits.apply(
    describe([containerNode(10, ref())]),
    () => runtime.entity,
  );
  assert.ok(
    runtime.dynamicCommands.some(
      (command) =>
        command.kind === "removeDynamicProperty" &&
        command.name === "node_1_part_background_pressed_color",
    ),
  );
});
