import { createRef, useState } from "react";
import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type PresentationView,
  type RootBinding,
} from "@ipp/client";
import {
  Asset,
  Entity,
  assetRef,
  createRoot,
  guiWorldOptions,
  type ReactWorldClient,
} from "@ipp/react";
import {
  Button as NativeButton,
  Layout,
  SkinProvider,
  type GuiControlHandle,
} from "@ipp/react/gui";
import {
  Button,
  Checkbox,
  Column,
  Dialog,
  Popover,
  SegmentedControl,
  TextField,
  TextLine,
  type GuiKitContract,
} from "@ipp/react/gui-kit";
import { cybertronSkin } from "@ipp/react/gui-cybertron";
import type { GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";
import { guiWidgetSizing } from "./gui-widget-sizing.js";

const EXTENT = [360, 390] as const;

export interface WidgetPresentation {
  <T>(
    binding: RootBinding,
    section: (view: PresentationView) => Promise<T>,
  ): Promise<T>;
}

/** Public widgets over real transport, native input, acknowledgements and completed frames. */
export async function guiWidgets(
  host: HostClientBase<Client>,
  contract: GuiContract & GuiKitContract,
  font: Uint8Array<ArrayBuffer>,
  present?: WidgetPresentation,
) {
  const world = (
    await host.createWorld(
      guiWorldOptions({
        temporary: true,
        canvas: { extent: EXTENT, unitsPerMetre: 1 },
      }),
    )
  ).reference;
  const client = await host.openWorld(world);
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (error) => errors.push(error),
  });
  const checkbox = createRef<GuiControlHandle>();
  const text = createRef<GuiControlHandle>();
  const trigger = createRef<GuiControlHandle>();
  const report: string[] = [];
  const edits: unknown[] = [];
  const selections: string[] = [];
  let selectionIntent = "flat";
  let selectionUpdates = Promise.resolve();
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  let binding: RootBinding | undefined;

  function Form({
    accept,
    configured,
  }: {
    readonly accept: boolean;
    readonly configured: boolean;
  }) {
    const [checked, setChecked] = useState(true);
    const [value, setValue] = useState("allowed");
    return (
      <Column
        id="widgets"
        width={EXTENT[0]}
        height={EXTENT[1]}
        padding={12}
        gap="compact"
      >
        <Checkbox
          id="checked"
          ref={checkbox}
          checked={checked}
          onCheckedChange={(next) => {
            edits.push(next);
            if (accept) setChecked(next);
          }}
        >
          Enabled
        </Checkbox>
        <TextField
          id="name"
          ref={text}
          label="Name"
          placeholder={configured ? "Search" : undefined}
          masked={configured ? true : undefined}
          padding={configured ? 9 : undefined}
          value={value}
          onChange={(next) => {
            edits.push(next);
            if (accept) setValue(next);
          }}
        />
        <Popover.Root id="options">
          <Popover.Trigger>
            <Button id="options-button" ref={trigger}>
              Options
            </Button>
          </Popover.Trigger>
          <Popover.Content width={200}>
            <TextLine text="Custom content" />
          </Popover.Content>
        </Popover.Root>
        <SkinProvider fontSize={10}>
          <Entity id="native-small">
            <Layout height={24} width={180} />
            <NativeButton label="Inherited native font" />
          </Entity>
        </SkinProvider>
        <Button
          id="owned-default"
          width={120}
          selected={configured ? true : undefined}
        >
          Owned default
        </Button>
        <Entity id="raw-omission">
          <Layout height={20} width={180} />
          <NativeButton
            label="Raw omission"
            selected={configured ? true : undefined}
          />
        </Entity>
        <Button width={120}>Generated ID</Button>
        <SegmentedControl
          id="selection"
          value={selectionIntent}
          options={[
            { value: "flat", label: "Flat" },
            { value: "curved", label: "Curved" },
          ]}
          onChange={(next) => {
            selections.push(next);
            if (accept) {
              // Native gallery parents serialize imperative React replacements
              // through an asynchronous application queue.
              selectionUpdates = selectionUpdates
                .then(async () => {
                  selectionIntent = next;
                  await root.render(scene(accept, configured));
                })
                .catch((error: Error) => {
                  errors.push(error);
                });
            }
          }}
        />
      </Column>
    );
  }

  const scene = (accept: boolean, configured = false) => (
    <>
      <Asset
        id="widgets-font"
        kind={17}
        data={font}
        encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
      />
      <SkinProvider
        id="widgets-skin"
        contract={contract}
        skin={cybertronSkin(contract)}
        font={assetRef("widgets-font")}
        fontSize={16}
        reducedMotion
      >
        <Form accept={accept} configured={configured} />
        <Dialog.Root id="widget-dialog">
          <Dialog.Content>
            <Dialog.Header title="Settings" />
            <Dialog.Body>
              <TextLine text="Dialog body" />
            </Dialog.Body>
            <Dialog.Actions>
              <Dialog.Close>Done</Dialog.Close>
            </Dialog.Actions>
          </Dialog.Content>
        </Dialog.Root>
      </SkinProvider>
    </>
  );

  const wait = async (
    ready: () => boolean | Promise<boolean>,
    label: string,
  ) => {
    const deadline = performance.now() + 15_000;
    while (!(await ready())) {
      check(
        performance.now() < deadline,
        `Widgets timeout: ${label}; ${errors.map((error) => error.message)}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    await root.flush();
  };

  try {
    await root.render(scene(false, true));
    await wait(
      () =>
        root.getAsset("widgets-font")?.status === "loaded" &&
        !!checkbox.current &&
        !!text.current &&
        !!trigger.current,
      "font and control readiness",
    );
    // Transparent layout roots do not implicitly author CanvasBounds. The
    // fixture opts into the ordinary System-written bounds component for
    // layout observation, without adding paint or authored geometry.
    const observationPage = await client.inspect();
    const probes = ["name", "options", "options/popover"].map((symbol) => {
      const entity = observationPage.entities.find(
        (entity) => entity.metadata.symbolicId === symbol,
      );
      check(entity !== undefined, `Missing widget layout probe: ${symbol}`);
      return entity;
    });
    const observed = await client.batch(
      probes
        .filter(
          (entity) =>
            !entity.components.some(
              (component) =>
                component.component === client.components.CanvasBounds!.id,
            ),
        )
        .map((entity) => ({
          kind: "insertComponent" as const,
          entity: { kind: "handle" as const, id: entity.id },
          component: client.components.CanvasBounds!.id,
          fields: [],
          adopt: true,
        })),
    );
    check(
      observed.ok,
      "Widget layout observation components were not admitted",
    );
    const targets = {
      checkbox: checkbox.current!,
      text: text.current!,
      trigger: trigger.current!,
    };
    binding = await host.setRootOutput(canvasOutput(world), {
      width: EXTENT[0],
      height: EXTENT[1],
      devicePixelRatio: 1,
    });
    const section = async (view: PresentationView) => {
      let sequence: bigint | undefined;
      const capture = async (label: string, record = true) => {
        const frame = await host.presentation.capture(view, {
          afterOutputs: [canvasOutput(world)],
          ...(sequence === undefined ? {} : { afterSequence: sequence }),
        });
        sequence = frame.sequence;
        const bytes = new Uint8Array(frame.pixels);
        if (record)
          images.push({
            label,
            width: EXTENT[0],
            height: EXTENT[1],
            pixels: Array.from(bytes),
            sequence,
          });
        return bytes;
      };
      const initial = await capture("widgets-initial");
      check(
        initial.filter((value, index) => index % 4 !== 3 && value > 32).length >
          1000,
        "Widgets frame lacks visible controls and text",
      );
      check(
        edits.length === 0,
        "Widget subscription seeds reached public change callbacks",
      );
      await root.render(scene(false));
      await capture("widgets-default-layout", false);
      const defaults = await client.inspect();
      const named = (symbol: string, component: string) => {
        const entity = defaults.entities.find(
          (entity) => entity.metadata.symbolicId === symbol,
        );
        const value = entity?.components.find(
          (item) => item.component === client.components[component]!.id,
        );
        check(
          value !== undefined,
          `Missing observed widget component: ${symbol}.${component}`,
        );
        return value.fields;
      };
      check(
        named("name/input", "GuiTextInput").placeholder === "" &&
          named("name/input", "GuiTextInput").masked === false,
        "Owned TextField omission did not restore placeholder/masking defaults",
      );
      check(
        named("name", "GuiLayout").padding_left === 0,
        "Owned named padding remained after prop omission",
      );
      check(
        named("owned-default", "GuiButton").selected === false,
        "Owned Button omission did not restore its default",
      );
      check(
        named("raw-omission", "GuiButton").selected === true,
        "Raw declaration omission changed its retained native field",
      );
      report.push(
        "owned wrapper omissions reset defaults while raw omissions preserve fields",
      );
      const generated = defaults.entities.find((entity) =>
        entity.components.some(
          (component) =>
            component.component === client.components.GuiButton!.id &&
            component.fields.label === "Generated ID",
        ),
      )!;
      const generatedBox = generated.components.find(
        (component) =>
          component.component === client.components.CanvasBounds!.id,
      )!.fields;
      const boxes = [
        ...[
          "checked",
          "name",
          "options",
          "native-small",
          "owned-default",
          "raw-omission",
        ].map((symbol) => named(symbol, "CanvasBounds")),
        generatedBox,
      ];
      for (let index = 0; index < boxes.length; index++) {
        const box = boxes[index]!;
        check(
          Number(box.height) > 0 &&
            Number(box.y) + Number(box.height) <= EXTENT[1],
          "Owned widget has no visible vertical extent",
        );
        if (index > 0) {
          const before = boxes[index - 1]!;
          check(
            Number(before.y) + Number(before.height) <= Number(box.y) + 0.01,
            "Column widgets overlap instead of fitting their intrinsic content",
          );
        }
      }
      report.push(
        "complete widgets occupy ordered non-overlapping native bounds",
      );
      const input = await host.input.open(view);
      try {
        const page = await client.inspectPage({
          collection: "entities",
          target: targets.checkbox.target.entity,
          limit: 1,
        });
        const bounds = page.entities[0]!.components.find(
          (component) =>
            component.component === client.components.CanvasBounds!.id,
        )!.fields;
        const point = [
          (Number(bounds.x) + Number(bounds.width) / 2) / EXTENT[0],
          (Number(bounds.y) + Number(bounds.height) / 2) / EXTENT[1],
        ] as const;
        for (let repetition = 0; repetition < 2; repetition++) {
          const before = edits.length;
          for (const kind of ["pointerDown", "pointerUp"] as const) {
            const outcome = await input.send({ kind, pointer: 1n, point });
            check(
              outcome.disposition === "routed",
              "Checkbox physical input was not routed",
            );
          }
          await wait(
            async () =>
              edits.length > before &&
              (await targets.checkbox.read()).checked === true,
            "repeated rejected physical checkbox edit restored",
          );
          const textBefore = edits.length;
          await targets.text.action({ kind: "text", value: "rejected" });
          await wait(
            async () =>
              edits.length > textBefore &&
              (await targets.text.read()).text === "allowed",
            "repeated rejected text restored",
          );
        }
        const selectedFields = async (key: string) => {
          const page = await client.inspect();
          return (
            page.entities
              .find(
                (entity) => entity.metadata.symbolicId === `selection/${key}`,
              )!
              .components.find(
                (component) =>
                  component.component === client.components.GuiButton!.id,
              )!.fields.selected === true
          );
        };
        const select = async (key: string) => {
          await root.flush();
          const page = await client.inspect();
          const bounds = page.entities
            .find(
              (entity) => entity.metadata.symbolicId === `selection/${key}`,
            )!
            .components.find(
              (component) =>
                component.component === client.components.CanvasBounds!.id,
            )!.fields;
          const point = [
            (Number(bounds.x) + Number(bounds.width) / 2) / EXTENT[0],
            (Number(bounds.y) + Number(bounds.height) / 2) / EXTENT[1],
          ] as const;
          for (const kind of [
            "pointerMove",
            "pointerDown",
            "pointerUp",
          ] as const) {
            const outcome = await input.send({ kind, pointer: 1n, point });
            check(
              outcome.disposition === "routed",
              "Segment physical input was not routed",
            );
          }
        };
        for (let repetition = 0; repetition < 2; repetition++) {
          const before = selections.length;
          await select("curved");
          await wait(
            async () =>
              selections.length === before + 1 &&
              (await selectedFields("flat")) &&
              !(await selectedFields("curved")),
            "rejected grouped selection restored",
          );
          check(
            selectionIntent === "flat",
            "Rejected selection changed application intent",
          );
        }
        report.push(
          "controlled native edits restore unchanged application intent",
        );
        const rejected = await capture("widgets-rejected");

        await root.render(scene(true));
        await targets.checkbox.action({ kind: "toggle" });
        await wait(
          async () => (await targets.checkbox.read()).checked === false,
          "accepted checked edit",
        );
        await targets.text.action({ kind: "text", value: "accepted" });
        await wait(
          async () => (await targets.text.read()).text === "accepted",
          "accepted text edit",
        );
        check(
          checkbox.current === targets.checkbox &&
            text.current === targets.text,
          "Controlled updates replaced native handles",
        );
        const selectionStart = selections.length;
        for (const wanted of ["curved", "flat", "curved", "flat"]) {
          await select(wanted);
          await wait(
            async () =>
              selectionIntent === wanted && (await selectedFields(wanted)),
            "accepted grouped selection",
          );
          await capture(`widgets-selection-${selections.length}`, false);
          check(
            selectionIntent === wanted &&
              (await selectedFields(wanted)) &&
              !(await selectedFields(wanted === "flat" ? "curved" : "flat")),
            "Completed grouped selection reverted after application acceptance",
          );
        }
        check(
          selections.slice(selectionStart).join(",") ===
            "curved,flat,curved,flat",
          "Grouped selection delivered stale correction callbacks",
        );
        report.push(
          "grouped physical selections settle accepted and rejected intent in both directions",
        );
        const accepted = await capture("widgets-accepted");
        let changed = 0;
        for (let index = 0; index < rejected.length; index++)
          if (rejected[index] !== accepted[index]) changed++;
        check(
          changed > 100,
          "Accepted value changes did not change the captured control pixels",
        );
        report.push(
          "accepted callbacks retain identity and update actual pixels",
        );

        await targets.trigger.action({ kind: "press" });
        await wait(async () => {
          const page = await client.inspect();
          const overlay = page.entities.find(
            (entity) => entity.metadata.symbolicId === "options/popover",
          );
          return (
            overlay?.components.find(
              (component) =>
                component.component === client.components.GuiBehavior!.id,
            )?.fields.visible === true
          );
        }, "custom popover trigger");
        await capture("widgets-popover");
        const popupPage = await client.inspect();
        const popup = popupPage.entities
          .find((entity) => entity.metadata.symbolicId === "options/popover")!
          .components.find(
            (component) =>
              component.component === client.components.CanvasBounds!.id,
          )!.fields;
        check(
          Number(popup.width) > 0 &&
            Number(popup.height) > 0 &&
            Number(popup.x) >= 0 &&
            Number(popup.y) >= 0 &&
            Number(popup.x) + Number(popup.width) <= EXTENT[0] &&
            Number(popup.y) + Number(popup.height) <= EXTENT[1],
          "Compound popover content is outside the completed canvas",
        );
        report.push("compound popover opens from its real custom trigger");
      } finally {
        await input.close();
      }
      const inspected = await client.inspect();
      const native = inspected.entities.find(
        (entity) => entity.metadata.symbolicId === "native-small",
      )!;
      check(
        native.components.find(
          (component) => component.component === client.components.GuiFont!.id,
        )?.fields.font_size === 10,
        "Nested provider font default did not reach the raw native button",
      );
      const widget = inspected.entities.find(
        (entity) => entity.metadata.symbolicId === "widgets",
      )!;
      const themes = inspected.entities.filter((entity) =>
        entity.metadata.symbolicId?.startsWith("widgets-skin/theme/"),
      );
      check(themes.length > 0, "Widget skin declared no themes");
      check(
        themes.every((entity) => entity.link.parent !== widget.id),
        "Provider themes joined the visible widget layout",
      );
      report.push(
        "nested providers retain fonts and independent World-local resources",
      );
    };
    if (present) await present(binding, section);
    else {
      const surface = await host.presentation.surface();
      const view = await host.presentation.select(surface, binding);
      try {
        await section(view);
      } finally {
        await host.presentation.clear(view);
      }
    }
    check(
      errors.length === 0,
      `Widget errors: ${errors.map((error) => error.message)}`,
    );
    const sizing = await guiWidgetSizing(host, contract, font, present);
    images.push(...sizing.images);
    report.push(...sizing.report);
    check(sizing.failure === null, sizing.failure ?? "Widget sizing failed");
    return { failure: null, images, report };
  } catch (error) {
    return {
      failure:
        error instanceof Error ? (error.stack ?? error.message) : String(error),
      images,
      report,
    };
  } finally {
    await selectionUpdates;
    await root.render(null);
    await root.unmount();
    if (binding) await host.clearRootOutput(binding).catch(() => {});
    await client.close();
    await host.destroyWorld(world);
  }
}
