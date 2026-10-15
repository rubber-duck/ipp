import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { createRoot } from "../src/root.js";
import { useEntityId } from "../src/scope/identity.js";
import { useWorld, validateWorldServices } from "../src/scope/world.js";
import type { ReactWorldClient } from "../src/reconciler/world-client.js";
import { ReactDataSourceRegistry } from "../src/data/registry.js";
import { ReactAssetRegistry } from "../src/assets/registry.js";

function client(): ReactWorldClient {
  return {
    session: 1n,
    schemaHash: 1n,
    components: {},
    manifest: { systems: [], components: [], operations: [] },
    async batch() {
      throw new Error("Empty scope fixture must not write");
    },
  };
}

test("generated identities retain hooks across explicit IDs and isolate receiving roots", async () => {
  const receiving = client();
  const roots = [createRoot(receiving), createRoot(receiving)];
  const ids: string[] = [];
  let current = "";
  function Probe({ id }: { id?: string }) {
    current = useEntityId(id);
    assert.equal(useWorld().client, receiving);
    return null;
  }
  try {
    for (const root of roots) {
      await root.render(createElement(Probe));
      ids.push(current);
    }
    assert.notEqual(ids[0], ids[1]);
    assert.notEqual(ids[0]!.split("/")[1], ids[1]!.split("/")[1]);
    await roots[0]!.render(createElement(Probe, { id: "explicit" }));
    assert.equal(current, "explicit");
    await roots[0]!.render(createElement(Probe));
    assert.equal(current, ids[0]);
    assert.throws(
      () => validateWorldServices(receiving, { codecs: { schemaHash: 2n } }),
      /receiving session schema/,
    );
  } finally {
    await Promise.all(roots.map((root) => root.unmount()));
  }
});

test("registry snapshot observers see declaration removal and closure", async () => {
  const receiving = client();
  const data = new ReactDataSourceRegistry(receiving, (error) => {
    throw error;
  });
  let dataChanges = 0;
  data.subscribeSnapshot(() => {
    dataChanges++;
  });
  const description = {
    identity: 1,
    props: { name: "borrowed", ownership: "borrowed" as const },
    signature: "borrowed",
  };
  data.setDesired([description]);
  await data.prepare([description]);
  assert.equal(data.get("borrowed")?.status, "borrowed");
  await data.releaseUnused([]);
  assert.equal(data.get("borrowed"), undefined);
  await data.prepare([description]);
  data.close();
  assert.equal(data.get("borrowed"), undefined);
  assert.equal(dataChanges, 4);
  const assets = new ReactAssetRegistry(
    {
      ...receiving,
      async registerAsset() {},
      async releaseAsset() {},
      onResourceChange() {
        return () => {};
      },
    },
    () => {},
    (error) => {
      throw error;
    },
  );
  let assetChanges = 0;
  assets.subscribeSnapshot(() => {
    assetChanges++;
  });
  const asset = {
    id: "mesh",
    identity: 1,
    kind: 1,
    variant: 0,
    bytes: new Uint8Array([1]),
    signature: "mesh",
    version: 1,
  };
  assets.setDesired([asset]);
  await assets.prepare([asset]);
  assert.equal(assets.get("mesh")?.status, "start");
  await assets.prepare([]);
  assert.equal(assets.get("mesh"), undefined);
  assets.close();
  assert.equal(assetChanges, 3);
});

test("owned World setup closes an acquired session before destroying failed setup", async () => {
  const { createWorldRoot } = await import("../src/scope/setup.js");
  const calls: string[] = [];
  const opened = {
    ...client(),
    worldReference: { id: 4n, incarnation: 1n },
    async close() {
      calls.push("close");
    },
  };
  const host = {
    sessions: new Map([[opened.session, opened]]),
    async createWorld() {
      return { reference: { id: 4n, incarnation: 1n } };
    },
    async openWorld() {
      return opened;
    },
    async destroyWorld() {
      calls.push("destroy");
    },
  } as unknown as import("../src/composition/attached-world.js").ReactCompositionHost;
  await assert.rejects(
    createWorldRoot(
      host,
      { selectedSystems: [] },
      { codecs: { schemaHash: 2n } },
    ),
    /receiving session schema/,
  );
  assert.deepEqual(calls, ["close", "destroy"]);
});
