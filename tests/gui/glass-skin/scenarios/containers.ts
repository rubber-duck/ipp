import type { Client, HostClientBase } from "@ipp/client";
import type { GlassKitContract } from "../../../../packages/ipp-react/src/gui-glass/kit-theme.js";
import type { ThemeContract } from "../../skin-lab/theme.js";
import {
  SpecimenSession,
  type Present,
} from "../../skin-lab/specimen-session.js";
import type { GlassSkinEnvironment } from "./glass-skin.js";
import { image } from "../../../../tools/shared-host/presentation.js";
import type { RgbaImage } from "../../../../tools/shared-host/images.js";
import { check } from "../../../harness/page/checks.js";
import {
  containersSpecimen,
  containerEvents,
  type ContainerOptions,
} from "../composites/containers-specimen.js";
import {
  checkContainerGlass,
  checkContainerTransmission,
  containerChangedPixels,
} from "../composites/containers-oracle.js";

function base64(bytes: Uint8Array) {
  let text = "";
  for (let start = 0; start < bytes.length; start += 0x8000)
    text += String.fromCharCode(...bytes.subarray(start, start + 0x8000));
  return btoa(text);
}

/** Real native/worker rendering, ordinary callbacks and authoritative scroll values. */
export async function exerciseGlassContainers(
  host: HostClientBase<Client>,
  contract: GlassKitContract & ThemeContract,
  font: Uint8Array<ArrayBuffer>,
  present: Present,
  environment: GlassSkinEnvironment,
) {
  const events = containerEvents();
  const session = await SpecimenSession.open({
    host,
    contract,
    font,
    themes: {},
    specimen: containersSpecimen(contract, events),
    world: "glass/containers",
  });
  const save = async (name: string, frame: RgbaImage) =>
    environment.capture({
      name,
      width: frame.width,
      height: frame.height,
      rgba: base64(frame.pixels),
    });
  const frame = async (options: ContainerOptions = {}) => {
    await session.update({
      specimen: containersSpecimen(contract, events, options),
    });
    const result = await session.capture(present, ["containers"]);
    check(result.leftover.length === 0, "Container capture left active input");
    return result.image;
  };
  try {
    check(
      (await session.assetSettled("ipp-glass/shader")).status === "loaded",
      "Container shader failed to load",
    );
    const cyan = await frame();
    await save("containers-cyan", cyan);
    const glass = checkContainerGlass(cyan);
    const backdrop = await frame({ backdrop: [0.8, 0.05, 0.35, 1] });
    const transmission = checkContainerTransmission(cyan, backdrop);
    await save("containers-opaque", backdrop);
    const violet = await frame({
      palette: { primary: "#CF9CFF", secondary: "#785CFF" },
    });
    await save("containers-violet", violet);
    const palettePixels = containerChangedPixels(cyan, violet);
    check(palettePixels > 200, "Container palette did not affect glass");
    const left = await frame({ light: { intensity: 1, x: 0.1, y: 0.1 } });
    const right = await frame({ light: { intensity: 1, x: 0.9, y: 0.1 } });
    await save("containers-light-left", left);
    await save("containers-light-right", right);
    const lightPixels = containerChangedPixels(left, right);
    check(
      lightPixels > 100,
      "Container directional light did not affect glass",
    );
    await frame();
    const controls = await session.interact(
      present,
      async ({ control, input, capture }) => {
        const window = control("close");
        const target = window.target;
        for (const kind of [
          "minimize",
          "maximize",
          "restore",
          "close",
        ] as const)
          check(
            (await control(kind).action({ kind: "press" })).ok,
            `Window ${kind} action refused`,
          );
        await capture();
        for (const kind of [
          "minimize",
          "maximize",
          "restore",
          "close",
        ] as const)
          check(events[kind] === 1, `Window ${kind} callback was lost`);
        const disabled = await control("disabled-expander").action({
          kind: "press",
        });
        check(
          !disabled.ok && disabled.error.reason === "Unavailable",
          "Disabled expander accepted press",
        );
        const expander = control("expander");
        check(
          (await expander.action({ kind: "press" })).ok,
          "Expander action refused",
        );
        await capture();
        check(events.expanded === false, "Expander failed to collapse");
        await expander.action({ kind: "press" });
        await capture();
        check(Boolean(events.expanded) === true, "Expander failed to reopen");
        const click = async (x: number, y: number) => {
          const point = [x / 860, y / 620] as const;
          await input.send({ kind: "pointerMove", pointer: 82n, point });
          const down = await input.send({
            kind: "pointerDown",
            pointer: 82n,
            point,
          });
          check(
            down.disposition === "routed" && !down.error,
            "Container physical pointer missed control",
          );
          await input.send({ kind: "pointerUp", pointer: 82n, point });
          await capture();
        };
        await click(373, 44);
        check(
          Number(events.close) === 2,
          "Window glass appearance blocked physical close hit",
        );
        await click(100, 362);
        check(
          String(events.selected) === "alpha",
          "Data grid glass blocked row selection callback",
        );
        const scroll = control("scroll");
        const before = await scroll.read();
        const routed = await input.send({
          kind: "wheel",
          point: [520 / 860, 470 / 620],
          delta: [0, 78],
        });
        check(
          routed.disposition === "routed" && !routed.error,
          "Glass scroll wheel missed native control",
        );
        const scrolled = image(await capture());
        await save("containers-scrolled", scrolled);
        const after = await scroll.read();
        check(
          Number(after.offset_y) > Number(before.offset_y),
          "Native scroll offset did not move",
        );
        const list = control("virtual");
        check(
          (await list.action({ kind: "scrollToIndex", index: 70 })).ok,
          "Virtual list semantic scroll refused",
        );
        const virtual = image(await capture());
        await save("containers-virtual", virtual);
        const listState = await list.read();
        check(
          Number(listState.anchor_index) >= 60,
          "Virtual list did not retain native anchor",
        );
        await session.update({
          specimen: containersSpecimen(contract, events, {
            palette: { primary: "#CF9CFF", secondary: "#785CFF" },
          }),
        });
        await capture();
        check(
          control("close").target.entity === target.entity &&
            control("close").target.incarnation === target.incarnation,
          "Retheming replaced window control identity",
        );
        check(
          (await scroll.read()).offset_y === after.offset_y,
          "Retheming reset scroll offset",
        );
        check(
          (await list.read()).anchor_index === listState.anchor_index,
          "Retheming reset virtual anchor",
        );
        return {
          windowCallback: true,
          physicalWindowHit: true,
          selectedRowCallback: true,
          disabledRefused: true,
          expander: true,
          scrollOffset: after.offset_y,
          virtualAnchor: listState.anchor_index,
          identity: true,
        };
      },
    );
    await session.update({
      specimen: containersSpecimen(contract, events, { editing: true }),
    });
    const editing = await session.interact(
      present,
      async ({ input, capture }) => {
        // Focus is taken by the existing CellEditor when its native control publishes.
        await input.send({
          kind: "pointerMove",
          pointer: 82n,
          point: [340 / 860, 362 / 620],
        });
        await input.send({
          kind: "pointerDown",
          pointer: 82n,
          point: [340 / 860, 362 / 620],
        });
        await input.send({
          kind: "pointerUp",
          pointer: 82n,
          point: [340 / 860, 362 / 620],
        });
        await capture();
        const native = input.nativeText;
        check(native, "Cell editor did not expose native text focus");
        check(
          (
            await input.editText(native.fence, {
              kind: "selection",
              start: 0,
              end: new TextEncoder().encode(native.text).length,
            })
          ).disposition === "routed",
          "Cell editor selection refused",
        );
        const focused = input.nativeText;
        check(focused, "Cell editor lost native focus");
        check(
          (
            await input.editText(focused.fence, {
              kind: "text",
              text: "SYNCED",
            })
          ).disposition === "routed",
          "Cell editor edit refused",
        );
        await input.send({ kind: "key", key: "enter" });
        const rendered = image(await capture());
        await save("containers-editing", rendered);
        check(
          events.edit === "SYNCED",
          "Cell editor failed to submit native text",
        );
        return { submitted: events.edit };
      },
    );
    await environment.record({
      containers: {
        glass,
        transmission,
        palettePixels,
        lightPixels,
        controls,
        editing,
      },
    });
    return { family: "containers" };
  } finally {
    await session.close();
  }
}
