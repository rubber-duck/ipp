/** Real React GuiRoot mount/update/unmount through a generated client.
 *
 * Transport-agnostic: this exercise runs against the native host in
 * `gui-react-root.test.ts`, and the same function fits a worker/WASM driver.
 * It uses the public React entry points only: an Entity/Surface/GuiRoot
 * declaration mounts through the producer lifecycle (no GuiRoot overlay),
 * updates patch in place on the same incarnation, and unmount removes the
 * tree and its reconciler-created producer. A sabotaged occupied root proves
 * partial-failure refusal and recovery through the same lifecycle: the
 * refused attempt releases its own binding/overlay while previously
 * acknowledged declarations and the foreign tree stay intact.
 */
import { createElement as h, StrictMode } from "react";
import type {
  GuiInspectResponse,
  GuiNodeHandle,
  WorldPersistenceHostClient,
} from "@ipp/client";
import {
  createRoot,
  Entity,
  Surface,
} from "../../../packages/ipp-react/src/index.js";
import {
  GuiRoot,
  Row,
  Slider,
  Text,
} from "../../../packages/ipp-react/src/gui.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../camera-fixtures.js";
import type { GuiTestClient } from "./gui-lifecycle.js";

function expect(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function rejects(operation: Promise<unknown>, message: string) {
  try {
    await operation;
  } catch {
    return;
  }
  throw new Error(message);
}

function contents(response: GuiInspectResponse) {
  return response.nodes.map((node) => node.content);
}

type SliderRef = { current: GuiNodeHandle | null };

function panel(
  text: string,
  sliderRef: SliderRef,
  slider: { readonly value: number; readonly min: number } = {
    value: 0.5,
    min: 0,
  },
) {
  return h(
    StrictMode,
    null,
    h(
      Entity,
      { id: "panel", key: "panel" },
      h(Surface, { width: 4, height: 3 }),
      h(
        GuiRoot,
        null,
        h(
          Row,
          null,
          h(Text, { text }),
          h(Slider, {
            nodeRef: sliderRef,
            value: slider.value,
            min: slider.min,
            max: 1,
            step: 0.05,
          }),
        ),
      ),
    ),
  );
}

function combined(text: string, occupiedText: string, sliderRef: SliderRef) {
  return h(
    StrictMode,
    null,
    h(
      Entity,
      { id: "panel", key: "panel" },
      h(Surface, { width: 4, height: 3 }),
      h(
        GuiRoot,
        null,
        h(
          Row,
          null,
          h(Text, { text }),
          h(Slider, {
            nodeRef: sliderRef,
            value: 0.5,
            min: 0,
            max: 1,
            step: 0.05,
          }),
        ),
      ),
    ),
    h(
      Entity,
      { bindTo: "occupied", key: "occupied" },
      h(Surface, { width: 4, height: 3 }),
      h(GuiRoot, null, h(Row, null, h(Text, { text: occupiedText }))),
    ),
  );
}

function fiftyNodePanel() {
  return h(
    Entity,
    { id: "batch-panel", key: "batch-panel" },
    h(Surface, { width: 4, height: 3 }),
    h(
      GuiRoot,
      null,
      h(
        Row,
        null,
        ...Array.from({ length: 49 }, (_, index) =>
          h(Text, { key: index, text: `node ${index}` }),
        ),
      ),
    ),
  );
}

async function panelEntity(
  client: GuiTestClient,
  symbolicId: string,
): Promise<bigint> {
  const found = (await client.inspect()).entities.find(
    (item) => item.metadata.symbolicId === symbolicId,
  );
  expect(found, `React panel entity ${symbolicId} disappeared`);
  return found.id;
}

/**
 * Mount, update and unmount real React declarations, including StrictMode
 * wrapping and recovery from a sabotaged occupied root.
 */
export async function exerciseGuiReactRoot(
  host: WorldPersistenceHostClient<GuiTestClient>,
) {
  const client = await host.createWorld({ symbolicId: "gui-react-root" });
  const errors: Error[] = [];
  const sliderRef: SliderRef = { current: null };
  const root = createRoot(client, {
    onError: (error) => {
      errors.push(error);
    },
  });

  // A production generated client receives the complete 50-node React diff
  // in one request-sized edit batch and publishes a completed frame.
  const originalEditGuiBatch = client.editGuiBatch.bind(client);
  let mountBatchRequests = 0;
  let mountBatchEdits = 0;
  client.editGuiBatch = async (edits) => {
    mountBatchRequests += 1;
    mountBatchEdits += edits.length;
    return await originalEditGuiBatch(edits);
  };
  const batchRoot = createRoot(client, {
    onError: (error) => errors.push(error),
  });
  await batchRoot.render(fiftyNodePanel());
  const batchPanelId = await panelEntity(client, "batch-panel");
  const batchPanel = await client.inspectGui({ entity: batchPanelId });
  expect(batchPanel.nodes.length === 50, "Batched React panel is incomplete");
  expect(
    mountBatchRequests === 1,
    "50-node mount used more than one GUI request",
  );
  expect(
    mountBatchEdits === 50,
    "50-node mount submitted the wrong edit count",
  );
  const fiftyNodeBatchRequests = mountBatchRequests;
  const fiftyNodeBatchEdits = mountBatchEdits;
  await client.waitForFrame();
  await batchRoot.unmount();
  client.editGuiBatch = originalEditGuiBatch;

  // Mount the ordinary panel first: a later refused adoption must preserve
  // these acknowledgements while releasing only what its own attempt
  // acquired. The panel stays mounted for the whole sabotage cycle, since
  // dropping and re-adding it would make its own live nodes look foreign.
  await root.render(panel("hello", sliderRef));
  const panelId = await panelEntity(client, "panel");
  const mounted = await client.inspectGui({ entity: panelId });
  expect(
    contents(mounted).length === 3,
    `React mount produced ${contents(mounted).length} nodes`,
  );
  const incarnation = mounted.rootIncarnation;

  // Sabotage: an occupied foreign root. React must refuse it without damage,
  // then recover once the sabotage clears through the producer lifecycle.
  const ref = { kind: "alias", alias: 1 } as const;
  const occupied = aliasId(
    await client.batch([
      createEntity(1, "occupied"),
      insertComponent(client, "Surface", ref, { width: 4, height: 3 }),
      insertComponent(client, "GuiRoot", ref),
    ]),
    1,
  );
  // ipp-9nx.23 reproduction on the real generated-client/native transport:
  // a successful binding acknowledgement must enrich the accepted alias
  // with its concrete entity. This was intermittently absent from older,
  // mixed artifacts; current matched artifacts must preserve it exactly.
  const bindingProbe = await client.batch([
    { kind: "createStateOverlayOwner", alias: 91 },
    {
      kind: "attachEntityOverlayBinding",
      owner: { kind: "alias", alias: 91 },
      alias: 92,
      symbolicId: "occupied",
      mode: "bound",
    },
  ]);
  successfulBatch(bindingProbe);
  const probeOwner = bindingProbe.stateOverlays.find(
    (resource) => resource.alias === 91 && resource.kind === "owner",
  );
  const probeBinding = bindingProbe.stateOverlays.find(
    (resource) =>
      resource.alias === 92 && resource.kind === "entityOverlayBinding",
  );
  expect(probeOwner, "Native binding probe omitted its owner alias");
  expect(probeBinding, "Native binding probe omitted its binding alias");
  expect(
    probeBinding.entity === occupied,
    "Native binding probe omitted or changed its entity enrichment",
  );
  successfulBatch(
    await client.batch([
      {
        kind: "releaseStateOverlayOwner",
        owner: { kind: "handle", id: probeOwner.id },
      },
    ]),
  );
  const sabotaged = await client.inspectGui({ entity: occupied });
  expect(sabotaged.nodes.length === 0, "A new GuiRoot must start empty");
  await client.editGui({
    action: "insert",
    entity: occupied,
    rootIncarnation: sabotaged.rootIncarnation,
    id: 1,
    index: 0,
    content: { kind: "text", text: "foreign" },
  });
  // The combined render refuses the occupied root. The attempt's binding and
  // overlay release while the acknowledged panel and the foreign tree stay.
  await rejects(
    root.render(combined("hello", "hi", sliderRef)),
    "React adopted a foreign occupied root",
  );
  expect(
    errors.some((error) => /another writer/.test(error.message)),
    "The occupied-root refusal was not reported",
  );
  const kept = await client.inspectGui({ entity: panelId });
  expect(
    contents(kept).length === 3 &&
      contents(kept).some(
        (content) => content.kind === "text" && content.text === "hello",
      ),
    "The refused mount disturbed the acknowledged panel",
  );
  const intact = await client.inspectGui({ entity: occupied });
  expect(
    intact.nodes.length === 1,
    "The refused mount damaged the foreign tree",
  );
  await client.editGui({
    action: "remove",
    handle: client.createGuiNodeHandle(occupied, intact.rootIncarnation, 1, 1),
  });
  successfulBatch(
    await client.batch([
      {
        kind: "removeComponent",
        entity: { kind: "handle", id: occupied },
        component: client.components.GuiRoot!.id,
      },
    ]),
  );
  // Rejected commits never retry unchanged work: recover with new content,
  // which re-acquires the released binding/overlay and creates the producer
  // on the bound entity through React itself. A lingering attempt overlay
  // would reject this producer batch at operation 0.
  await root.render(combined("hello", "recovered", sliderRef));
  const adopted = await client.inspectGui({ entity: occupied });
  expect(
    contents(adopted).some(
      (content) => content.kind === "text" && content.text === "recovered",
    ),
    "React did not populate the cleared root",
  );
  const recoveredPanel = await client.inspectGui({ entity: panelId });
  expect(
    recoveredPanel.rootIncarnation === incarnation,
    "Recovery recreated the panel producer",
  );

  // Initial values are insertion-only. A value-only rerender emits no write,
  // an incompatible domain rejects without changing value/revision, and the
  // caller must make an explicit revision-fenced correction before retrying.
  const sliderHandle = sliderRef.current;
  expect(sliderHandle, "React slider ref was not acknowledged");
  let slider = (
    await client.inspectGui({
      entity: sliderHandle.entity,
      nodeId: sliderHandle.nodeId,
      maxDepth: 1,
    })
  ).nodes[0];
  expect(slider, "React slider disappeared");
  const initialSliderRevision = slider.controlRevision;
  await client.editGui({
    action: "setControlValue",
    handle: sliderHandle,
    expectedRevision: slider.controlRevision,
    value: { kind: "scalar", value: 0.8 },
  });
  await root.render(panel("hello", sliderRef, { value: 0.1, min: 0 }));
  slider = (
    await client.inspectGui({
      entity: sliderHandle.entity,
      nodeId: sliderHandle.nodeId,
      maxDepth: 1,
    })
  ).nodes[0];
  expect(
    slider?.controlValue.kind === "scalar" &&
      Math.abs(slider.controlValue.value - 0.8) < 1e-6 &&
      slider.controlRevision === initialSliderRevision + 1,
    `An initial-only slider rerender replayed its authored value: ${JSON.stringify(slider)}`,
  );
  await rejects(
    root.render(panel("hello", sliderRef, { value: 0.1, min: 0.9 })),
    "An incompatible slider domain was accepted",
  );
  slider = (
    await client.inspectGui({
      entity: sliderHandle.entity,
      nodeId: sliderHandle.nodeId,
      maxDepth: 1,
    })
  ).nodes[0];
  expect(
    slider?.content.kind === "slider" &&
      slider.content.min === 0 &&
      slider.controlValue.kind === "scalar" &&
      Math.abs(slider.controlValue.value - 0.8) < 1e-6 &&
      slider.controlRevision === initialSliderRevision + 1,
    "Rejected slider bounds mutated configuration, value, or revision",
  );
  await client.editGui({
    action: "setControlValue",
    handle: sliderHandle,
    expectedRevision: slider.controlRevision,
    value: { kind: "scalar", value: 0.95 },
  });

  // Changing the text makes this a corrected commit rather than an unchanged
  // rejected retry; the compatible domain lands while value remains runtime-owned.
  await root.render(panel("hello world", sliderRef, { value: 0.1, min: 0.9 }));
  const updated = await client.inspectGui({ entity: panelId });
  expect(
    updated.rootIncarnation === incarnation,
    "An update recreated the producer root",
  );
  expect(
    updated.nodes.map((node) => node.id).join() === "1,2,3",
    "An update replaced node identities",
  );
  expect(
    contents(updated).some(
      (content) => content.kind === "text" && content.text === "hello world",
    ),
    "An update did not patch the text",
  );
  const correctedSlider = updated.nodes.find(
    (node) => node.id === sliderHandle.nodeId,
  );
  expect(
    correctedSlider?.content.kind === "slider" &&
      Math.abs(correctedSlider.content.min - 0.9) < 1e-6 &&
      correctedSlider.controlValue.kind === "scalar" &&
      Math.abs(correctedSlider.controlValue.value - 0.95) < 1e-6 &&
      correctedSlider.controlRevision === initialSliderRevision + 2,
    "Corrected slider bounds replayed the authored initial value",
  );
  await root.unmount();
  await rejects(
    client.inspectGui({ entity: panelId }),
    "An unmounted root is still inspectable",
  );

  // Remount on a fresh root observes a new incarnation, fencing the old one.
  const second = createRoot(client, {
    onError: (error) => {
      errors.push(error);
    },
  });
  await second.render(panel("again", sliderRef));
  const panelAgain = await panelEntity(client, "panel");
  const remounted = await client.inspectGui({ entity: panelAgain });
  expect(
    String(remounted.rootIncarnation) !== String(incarnation),
    "A remount reused the retired incarnation",
  );
  await second.unmount();
  return {
    incarnation: String(incarnation),
    remountedIncarnation: String(remounted.rootIncarnation),
    bindingEntityEnrichment: String(probeBinding.entity),
    initialSliderRevision,
    correctedSliderRevision: correctedSlider.controlRevision,
    mountBatchRequests: fiftyNodeBatchRequests,
    mountBatchEdits: fiftyNodeBatchEdits,
  };
}
