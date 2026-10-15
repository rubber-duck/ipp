import { createRef, Suspense } from "react";
import {
  canvasOutput,
  type Client,
  type HostClientBase,
  type PresentationView,
  type RootBinding,
} from "@ipp/client";
import {
  Asset,
  Children,
  Entity,
  PresentationGate,
  assetRef,
  createRoot,
  type ReactWorldClient,
} from "@ipp/react";
import {
  Box,
  Font,
  Layout,
  Style,
  TextInput,
  type GuiControlHandle,
} from "@ipp/react/gui";
import type { GuiContract } from "../pages/gui-authoring.js";
import { check } from "../../../harness/page/checks.js";

export interface RetentionPresentation {
  <T>(
    binding: RootBinding,
    section: (view: PresentationView) => Promise<T>,
  ): Promise<T>;
}

/** Mounted urgent Suspense over a real native input, layout, transport and frames. */
export async function guiPresentationRetention(
  host: HostClientBase<Client>,
  _contract: GuiContract,
  font: Uint8Array<ArrayBuffer>,
  present?: RetentionPresentation,
) {
  const extent = [240, 220] as const;
  const world = (
    await host.createWorld({
      temporary: true,
      selectedSystems: [
        "ipp.animation",
        "ipp.gui",
        "ipp.gui-layout",
        "ipp.canvas",
        "ipp.asset-dependencies",
        "ipp.lifecycle-publisher",
      ],
      canvas: { extent, unitsPerMetre: 1 },
    })
  ).reference;
  const client = await host.openWorld(world);
  const errors: Error[] = [];
  const root = createRoot(client as unknown as ReactWorldClient, {
    onError: (error) => errors.push(error),
  });
  const input = createRef<GuiControlHandle>();
  const images: {
    label: string;
    width: number;
    height: number;
    pixels: number[];
    sequence: bigint;
  }[] = [];
  const report: string[] = [];
  let binding: RootBinding | undefined;
  let suspended = false;
  let release!: () => void;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  let changes = 0;
  let outerChanges = 0;
  function Content() {
    if (suspended) throw pending;
    return (
      <Entity id="retained-content">
        <PresentationGate />
        <Layout kind={2} width={200} height={120} />
        <Children>
          <Entity id="retained-mark">
            <Layout width={160} height={40} />
            <Style red={1} green={0} blue={0} />
            <Box width={160} height={40} />
          </Entity>
          <Entity id="retained-input">
            <Layout width={200} height={40} />
            <Font source={assetRef("retention-font")} font_size={16} />
            <TextInput
              ref={input}
              text="authored"
              onTextCommit={() => {
                changes++;
              }}
            />
          </Entity>
        </Children>
      </Entity>
    );
  }
  const scene = () => (
    <>
      <Asset
        id="retention-font"
        kind={17}
        data={font}
        encode={(bytes: Uint8Array<ArrayBuffer>) => bytes}
      />
      <Entity
        id="retention-root"
        onAction={() => {
          outerChanges++;
        }}
      >
        <Layout kind={2} width={extent[0]} height={extent[1]} />
        <Children>
          <Suspense
            fallback={
              <Entity id="retention-fallback">
                <Layout width={160} height={40} />
                <Style red={0} green={1} blue={0} />
                <Box width={160} height={40} />
              </Entity>
            }
          >
            <Content />
          </Suspense>
          <Entity id="retention-tail">
            <Layout width={80} height={20} />
            <Style red={0} green={0} blue={1} />
            <Box width={80} height={20} />
          </Entity>
        </Children>
      </Entity>
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
        `Retention timeout ${label}; ${errors.map((error) => error.message)}`,
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    await root.flush();
  };
  const entity = async (id: string) => {
    const page = await client.inspect();
    const found = page.entities.find(
      (entity) => entity.metadata.symbolicId === id,
    );
    check(found, `Missing retained entity ${id}`);
    return found;
  };
  const count = (pixels: Uint8Array, channel: number) => {
    let count = 0;
    for (let i = 0; i < pixels.length; i += 4)
      if (
        pixels[i + channel]! > 180 &&
        pixels[i + ((channel + 1) % 3)]! < 80 &&
        pixels[i + ((channel + 2) % 3)]! < 80
      )
        count++;
    return count;
  };
  try {
    await root.render(scene());
    await wait(
      () =>
        root.getAsset("retention-font")?.status === "loaded" && !!input.current,
      "font/input",
    );
    const original = input.current!;
    const target = original.target;
    await original.action({ kind: "text", value: "native edit" });
    await original.action({ kind: "focus" });
    await wait(
      async () => (await original.read()).text === "native edit",
      "native edit",
    );
    const before = await entity("retained-input");
    const initialChanges = changes;
    binding = await host.setRootOutput(canvasOutput(world), {
      width: extent[0],
      height: extent[1],
      devicePixelRatio: 1,
    });
    const section = async (view: PresentationView) => {
      let sequence: bigint | undefined;
      const capture = async (label: string) => {
        const frame = await host.presentation.capture(view, {
          afterOutputs: [canvasOutput(world)],
          ...(sequence === undefined ? {} : { afterSequence: sequence }),
        });
        sequence = frame.sequence;
        const pixels = new Uint8Array(frame.pixels);
        images.push({
          label,
          width: extent[0],
          height: extent[1],
          pixels: Array.from(pixels),
          sequence,
        });
        return pixels;
      };
      const initial = await capture("retention-edited");
      check(
        (await client.inspectPage({ collection: "guiFocus" })).guiFocus?.some(
          (focus) => focus.target.entity === target.entity,
        ),
        "Input focus was not established",
      );
      check(count(initial, 0) > 4000, "Mounted content has no red mark");
      suspended = true;
      await root.render(scene());
      await wait(() => input.current === null, "hidden ref fenced");
      const hiddenOuterChanges = outerChanges;
      const hidden = await entity("retained-input");
      check(hidden.id === before.id, "Urgent hide replaced native entity");
      const hiddenControl = hidden.components.find(
        (component) =>
          component.component === client.components.GuiTextInput!.id,
      )!;
      check(
        hiddenControl.fields.text === "native edit",
        "Hide lost native input state/incarnation",
      );
      let fenced = false;
      try {
        await original.action({ kind: "focus" });
      } catch {
        fenced = true;
      }
      check(fenced, "Hidden React handle was not fenced");
      const refused = await client.batch([
        {
          kind: "guiAction",
          entity: { kind: "handle", id: target.entity },
          component: target.component,
          incarnation: target.incarnation,
          action: { kind: "focus" },
        },
      ]);
      check(
        !refused.ok && refused.error.reason === "Unavailable",
        "Hidden input accepted semantic focus",
      );
      const frame = await capture("retention-hidden-fallback");
      check(
        !(await client.inspectPage({ collection: "guiFocus" })).guiFocus?.some(
          (focus) => focus.target.entity === target.entity,
        ),
        "Hide did not cancel native focus",
      );
      check(
        count(frame, 0) < 20 && count(frame, 1) > 4000,
        "Hidden content remains painted behind fallback",
      );
      const bounds = before.components.find(
        (component) =>
          component.component === client.components.CanvasBounds!.id,
      )!.fields;
      const point = [
        (Number(bounds.x) + Number(bounds.width) / 2) / extent[0],
        (Number(bounds.y) + Number(bounds.height) / 2) / extent[1],
      ] as const;
      const physical = await host.input.open(view);
      try {
        const hit = await physical.send({
          kind: "pointerDown",
          pointer: 2n,
          point,
        });
        check(
          hit.disposition !== "routed",
          "Hidden input remained physically hittable",
        );
        await physical.send({ kind: "pointerUp", pointer: 2n, point });
      } finally {
        await physical.close();
      }
      const tail = await entity("retention-tail");
      check(
        Number(
          tail.components.find(
            (component) =>
              component.component === client.components.CanvasBounds!.id,
          )!.fields.y,
        ) < 60,
        "Hidden content still occupies flow layout",
      );
      await client.batch([
        {
          kind: "setField",
          entity: { kind: "handle", id: target.entity },
          component: target.component,
          field: {
            offset: client.components.GuiTextInput!.fields.text!.offset,
            value: { kind: "string", value: "hidden native edit" },
          },
        },
      ]);
      await root.flush();
      check(
        (await entity("retained-input")).components.find(
          (component) => component.component === target.component,
        )!.fields.text === "hidden native edit",
        "Hidden native values stopped accepting other writers",
      );
      check(
        outerChanges === hiddenOuterChanges,
        "Outside ancestor received hidden control callbacks",
      );
      check(changes === initialChanges, "Hidden control dispatched callbacks");
      report.push(
        "urgent hide retains value/incarnation, fences refs and actions, collapses flow and paints only fallback",
      );
      suspended = false;
      release();
      await root.render(scene());
      await wait(() => !!input.current, "revealed ref");
      check(
        input.current!.target.entity === target.entity &&
          input.current!.target.incarnation === target.incarnation,
        "Reveal changed native control identity",
      );
      check(
        (await input.current!.read()).text === "hidden native edit",
        "Reveal restored stale authored input text",
      );
      const revealed = await capture("retention-revealed");
      check(
        !(await client.inspectPage({ collection: "guiFocus" })).guiFocus?.some(
          (focus) => focus.target.entity === target.entity,
        ),
        "Reveal restored obsolete focus",
      );
      check(
        count(revealed, 0) > 4000 && count(revealed, 1) < 20,
        "Reveal did not restore content pixels/remove fallback",
      );
      report.push(
        "reveal reacquires same native target/value with actual completed frame",
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
      `Retention errors: ${errors.map((error) => error.message)}`,
    );
    return { failure: null, images, report };
  } catch (error) {
    return {
      failure:
        error instanceof Error ? (error.stack ?? error.message) : String(error),
      images,
      report,
    };
  } finally {
    release();
    await root.render(null);
    await root.unmount();
    if (binding) await host.clearRootOutput(binding).catch(() => {});
    await client.close();
    await host.destroyWorld(world);
  }
}
