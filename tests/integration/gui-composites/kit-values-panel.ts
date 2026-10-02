import { createElement as h } from "react";
import {
  Children,
  Entity,
  createRoot,
  type ReactWorldClient,
} from "../../../packages/ipp-react/src/index.js";
import { Layout } from "../../../packages/ipp-react/src/gui.js";
import {
  GuiKit,
  Knob,
  NumericStepper,
  RangeSlider,
} from "../../../packages/ipp-react/src/gui-kit.js";
import {
  componentFields,
  readControl,
  symbols,
  type PanelSpec,
} from "./page.js";

/**
 * The GUI kit's value composites: a panel World whose React root declares a
 * range slider over 0..100 m holding 20 to 80, a knob over 0..100% at 50 and
 * a numeric stepper over -4..4 EV at 1.25, at half the kit's design size. The
 * application callbacks record what reaches them; the snapshot reads the
 * committed values beside the readouts, unit label and error line the
 * composites show for them.
 */
export const KIT_VALUES_PANEL = [192, 192] as const;

/** Half the design body size: the range's rail 32/3 deep, the dial 40. */
const FONT_SIZE = 8;
const MARGIN = 4;

/** What each composite reported to the application, in order. */
export interface ValuesReports {
  readonly range: (readonly number[])[];
  readonly knob: number[];
  readonly stepper: number[];
}

/** The panel's committed values and readouts, beside the reports. */
export interface ValuesSnapshot extends ValuesReports {
  /** The range slider's committed lower and upper values. */
  readonly rangeValue: readonly number[];
  /** The readouts under the range's thumbs, lower first. */
  readonly rangeReadouts: readonly string[];
  /** Where each readout's mark sits along the rail, -1 to 1. */
  readonly rangeMarks: readonly number[];
  readonly knobValue: number;
  readonly knobReadout: string;
  /** The stepper's committed number, its unit label and its error, or "". */
  readonly stepperValue: number;
  readonly stepperUnits: string;
  readonly stepperError: string;
}

export function kitValuesPanel(origin: readonly [number, number]) {
  return {
    name: "kit-values",
    origin,
    extent: KIT_VALUES_PANEL,
    async build({ client, font, contract, report }) {
      const [width, height] = KIT_VALUES_PANEL;
      const reports: ValuesReports = { range: [], knob: [], stepper: [] };
      const root = createRoot(client as unknown as ReactWorldClient, {
        onError: report,
      });
      await root.render(
        h(
          GuiKit,
          { contract, font, fontSize: FONT_SIZE },
          h(
            Entity,
            { id: "values-root" },
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
              h(RangeSlider, {
                id: "val-range",
                label: "Distance",
                min: 0,
                max: 100,
                step: 1,
                units: "m",
                defaultValue: [20, 80],
                onChange: (value) => reports.range.push(value),
              }),
              h(Knob, {
                id: "val-knob",
                label: "Gain",
                min: 0,
                max: 100,
                step: 1,
                units: "%",
                defaultValue: 50,
                onChange: (value) => reports.knob.push(value),
                layout: { margin_top: MARGIN },
              }),
              h(NumericStepper, {
                id: "val-step",
                label: "Exposure",
                min: -4,
                max: 4,
                step: 0.25,
                precision: 2,
                units: "EV",
                defaultValue: 1.25,
                onChange: (value) => reports.stepper.push(value),
                layout: { margin_top: MARGIN },
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
        /** The panel's committed values and readouts now. */
        async snapshot(): Promise<ValuesSnapshot> {
          const [
            range,
            lower,
            upper,
            knob,
            readout0,
            readout1,
            knobText,
            stepper,
            stepperUnits,
            stepperError,
          ] = await fieldsOf(await symbols(client), [
            ["val-range/slider", "GuiSlider"],
            ["val-range/thumb/0", "GuiLayout"],
            ["val-range/thumb/1", "GuiLayout"],
            ["val-knob/dial", "GuiSlider"],
            ["val-range/readout/0/text", "CanvasText"],
            ["val-range/readout/1/text", "CanvasText"],
            ["val-knob/readout/text", "CanvasText"],
            ["val-step/field", "GuiTextInput"],
            ["val-step/units", "CanvasText"],
            ["val-step/error/text", "CanvasText"],
          ]);
          return {
            rangeValue: [Number(range?.value), Number(range?.upper)],
            rangeReadouts: [
              String(readout0?.text ?? ""),
              String(readout1?.text ?? ""),
            ],
            rangeMarks: [Number(lower?.align_x), Number(upper?.align_x)],
            knobValue: Number(knob?.value),
            knobReadout: String(knobText?.text ?? ""),
            stepperValue: Number(stepper?.value),
            stepperUnits: String(stepperUnits?.text ?? ""),
            stepperError: String(stepperError?.text ?? ""),
            range: [...reports.range],
            knob: [...reports.knob],
            stepper: [...reports.stepper],
          };
        },
        /** A control's evaluated box in the panel's canvas. */
        async bounds(symbol: string) {
          const entity = (await symbols(client)).get(symbol);
          const state =
            entity === undefined
              ? undefined
              : await readControl(client, entity);
          if (!state) throw new Error(`Values control ${symbol} is missing`);
          return state.bounds;
        },
        async close() {
          await root.unmount();
        },
      };
    },
  } satisfies PanelSpec;
}

export type KitValuesPanel = Awaited<
  ReturnType<ReturnType<typeof kitValuesPanel>["build"]>
>;
