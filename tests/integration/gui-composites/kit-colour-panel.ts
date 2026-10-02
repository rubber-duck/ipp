import { createElement as h } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../packages/ipp-react/src/index.js";
import { Layout, type GuiHsva } from "../../../packages/ipp-react/src/gui.js";
import {
  ColorPicker,
  GuiKit,
} from "../../../packages/ipp-react/src/gui-kit.js";
import {
  componentFields,
  readControl,
  symbols,
  type PanelSpec,
} from "./page.js";

/**
 * The GUI kit's colour picker: a panel World whose React root declares one
 * picker with its alpha rail and one preset, the language's error magenta
 * #F4449F, at half the kit's design size, starting at the sheet's #54F4FF.
 * The application records every colour the picker reports; the snapshot
 * reads the control's colour beside the hex, channels and error the picker
 * derives from it.
 */
export const KIT_COLOUR_PANEL = [192, 192] as const;

/** Half the design body size: the control 120 x 100, its rails 12 wide. */
export const KIT_COLOUR_FONT = 8;
const MARGIN = 4;

/** The sheet's #54F4FF as HSV on sRGB-encoded values, opaque. */
export const KIT_COLOUR_START: GuiHsva = {
  hue: (4 - 160 / 171) / 6,
  saturation: 171 / 255,
  value: 1,
  alpha: 1,
};

/** The preset: #F4449F as HSV on sRGB-encoded values, opaque. */
export const KIT_COLOUR_PRESET: GuiHsva = {
  hue: (6 - (0x9f - 0x44) / (0xf4 - 0x44)) / 6,
  saturation: (0xf4 - 0x44) / 0xf4,
  value: 0xf4 / 255,
  alpha: 1,
};

/** The picker's state as an application and a reader of its fields see it. */
export interface PickerSnapshot {
  /** The colour the control holds. */
  readonly color: GuiHsva;
  /** The hex field's text, the R, G and B bytes and the A percent shown. */
  readonly hex: string;
  readonly channels: readonly number[];
  /** The error line's text, or "". */
  readonly error: string;
  /** The colours the application received, in order. */
  readonly reports: readonly GuiHsva[];
}

export function kitColourPanel(origin: readonly [number, number]) {
  return {
    name: "kit-colour",
    origin,
    extent: KIT_COLOUR_PANEL,
    async build({ client, font, contract, report }) {
      const [width, height] = KIT_COLOUR_PANEL;
      const reports: GuiHsva[] = [];
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      await root.render(
        h(
          GuiKit,
          { contract, font, fontSize: KIT_COLOUR_FONT },
          h(
            Entity,
            { id: "colour-kit-root" },
            h(Layout, {
              kind: 2,
              width,
              height,
              padding_left: MARGIN,
              padding_right: MARGIN,
              padding_top: MARGIN,
              padding_bottom: MARGIN,
            }),
            h(
              Children,
              null,
              h(ColorPicker, {
                id: "kit-pick",
                label: "Color",
                defaultValue: KIT_COLOUR_START,
                presets: [{ value: KIT_COLOUR_PRESET, label: "Magenta" }],
                onChange: (value) => reports.push(value),
              }),
            ),
          ),
        ),
      );

      /** One component's fields of each declared entity among `wanted`. */
      const fieldsOf = (
        ids: Map<string, bigint>,
        wanted: readonly (readonly [string, string])[],
      ) =>
        Promise.all(
          wanted.map(async ([symbol, component]) => {
            const entity = ids.get(symbol);
            if (entity === undefined) return undefined;
            const page = await client.inspectPage({
              collection: "entities",
              target: entity,
              limit: 1,
            });
            return componentFields(client, page, entity, component);
          }),
        );

      return {
        reports,
        world: client.worldReference!,
        /** The picker's colour and what it shows now. */
        async snapshot(): Promise<PickerSnapshot> {
          const [control, hex, red, green, blue, alpha, error] = await fieldsOf(
            await symbols(client),
            [
              ["kit-pick/control", "GuiColor"],
              ["kit-pick/hex/field", "GuiTextInput"],
              ["kit-pick/red/field", "GuiTextInput"],
              ["kit-pick/green/field", "GuiTextInput"],
              ["kit-pick/blue/field", "GuiTextInput"],
              ["kit-pick/alpha/field", "GuiTextInput"],
              ["kit-pick/error/text", "CanvasText"],
            ],
          );
          return {
            color: {
              hue: Number(control?.hue),
              saturation: Number(control?.saturation),
              value: Number(control?.value),
              alpha: Number(control?.alpha),
            },
            hex: String(hex?.text ?? ""),
            channels: [red, green, blue, alpha].map((field) =>
              Number(field?.value),
            ),
            error: String(error?.text ?? ""),
            reports: [...reports],
          };
        },
        /** A control's evaluated box in the panel's canvas. */
        async bounds(symbol: string) {
          const entity = (await symbols(client)).get(symbol);
          const state =
            entity === undefined
              ? undefined
              : await readControl(client, entity);
          if (!state) throw new Error(`Picker control ${symbol} is missing`);
          return state.bounds;
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type KitColourPanel = Awaited<
  ReturnType<ReturnType<typeof kitColourPanel>["build"]>
>;
