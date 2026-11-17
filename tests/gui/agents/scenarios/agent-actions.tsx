import { createElement as h } from "react";
import type {
  AssetWorldClient,
  Client,
  Command,
  GuiAgent,
  GuiAgentControl,
  GuiAgentSnapshot,
  GuiWorldClient,
  HostClientBase,
  PresentedCapture,
  PresentationView,
} from "@ipp/client";
import type { createGuiAgent } from "../../../../packages/ipp-client/src/gui-agent.js";
import type {
  guiAgentResultSchema,
  guiAgentTools,
} from "../../../../packages/ipp-client/src/gui-agent-schema.js";
import { canvasOutput } from "../../../../packages/ipp-client/src/references.js";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
  type ReactWorldRoot,
} from "../../../../packages/ipp-react/src/index.js";
import { Layout } from "../../../../packages/ipp-react/src/gui.js";
import {
  Button,
  TextField,
  RadioGroup,
  GuiKit,
  type GuiKitContract,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import { cybertronSkin } from "../../../../packages/ipp-react/src/gui-cybertron.js";
import { check } from "../../../harness/page/checks.js";
import {
  aliasId,
  componentFields,
  createEntity,
  insertComponent,
  successfulBatch,
} from "../../../fixtures/commands.js";
import {
  GUI,
  RENDER,
  LIFECYCLE,
  selectSystems,
} from "../../../fixtures/system-selections.js";
import { verifyAgentGroupOrder } from "./group-order.js";
import { assetsLoaded } from "../../support/mounted-panel.js";

export const AGENT_FORM_SIZE = { width: 320, height: 400 };
export interface AgentActionsEnvironment {
  capture(name: string, image: PresentedCapture): Promise<void>;
  record(value: object): Promise<void>;
  /** Driver cuts the real transport after the next submitted message. */
  loseConnectionOnNextSend(): void;
}
export interface AgentActionsContract extends GuiKitContract {
  createGuiAgent: typeof createGuiAgent;
  guiAgentResultSchema: typeof guiAgentResultSchema;
  guiAgentTools: typeof guiAgentTools;
}
type AgentClient = GuiWorldClient & AssetWorldClient;

function one(
  snapshot: GuiAgentSnapshot,
  label: string,
  world = "main",
): GuiAgentControl {
  const matches = snapshot.controls.filter(
    (entry) => entry.label === label && entry.world === world,
  );
  check(
    matches.length === 1,
    `Expected one ${world}/${label}, found ${matches.length}`,
  );
  return matches[0]!;
}

function write(
  client: Client,
  control: GuiAgentControl,
  component: string,
  values: Record<string, string | boolean | number>,
): Command {
  const fields = componentFields(client, component, values);
  check(fields.length === 1, "Fixture writes one field");
  return {
    kind: "setField",
    entity: { kind: "handle", id: BigInt(control.entity) },
    component: client.components[component]!.id,
    field: fields[0]!,
  };
}

/** One reusable scenario, without process launch, URLs, wire layout or simulation steps. */
export async function agentActions(
  host: HostClientBase<AgentClient>,
  contract: AgentActionsContract,
  fontBytes: ArrayBuffer,
  environment: AgentActionsEnvironment,
) {
  const worlds = await Promise.all(
    ["main", "other"].map((name) =>
      host.createWorld({
        symbolicId: `agent-${name}`,
        temporary: true,
        selectedSystems: selectSystems(GUI, RENDER, LIFECYCLE),
        canvas: {
          extent: [AGENT_FORM_SIZE.width, AGENT_FORM_SIZE.height],
          unitsPerMetre: AGENT_FORM_SIZE.width,
        },
      }),
    ),
  );
  const clients = await Promise.all(
    worlds.map((world) => host.openWorld(world.reference)),
  );
  const [main, other] = clients as [AgentClient, AgentClient];
  const writer = await host.openWorld(worlds[0]!.reference);
  const roots: ReactWorldRoot[] = [];
  const errors: Error[] = [];
  const reports = {
    presses: 0,
    texts: [] as string[],
    controlled: [] as string[],
    selected: [] as string[],
  };
  let view: PresentationView | undefined;
  let disconnected = false;
  const effects: string[] = [];
  const subscription = await main.subscribeGuiEffects((effect) =>
    effects.push(effect.effect.kind),
  );
  try {
    for (const [index, client] of clients.entries()) {
      const font = await client.createAsset(17, fontBytes.slice(0));
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: (error) => errors.push(error),
      });
      roots.push(root);
      await root.render(
        h(
          GuiKit,
          {
            contract,
            skin: cybertronSkin(contract),
            font: font.source,
            fontSize: 14,
            reducedMotion: true,
          },
          h(
            Entity,
            { id: "form" },
            h(Layout, {
              kind: 2,
              width: 320,
              height: 400,
              gap: 8,
              padding_left: 12,
              padding_top: 12,
            }),
            h(
              Children,
              null,
              h(Button, {
                id: "save",
                label: "Save",
                onPress: () => reports.presses++,
              }),
              ...(index
                ? []
                : [
                    h(TextField, {
                      key: "project",
                      id: "project",
                      label: "Project",
                      defaultValue: "Apollo",
                      onChange: (value) => reports.texts.push(value),
                    }),
                    h(TextField, {
                      key: "controlled",
                      id: "controlled",
                      label: "Controlled",
                      value: "Fixed",
                      onChange: (value) => reports.controlled.push(value),
                    }),
                    h(RadioGroup, {
                      key: "region",
                      id: "region",
                      label: "Region",
                      defaultValue: "eu",
                      options: [
                        { value: "eu", label: "Europe" },
                        { value: "us", label: "America" },
                        {
                          value: "off",
                          label: "Disabled region",
                          disabled: true,
                        },
                      ],
                      onChange: (value) => reports.selected.push(value),
                    }),
                    h(Button, {
                      key: "disabled",
                      id: "disabled",
                      label: "Disabled",
                      disabled: true,
                    }),
                  ]),
            ),
          ),
        ),
      );
      await assetsLoaded(client, [font.source]);
    }
    const agent = contract.createGuiAgent(
      { main, other },
      {
        afterAction: async (client, outcome) => {
          // The field stream precedes frame progress. Flush the known application's
          // controlled reconciliation after that observation, then observe its frame.
          await client.waitForFrame(outcome.tick);
          await roots[client === main ? 0 : 1]!.flush();
          await client.waitForFrame();
        },
      },
    );
    const initial = await agent.snapshot();
    check(
      initial.observations.every(
        (entry) => entry.status === "observed" && !entry.truncated,
      ),
      "Both World observations complete",
    );
    check(
      initial.controls.filter((entry) => entry.label === "Save").length === 2,
      "Duplicate labels survive World discovery",
    );
    check(
      one(initial, "Disabled").actions.length === 0,
      "Disabled controls disclose no available actions",
    );
    const group = initial.controls.find(
      (entry) => entry.world === "main" && entry.kind === "group",
    );
    check(
      group,
      "Native group is discoverable without inventing a composite label",
    );
    const items = await agent.query({ within: group.ref });
    check(
      items.controls.length === 3,
      "Only three native group items, excluding nested label controls",
    );
    check(
      items.controls.map((entry) => entry.label).join(",") ===
        "Europe,America,Disabled region",
      "Discovery preserves native tree item order",
    );
    const america = one(items, "America");
    const project = one(initial, "Project");
    const controlled = one(initial, "Controlled");
    const save = one(initial, "Save");
    const disabled = one(initial, "Disabled");

    const binding = await host.setRootOutput(
      canvasOutput(worlds[0]!.reference),
      { ...AGENT_FORM_SIZE, devicePixelRatio: 1 },
    );
    view = await host.presentation.select(
      await host.presentation.surface(),
      binding,
    );
    async function capture(name: string) {
      const frame = await host.presentation.capture(view!, {
        afterOutputs: [canvasOutput(worlds[0]!.reference)],
      });
      check(
        frame.failedDrawCalls === 0 && frame.drawCalls > 0,
        `${name} completed actual GUI draws`,
      );
      check(
        frame.sources.some(
          (source) => source.output.world.id === worlds[0]!.reference.id,
        ),
        "Capture witnesses the form World",
      );
      const pixels = new Uint8Array(frame.pixels);
      let chromatic = 0;
      for (let index = 0; index < pixels.length; index += 4)
        if (
          Math.max(pixels[index]!, pixels[index + 1]!, pixels[index + 2]!) -
            Math.min(pixels[index]!, pixels[index + 1]!, pixels[index + 2]!) >
          30
        )
          chromatic++;
      check(
        chromatic > 500,
        `${name} contains meaningful skinned control pixels (${chromatic})`,
      );
      await environment.capture(name, frame);
      return frame;
    }
    const before = await capture("before");
    const ambiguous = await agent.act({
      steps: [{ target: { label: "Save" }, action: { kind: "press" } }],
    });
    check(
      ambiguous.steps[0]?.error === "AmbiguousTarget" &&
        ambiguous.steps[0].matches.length === 2 &&
        reports.presses === 0,
      "Ambiguity changes nothing",
    );
    const changed = await agent.act({
      steps: [
        { target: { ref: save.ref }, action: { kind: "press" } },
        {
          target: { ref: project.ref },
          action: { kind: "text", value: "Atlas" },
        },
        { target: { ref: america.ref }, action: { kind: "press" } },
        {
          target: { ref: controlled.ref },
          action: { kind: "text", value: "Rejected intent" },
        },
      ],
    });
    check(
      changed.ok && changed.steps.every((step) => step.batch?.ok),
      "Representative actions have native acknowledgements",
    );
    check(
      one(changed.observed, "Project").state.text === "Atlas",
      "Final observed text matches accepted edit",
    );
    check(
      one(changed.observed, "Controlled").state.text === "Fixed",
      "Controlled application convergence differs honestly from successful action ACK",
    );
    check(
      Number(reports.presses) === 1 &&
        reports.texts.includes("Atlas") &&
        reports.controlled.includes("Rejected intent") &&
        reports.selected.includes("us"),
      "Real React callbacks observe committed actions",
    );
    check(
      effects.includes("pressed"),
      "Ordinary GUI observer receives semantic press effects",
    );
    const selected = await agent.query({ within: group.ref });
    check(
      one(selected, "America").state.selected === true &&
        one(selected, "Europe").state.selected === false,
      "Native single selection commits both item fields",
    );
    const after = await capture("after");
    let different = 0;
    const oldPixels = new Uint8Array(before.pixels);
    const newPixels = new Uint8Array(after.pixels);
    for (let index = 0; index < oldPixels.length; index += 4)
      if (
        Math.abs(oldPixels[index]! - newPixels[index]!) +
          Math.abs(oldPixels[index + 1]! - newPixels[index + 1]!) +
          Math.abs(oldPixels[index + 2]! - newPixels[index + 2]!) >
        50
      )
        different++;
    check(
      different > 40,
      `Accepted text/selection changes complete visible pixels (${different})`,
    );

    successfulBatch(
      await writer.batch([
        write(writer, project, "GuiTextInput", { text: "Other writer" }),
      ]),
    );
    check(
      one(await agent.snapshot(), "Project").state.text === "Other writer",
      "Independent writer is read from runtime, without mirrored state",
    );
    const partial = await agent.act({
      steps: [
        {
          target: { ref: project.ref },
          action: { kind: "text", value: "Prefix remains" },
        },
        { target: { ref: disabled.ref }, action: { kind: "press" } },
        {
          target: { ref: project.ref },
          action: { kind: "text", value: "Must not run" },
        },
      ],
    });
    check(
      !partial.ok &&
        partial.steps[0]?.status === "acknowledged" &&
        partial.steps[1]?.error === "Unavailable" &&
        partial.steps[2]?.status === "notRun",
      "Partial failure preserves acknowledged prefix and skips suffix",
    );
    check(
      one(partial.observed, "Project").state.text === "Prefix remains",
      "Observed values retain prefix effects",
    );
    const notSent = await agent.act({
      steps: [
        {
          target: { ref: project.ref },
          action: { kind: "text", value: "x".repeat(2_000_000) },
        },
      ],
    });
    check(
      notSent.steps[0]?.status === "rejected" &&
        notSent.steps[0].error === "IPP_REQUEST_NOT_SENT" &&
        notSent.steps[0].batch === null,
      "Generated encoding rejection is definitely unapplied, not transport uncertainty",
    );
    check(
      one(notSent.observed, "Project").state.text === "Prefix remains",
      "Locally rejected request leaves native value unchanged and session usable",
    );

    successfulBatch(
      await writer.batch([
        write(writer, project, "GuiBehavior", { visible: false }),
      ]),
    );
    const hidden = await agent.query({ label: "Project" });
    check(
      !one(hidden, "Project").visible &&
        one(hidden, "Project").actions.length === 0,
      "Hidden state remains discoverable and non-actionable",
    );
    check(
      (
        await agent.act({
          steps: [
            {
              target: { ref: project.ref },
              action: { kind: "text", value: "hidden" },
            },
          ],
        })
      ).steps[0]?.error === "Unavailable",
      "Native rejects hidden target",
    );
    successfulBatch(
      await writer.batch([
        write(writer, project, "GuiBehavior", { visible: true }),
      ]),
    );
    successfulBatch(
      await writer.batch([
        write(writer, project, "PresentationGate", { open: false }),
      ]),
    );
    check(
      one(await agent.snapshot(), "Project").available === false,
      "Retained presentation gate exposes native unavailability",
    );
    check(
      (
        await agent.act({
          steps: [{ target: { ref: project.ref }, action: { kind: "focus" } }],
        })
      ).steps[0]?.error === "Unavailable",
      "Native rejects retained unavailable target",
    );
    successfulBatch(
      await writer.batch([
        write(writer, project, "PresentationGate", { open: true }),
        write(writer, project, "GuiTextInput", { masked: true }),
      ]),
    );
    check(
      one(await agent.snapshot(), "Project").state.text === null,
      "Masked text is redacted from semantic observations",
    );
    successfulBatch(
      await writer.batch([
        write(writer, project, "GuiTextInput", { numeric: true }),
        write(writer, project, "GuiTextInput", { value: 1234 }),
      ]),
    );
    const masked = one(await agent.query({ label: "Project" }), "Project");
    check(
      masked.state.text === null &&
        masked.state.value === null &&
        masked.state.numeric === true,
      "Masked numeric text and committed number are both redacted",
    );
    const maskedAction = await agent.act({
      steps: [
        {
          target: { ref: project.ref },
          action: { kind: "scalar", value: 5678 },
        },
      ],
    });
    check(
      maskedAction.ok &&
        one(maskedAction.observed, "Project").state.value === null,
      "Post-action observations retain numeric redaction",
    );
    const actualNumber = await main.inspectPage({
      collection: "entities",
      target: BigInt(project.entity),
    });
    check(
      actualNumber.entities[0]?.components.find(
        (entry) => entry.component === project.component,
      )?.fields.value === 5678,
      "Redaction does not replace native committed number",
    );
    successfulBatch(
      await writer.batch([
        write(writer, project, "GuiTextInput", { masked: false }),
        write(writer, project, "GuiTextInput", { numeric: false }),
      ]),
    );

    // Unmount releases observers but preserves authored content, so replacement
    // tests do not race React's owning declarations.
    await Promise.all(roots.map((root) => root.unmount()));
    roots.length = 0;
    await verifyAgentGroupOrder(other, contract.createGuiAgent);
    const nestedGroups = successfulBatch(
      await writer.batch([
        createEntity(1, "outer-group"),
        insertComponent(writer, "GuiGroup", { kind: "alias", alias: 1 }),
        createEntity(2, "nested-group-control"),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 2 },
          placement: { parent: { kind: "alias", alias: 1 }, before: null },
        },
        insertComponent(writer, "GuiGroup", { kind: "alias", alias: 2 }),
        insertComponent(
          writer,
          "GuiButton",
          { kind: "alias", alias: 2 },
          { label: "Nested group owner" },
        ),
        createEntity(3, "nested-group-option"),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 3 },
          placement: { parent: { kind: "alias", alias: 2 }, before: null },
        },
        insertComponent(
          writer,
          "GuiButton",
          { kind: "alias", alias: 3 },
          { label: "Nested option" },
        ),
        createEntity(4, "outer-group-option"),
        {
          kind: "placeEntity",
          entity: { kind: "alias", alias: 4 },
          placement: { parent: { kind: "alias", alias: 1 }, before: null },
        },
        insertComponent(
          writer,
          "GuiButton",
          { kind: "alias", alias: 4 },
          { label: "Outer option" },
        ),
      ]),
    );
    const groupAgent = contract.createGuiAgent({ main });
    const groupSnapshot = await groupAgent.snapshot();
    const outerGroup = groupSnapshot.controls.find(
      (control) =>
        control.kind === "group" &&
        control.entity === String(aliasId(nestedGroups, 1)),
    )!;
    const innerGroup = groupSnapshot.controls.find(
      (control) =>
        control.kind === "group" &&
        control.entity === String(aliasId(nestedGroups, 2)),
    )!;
    check(
      (await groupAgent.query({ within: outerGroup.ref })).controls
        .map((control) => control.label)
        .join(",") === "Outer option",
      "Ancestor group excludes nested group entity even when it also holds a control",
    );
    check(
      (await groupAgent.query({ within: innerGroup.ref })).controls
        .map((control) => control.label)
        .join(",") === "Nested option",
      "Nested group's own items remain discoverable",
    );
    let watchStarts = 0;
    let watchRemovals = 0;
    let injectRace = true;
    let duplicate: bigint | undefined;
    const racedClient = new Proxy(main, {
      get(client, key) {
        if (key === "watchLifecycle")
          return async (...args: Parameters<Client["watchLifecycle"]>) => {
            const watch = await client.watchLifecycle(...args);
            watchStarts++;
            // Replace between the baseline and inspection. The adapter must wait
            // through the remove ACK and exclude this changed lifetime.
            if (injectRace) {
              injectRace = false;
              const entity = {
                kind: "handle" as const,
                id: BigInt(project.entity),
              };
              const outcome = await writer.batch([
                {
                  kind: "removeComponent",
                  entity,
                  component: project.component,
                },
                insertComponent(writer, "GuiTextInput", entity, {
                  text: "Raced replacement",
                }),
                createEntity(1, `raced-duplicate-${watchStarts}`),
                insertComponent(
                  writer,
                  "GuiButton",
                  { kind: "alias", alias: 1 },
                  { label: "Save" },
                ),
              ]);
              duplicate = aliasId(outcome, 1);
            }
            let removed = false;
            return {
              ...watch,
              async remove() {
                const cuts = await watch.remove();
                if (!removed) {
                  watchRemovals++;
                  removed = true;
                }
                return cuts;
              },
            };
          };
        const value: unknown = Reflect.get(client, key, client);
        return typeof value === "function" ? value.bind(client) : value;
      },
    });
    const racedAgent = contract.createGuiAgent({ main: racedClient });
    const raced = await racedAgent.snapshot();
    check(
      raced.observations[0]?.invalidated === 2 &&
        !raced.controls.some((control) => control.label === "Project"),
      "Changed lifetime and unwatched added control make discovery explicitly incomplete",
    );
    check(
      watchStarts === 1 && watchRemovals === 1,
      "Temporary lifecycle watch removal is acknowledged before snapshot returns",
    );
    successfulBatch(
      await writer.batch([
        { kind: "delete", entity: { kind: "handle", id: duplicate! } },
      ]),
    );
    injectRace = true;
    const incomplete = await racedAgent.act({
      steps: [{ target: { label: "Save" }, action: { kind: "press" } }],
    });
    check(
      incomplete.steps[0]?.error === "IncompleteObservation" &&
        incomplete.steps[0].batch === null,
      "Known omitted duplicate prevents label-based submission",
    );
    check(
      Number(watchStarts) === Number(watchRemovals),
      "All temporary observation watches have acknowledged cleanup",
    );
    successfulBatch(
      await writer.batch([
        { kind: "delete", entity: { kind: "handle", id: duplicate! } },
      ]),
    );
    const direct: GuiAgent = contract.createGuiAgent({ main, other });
    const old = one(await direct.snapshot(), "Project");
    const entity = { kind: "handle" as const, id: BigInt(old.entity) };
    successfulBatch(
      await writer.batch([
        { kind: "removeComponent", entity, component: old.component },
        insertComponent(writer, "GuiTextInput", entity, {
          text: "Replacement",
        }),
      ]),
    );
    const stale = await direct.act({
      steps: [
        {
          target: { ref: old.ref },
          action: { kind: "text", value: "Wrong lifetime" },
        },
      ],
    });
    check(
      stale.steps[0]?.error === "StaleTarget" &&
        one(stale.observed, "Project").state.text === "Replacement",
      "Old reference cannot retarget replaced component",
    );
    check(
      one(stale.observed, "Project").ref !== old.ref,
      "Replacement receives a new reference",
    );
    const isolated = contract.createGuiAgent({ main });
    check(
      (
        await isolated.act({
          steps: [{ target: { ref: old.ref }, action: { kind: "press" } }],
        })
      ).steps[0]?.error === "StaleReference",
      "References cannot cross adapter instances",
    );
    const bounded = await contract
      .createGuiAgent({ main }, { maxEntities: 1 })
      .snapshot();
    check(
      bounded.observations[0]?.truncated,
      "Bounded enumeration explicitly discloses truncation",
    );
    check(
      errors.length === 0,
      `React errors: ${errors.map((error) => error.message).join("; ")}`,
    );
    await subscription.unsubscribe();
    await host.presentation.clear(view);
    view = undefined;
    const finalSave = one(await direct.snapshot(), "Save");
    await environment.record({
      schemas: {
        tools: contract.guiAgentTools,
        result: contract.guiAgentResultSchema,
      },
      initial,
      changed,
      partial,
      notSent,
      stale,
      raced,
      incomplete,
      watchStarts,
      watchRemovals,
      reports,
      different,
      schemaHashes: initial.observations.map((entry) => entry.world.schemaHash),
    });
    environment.loseConnectionOnNextSend();
    const lost = await direct.act({
      steps: [
        { target: { ref: finalSave.ref }, action: { kind: "press" } },
        { target: { ref: finalSave.ref }, action: { kind: "press" } },
      ],
    });
    disconnected = true;
    check(
      lost.steps[0]?.status === "unknown" &&
        lost.steps[0].batch === null &&
        lost.steps[1]?.status === "notRun",
      "Lost acknowledgement remains unknown with no retry or suffix",
    );
    check(
      lost.observed.observations.every(
        (entry) => entry.status === "unavailable",
      ),
      "Disconnected Worlds are explicitly unavailable",
    );
    const closed = await direct.act({
      steps: [{ target: { ref: finalSave.ref }, action: { kind: "press" } }],
    });
    check(
      closed.steps[0]?.error === "SessionClosed",
      "Known closed session is rejected before submission",
    );
    await environment.record({ lost, closed });
    return {
      presses: reports.presses,
      callbacks: reports,
      different,
      worlds: initial.observations.map((entry) => entry.world),
    };
  } finally {
    await Promise.all(roots.map((root) => root.unmount().catch(() => {})));
    if (!disconnected) {
      await subscription.unsubscribe().catch(() => {});
      if (view) await host.presentation.clear(view).catch(() => {});
      await Promise.all(
        worlds.map((world) =>
          host.destroyWorld(world.reference).catch(() => {}),
        ),
      );
    }
    await host.close();
  }
}
