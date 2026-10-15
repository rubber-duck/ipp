import type {
  Client,
  GuiPhysicalContext,
  GuiPhysicalKey,
  PresentedCapture,
} from "@ipp/client";
import type { GuiControlHandle } from "@ipp/react/gui";
import { check } from "../../../harness/page/checks.js";
import {
  GLASS_SLIDER_CASES,
  GLASS_VALUE_RECTS,
  GLASS_VALUES_EXTENT,
} from "../fixtures/values.js";
import type { GlassValuesDriver } from "../scenarios/values.js";

export interface GlassValuesInteraction {
  readonly client: Client;
  readonly input: GuiPhysicalContext;
  control(id: string): GuiControlHandle;
  capture(): Promise<PresentedCapture>;
}

/** Uses the lab's active physical context; the lab owns its lifetime. */
export function valuesDriver(
  interaction: GlassValuesInteraction,
  save: (name: string, capture: PresentedCapture) => Promise<void>,
): GlassValuesDriver {
  const pointer = 4703n;
  const at = (id: string, fraction = 0.5): readonly [number, number] => {
    const rect = GLASS_VALUE_RECTS[id];
    check(rect, `Missing glass control bounds for ${id}`);
    const [x, y, width, height] = rect;
    const slider = GLASS_SLIDER_CASES.find((entry) => entry.id === id);
    // Independent contained-thumb geometry: centres stay half a 0.75-side
    // thumb in from the control's ends. Vertical minimum is at the bottom.
    const inset = 0.375 * Math.min(width, height);
    const px =
      slider?.axis === 0
        ? x + inset + fraction * (width - 2 * inset)
        : x + width / 2;
    const py =
      slider?.axis === 1
        ? y + height - inset - fraction * (height - 2 * inset)
        : y + height / 2;
    return [px / GLASS_VALUES_EXTENT[0], py / GLASS_VALUES_EXTENT[1]];
  };
  const send = interaction.input.send.bind(interaction.input);
  const keyMap: Record<string, GuiPhysicalKey> = {
    Space: "space",
    Home: "home",
    End: "end",
    ArrowLeft: "left",
  };
  return {
    async click(id) {
      const point = at(id);
      await send({ kind: "pointerMove", pointer, point });
      await send({ kind: "pointerDown", pointer, point });
      await send({ kind: "pointerUp", pointer, point });
    },
    async drag(id, from, to) {
      await send({ kind: "pointerMove", pointer, point: at(id, from) });
      await send({ kind: "pointerDown", pointer, point: at(id, from) });
      try {
        for (let index = 1; index <= 4; index++)
          await send({
            kind: "pointerMove",
            pointer,
            point: at(id, from + ((to - from) * index) / 4),
          });
      } finally {
        await send({ kind: "pointerUp", pointer, point: at(id, to) });
      }
    },
    async focus(id, part = 0) {
      const outcome = await interaction
        .control(id)
        .action({ kind: "focus", part });
      check(outcome.ok, `Focus refused for ${id}`);
    },
    async key(name) {
      const shift = name.startsWith("Shift+");
      const key = keyMap[shift ? name.slice(6) : name];
      check(key, `Unknown glass scenario key ${name}`);
      await send({ kind: "key", key, shift });
    },
    async checked(id) {
      return (await interaction.control(id).read()).checked === true;
    },
    async values(id) {
      const fields = await interaction.control(id).read();
      return [Number(fields.value), Number(fields.upper)];
    },
    async focusedPart(id) {
      const target = interaction.control(id).target;
      const page = await interaction.client.inspectPage({
        collection: "guiFocus",
        target: target.entity,
      });
      return page.guiFocus?.find(
        (record) => record.target.entity === target.entity,
      )?.part;
    },
    async capture(name) {
      await save(name, await interaction.capture());
    },
  };
}
