/** Real React ordinary-GUI mount/update/unmount through a generated client.
 *
 * Transport-agnostic: this exercise runs against the native Host in
 * `gui-react-root.test.ts`, and the same function fits a worker/WASM driver.
 * It uses the public React entry points only: Entity declarations with
 * Layout, control and theme components mount as ordinary entities of the
 * World canvas in one acknowledged batch, updates write only changed fields on
 * the same entities and control incarnations, unmount deletes nothing, and
 * removing the declarations deletes the declared entities. A foreign
 * CanvasStyle on a bound entity proves refusal and adoption through the same
 * lifecycle: an invalid declaration over it is refused without effect and
 * leaves the acknowledged panel intact, and the correction adopts the foreign
 * style, keeps its foreign child and keeps the
 * acknowledged entities, control incarnations and refs. Themed buttons prove
 * that label-only commits never rewrite themes or skin references, and a
 * checkbox's current and toggled values reach entity action listeners in
 * capture and bubble order.
 */
import { createElement as h, createRef, StrictMode } from "react";
import type { Client, Command, WorldReference } from "@ipp/client";
import { guiAction } from "../../fixtures/gui-actions.js";
import {
  Children,
  createRoot,
  Entity,
  ReactWorldBatchRejectedError,
} from "../../../packages/ipp-react/src/index.js";
import {
  Button,
  Checkbox,
  Layout,
  Skin,
  Slider,
  Style,
  Text,
  Theme,
  type GuiControlHandle,
} from "../../../packages/ipp-react/src/gui.js";
import {
  aliasId,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../fixtures/commands.js";
import { check } from "../../harness/page/checks.js";
import {
  alias,
  cleanup,
  control,
  encoded,
  entitiesByName,
  fieldsOf,
  LAYOUT,
  named,
  openGui,
  place,
  rejectedFor,
  treeRows,
  type GuiContract,
  type GuiHost,
  type GuiTestClient,
} from "./gui-lifecycle.js";
import {
  LIFECYCLE,
  GUI,
  selectSystems,
} from "../../fixtures/system-selections.js";

type ControlRef = ReturnType<typeof createRef<GuiControlHandle>>;

interface SliderConfiguration {
  readonly value: number;
  readonly min: number;
}

function panelEntity(
  text: string,
  sliderRef: ControlRef,
  slider: SliderConfiguration,
) {
  return h(
    Entity,
    { id: "panel", key: "panel" },
    h(Layout, { kind: LAYOUT.row, width: 4, height: 3 }),
    h(
      Children,
      null,
      h(
        Entity,
        { id: "panel-text", key: "text" },
        h(Layout, { width: 2, height: 1 }),
        h(Text, { text }),
      ),
      h(
        Entity,
        { id: "panel-slider", key: "slider" },
        h(Layout, { width: 2, height: 1 }),
        h(Slider, {
          ref: sliderRef,
          value: slider.value,
          min: slider.min,
          max: 1,
          step: 0.05,
        }),
      ),
    ),
  );
}

const DEFAULT_SLIDER: SliderConfiguration = { value: 0.5, min: 0 };

function panel(
  text: string,
  sliderRef: ControlRef,
  slider: SliderConfiguration = DEFAULT_SLIDER,
) {
  return h(StrictMode, null, panelEntity(text, sliderRef, slider));
}

/** The panel plus a CanvasStyle declared on the foreign producer entity. */
function combined(
  text: string,
  occupiedText: string,
  sliderRef: ControlRef,
  slider: SliderConfiguration,
  opacity = 0.25,
) {
  return h(
    StrictMode,
    null,
    panelEntity(text, sliderRef, slider),
    h(
      Entity,
      { bindTo: "occupied", key: "occupied" },
      h(Style, { opacity }),
      h(
        Children,
        null,
        h(
          Entity,
          { id: "occupied-text", key: "text" },
          h(Text, { text: occupiedText }),
        ),
      ),
    ),
  );
}

/** A layout root with 49 child text entities: 50 declared entities. */
function fiftyEntityPanel() {
  return h(
    Entity,
    { id: "batch-panel", key: "batch-panel" },
    h(Layout, { kind: LAYOUT.column, width: 4, height: 3 }),
    h(
      Children,
      null,
      ...Array.from({ length: 49 }, (_, index) =>
        h(
          Entity,
          { id: `batch-text-${index}`, key: index },
          h(Text, { text: `node ${index}` }),
        ),
      ),
    ),
  );
}

/** Two themed buttons: the first theme precedes an unchanged second one. */
function themedPanel(
  first: Uint8Array<ArrayBuffer>,
  steady: Uint8Array<ArrayBuffer>,
  label: string,
) {
  return h(
    StrictMode,
    null,
    h(
      Entity,
      { id: "theme-first", key: "theme-first" },
      h(Theme, { parts: first }),
    ),
    h(
      Entity,
      { id: "theme-steady", key: "theme-steady" },
      h(Theme, { parts: steady }),
    ),
    h(
      Entity,
      { id: "theme-panel", key: "theme-panel" },
      h(Layout, { kind: LAYOUT.row, width: 4, height: 3 }),
      h(
        Children,
        null,
        h(
          Entity,
          { id: "theme-button-first", key: "first" },
          h(Layout, { width: 2, height: 1 }),
          h(Skin, { theme: "theme-first" }),
          h(Button, { label }),
        ),
        h(
          Entity,
          { id: "theme-button-steady", key: "steady" },
          h(Layout, { width: 2, height: 1 }),
          h(Skin, { theme: "theme-steady" }),
          h(Button, { label: "steady" }),
        ),
      ),
    ),
  );
}

/** One full-panel checkbox whose values travel through every action
 * listener level. */
function actionPanel(log: string[], checkboxRef: ControlRef) {
  return h(
    Entity,
    {
      id: "action-panel",
      key: "action-panel",
      onActionCapture: () => void log.push("capture:root"),
      onAction: () => void log.push("bubble:root"),
    },
    h(Layout, { kind: LAYOUT.column, width: 4, height: 3 }),
    h(
      Children,
      null,
      h(
        Entity,
        {
          id: "action-row",
          key: "row",
          onAction: () => void log.push("bubble:row"),
        },
        h(Layout, { kind: LAYOUT.row, width: 4, height: 3 }),
        h(
          Children,
          null,
          h(
            Entity,
            {
              id: "action-checkbox",
              key: "checkbox",
              onAction: () => void log.push("bubble:checkbox"),
            },
            h(Layout, { width: 4, height: 3 }),
            h(Checkbox, {
              ref: checkboxRef,
              checked: false,
              onToggle: () => void log.push("toggle"),
            }),
          ),
        ),
      ),
    ),
  );
}

/** Batches submitted through a React root. */
interface Recorded {
  readonly commands: Command[];
}

/** Wrap `client` so each React batch records. */
function recording(client: GuiTestClient, batches: Recorded[]): GuiTestClient {
  return new Proxy(client, {
    get(target, property) {
      if (property === "batch")
        return async (commands: Command[]) => {
          const outcome = await target.batch(commands);
          batches.push({ commands });
          return outcome;
        };
      const value = Reflect.get(target, property, target) as unknown;
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}

async function scalar(handle: GuiControlHandle) {
  const { value } = await handle.read();
  check(typeof value === "number", "React slider has no scalar value");
  return value;
}

function near(actual: unknown, expected: number) {
  return typeof actual === "number" && Math.abs(actual - expected) < 1e-6;
}

/**
 * Mount, update and unmount real React declarations, including StrictMode
 * wrapping, refusal and adoption of a foreign CanvasStyle.
 */
export async function exerciseGuiReactRoot(
  host: GuiHost,
  contract: GuiContract,
) {
  const worlds: WorldReference[] = [];
  const sessions: Client[] = [];
  let completed = false;
  try {
    // The World is the canvas; its extent holds every 4 x 3 scenario root.
    const created = await host.createWorld({
      selectedSystems: selectSystems(GUI, LIFECYCLE),
      symbolicId: "gui-react-root",
      canvas: { extent: [4, 3], unitsPerMetre: 1 },
    });
    worlds.push(created.reference);
    const client = await openGui(host, created.reference);
    sessions.push(client);
    const errors: Error[] = [];

    // A production generated client receives the complete 50-entity React
    // declaration as one batch, independent of the entity count: component
    // and link declarations name their entity bindings by batch aliases.
    const mountBatches: Recorded[] = [];
    const batchRoot = createRoot(recording(client, mountBatches), {
      onError: (error) => errors.push(error),
    });
    await batchRoot.render(fiftyEntityPanel());
    const mountBatchRequests = mountBatches.length;
    const count = (index: number, kind: Command["kind"]) =>
      mountBatches[index]?.commands.filter(
        (command) =>
          command.kind === kind &&
          (command.kind !== "create" || command.adopt === true),
      ).length;
    const mountDeclarations = count(0, "create");
    check(
      mountBatchRequests === 1 &&
        mountDeclarations === 50 &&
        // One Layout on the root, one Text per child.
        count(0, "insertComponent") === 50 &&
        count(0, "placeEntity") === 49,
      `The 50-entity mount was not one batch: ${encoded(mountBatches.map((batch) => batch.commands.map((command) => command.kind)))}`,
    );
    await client.waitForFrame();
    const batchRows = await treeRows(
      client,
      named(await entitiesByName(client), "batch-panel").id,
    );
    check(batchRows.length === 50, "The batched React panel is incomplete");
    // Removing the declarations deletes the panel; unmount deletes nothing.
    await batchRoot.render(null);
    await batchRoot.unmount();

    // Mount the ordinary panel first: a later refused declaration must
    // preserve these acknowledgements.
    const sliderRef = createRef<GuiControlHandle>();
    const rootBatches: Recorded[] = [];
    const root = createRoot(recording(client, rootBatches), {
      onError: (error) => errors.push(error),
    });
    await root.render(panel("hello", sliderRef));
    const mounted = await entitiesByName(client);
    const panelId = named(mounted, "panel").id;
    const mountedRows = await treeRows(client, panelId);
    check(
      encoded(mountedRows.map((row) => row.entity)) ===
        encoded([
          panelId,
          named(mounted, "panel-text").id,
          named(mounted, "panel-slider").id,
        ]),
      `React mount produced ${encoded(mountedRows)}`,
    );
    const sliderHandle = sliderRef.current;
    check(sliderHandle, "React slider ref was not acknowledged");
    const sliderTarget = sliderHandle.target;

    // `value` is an ordinary field: React writes it when its declaration
    // changes and never replays an unchanged declaration over a newer write.
    const initial = await scalar(sliderHandle);
    check(near(initial, 0.5), `React slider initialization ${initial}`);
    check(
      await sliderHandle.compareAndSet("value", 0.5, 0.8),
      "Slider compare-and-set failed",
    );
    await root.render(panel("hello", sliderRef));
    check(
      near(await scalar(sliderHandle), 0.8),
      "An unchanged slider declaration replayed its value over a newer write",
    );
    await root.render(panel("hello", sliderRef, { value: 0.1, min: 0 }));
    check(
      near(await scalar(sliderHandle), 0.1),
      "A changed slider declaration was not written",
    );
    // An invalid domain (min above max) is refused without effect.
    const refusedDomain = await root
      .render(panel("hello", sliderRef, { value: 0.1, min: 2 }))
      .then(
        () => undefined,
        (error: unknown) => error,
      );
    check(
      refusedDomain instanceof ReactWorldBatchRejectedError,
      `An invalid slider domain was accepted: ${String(refusedDomain)}`,
    );
    check(
      sliderRef.current === sliderHandle,
      "The rejected commit dropped the acknowledged slider ref",
    );
    // Read and correct the control through the World session and the ref.
    const sliderEntity = named(await entitiesByName(client), "panel-slider").id;
    const refused = await control(client, sliderEntity);
    check(
      refused.value.kind === "scalar" &&
        near(refused.value.value, 0.1) &&
        refused.fields.min === 0 &&
        encoded(refused.target) === encoded(sliderTarget),
      `Rejected slider bounds changed the slider: ${encoded(refused)}`,
    );
    check(
      !(await sliderHandle.compareAndSet("value", 0.8, 0.95)),
      "A stale slider compare-and-set was accepted",
    );
    check(
      near(await scalar(sliderHandle), 0.1),
      "A refused compare-and-set changed the slider",
    );
    check(
      await sliderHandle.compareAndSet("value", 0.1, 0.95),
      "The slider compare-and-set failed",
    );

    // A text change patches in place on the same entities and incarnations,
    // writing only the changed fields.
    const corrected = { value: 0.1, min: 0.9 };
    await root.render(panel("hello world", sliderRef, corrected));
    const updated = await entitiesByName(client);
    const correctedSlider = await control(
      client,
      named(updated, "panel-slider").id,
    );
    check(
      named(updated, "panel").id === panelId &&
        encoded((await treeRows(client, panelId)).map((row) => row.entity)) ===
          encoded(mountedRows.map((row) => row.entity)) &&
        encoded(correctedSlider.target) === encoded(sliderTarget),
      "An update replaced entity or control identities",
    );
    check(
      fieldsOf(client, named(updated, "panel-text").components, "CanvasText")
        ?.text === "hello world",
      "An update did not patch the text",
    );
    check(
      correctedSlider.fields.min === Math.fround(0.9) &&
        correctedSlider.value.kind === "scalar" &&
        near(correctedSlider.value.value, 0.95),
      `Corrected slider configuration replayed its unchanged declared value: ${encoded(correctedSlider)}`,
    );
    // A foreign producer CanvasStyle with its own child on the entity a
    // later declaration binds to.
    const occupiedOutcome = successfulBatch(
      await client.batch([
        createEntity(1, "occupied"),
        insertComponent(client, "CanvasStyle", alias(1), { opacity: 0.75 }),
        createEntity(2, "occupied-foreign"),
        insertComponent(client, "CanvasText", alias(2), { text: "foreign" }),
        place(alias(2), alias(1)),
      ]),
    );
    const occupied = aliasId(occupiedOutcome, 1);
    // On the real generated-client transport, an outcome reports the entity
    // each symbolic reference resolved to.
    const symbolProbe = successfulBatch(
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "symbol", symbol: "occupied" },
          component: client.components.CanvasStyle!.id,
          field: {
            offset: client.components.CanvasStyle!.fields.opacity!.offset,
            value: { kind: "f32", value: 0.5 },
          },
        },
      ]),
    );
    check(
      encoded(symbolProbe.symbols) ===
        encoded([{ symbol: "occupied", id: occupied }]),
      `Native symbol resolution omitted or changed its entity: ${encoded(symbolProbe.symbols)}`,
    );
    // An invalid CanvasStyle declared over the foreign one is refused
    // without effect; the applied prefix of the batch stays.
    const refusal = await root
      .render(combined("hello", "hi", sliderRef, corrected, 1.1))
      .then(
        () => undefined,
        (error: unknown) => error,
      );
    check(
      refusal instanceof ReactWorldBatchRejectedError,
      `React wrote an invalid CanvasStyle over a foreign producer: ${String(refusal)}`,
    );
    const kept = await entitiesByName(client);
    const keptSlider = await control(client, named(kept, "panel-slider").id);
    check(
      named(kept, "panel").id === panelId &&
        fieldsOf(client, named(kept, "panel-text").components, "CanvasText")
          ?.text === "hello" &&
        encoded(keptSlider.target) === encoded(sliderTarget),
      `The refused declaration disturbed the acknowledged panel: ${encoded({ panel: kept.get("panel"), keptSlider, sliderTarget })}`,
    );
    const foreign = named(kept, "occupied");
    check(
      fieldsOf(client, foreign.components, "CanvasStyle")?.opacity === 0.5 &&
        named(kept, "occupied-foreign").link.parent === occupied,
      `The refused declaration damaged the foreign producer: ${encoded(foreign)}`,
    );
    // Rejected commits never retry unchanged work: the corrected declaration
    // adopts the foreign style and writes its declared fields over it, and
    // the foreign child stays.
    const recoveryMark = rootBatches.length;
    await root.render(combined("hello", "recovered", sliderRef, corrected));
    const recoveryCommands = rootBatches
      .slice(recoveryMark)
      .map((batch) => batch.commands.map((command) => command.kind));
    const adopted = await entitiesByName(client);
    check(
      named(adopted, "occupied").id === occupied &&
        fieldsOf(client, named(adopted, "occupied").components, "CanvasStyle")
          ?.opacity === 0.25 &&
        fieldsOf(
          client,
          named(adopted, "occupied-text").components,
          "CanvasText",
        )?.text === "recovered" &&
        named(adopted, "occupied-text").link.parent === occupied &&
        named(adopted, "occupied-foreign").link.parent === occupied,
      "React did not adopt the foreign CanvasStyle",
    );
    // Checked last so every other lifecycle assertion still runs.
    const recoveredPanel = named(adopted, "panel").id;
    const recoveredRef = sliderRef.current === sliderHandle;

    // Unmount deletes nothing: every entity and the adopted style stay.
    await root.unmount();
    const unmounted = await entitiesByName(client);
    check(
      named(unmounted, "panel").id === panelId &&
        unmounted.has("panel-slider") &&
        unmounted.has("occupied-text") &&
        fieldsOf(client, named(unmounted, "occupied").components, "CanvasStyle")
          ?.opacity === 0.25 &&
        unmounted.has("occupied-foreign"),
      "Unmount deleted a declared entity, the adopted CanvasStyle or a foreign child",
    );
    // A fresh root adopts the same declarations and removing them deletes
    // the declared entities and removes the bound entity's declared style;
    // the bound entity and its foreign child stay.
    const remover = createRoot(client, {
      onError: (error) => errors.push(error),
    });
    await remover.render(
      combined("hello", "recovered", createRef<GuiControlHandle>(), corrected),
    );
    check(
      named(await entitiesByName(client), "panel").id === panelId,
      "A fresh root did not adopt the unmounted root's panel",
    );
    await remover.render(null);
    await remover.unmount();
    const removed = await entitiesByName(client);
    check(
      !removed.has("panel") &&
        !removed.has("panel-slider") &&
        !removed.has("occupied-text"),
      "Removing the declarations left declared entities",
    );
    check(
      removed.has("occupied") &&
        fieldsOf(
          client,
          named(removed, "occupied").components,
          "CanvasStyle",
        ) === undefined &&
        removed.has("occupied-foreign"),
      "Removal deleted the bound entity or its foreign child, or kept its declared CanvasStyle",
    );
    const retired = await guiAction(client, sliderTarget, {
      kind: "scalar",
      value: 0.1,
    });
    check(
      rejectedFor(retired, "StaleTarget"),
      `A removed control target was accepted: ${encoded(retired)}`,
    );

    // Remount on a fresh root observes new identities, fencing the old ones.
    const secondRef = createRef<GuiControlHandle>();
    const second = createRoot(client, {
      onError: (error) => errors.push(error),
    });
    await second.render(panel("again", secondRef));
    const remounted = await entitiesByName(client);
    const remountedTarget = secondRef.current?.target;
    check(
      named(remounted, "panel").id !== panelId &&
        remountedTarget !== undefined &&
        (remountedTarget.entity !== sliderTarget.entity ||
          remountedTarget.incarnation !== sliderTarget.incarnation),
      "A remount reused the retired identities",
    );
    await second.unmount();

    const reported = errors.length;
    const themes = await exerciseThemeStability(client, contract, errors);
    const actions = await exerciseRootActionListeners(client, errors);
    check(
      errors.length === reported,
      `Unexpected React GUI errors: ${errors.slice(reported)}`,
    );
    // A refused sibling declaration and its correction keep the keyed,
    // previously acknowledged panel entities.
    check(
      recoveredPanel === panelId,
      `Recovery recreated the acknowledged panel: ${encoded(recoveryCommands)}`,
    );
    check(recoveredRef, "Recovery replaced the acknowledged slider ref");
    completed = true;
    return {
      panel: String(panelId),
      remountedPanel: String(named(remounted, "panel").id),
      sliderIncarnation: String(sliderTarget.incarnation),
      remountedIncarnation: String(remountedTarget.incarnation),
      symbolResolution: String(occupied),
      correctedSliderValue: correctedSlider.value,
      refusal: String(refusal),
      mountBatchRequests,
      mountDeclarations,
      themeEntities: themes.entities,
      themeEditsAfterSettle: themes.editsAfterSettle,
      rootActionOrder: actions,
    };
  } finally {
    await cleanup(host, sessions, worlds, completed);
  }
}

/**
 * Themes are ordinary entities: once declared, label-only commits send no
 * theme or skin writes, and the theme entities and skin references stay
 * unchanged.
 */
async function exerciseThemeStability(
  client: GuiTestClient,
  contract: GuiContract,
  errors: Error[],
) {
  const batches: Recorded[] = [];
  const root = createRoot(recording(client, batches), {
    onError: (error) => errors.push(error),
  });
  const background = contract.guiPaintPartIndex({ part: "background" });
  const theme = (green: number) =>
    contract.GuiTheme.encodeParts({
      nextSlot: 1,
      rows: new Map([[0, { part: background, color: [0.1, green, 0.2, 1] }]]),
    });
  const steady = theme(0.3);
  const fresh = theme(0.7);
  const themeComponents = new Set([
    client.components.GuiTheme!.id,
    client.components.GuiSkin!.id,
  ]);
  await root.render(themedPanel(fresh, steady, "first"));
  /** Commands that write a theme or skin component. */
  const themeWrite = (command: Command) =>
    (command.kind === "insertComponent" ||
      command.kind === "setField" ||
      command.kind === "setFieldIf" ||
      command.kind === "setDynamicProperty" ||
      command.kind === "removeDynamicProperty" ||
      command.kind === "removeComponent") &&
    themeComponents.has(command.component);
  check(
    batches.some((batch) =>
      batch.commands.some(
        (command) =>
          command.kind === "insertComponent" &&
          themeComponents.has(command.component),
      ),
    ),
    "React inserted no theme or skin components",
  );
  const references = async () => {
    const rows = await entitiesByName(client);
    return {
      entities: [named(rows, "theme-first").id, named(rows, "theme-steady").id],
      skins: ["theme-button-first", "theme-button-steady"].map(
        (name) =>
          fieldsOf(client, named(rows, name).components, "GuiSkin")?.theme,
      ),
    };
  };
  const settled = await references();
  check(
    encoded(settled.skins) === encoded(settled.entities),
    `Skins do not reference their theme entities: ${encoded(settled)}`,
  );
  const mark = batches.length;

  // Label edits reach the controls without touching either theme.
  for (const label of ["second", "third"]) {
    await root.render(themedPanel(fresh, steady, label));
    await client.waitForFrame();
    const current = await references();
    check(
      encoded(current) === encoded(settled),
      `Theme entities or skin references churned: ${encoded(settled)} -> ${encoded(current)}`,
    );
  }
  const editsAfterSettle = batches
    .slice(mark)
    .flatMap((batch) => batch.commands)
    .filter(themeWrite).length;
  check(
    editsAfterSettle === 0,
    `Unchanged themes sent ${editsAfterSettle} theme or skin edits`,
  );
  check(
    batches.length > mark,
    "Label-only commits wrote nothing, so theme stability was not exercised",
  );
  await root.unmount();
  return { entities: settled.entities.map(String), editsAfterSettle };
}

/**
 * A checkbox's current value, then its toggled value, each run the control
 * listener and then entity action listeners along the declaration path: root
 * capture first and root bubble last, once each.
 */
async function exerciseRootActionListeners(
  client: GuiTestClient,
  errors: Error[],
): Promise<string[][]> {
  const log: string[] = [];
  const checkboxRef = createRef<GuiControlHandle>();
  const root = createRoot(client, { onError: (error) => errors.push(error) });
  await root.render(actionPanel(log, checkboxRef));
  await client.waitForFrame();
  const checkbox = checkboxRef.current;
  check(checkbox, "React checkbox ref was not acknowledged");
  const expected = [
    "toggle",
    "capture:root",
    "bubble:checkbox",
    "bubble:row",
    "bubble:root",
  ];
  const settled = async (label: string) => {
    for (let frame = 0; frame < 30 && !log.includes("bubble:root"); frame += 1)
      await client.waitForFrame();
    check(
      log.join() === expected.join(),
      `Entity action listeners ran out of order for the ${label} value: ${log}`,
    );
  };
  // The current value is delivered first, through the same path.
  await settled("current");
  const orders = [[...log]];
  log.length = 0;
  check(
    (await checkbox.action({ kind: "toggle" })).ok,
    "The checkbox toggle did not apply",
  );
  await settled("toggled");
  orders.push([...log]);
  await root.unmount();
  return orders;
}
