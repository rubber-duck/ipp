/** Shared control proportions and constrained form sizing on both client skins. */
import type { Client, HostClientBase, RootBinding } from "@ipp/client";
import {
  Asset,
  assetRef,
  createRoot,
  guiWorldOptions,
  type ReactWorldClient,
} from "@ipp/react";
import {
  Button,
  Column,
  GuiKit,
  Panel,
  PanelHeader,
  Row,
  ScrollArea,
  Switch,
  TextLine,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import { glassSkin } from "@ipp/react/gui-glass";
import type { GuiContract } from "../pages/gui-authoring.js";
import type { WidgetPresentation } from "./gui-widgets.js";
import { check } from "../../../harness/page/checks.js";

export async function guiWidgetSizing(
  host: HostClientBase<Client>,
  contract: GuiContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present?: WidgetPresentation,
) {
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const report: string[] = [];
  const differences: number[] = [];
  for (const [name, skin] of [
    ["cybertron", cybertronSkin(contract)],
    ["glass", glassSkin(contract)],
  ] as const) {
    const world = (
      await host.createWorld(
        guiWorldOptions({
          temporary: true,
          canvas: { extent: [400, 460], unitsPerMetre: 1 },
        }),
      )
    ).reference;
    const client = await host.openWorld(world);
    const root = createRoot(client as unknown as ReactWorldClient);
    let binding: RootBinding | undefined;
    let presses = 0;
    try {
      await root.render(
        <>
          <Asset
            id="sizing-font"
            kind={17}
            data={font}
            encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
          />
          <GuiKit
            skin={skin}
            contract={contract}
            font={assetRef("sizing-font")}
            fontSize={17}
          >
            <Column width={400} height={460} padding={20} gap={12}>
              <TextLine text={`${name} control defaults`} tone="accent" />
              <Row>
                <TextLine text="Smooth trace" layout={{ flex: 1 }} />
                <Switch id="size-toggle" semanticLabel="Smooth trace" />
              </Row>
              <Switch id="size-labelled" label="Labelled switch" />
              <Panel height={240} padding={0}>
                <PanelHeader title="Scrollable form" />
                <ScrollArea id="size-scroll" flex={1} gap={0}>
                  <Column height={200}>
                    <TextLine text="More form content" />
                  </Column>
                  <Button
                    id="size-pulse"
                    label="Send pulse"
                    onPress={() => {
                      presses++;
                    }}
                  />
                </ScrollArea>
              </Panel>
            </Column>
          </GuiKit>
        </>,
      );
      binding = await host.setRootOutput(
        { kind: "canvas", world },
        { width: 400, height: 460, devicePixelRatio: 1 },
      );
      const section: Parameters<WidgetPresentation>[1] = async (view) => {
        for (let count = 0; count < 100; count++) {
          const state = await client.inspect();
          check(
            !state.resources.some((resource) => resource.status === "failed"),
            `${name}: resources failed`,
          );
          if (state.resources.every((resource) => resource.status === "loaded"))
            break;
          check(count < 99, `${name}: resources did not load`);
          await client.waitForFrame(state.tick);
        }
        const rect = async (id: string) => {
          const entity = (await client.inspect()).entities.find(
            (entity) => entity.metadata.symbolicId === id,
          );
          const fields = entity?.components.find(
            (component) =>
              component.component === client.components.CanvasBounds!.id,
          )?.fields;
          check(fields, `${name}: ${id} evaluated bounds`);
          return ["x", "y", "width", "height"].map((field) =>
            Number(fields[field]),
          );
        };
        const before = await host.presentation.capture(view);
        const save = (label: string, frame: typeof before) =>
          images.push({
            label,
            width: 400,
            height: 460,
            pixels: Array.from(new Uint8Array(frame.pixels)),
            sequence: frame.sequence,
          });
        save(`${name}-control-sizes-before`, before);
        const toggle = await rect("size-toggle");
        const labelled = await rect("size-labelled");
        const button = await rect("size-pulse");
        const factor = 17 / skin.tokens.em;
        check(
          Math.abs(toggle[2]! - 40 * factor) < 0.01 &&
            Math.abs(toggle[3]! - 22 * factor) < 0.01,
          `${name}: switch must use wide low skin dimensions: ${toggle}`,
        );
        check(
          Math.abs(button[3]! - skin.tokens.controlHeight * factor) < 0.01,
          `${name}: constrained form shrank its terminal button: ${button}`,
        );
        const input = await host.input.open(view);
        const click = async (box: number[], track = false) => {
          const point = [
            (box[0]! + (track ? 40 * factor * 0.85 : box[2]! / 2)) / 400,
            (box[1]! + box[3]! / 2) / 460,
          ] as const;
          await input.send({ kind: "pointerMove", pointer: 1n, point });
          const down = await input.send({
            kind: "pointerDown",
            pointer: 1n,
            point,
          });
          await input.send({ kind: "pointerUp", pointer: 1n, point });
          check(
            down.disposition === "routed",
            `${name}: real pointer input routed`,
          );
          await root.flush();
          await host.presentation.frame(view, {
            afterOutputs: [{ kind: "canvas", world }],
          });
        };
        try {
          await click(toggle, true);
          await click(labelled, true);
          const scroll = await rect("size-scroll");
          await input.send({
            kind: "wheel",
            point: [
              (scroll[0]! + scroll[2]! / 2) / 400,
              (scroll[1]! + scroll[3]! / 2) / 460,
            ],
            delta: [0, 400],
          });
          await host.presentation.frame(view);
          await click(await rect("size-pulse"));
          check(
            presses === 1,
            `${name}: natural-size button remains usable after scrolling`,
          );
          const after = await host.presentation.capture(view);
          save(`${name}-control-sizes-after`, after);
          let changed = 0;
          const first = new Uint8Array(before.pixels),
            last = new Uint8Array(after.pixels);
          for (
            let y = Math.ceil(labelled[1]! + 2);
            y < labelled[1]! + labelled[3]! - 2;
            y++
          )
            for (
              let x = Math.ceil(labelled[0]! + 40 * factor * 0.6);
              x < labelled[0]! + 40 * factor - 2;
              x++
            ) {
              const at = (y * 400 + x) * 4;
              if (
                Math.abs(first[at]! - last[at]!) +
                  Math.abs(first[at + 1]! - last[at + 1]!) +
                  Math.abs(first[at + 2]! - last[at + 2]!) >
                40
              )
                changed++;
            }
          differences.push(changed);
          report.push(
            `${name}: switch ${toggle[2]}×${toggle[3]}, terminal button height ${button[3]}, labelled right-track changed pixels ${changed}`,
          );
        } finally {
          await input.close();
        }
      };
      if (present) await present(binding, section);
      else {
        const view = await host.presentation.select(
          await host.presentation.surface(),
          binding,
        );
        try {
          await section(view);
        } finally {
          await host.presentation.clear(view);
        }
      }
    } finally {
      await root.render(null);
      await root.unmount();
      if (binding) await host.clearRootOutput(binding);
      await client.close();
      await host.destroyWorld(world);
    }
  }
  // Labeled tracks use the same authored56:24 ratio as unlabeled controls.
  return {
    failure: differences.every((changed) => changed > 40)
      ? null
      : `Labelled switches did not move into their wide track: ${differences}`,
    images,
    report,
  };
}
