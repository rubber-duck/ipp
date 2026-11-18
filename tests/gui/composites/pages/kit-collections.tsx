import { TransformedControls } from "./transformed-controls.js";
import { useState } from "react";
import type { GuiPhysicalContext } from "@ipp/client";
import type { GuiControlHandle } from "../../../../packages/ipp-react/src/gui.js";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import {
  createRoot,
  type ReactWorldClient,
} from "../../../../packages/ipp-react/src/index.js";
import {
  Button,
  Column,
  Command,
  Combobox,
  GuiKit,
  HoverCard,
  Menubar,
  NavigationMenu,
  TextField,
  TriggerMenu,
  type TriggerMatch,
} from "../../../../packages/ipp-react/src/gui-kit.js";
import {
  canvasOf,
  componentFields,
  openCompositePage,
  overlayVisible,
  readControl,
  symbols,
  type CompositeSetup,
  type PanelSpec,
} from "./composites.js";
export { nativePresentationTransport, workerTransport } from "./composites.js";

function MentionField({
  context,
  events,
}: {
  context: GuiPhysicalContext | undefined;
  events: string[];
}) {
  const [field, setField] = useState<GuiControlHandle | null>(null);
  return (
    <>
      <TextField id="mention" label="Mention" multiline ref={setField} />
      {context && (
        <TriggerMenu
          id="mention-menu"
          context={context}
          target={field?.target ?? null}
          triggers={["@"]}
          candidates={mentions}
          onInsert={(option) => events.push("insert/" + option.key)}
          onError={(reason) => events.push(reason.message)}
        />
      )}
    </>
  );
}

const mentions = async ({ query }: TriggerMatch, signal: AbortSignal) => {
  // Hold the old result long enough for replacement queries to resolve first.
  if (query === "a") await new Promise((resolve) => setTimeout(resolve, 250));
  if (signal.aborted) return [];
  return ["alex", "alice"]
    .filter((name) => name.startsWith(query))
    .map((name) => ({ key: name, label: name, insert: "@" + name + " " }));
};

const panel = {
  name: "collections",
  origin: [0, 0],
  extent: [312, 312],
  async build({ client, font, contract, report }) {
    const root = createRoot(client as unknown as ReactWorldClient, {
      onError: report,
    });
    const events: string[] = [];
    const actions = [
      {
        key: "projects",
        label: "Projects",
        children: [
          { key: "locked", label: "Locked", disabled: true },
          { key: "create", label: "Create project" },
        ],
      },
      { key: "save", label: "Save" },
    ];
    const options = [
      { key: "disabled", label: "Almanac", disabled: true },
      { key: "alpine", label: "Alpine" },
      { key: "bay", label: "Bay" },
    ];
    let choiceMode: "multiple" | "limited" | "controlled" | "free" = "multiple";
    let input: GuiPhysicalContext | undefined;
    const draw = (glass: boolean) =>
      root.render(
        <GuiKit
          contract={contract}
          skin={glass ? glassSkin(contract) : cybertronSkin(contract)}
          font={font}
          fontSize={8}
        >
          <Column id="suite" width={210} height={310} padding={4} gap={2}>
            <Command
              id="palette"
              items={actions}
              onCommand={(key, path) => events.push([...path, key].join("/"))}
            />
            <Combobox
              id="combo"
              label="Destination"
              key={choiceMode === "limited" ? "multiple" : choiceMode}
              multiple={choiceMode === "multiple" || choiceMode === "limited"}
              allowCustomValue={choiceMode === "free"}
              {...(choiceMode === "controlled" ? { value: ["bay"] } : {})}
              options={
                choiceMode === "limited"
                  ? options.filter((option) => option.key !== "alpine")
                  : options
              }
              onChange={(keys) => events.push("selection:" + keys.join(","))}
            />
            <Menubar
              id="bar"
              label="Commands"
              menus={[
                {
                  key: "file",
                  label: "File",
                  items: [{ key: "save", label: "Save" }],
                },
                { key: "locked", label: "Locked", disabled: true, items: [] },
                {
                  key: "edit",
                  label: "Edit",
                  items: [
                    { key: "cut", label: "Cut", disabled: true },
                    { key: "copy", label: "Copy" },
                  ],
                },
              ]}
              onCommand={(menu, key) => events.push(menu + "/" + key)}
            />
            <NavigationMenu
              id="nav"
              label="Navigation"
              activeDestination="home"
              sections={[
                {
                  id: "places",
                  label: "Places",
                  destinations: [
                    { id: "home", label: "Home" },
                    { id: "hidden", label: "Hidden", disabled: true },
                    { id: "map", label: "Map" },
                  ],
                },
                {
                  id: "library",
                  label: "Library",
                  destinations: [{ id: "docs", label: "Documentation" }],
                },
              ]}
              onNavigate={(item) => events.push("route/" + item.id)}
            />
            <Button id="preview-trigger" label="Preview">
              <HoverCard id="preview" interactive>
                <Button
                  id="preview/detail"
                  label="Details"
                  onPress={() => events.push("details")}
                />
              </HoverCard>
            </Button>
            <MentionField context={input} events={events} />
          </Column>
        </GuiKit>,
      );
    await draw(false);
    return {
      events,
      draw,
      scale(factor: number, drawer = false) {
        return root.render(
          <GuiKit
            contract={contract}
            skin={cybertronSkin(contract)}
            font={font}
            fontSize={8}
          >
            <TransformedControls
              scale={factor}
              drawer={drawer}
              report={(value) => events.push(value)}
            />
          </GuiKit>,
        );
      },
      choice(mode: typeof choiceMode) {
        choiceMode = mode;
        return draw(false);
      },
      bind(context: GuiPhysicalContext) {
        input = context;
        return draw(false);
      },
      close: () => root.unmount(),
    };
  },
} satisfies PanelSpec;
export const CANVAS = canvasOf([panel]);

export async function prepare(setup: CompositeSetup) {
  const page = await openCompositePage(setup, [panel]);
  const instance = page.panel("collections");
  const kit = instance.build as Awaited<ReturnType<typeof panel.build>>;
  await kit.bind(page.input);
  const ids = () => symbols(instance.client);
  const entity = async (name: string) => {
    let found: bigint | undefined;
    await page.until(
      async () => {
        found = (await ids()).get(name);
        return found !== undefined;
      },
      () => "Missing " + name,
    );
    return found!;
  };
  const observed = async (name: string, component: string) => {
    const id = await entity(name);
    const snapshot = await instance.client.inspectPage({
      collection: "entities",
      target: id,
      limit: 1,
    });
    return componentFields(instance.client, snapshot, id, component);
  };
  const box = async (name: string) => {
    const fields = await observed(name, "CanvasBounds");
    if (!fields) throw new Error("Missing CanvasBounds on " + name);
    return {
      x: Number(fields.x),
      y: Number(fields.y),
      width: Number(fields.width),
      height: Number(fields.height),
      localWidth: Number(fields.local_width),
      scaleX: Number(fields.scale_x),
    };
  };
  return {
    ...page.steps,
    scaled: kit.scale,
    scaleBox: box,
    async layoutLength(name: string, field: "width" | "height", value: number) {
      await page.until(
        async () =>
          Math.abs(
            Number((await observed(name, "GuiLayout"))?.[field]) - value,
          ) < 0.05,
        () => name + " " + field + " should be " + value,
      );
    },
    async expectScaleBox(name: string, width: number, scale: number) {
      let latest: Awaited<ReturnType<typeof box>> | undefined;
      await page.until(
        async () => {
          latest = await box(name);
          return (
            Math.abs(latest.width - width * scale) < 0.01 &&
            Math.abs(latest.localWidth - width) < 0.01 &&
            Math.abs(latest.scaleX - scale) < 0.01
          );
        },
        () => name + " " + JSON.stringify(latest),
      );
      return latest!;
    },
    async tokenMargin(expected?: number) {
      let margin = Infinity;
      await page.until(
        async () => {
          margin = Number(
            (await observed("scaled-token/input/0/content", "GuiLayout"))
              ?.margin_left,
          );
          return expected === undefined
            ? margin < -5
            : Math.abs(margin - expected) < 0.01;
        },
        () => "Token scroll " + margin + " expected " + expected,
      );
      return margin;
    },
    async collectionPoint(name: string) {
      const control = await readControl(instance.client, await entity(name));
      if (!control) throw new Error("Missing control " + name);
      const [x, y, width, height] = control.bounds;
      return [x + width / 2, y + height / 2] as const;
    },
    async shown(name: string, open: boolean, scenario = "") {
      const id = await entity(name);
      await page.until(
        async () => (await overlayVisible(instance.client, id)) === open,
        () => name + " visible " + open + " " + scenario,
      );
    },
    async focused(name: string) {
      const id = await entity(name);
      await page.until(
        async () =>
          (await instance.client.inspectPage({ collection: "guiFocus" }))
            .guiFocus?.[0]?.target.entity === id,
        () => "Focus on " + name,
      );
    },
    async activeRow(name: string) {
      const id = await entity(name);
      let names: string[] = [];
      const table = await ids();
      await page.until(
        async () => {
          const items =
            (
              await instance.client.inspectPage({
                collection: "guiActiveItems",
              })
            ).guiActiveItems ?? [];
          names = items.map(
            (item) =>
              [...table].find((pair) => pair[1] === item.target.entity)?.[0] ??
              String(item.target.entity),
          );
          return items.some((item) => item.target.entity === id);
        },
        () =>
          "Expected active row " +
          name +
          "; actual " +
          JSON.stringify(names) +
          "; native text " +
          page.input.nativeText?.text,
      );
    },
    async absent(name: string) {
      await page.until(
        async () => !(await ids()).has(name),
        () => name + " is still mounted; text " + page.input.nativeText?.text,
      );
    },
    async native(name: string) {
      return page.expectNativeBuffer(await entity(name));
    },
    async event(expected: string, count = 1) {
      await page.until(
        () =>
          Promise.resolve(
            kit.events.filter((event) => event === expected).length === count,
          ),
        () => expected + " in " + JSON.stringify(kit.events),
      );
    },
    choice: kit.choice,
    async field(name: string, key: string, expected: string | boolean) {
      const id = await entity(name);
      await page.until(
        async () =>
          (await readControl(instance.client, id))?.fields[key] === expected,
        () => name + " " + key + " = " + expected,
      );
    },
    async nativeText(text: string) {
      await page.until(
        async () => page.input.nativeText?.text === text,
        () => "Expected " + text + " got " + page.input.nativeText?.text,
      );
    },
    async compose(text: string) {
      const field = document.activeElement;
      if (!(field instanceof HTMLTextAreaElement))
        throw new Error("Expected native editing buffer");
      field.dispatchEvent(
        new CompositionEvent("compositionstart", { bubbles: true }),
      );
      field.dispatchEvent(
        new CompositionEvent("compositionupdate", {
          data: text,
          bubbles: true,
        }),
      );
      await page.until(
        async () => page.input.nativeText?.composition?.text === text,
        () => "Missing marked text " + text,
      );
    },
    async finishComposition(text: string) {
      document.activeElement?.dispatchEvent(
        new CompositionEvent("compositionend", { data: text, bubbles: true }),
      );
      await page.until(
        async () => !page.input.nativeText?.composition,
        () => "Still composing",
      );
    },
    async caretPoint() {
      const caret = page.input.nativeText?.caretBounds;
      if (!caret) throw new Error("Missing native caret bounds");
      return [...caret];
    },
    async skin(glass: boolean) {
      const earlier = new Uint8Array(
        (await page.host.presentation.capture(page.view)).pixels,
      );
      await kit.draw(glass);
      await page.until(
        async () => {
          const pixels = new Uint8Array(
            (await page.host.presentation.capture(page.view)).pixels,
          );
          let changed = 0;
          for (let i = 0; i < pixels.length; i += 4) {
            if (
              Math.abs(pixels[i]! - earlier[i]!) +
                Math.abs(pixels[i + 1]! - earlier[i + 1]!) +
                Math.abs(pixels[i + 2]! - earlier[i + 2]!) >
                30 &&
              ++changed > 100
            )
              return true;
          }
          return false;
        },
        () => "New skin did not reach the presented frame",
      );
    },
  };
}
