import assert from "node:assert/strict";
import test from "node:test";
import type { BatchOutcome, Command, ClientClosure } from "@ipp/client";
import { ReactWorldCommits } from "../src/commits.js";
import { ReactWorldTree } from "../src/tree.js";
import type { ReactWorldClient } from "../src/contract.js";
import { attachmentIdentity } from "../src/attachment-identity.js";
import {
  AttachedWorldSlot,
  describeAttachedWorld,
} from "../src/attached-world.js";

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
    capabilities: {
      spatial: false,
      textures: false,
      builtinAssets: false,
      picking: false,
      debugGeometry: false,
      pbr: false,
      shadows: false,
      skeletalAnimation: false,
      meshPoses: false,
    },
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
