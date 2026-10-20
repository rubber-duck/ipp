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
  PanelBody,
  PanelFooter,
  Form,
  Field,
  TextField,
  Dropdown,
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
  try {
    for (const [name, skin] of [
      ["cybertron", cybertronSkin(contract)],
      ["glass", glassSkin(contract)],
    ] as const) {
      const world = (
        await host.createWorld(
          guiWorldOptions({
            temporary: true,
            canvas: { extent: [400, 640], unitsPerMetre: 1 },
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
          { width: 400, height: 640, devicePixelRatio: 1 },
        );
        const section: Parameters<WidgetPresentation>[1] = async (view) => {
          for (let count = 0; count < 100; count++) {
            const state = await client.inspect();
            check(
              !state.resources.some((resource) => resource.status === "failed"),
              `${name}: resources failed`,
            );
            if (
              state.resources.every((resource) => resource.status === "loaded")
            )
              break;
            check(count < 99, `${name}: resources did not load`);
            await client.waitForFrame(state.tick);
          }
          const rect = async (id: string) => {
            let state = await client.inspect();
            const boundsId = client.components.CanvasBounds!.id;
            const entity = state.entities.find(
              (entity) => entity.metadata.symbolicId === id,
            );
            check(entity, `${name}: ${id} exists`);
            if (
              !entity.components.some(
                (component) => component.component === boundsId,
              )
            ) {
              const inserted = await client.batch([
                {
                  kind: "insertComponent",
                  entity: { kind: "symbol", symbol: id },
                  component: boundsId,
                  fields: [],
                },
              ]);
              check(inserted.ok, `${name}: observe layout bounds`);
              await client.waitForFrame(state.tick);
              state = await client.inspect();
            }
            const fields = state.entities
              .find((entity) => entity.metadata.symbolicId === id)
              ?.components.find(
                (component) => component.component === boundsId,
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
              height: 640,
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
              (box[1]! + box[3]! / 2) / 640,
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
                (scroll[1]! + scroll[3]! / 2) / 640,
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
          // The same ordinary composition adapts to width and font scale with
          // logical author dimensions, token insets, and no manual child margins.
          let controlIdentities: bigint[] | undefined;
          for (const [panelWidth, fontSize] of [
            [280, 16],
            [360, 20],
          ] as const) {
            let applied = 0;
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
                  fontSize={fontSize}
                >
                  <Panel id="layout-panel" width={panelWidth}>
                    <PanelHeader id="layout-header" title="Appearance" />
                    <PanelBody id="layout-body">
                      <Form id="layout-form">
                        <TextField
                          id="layout-name"
                          label="Name"
                          defaultValue="Bridge"
                          description="Shown in the scene"
                          error="Choose a unique name"
                        />
                        <Field id="layout-material-field" label="Material">
                          <Dropdown
                            id="layout-material"
                            label="Material"
                            options={[{ key: "glass", label: "Glass" }]}
                            defaultValue="glass"
                          />
                        </Field>
                        <Row id="layout-caption-row">
                          <TextLine
                            id="layout-caption"
                            text="Ready"
                            size="small"
                          />
                          <TextLine
                            text="Local changes"
                            size="small"
                            tone="neutral"
                          />
                        </Row>
                      </Form>
                    </PanelBody>
                    <PanelFooter
                      id="layout-footer"
                      leading={
                        panelWidth === 360 ? (
                          <TextLine
                            id="layout-status"
                            text="Ready"
                            size="small"
                          />
                        ) : undefined
                      }
                    >
                      <Button
                        id="layout-cancel"
                        width={72}
                        label="Cancel"
                        variant="secondary"
                      />
                      <Button
                        id="layout-apply"
                        width={72}
                        label="Apply"
                        onPress={() => {
                          applied++;
                        }}
                      />
                    </PanelFooter>
                  </Panel>
                </GuiKit>
              </>,
            );
            await root.flush();
            await host.presentation.frame(view, {
              afterOutputs: [{ kind: "canvas", world }],
            });
            const panel = await rect("layout-panel"),
              body = await rect("layout-body");
            const field = await rect("layout-name"),
              select = await rect("layout-material");
            const inputBox = await rect("layout-name/input"),
              label = await rect("layout-name/field/label"),
              help = await rect("layout-name/field/description"),
              error = await rect("layout-name/field/error"),
              materialField = await rect("layout-material-field"),
              header = await rect("layout-header"),
              footer = await rect("layout-footer");
            const cancel = await rect("layout-cancel"),
              apply = await rect("layout-apply");
            const row = await rect("layout-caption-row"),
              caption = await rect("layout-caption");
            const scale = fontSize / skin.tokens.em;
            const inset =
              (skin.tokens.spacing?.inset ?? skin.tokens.inset) * scale;
            const labelGap =
              (skin.tokens.spacing?.label ?? skin.tokens.inset / 2) * scale;
            const near = (a: number, b: number, message: string) =>
              check(Math.abs(a - b) < 0.1, `${name}: ${message}: ${a} vs ${b}`);
            near(
              panel[2]!,
              panelWidth,
              "explicit width stays in logical units",
            );
            near(body[0]!, panel[0]!, "body begins at the unpadded frame");
            near(field[0]! - panel[0]!, inset, "one content inset");
            near(field[2]!, panelWidth - 2 * inset, "form fills content width");
            near(select[2]!, field[2]!, "select and text field share width");
            near(body[1]!, header[1]! + header[3]!, "body follows header");
            near(field[1]! - body[1]!, inset, "body top inset");
            near(footer[1]! - row[1]! - row[3]!, inset, "body bottom inset");
            near(
              inputBox[3]!,
              skin.tokens.controlHeight * scale,
              "text input keeps full control height",
            );
            near(
              select[3]!,
              inputBox[3]!,
              "select and text input heights match",
            );
            for (const [previous, next] of [
              [label, inputBox],
              [inputBox, help],
              [help, error],
            ])
              near(
                next![1]! - previous![1]! - previous![3]!,
                labelGap,
                "field label/help/error spacing",
              );
            near(
              materialField[1]! - field[1]! - field[3]!,
              (skin.tokens.spacing?.field ?? skin.tokens.inset) * scale,
              "form field spacing",
            );
            near(
              cancel[1]! - footer[1]!,
              labelGap + skin.tokens.lineWidth * scale,
              "footer top inset follows separator",
            );
            near(
              footer[1]! + footer[3]! - cancel[1]! - cancel[3]!,
              labelGap,
              "footer bottom inset",
            );
            if (panelWidth === 360) {
              const status = await rect("layout-status");
              near(
                status[0]! - panel[0]!,
                inset,
                "footer status stays at leading edge",
              );
              check(
                status[0]! + status[2]! <= cancel[0]! - labelGap + 0.1,
                `${name}: status leaves room for actions`,
              );
            }
            const state = await client.inspect();
            const identities = [
              "layout-name/input",
              "layout-material",
              "layout-apply",
            ].map(
              (id) =>
                state.entities.find(
                  (entity) => entity.metadata.symbolicId === id,
                )!.id,
            );
            if (controlIdentities)
              check(
                identities.every(
                  (id, index) => id === controlIdentities![index],
                ),
                `${name}: resizing preserves control identities`,
              );
            controlIdentities = identities;
            near(
              row[3]!,
              caption[3]!,
              "text-only row has no control minimum height",
            );
            near(apply[2]!, 72, "action width does not scale with font");
            near(
              apply[3]!,
              skin.tokens.controlHeight * scale,
              "footer preserves action height",
            );
            near(
              apply[0]! - cancel[0]! - cancel[2]!,
              labelGap,
              "action gap uses skin scale",
            );
            near(
              panel[0]! + panelWidth - apply[0]! - apply[2]!,
              inset,
              "actions end at content edge",
            );
            check(
              panel[1]! + panel[3]! <= 640,
              `${name}: natural panel fits the canvas`,
            );
            const frame = await host.presentation.capture(view, {
              afterOutputs: [{ kind: "canvas", world }],
            });
            save(`${name}-layout-${panelWidth}-${fontSize}`, frame);
            const pixels = new Uint8Array(frame.pixels);
            for (const box of [inputBox, select, cancel, apply]) {
              let ink = 0;
              for (
                let y = Math.ceil(box[1]! + 6);
                y < box[1]! + box[3]! - 6;
                y++
              )
                for (
                  let x = Math.ceil(box[0]! + 6);
                  x < box[0]! + box[2]! - 6;
                  x++
                ) {
                  const at = (y * 400 + x) * 4;
                  if (pixels[at]! + pixels[at + 1]! + pixels[at + 2]! > 300)
                    ink++;
                }
              check(
                ink > 15,
                `${name}: control ${box} has ${ink} visible pixels`,
              );
            }
            const actionInput = await host.input.open(view);
            try {
              const point = [
                (apply[0]! + apply[2]! / 2) / 400,
                (apply[1]! + apply[3]! / 2) / 640,
              ] as const;
              await actionInput.send({
                kind: "pointerMove",
                pointer: 1n,
                point,
              });
              const down = await actionInput.send({
                kind: "pointerDown",
                pointer: 1n,
                point,
              });
              const up = await actionInput.send({
                kind: "pointerUp",
                pointer: 1n,
                point,
              });
              check(
                down.disposition === "routed" && up.disposition === "routed",
                `${name}: action receives routed pointer input`,
              );
              await root.flush();
              await host.presentation.frame(view);
              check(
                applied === 1,
                `${name}: trailing action receives real input`,
              );
            } finally {
              await actionInput.close();
            }
            report.push(
              `${name}: layout ${panelWidth} at font ${fontSize} has one inset, aligned fields and visible trailing actions`,
            );
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
  } catch (error) {
    return {
      failure: error instanceof Error ? error.message : String(error),
      images,
      report,
    };
  }
  // Labeled and unlabeled switches use the same track proportions.
  return {
    failure: differences.every((changed) => changed > 40)
      ? null
      : `Labelled switches did not move into their wide track: ${differences}`,
    images,
    report,
  };
}
