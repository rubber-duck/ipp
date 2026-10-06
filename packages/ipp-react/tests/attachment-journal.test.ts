import assert from "node:assert/strict";
import test from "node:test";
import type { BatchOutcome, Command, ClientClosure } from "@ipp/client";
import { ReactWorldCommits } from "../src/reconciler/commits.js";
import { ReactWorldContainer } from "../src/reconciler/host-config.js";
import { ReactAttachmentGroup } from "../src/composition/attachment-state.js";
import { ReactWorldTree } from "../src/reconciler/tree.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";
import { attachmentIdentity } from "../src/composition/attachment-identity.js";
import {
  AttachedWorldSlot,
  describeAttachedWorld,
  type ReactCompositionHost,
} from "../src/composition/attached-world.js";

test("attachment identities preserve primitive types and canonical field order", () => {
  assert.notEqual(attachmentIdentity(123n), attachmentIdentity("123n"));
  assert.notEqual(
    attachmentIdentity(["bigint", "123"]),
    attachmentIdentity(123n),
  );
  assert.equal(
    attachmentIdentity({ id: 1n, incarnation: 2n }),
    attachmentIdentity({ incarnation: 2n, id: 1n }),
  );
  const props = {
    slot: new AttachedWorldSlot(),
    anchor: "123n",
    child: { borrow: { id: 1n, incarnation: 1n } },
    attachment: { mode: "surface-camera", output: { entity: "123n" } },
  };
  const original = describeAttachedWorld(props).signature;
  assert.notEqual(
    original,
    describeAttachedWorld({ ...props, anchor: 123n }).signature,
  );
  assert.notEqual(
    original,
    describeAttachedWorld({
      ...props,
      attachment: { mode: "surface-camera", output: { entity: 123n } },
    }).signature,
  );
  assert.notEqual(
    attachmentIdentity({ create: { metadata: { symbolicId: "123n" } } }),
    attachmentIdentity({ create: { metadata: { symbolicId: 123n } } }),
  );
});

function clientWith(batch: ReactWorldClient["batch"]): ReactWorldClient {
  return {
    session: 1n,
    schemaHash: 1n,
    components: {},
    batch,
  };
}

/** An outcome whose creations report `created` handles in alias order. */
function outcome(
  operations: Command[] = [],
  created: bigint[] = [],
): BatchOutcome {
  const aliases = operations.flatMap((command) =>
    command.kind === "create" ? [command.alias] : [],
  );
  return {
    ok: true,
    batchId: 9n,
    tick: 1n,
    aliases: created.map((id, index) => ({ alias: aliases[index]!, id })),
    symbols: [],
    effects: [],
  };
}

function description(client: ReactWorldClient, ids = ["journal-entity"]) {
  const tree = new ReactWorldTree(client);
  for (const id of ids) tree.children.push(tree.instance("ipp-entity", { id }));
  return tree.describe();
}

test("a failed batch outcome retains the entity its applied prefix created for cleanup", async () => {
  const calls: Command[][] = [];
  const client = clientWith(async (operations) => {
    calls.push(operations);
    if (calls.length > 1) return outcome();
    return {
      ...outcome(operations, [32n]),
      ok: false,
      error: { scope: "operation", operation: 1, reason: "DuplicateAlias" },
    };
  });
  const commits = new ReactWorldCommits(client, { onError: () => {} });
  const tree = new ReactWorldTree(client);
  for (const id of ["journal-a", "journal-b"])
    tree.children.push(tree.instance("ipp-entity", { id }));
  await assert.rejects(commits.capture(tree.describe()));
  assert.deepEqual(
    calls[0]!.map((command) =>
      command.kind === "create"
        ? [command.metadata.symbolicId, command.adopt]
        : command.kind,
    ),
    [
      ["journal-a", true],
      ["journal-b", true],
    ],
  );
  // Removing the declarations deletes exactly the applied prefix's entity.
  tree.children.length = 0;
  await commits.capture(tree.describe());
  assert.deepEqual(calls.slice(1).flat(), [
    { kind: "delete", entity: { kind: "handle", id: 32n } },
  ]);
  await commits.dispose();
  assert.equal(calls.length, 2, "unmount deletes nothing");
});

test("a batch without any outcome is fatal and never guesses ownership", async () => {
  const lost = new Error("transport lost the final page reply");
  const calls: Command[][] = [];
  const client = clientWith(async (operations) => {
    calls.push(operations);
    throw lost;
  });
  const commits = new ReactWorldCommits(client, { onError: () => {} });
  await assert.rejects(commits.capture(description(client)), lost);
  await assert.rejects(commits.capture(description(client)), lost);
  assert.equal(calls.length, 1);
});

test("synchronous closure fences submission before its notification microtask", async () => {
  let submitCount = 0;
  const client = clientWith(async () => {
    submitCount++;
    return outcome();
  });
  let resolve!: (closure: ClientClosure) => void;
  const closed = new Promise<ClientClosure>((accept) => {
    resolve = accept;
  });
  let closure: ClientClosure | undefined;
  Object.defineProperties(client, {
    closed: { value: closed },
    closure: { get: () => closure },
  });
  const commits = new ReactWorldCommits(client, { onError: () => {} });
  closure = { reason: new Error("generic terminal reason") };
  resolve(closure);
  await assert.rejects(
    commits.capture(description(client)),
    /generic terminal reason/,
  );
  assert.equal(submitCount, 0);
  await commits.dispose();
});

for (const typed of [true, false]) {
  test(`an attachment waiting for ${typed ? "rejected declarations" : "a lost outcome"} preserves the reporting owner`, async () => {
    const parentErrors: Error[] = [];
    const childErrors: Error[] = [];
    let failing = true;
    const parent = clientWith(async (operations) => {
      if (!failing) return outcome(operations, [32n]);
      if (!typed) throw new Error("lost outcome");
      return {
        ...outcome(),
        ok: false,
        error: { scope: "operation", operation: 0, reason: "InvalidValue" },
      };
    });
    const child = Object.assign(
      clientWith(async () => outcome()),
      {
        closed: new Promise<never>(() => {}),
        close: async () => {},
      },
    );
    const tree = new ReactWorldTree(parent);
    const anchor = tree.instance("ipp-entity", { id: "anchor" });
    const slot = new AttachedWorldSlot();
    tree.children.push(
      anchor,
      tree.instance("ipp-attached-world", {
        slot,
        anchor: "anchor",
        child: { borrow: { id: 2n, incarnation: 1n } },
        attachment: { mode: "spatial" },
        onError: (error: Error) => childErrors.push(error),
      }),
    );
    const options = { onError: (error: Error) => parentErrors.push(error) };
    const commits = new ReactWorldCommits(parent, options);
    const container = new ReactWorldContainer(tree, commits);
    const host = {
      openWorld: async () => child,
    } as unknown as ReactCompositionHost;
    const group = new ReactAttachmentGroup(host, container, parent, options);
    try {
      container.capture();
      container.publish();
      await assert.rejects(
        commits.checkpoint(),
        typed ? /InvalidValue/ : /lost outcome/,
      );
      for (let i = 0; i < 8; i++)
        await new Promise<void>((resolve) => setImmediate(resolve));
      if (typed) {
        assert.equal(parentErrors.length, 1);
        assert.equal(childErrors.length, 0);
      } else {
        assert.ok(parentErrors.length > 0);
        assert.ok(
          childErrors.length > 0,
          "unclassified failures retain the child boundary path",
        );
        assert.ok(
          [...parentErrors, ...childErrors].every(
            (error) => error.message === "lost outcome",
          ),
        );
      }
      assert.equal(slot.error, undefined);
      assert.ok(slot.container, "healthy child scope stays available");
      if (typed) {
        failing = false;
        anchor.props = { id: "corrected-anchor" };
        tree.restructure();
        container.capture();
        container.publish();
        await group.settled();
        assert.equal(parentErrors.length, 1);
        assert.equal(childErrors.length, 0);
      }
    } finally {
      group.fence();
      await group.dispose();
    }
  });
}
